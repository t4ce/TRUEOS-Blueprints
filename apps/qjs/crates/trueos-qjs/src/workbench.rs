//! Blueprint-owned persistent QuickJS workbench.
//!
//! A `Workbench` is intentionally neither `Send` nor `Sync`.  Its QuickJS
//! pointers are touched only by the Blueprint task that created it.  That
//! preserves QuickJS' single-context ownership rule while allowing worker
//! runtimes to retain their independent lane/task ownership.

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::VecDeque,
    format,
    string::{String, ToString},
};
use core::{
    ffi::{c_char, c_void},
    marker::PhantomData,
    ptr,
};

use crate as qjs;

const OUTPUT_LINE_CAP: usize = 256;
const OUTPUT_BYTES_CAP: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalMode {
    Auto,
    Script,
    Module,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalResult {
    pub ok: bool,
    pub mode: EvalMode,
    pub eval_count: u64,
    pub text: String,
}

struct ContextOutput {
    lines: VecDeque<String>,
    bytes: usize,
}

struct Vm {
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    output: Box<ContextOutput>,
    eval_count: u64,
}

impl Drop for Vm {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                qjs::JS_SetContextOpaque(self.ctx, ptr::null_mut());
                qjs::workers::terminate_all_for_context(self.ctx);
                qjs::async_ops::drain_all_for_context(self.ctx);
                qjs::workers::drain_all_for_context(self.ctx);
                qjs::timers::drain_all_for_context(self.ctx);
                qjs::JS_FreeContext(self.ctx);
                self.ctx = ptr::null_mut();
            }
            if !self.rt.is_null() {
                qjs::JS_FreeRuntime(self.rt);
                self.rt = ptr::null_mut();
            }
        }
    }
}

/// One persistent runtime and context owned by the qjs Blueprint.
pub struct Workbench {
    vm: Option<Vm>,
    // Raw QuickJS pointers must never leave the Blueprint owner task.
    _not_send_or_sync: PhantomData<alloc::rc::Rc<()>>,
}

impl Workbench {
    pub const fn new() -> Self {
        Self {
            vm: None,
            _not_send_or_sync: PhantomData,
        }
    }

    pub fn is_open(&self) -> bool {
        self.vm.is_some()
    }

    /// Evaluate in the persistent context, creating it lazily on first use.
    pub fn eval(&mut self, source: &str, requested_mode: EvalMode) -> Result<EvalResult, String> {
        if source.is_empty() {
            return Err("source is empty".to_string());
        }
        let vm = self.ensure_vm()?;
        vm.eval_count = vm.eval_count.saturating_add(1);
        let mode = match requested_mode {
            EvalMode::Script => EvalMode::Script,
            EvalMode::Module => EvalMode::Module,
            EvalMode::Auto if source_uses_module_syntax(source) => EvalMode::Module,
            EvalMode::Auto => EvalMode::Script,
        };
        let filename = format!("<qjs-workbench-{:04}.mjs>\0", vm.eval_count);
        let value = unsafe {
            qjs::js_eval_bytes(
                vm.ctx,
                source.as_bytes(),
                filename.as_ptr() as *const c_char,
                match mode {
                    EvalMode::Module => qjs::JS_EVAL_TYPE_MODULE,
                    EvalMode::Auto | EvalMode::Script => qjs::JS_EVAL_TYPE_GLOBAL,
                },
            )
        };
        if value.is_exception() {
            return Ok(EvalResult {
                ok: false,
                mode,
                eval_count: vm.eval_count,
                text: unsafe { exception_to_string(vm.ctx) },
            });
        }

        let text = unsafe { value_to_display_string(vm.ctx, value) }
            .unwrap_or_else(|| "undefined".to_string());
        unsafe { qjs::js_free_value(vm.ctx, value) };
        Ok(EvalResult {
            ok: true,
            mode,
            eval_count: vm.eval_count,
            text,
        })
    }

    /// Pump worker messages, async operations, timers, and QuickJS jobs once.
    pub fn poll(&mut self) -> String {
        let Some(vm) = self.vm.as_mut() else {
            return String::new();
        };
        if !unsafe { qjs::vm::pump_runtime_once(vm.rt, vm.ctx, "qjs-workbench") } {
            push_output(
                vm.output.as_mut(),
                "runtime fault; reset the workbench VM".to_string(),
            );
        }
        take_output(vm.output.as_mut())
    }

    /// Drop the VM and all VM-owned state. The next evaluation gets a fresh VM.
    pub fn close(&mut self) {
        self.vm.take();
    }

    fn ensure_vm(&mut self) -> Result<&mut Vm, String> {
        if self.vm.is_none() {
            self.vm = Some(create_vm()?);
        }
        Ok(self.vm.as_mut().expect("workbench VM was just initialized"))
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}

fn create_vm() -> Result<Vm, String> {
    let rt = unsafe { qjs::JS_NewRuntime() };
    if rt.is_null() {
        return Err("failed to create QuickJS runtime".to_string());
    }
    unsafe {
        qjs::qjs_diag::install_runtime(rt);
        qjs::node::install(rt);
    }
    let ctx = unsafe { qjs::JS_NewContext(rt) };
    if ctx.is_null() {
        unsafe { qjs::JS_FreeRuntime(rt) };
        return Err("failed to create QuickJS context".to_string());
    }
    unsafe {
        qjs::qjs_diag::install_context(ctx);
        qjs::node::install_globals_with_profile(ctx, qjs::node::RuntimeProfile::Shell);
    }
    let mut output = Box::new(ContextOutput {
        lines: VecDeque::new(),
        bytes: 0,
    });
    unsafe {
        qjs::JS_SetContextOpaque(ctx, output.as_mut() as *mut ContextOutput as *mut c_void);
        install_workbench_globals(ctx);
    }
    Ok(Vm {
        rt,
        ctx,
        output,
        eval_count: 0,
    })
}

unsafe fn read_js_string(ctx: *mut qjs::JSContext, value: qjs::JSValueConst) -> Option<String> {
    let mut len = 0usize;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len, value, 0);
    if cstr.is_null() {
        return None;
    }
    let text = core::str::from_utf8(core::slice::from_raw_parts(cstr as *const u8, len))
        .ok()
        .map(ToString::to_string);
    qjs::JS_FreeCString(ctx, cstr);
    text
}

unsafe fn value_to_display_string(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<String> {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return read_js_string(ctx, value);
    }
    let json = qjs::JS_GetPropertyStr(ctx, global, b"JSON\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if json.is_exception() {
        return read_js_string(ctx, value);
    }
    let stringify = qjs::JS_GetPropertyStr(ctx, json, b"stringify\0".as_ptr() as *const c_char);
    if stringify.is_exception() {
        qjs::js_free_value(ctx, json);
        return read_js_string(ctx, value);
    }
    let argument = qjs::js_dup_value(ctx, value);
    let rendered = qjs::JS_Call(ctx, stringify, json, 1, &argument);
    qjs::js_free_value(ctx, argument);
    qjs::js_free_value(ctx, stringify);
    qjs::js_free_value(ctx, json);
    if rendered.is_exception() {
        let exception = qjs::JS_GetException(ctx);
        qjs::js_free_value(ctx, exception);
        return read_js_string(ctx, value);
    }
    if rendered.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, rendered);
        return read_js_string(ctx, value);
    }
    let text = read_js_string(ctx, rendered);
    qjs::js_free_value(ctx, rendered);
    text
}

unsafe fn exception_to_string(ctx: *mut qjs::JSContext) -> String {
    let exception = qjs::JS_GetException(ctx);
    let stack = qjs::JS_GetPropertyStr(ctx, exception, b"stack\0".as_ptr() as *const c_char);
    let message = if !stack.is_exception() && stack.tag != qjs::JS_TAG_UNDEFINED {
        read_js_string(ctx, stack)
    } else {
        None
    }
    .or_else(|| read_js_string(ctx, exception))
    .unwrap_or_else(|| "<exception>".to_string());
    qjs::js_free_value(ctx, stack);
    qjs::js_free_value(ctx, exception);
    message
}

fn push_output(output: &mut ContextOutput, line: String) {
    let line_bytes = line.len();
    while output.lines.len() >= OUTPUT_LINE_CAP
        || output.bytes.saturating_add(line_bytes) > OUTPUT_BYTES_CAP
    {
        let Some(discarded) = output.lines.pop_front() else {
            break;
        };
        output.bytes = output.bytes.saturating_sub(discarded.len());
    }
    if line_bytes <= OUTPUT_BYTES_CAP {
        output.bytes = output.bytes.saturating_add(line_bytes);
        output.lines.push_back(line);
    }
}

fn take_output(output: &mut ContextOutput) -> String {
    let mut text = String::new();
    while let Some(line) = output.lines.pop_front() {
        output.bytes = output.bytes.saturating_sub(line.len());
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&line);
    }
    text
}

unsafe extern "C" fn workbench_print(
    ctx: *mut qjs::JSContext,
    _this: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let output = qjs::JS_GetContextOpaque(ctx) as *mut ContextOutput;
    if output.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let mut line = String::new();
    if argc > 0 && !argv.is_null() {
        for value in core::slice::from_raw_parts(argv, argc as usize) {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(
                &value_to_display_string(ctx, *value)
                    .or_else(|| read_js_string(ctx, *value))
                    .unwrap_or_else(|| "<value>".to_string()),
            );
        }
    }
    let len = line.len();
    push_output(&mut *output, line);
    qjs::JS_NewFloat64(ctx, len as f64)
}

unsafe fn install_workbench_globals(ctx: *mut qjs::JSContext) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let print = qjs::JS_NewCFunction2(
        ctx,
        Some(workbench_print),
        b"print\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, b"print\0".as_ptr() as *const c_char, print);
    let console = qjs::JS_GetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char);
    if !console.is_exception() {
        for name in [
            b"log\0".as_slice(),
            b"info\0".as_slice(),
            b"warn\0".as_slice(),
            b"error\0".as_slice(),
        ] {
            let logger = qjs::JS_NewCFunction2(
                ctx,
                Some(workbench_print),
                name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, console, name.as_ptr() as *const c_char, logger);
        }
    }
    qjs::js_free_value(ctx, console);
    qjs::js_free_value(ctx, global);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
}

/// Detect static module syntax without being fooled by strings or comments.
pub fn source_uses_module_syntax(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut mode = ScanMode::Normal;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        match mode {
            ScanMode::Normal => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    mode = ScanMode::LineComment;
                    index += 2;
                    continue;
                }
                if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    mode = ScanMode::BlockComment;
                    index += 2;
                    continue;
                }
                match byte {
                    b'\'' => mode = ScanMode::SingleQuote,
                    b'"' => mode = ScanMode::DoubleQuote,
                    b'`' => mode = ScanMode::Backtick,
                    b'_' | b'$' | b'a'..=b'z' | b'A'..=b'Z' => {
                        let start = index;
                        index += 1;
                        while bytes
                            .get(index)
                            .is_some_and(|b| *b == b'_' || *b == b'$' || b.is_ascii_alphanumeric())
                        {
                            index += 1;
                        }
                        let token = &source[start..index];
                        if token == "export" {
                            return true;
                        }
                        if token == "import" {
                            let mut next = index;
                            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                                next += 1;
                            }
                            if bytes.get(next) != Some(&b'(') {
                                return true;
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            ScanMode::SingleQuote | ScanMode::DoubleQuote | ScanMode::Backtick => {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if matches!(
                    (mode, byte),
                    (ScanMode::SingleQuote, b'\'')
                        | (ScanMode::DoubleQuote, b'"')
                        | (ScanMode::Backtick, b'`')
                ) {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::LineComment if byte == b'\n' => mode = ScanMode::Normal,
            ScanMode::BlockComment if byte == b'*' && bytes.get(index + 1) == Some(&b'/') => {
                mode = ScanMode::Normal;
                index += 2;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::source_uses_module_syntax;
    #[test]
    fn module_scan_ignores_dynamic_imports_comments_and_strings() {
        assert!(source_uses_module_syntax("import { readFile } from 'fs';"));
        assert!(source_uses_module_syntax("export const answer = 42;"));
        assert!(!source_uses_module_syntax("import('fs').then(print)"));
        assert!(!source_uses_module_syntax("// import x from 'y'\n1 + 1"));
    }
}
