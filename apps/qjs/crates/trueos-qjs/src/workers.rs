#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_int};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use spin::Mutex;

use crate as qjs;

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    qjs::platform::sys::write_stderr(bytes);
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

static WORKER_SEQ: AtomicU32 = AtomicU32::new(1);

static LOGGED_WORKER_API_USE: AtomicBool = AtomicBool::new(false);
static CHILD_WORKER_CONTEXT: AtomicBool = AtomicBool::new(false);
static CHILD_WARNED_NO_HANDLER: AtomicBool = AtomicBool::new(false);

const WORKER_TASK_POOL: usize = 32;

static WORKERS: Mutex<BTreeMap<u32, WorkerState>> = Mutex::new(BTreeMap::new());

// (ctx, worker_id) -> message callbacks
struct JsCallback {
    val: qjs::JSValue,
}

// QuickJS values are not thread-safe, but we only ever access callbacks from the owning VM/task.
// This matches the same "single owning task" assumption used elsewhere in TRUEOS QJS glue.
unsafe impl Send for JsCallback {}

struct ContextWorkerState {
    worker_id: Option<u32>,
    main_on_message: BTreeMap<u32, JsCallback>,
    worker_on_message: BTreeMap<u32, JsCallback>,
    owned_workers: Vec<u32>,
}

impl ContextWorkerState {
    fn new() -> Self {
        Self {
            worker_id: None,
            main_on_message: BTreeMap::new(),
            worker_on_message: BTreeMap::new(),
            owned_workers: Vec::new(),
        }
    }
}

static CONTEXT_WORKERS: Mutex<BTreeMap<usize, ContextWorkerState>> = Mutex::new(BTreeMap::new());

struct WorkerState {
    /// Opaque VMX child-Hull endpoint in the parent Blueprint.  The child
    /// itself addresses its parent as handle zero and therefore stores none.
    child_handle: u64,
}

impl WorkerState {
    fn new(child_handle: u64) -> Self {
        Self { child_handle }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CtxRole {
    Main,
    Worker(u32),
}

pub fn ctx_role(ctx: *mut qjs::JSContext) -> CtxRole {
    if ctx.is_null() {
        return CtxRole::Main;
    }
    let key = ctx as usize;
    if let Some(id) = CONTEXT_WORKERS
        .lock()
        .get(&key)
        .and_then(|state| state.worker_id)
    {
        return CtxRole::Worker(id);
    }
    CtxRole::Main
}

/// Mark the hidden child Blueprint's one VM context as a worker context before
/// module exports are evaluated. This is process-local; QuickJS values never
/// cross the VMX child boundary.
pub fn enter_child_context(ctx: *mut qjs::JSContext, worker_id: u32) {
    if ctx.is_null() {
        return;
    }
    CHILD_WORKER_CONTEXT.store(true, Ordering::Release);
    CONTEXT_WORKERS
        .lock()
        .entry(ctx as usize)
        .or_insert_with(ContextWorkerState::new)
        .worker_id = Some(worker_id);
}

pub fn leave_child_context(ctx: *mut qjs::JSContext) {
    if !ctx.is_null() {
        CONTEXT_WORKERS.lock().remove(&(ctx as usize));
    }
    CHILD_WORKER_CONTEXT.store(false, Ordering::Release);
}

fn worker_state_mut<F, R>(worker_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut WorkerState) -> R,
{
    let mut map = WORKERS.lock();
    let st = map.get_mut(&worker_id)?;
    Some(f(st))
}

pub fn spawn_eval(code_utf8: &[u8]) -> Result<u32, i32> {
    spawn_eval_on_slot_inner(code_utf8)
}

pub fn spawn_eval_on_slot(cpu_slot: u32, code_utf8: &[u8]) -> Result<u32, i32> {
    // VMX lane policy belongs to the kernel. Keep the source-compatible API,
    // but treat a former executor slot as a scheduling hint only.
    let _ = cpu_slot;
    spawn_eval_on_slot_inner(code_utf8)
}

fn spawn_eval_on_slot_inner(code_utf8: &[u8]) -> Result<u32, i32> {
    if code_utf8.is_empty() {
        return Err(-2);
    }

    let worker_id = WORKER_SEQ.fetch_add(1, Ordering::Relaxed);
    {
        let workers = WORKERS.lock();
        let active = workers
            .values()
            .filter(|state| {
                v::child_hull::status(state.child_handle) < v::child_hull::STATUS_EXITED
            })
            .count();
        if active >= WORKER_TASK_POOL {
            return Err(v::child_hull::ERR_QUEUE_FULL);
        }
    }

    if !LOGGED_WORKER_API_USE.swap(true, Ordering::AcqRel) {
        log_str("qjs-worker: Worker API path used; worker spawn requested\n");
    }

    let child_handle = v::child_hull::spawn(code_utf8)?;
    WORKERS
        .lock()
        .insert(worker_id, WorkerState::new(child_handle));
    log_str(&format!(
        "qjs-worker: worker#{} child Hull spawned\n",
        worker_id
    ));
    Ok(worker_id)
}

pub fn terminate(worker_id: u32) {
    let _ = worker_state_mut(worker_id, |st| {
        let _ = v::child_hull::terminate(st.child_handle);
    });
}

pub fn terminate_all_for_context(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let key = ctx as usize;
    let ids = CONTEXT_WORKERS
        .lock()
        .get_mut(&key)
        .map(|state| core::mem::take(&mut state.owned_workers))
        .unwrap_or_default();
    for worker_id in ids {
        terminate(worker_id);
    }
}

pub fn post_to_worker(worker_id: u32, msg: &[u8]) -> Result<(), i32> {
    if CHILD_WORKER_CONTEXT.load(Ordering::Acquire) {
        return Err(-2);
    }
    worker_state_mut(worker_id, |st| {
        v::child_hull::send(st.child_handle, msg)?;
        Ok(())
    })
    .ok_or(-2)?
}

pub fn post_to_parent(worker_id: u32, msg: &[u8]) -> Result<(), i32> {
    if CHILD_WORKER_CONTEXT.load(Ordering::Acquire) {
        return v::child_hull::send(v::child_hull::HANDLE_PARENT, msg).map(|_| ());
    }
    let _ = worker_id;
    let _ = msg;
    Err(-2)
}

pub fn take_parent_message(worker_id: u32) -> Option<Vec<u8>> {
    if CHILD_WORKER_CONTEXT.load(Ordering::Acquire) {
        return None;
    }
    let handle = worker_state_mut(worker_id, |st| st.child_handle)?;
    crate::child_worker::receive_one(handle).ok().flatten()
}

pub fn worker_exited(worker_id: u32) -> bool {
    let map = WORKERS.lock();
    map.get(&worker_id)
        .is_some_and(|st| v::child_hull::status(st.child_handle) >= v::child_hull::STATUS_EXITED)
}

pub fn has_pending_for_ctx(ctx: *mut qjs::JSContext) -> bool {
    match ctx_role(ctx) {
        CtxRole::Main => false,
        CtxRole::Worker(_) => CHILD_WORKER_CONTEXT.load(Ordering::Acquire),
    }
}

unsafe fn js_string(ctx: *mut qjs::JSContext, bytes: &[u8]) -> qjs::JSValue {
    qjs::jsbind::new_string(ctx, bytes)
}

unsafe fn call_on_message(ctx: *mut qjs::JSContext, cb: qjs::JSValue, msg_bytes: &[u8]) {
    let arg = unsafe { js_string(ctx, msg_bytes) };
    let _ = unsafe { qjs::jsbind::call1(ctx, cb, qjs::JSValue::undefined(), arg) };
    unsafe { qjs::js_free_value(ctx, arg) };
}

pub unsafe fn pump(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    let mut progress = false;

    match ctx_role(ctx) {
        CtxRole::Main => {
            // Drain all outbound worker->parent messages and deliver to callbacks registered
            // in the *current* main context.
            let key_ctx = ctx as usize;

            // Snapshot which workers have pending messages first to avoid holding the WORKERS lock
            // while calling into JS.
            let pending_ids: Vec<u32> = WORKERS.lock().keys().copied().collect();

            for worker_id in pending_ids {
                loop {
                    let msg = take_parent_message(worker_id);
                    let Some(msg) = msg else { break };
                    progress = true;

                    let cb = CONTEXT_WORKERS
                        .lock()
                        .get(&key_ctx)
                        .and_then(|state| state.main_on_message.get(&worker_id))
                        .map(|c| c.val);
                    if let Some(cb) = cb {
                        unsafe { call_on_message(ctx, cb, msg.as_slice()) };
                    }
                }

                // The kernel intentionally keeps final child-to-parent frames
                // after exit. Reap only after the queue above has been drained,
                // otherwise a fast worker could lose its final message.
                let exited = worker_state_mut(worker_id, |state| {
                    v::child_hull::status(state.child_handle) >= v::child_hull::STATUS_EXITED
                })
                .unwrap_or(false);
                if exited {
                    WORKERS.lock().remove(&worker_id);
                }
            }
        }
        CtxRole::Worker(worker_id) => {
            let key_ctx = ctx as usize;
            loop {
                let msg = if CHILD_WORKER_CONTEXT.load(Ordering::Acquire) {
                    crate::child_worker::receive_one(v::child_hull::HANDLE_PARENT)
                        .ok()
                        .flatten()
                } else {
                    None
                };
                let Some(msg) = msg else { break };
                progress = true;

                let cb = CONTEXT_WORKERS
                    .lock()
                    .get(&key_ctx)
                    .and_then(|state| state.worker_on_message.get(&worker_id))
                    .map(|c| c.val);
                if let Some(cb) = cb {
                    unsafe { call_on_message(ctx, cb, msg.as_slice()) };
                } else {
                    let should_warn = !CHILD_WARNED_NO_HANDLER.swap(true, Ordering::AcqRel);
                    if should_warn {
                        let _ = post_to_parent(
                            worker_id,
                            b"{\"ok\":0,\"dbg\":\"worker-no-onmessage-handler\"}",
                        );
                    }
                }
            }
        }
    }

    progress
}

pub unsafe fn set_on_message(ctx: *mut qjs::JSContext, worker_id: u32, cb: qjs::JSValue) {
    if ctx.is_null() {
        return;
    }
    let key_ctx = ctx as usize;
    let mut states = CONTEXT_WORKERS.lock();
    let state = states
        .entry(key_ctx)
        .or_insert_with(ContextWorkerState::new);
    let map = match ctx_role(ctx) {
        CtxRole::Main => &mut state.main_on_message,
        CtxRole::Worker(_) => &mut state.worker_on_message,
    };

    if let Some(prev) = map.remove(&worker_id) {
        unsafe { qjs::js_free_value(ctx, prev.val) };
    }
    map.insert(
        worker_id,
        JsCallback {
            val: unsafe { qjs::js_dup_value(ctx, cb) },
        },
    );
}

pub unsafe fn drain_all_for_context(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let key_ctx = ctx as usize;
    if let Some(state) = CONTEXT_WORKERS.lock().remove(&key_ctx) {
        for (_, v) in state.main_on_message {
            qjs::js_free_value(ctx, v.val);
        }
        for (_, v) in state.worker_on_message {
            qjs::js_free_value(ctx, v.val);
        }
    }
}

// --- JS bindings for node:worker_threads ---

const WORKER_ID_PROP: &[u8] = b"__wid\0";

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
fn js_null() -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: 0 },
        tag: qjs::JS_TAG_NULL,
    }
}

unsafe fn arg_to_bytes(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<Vec<u8>> {
    unsafe { qjs::jsbind::to_bytes(ctx, v) }
}

unsafe fn worker_id_from_this(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
) -> Option<u32> {
    let v =
        unsafe { qjs::JS_GetPropertyStr(ctx, this_val, WORKER_ID_PROP.as_ptr() as *const c_char) };
    if v.is_exception() {
        return None;
    }
    let id = if v.tag == qjs::JS_TAG_INT {
        (unsafe { v.u.int32 }) as i64
    } else {
        -1
    };
    unsafe { qjs::js_free_value(ctx, v) };
    if id <= 0 { None } else { Some(id as u32) }
}

#[inline]
unsafe fn event_name_is_message(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> bool {
    let Some(name) = (unsafe { arg_to_bytes(ctx, v) }) else {
        return false;
    };
    name.as_slice() == b"message"
}

/// Worker constructor: `new Worker(<code_string>)` (MVP: eval string only).
pub unsafe extern "C" fn js_worker_ctor(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::exception();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JS_NewObject(ctx);
    }

    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(code) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };

    let worker_id = match spawn_eval(code.as_slice()) {
        Ok(id) => id,
        Err(_) => return qjs::JSValue::exception(),
    };
    CONTEXT_WORKERS
        .lock()
        .entry(ctx as usize)
        .or_insert_with(ContextWorkerState::new)
        .owned_workers
        .push(worker_id);

    // Constructor mode (`new Worker(...)`) passes a pre-created `this` with Worker.prototype.
    // Keep that object so class inheritance/prototype checks work for browser-style code.
    let obj = if this_val.tag == qjs::JS_TAG_OBJECT {
        qjs::js_dup_value(ctx, this_val)
    } else {
        qjs::JS_NewObject(ctx)
    };
    if obj.is_exception() {
        return obj;
    }

    // __wid (internal)
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        WORKER_ID_PROP.as_ptr() as *const c_char,
        js_int32(worker_id as i32),
    );

    // threadId
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"threadId\0".as_ptr() as *const c_char,
        js_int32(worker_id as i32),
    );

    // postMessage(message)
    let pm = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_post_message),
        b"postMessage\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"postMessage\0".as_ptr() as *const c_char, pm);

    // terminate()
    let term = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_terminate),
        b"terminate\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"terminate\0".as_ptr() as *const c_char, term);

    // onmessage(cb)
    let onm = qjs::JS_NewCFunction2(
        ctx,
        Some(js_worker_on_message),
        b"onMessage\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"onMessage\0".as_ptr() as *const c_char, onm);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"onmessage\0".as_ptr() as *const c_char,
        qjs::JSValue::undefined(),
    );

    obj
}

pub unsafe extern "C" fn js_worker_add_event_listener(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    if unsafe { event_name_is_message(ctx, args[0]) } {
        unsafe { set_on_message(ctx, worker_id, args[1]) };
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_remove_event_listener(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // No-op MVP. We keep this for browser API shape compatibility.
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_post_message(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(msg) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };
    let _ = post_to_worker(worker_id, msg.as_slice());
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_terminate(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if let Some(worker_id) = unsafe { worker_id_from_this(ctx, this_val) } {
        terminate(worker_id);
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_worker_on_message(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    let Some(worker_id) = (unsafe { worker_id_from_this(ctx, this_val) }) else {
        return qjs::JSValue::undefined();
    };
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    unsafe { set_on_message(ctx, worker_id, args[0]) };
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_parent_port_post_message(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let Some(msg) = (unsafe { arg_to_bytes(ctx, args[0]) }) else {
        return qjs::JSValue::exception();
    };

    match ctx_role(ctx) {
        CtxRole::Worker(id) => {
            let _ = post_to_parent(id, msg.as_slice());
        }
        CtxRole::Main => {
            // No parentPort on main.
        }
    }
    qjs::JSValue::undefined()
}

pub unsafe extern "C" fn js_parent_port_on_message(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() {
        return qjs::JSValue::undefined();
    }
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let cb = args[0];
    match ctx_role(ctx) {
        CtxRole::Worker(id) => unsafe { set_on_message(ctx, id, cb) },
        CtxRole::Main => {
            // No-op.
        }
    }
    qjs::JSValue::undefined()
}

pub unsafe fn install_worker_threads_exports(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
    if ctx.is_null() || m.is_null() {
        return -1;
    }

    match ctx_role(ctx) {
        CtxRole::Main => {
            let worker_fn = qjs::JS_NewCFunction2(
                ctx,
                Some(js_worker_ctor),
                b"Worker\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_CONSTRUCTOR,
                0,
            );
            let _ =
                qjs::JS_SetModuleExport(ctx, m, b"Worker\0".as_ptr() as *const c_char, worker_fn);
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"isMainThread\0".as_ptr() as *const c_char,
                js_bool(true),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"parentPort\0".as_ptr() as *const c_char,
                js_null(),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"threadId\0".as_ptr() as *const c_char,
                js_int32(0),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"workerData\0".as_ptr() as *const c_char,
                qjs::JSValue::undefined(),
            );
        }
        CtxRole::Worker(id) => {
            let port = qjs::JS_NewObject(ctx);
            let pm = qjs::JS_NewCFunction2(
                ctx,
                Some(js_parent_port_post_message),
                b"postMessage\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let onm = qjs::JS_NewCFunction2(
                ctx,
                Some(js_parent_port_on_message),
                b"onMessage\0".as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ =
                qjs::JS_SetPropertyStr(ctx, port, b"postMessage\0".as_ptr() as *const c_char, pm);
            let _ =
                qjs::JS_SetPropertyStr(ctx, port, b"onMessage\0".as_ptr() as *const c_char, onm);

            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"isMainThread\0".as_ptr() as *const c_char,
                js_bool(false),
            );
            let _ =
                qjs::JS_SetModuleExport(ctx, m, b"parentPort\0".as_ptr() as *const c_char, port);
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"threadId\0".as_ptr() as *const c_char,
                js_int32(id as i32),
            );
            let _ = qjs::JS_SetModuleExport(
                ctx,
                m,
                b"workerData\0".as_ptr() as *const c_char,
                qjs::JSValue::undefined(),
            );
        }
    }

    0
}

#[inline]
pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"worker_threads" && name != b"node:worker_threads" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn worker_threads_module_init(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
    ) -> c_int {
        unsafe { install_worker_threads_exports(ctx, m) }
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(worker_threads_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char) };
        }};
    }

    add_export!("Worker");
    add_export!("isMainThread");
    add_export!("parentPort");
    add_export!("threadId");
    add_export!("workerData");

    m
}
