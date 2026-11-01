use std::{ffi::{CStr, CString, c_int, c_void}, io::{self, Read}, time::{Duration, Instant}};
use serde_json::{Value, json};
use trueos_qjs as q;

unsafe extern "C" {
    fn JS_SetMemoryLimit(rt: *mut q::JSRuntime, limit: usize);
    fn JS_SetMaxStackSize(rt: *mut q::JSRuntime, limit: usize);
    fn JS_SetInterruptHandler(rt: *mut q::JSRuntime,
        handler: Option<unsafe extern "C" fn(*mut q::JSRuntime, *mut c_void) -> c_int>, opaque: *mut c_void);
}
unsafe extern "C" fn interrupt(_: *mut q::JSRuntime, opaque: *mut c_void) -> c_int {
    (Instant::now() >= unsafe { *(opaque as *const Instant) }) as c_int
}
unsafe fn string(ctx: *mut q::JSContext, value: q::JSValue) -> String {
    let ptr = unsafe { q::js_to_cstring(ctx, value) };
    if ptr.is_null() { return "<unprintable exception>".into(); }
    let result = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    unsafe { q::JS_FreeCString(ctx, ptr) }; result
}
unsafe fn exception(ctx: *mut q::JSContext) -> String {
    let value = unsafe { q::JS_GetException(ctx) };
    let mut result = unsafe { string(ctx, value) };
    let stack = unsafe { q::JS_GetPropertyStr(ctx, value, c"stack".as_ptr()) };
    if stack.is_exception() {
        let nested = unsafe { q::JS_GetException(ctx) };
        unsafe { q::js_free_value(ctx, nested) };
    } else if stack.tag != q::JS_TAG_UNDEFINED {
        result.push('\n'); result.push_str(&unsafe { string(ctx, stack) });
    }
    unsafe { q::js_free_value(ctx, stack); q::js_free_value(ctx, value); } result
}
unsafe fn eval(ctx: *mut q::JSContext, source: &str, name: &str, module: bool) -> Result<String, String> {
    let source = CString::new(source).map_err(|e| e.to_string())?;
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let value = unsafe { q::JS_Eval(ctx, source.as_ptr(), source.as_bytes().len(), name.as_ptr(), if module { 1 } else { 0 }) };
    if value.is_exception() { return Err(unsafe { exception(ctx) }); }
    let result = unsafe { string(ctx, value) };
    unsafe { q::js_free_value(ctx, value) }; Ok(result)
}
unsafe extern "C" fn rejection(ctx: *mut q::JSContext, promise: q::JSValue, reason: q::JSValue, handled: c_int, opaque: *mut c_void) {
    let pending = unsafe { &mut *(opaque as *mut std::collections::BTreeMap<usize, String>) };
    let id = unsafe { promise.u.ptr } as usize;
    if handled != 0 { pending.remove(&id); } else { pending.insert(id, unsafe { string(ctx, reason) }); }
}
fn run(input: Value) -> Result<Value, String> {
    let mut errors = Vec::new();
    let mut rejections = std::collections::BTreeMap::<usize, String>::new();
    let mut deadline = Instant::now() + Duration::from_secs(8);
    unsafe {
        let rt = q::JS_NewRuntime(); if rt.is_null() { return Err("runtime allocation failed".into()); }
        JS_SetMemoryLimit(rt, 256 * 1024 * 1024); JS_SetMaxStackSize(rt, 1024 * 1024);
        JS_SetInterruptHandler(rt, Some(interrupt), (&mut deadline as *mut Instant).cast());
        q::JS_SetHostPromiseRejectionTracker(rt, Some(rejection), (&mut rejections as *mut std::collections::BTreeMap<usize, String>).cast());
        let ctx = q::JS_NewContext(rt);
        if ctx.is_null() { q::JS_FreeRuntime(rt); return Err("context allocation failed".into()); }
        let boot = format!("globalThis.__page = {};\n{}", input["page"], format!("{}\n{}", include_str!("web_host.js"), include_str!("virtual_dom.js")));
        if let Err(error) = eval(ctx, &boot, "web_host.js", false) { errors.push(json!({"phase":"bootstrap","error":error})); }
        let scripts = input["scripts"].as_array().ok_or("scripts must be an array")?;
        for script in scripts {
            if Instant::now() >= deadline { errors.push(json!({"phase":"budget","error":"execution deadline exceeded"})); break; }
            let name = script["url"].as_str().unwrap_or("inline");
            if let Err(error) = eval(ctx, script["source"].as_str().unwrap_or(""), name, script["module"].as_bool().unwrap_or(false)) {
                errors.push(json!({"phase":"script","url":name,"error":error}));
            }
            for _ in 0..1000 {
                let mut job_ctx = ctx;
                let rc = q::JS_ExecutePendingJob(rt, &mut job_ctx);
                if rc < 0 { errors.push(json!({"phase":"promise","error":exception(job_ctx)})); break; }
                if rc == 0 { break; }
            }
        }
        if Instant::now() < deadline {
            if let Err(error) = eval(ctx, "__domReady(); __pumpTimers(1000)", "timers", false) { errors.push(json!({"phase":"timers","error":error})); }
            for _ in 0..1000 {
                let mut job_ctx = ctx; let rc = q::JS_ExecutePendingJob(rt, &mut job_ctx);
                if rc < 0 { errors.push(json!({"phase":"promise","error":exception(job_ctx)})); break; }
                if rc == 0 { break; }
            }
        }
        // Allow reporting even if the site exhausted its execution budget.
        deadline = Instant::now() + Duration::from_millis(500);
        std::hint::black_box(&deadline);
        let host = eval(ctx, "JSON.stringify(__hostReport())", "report", false)
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
        let pending_jobs = q::JS_IsJobPending(rt) != 0;
        q::JS_FreeContext(ctx); q::JS_FreeRuntime(rt);
        Ok(json!({"errors":errors,"unhandled_rejections":rejections.values().collect::<Vec<_>>(),"host":host?,"pending_jobs":pending_jobs,"scripts_attempted":scripts.len()}))
    }
}
fn main() {
    let result = (|| { let mut input = String::new(); io::stdin().take(40 * 1024 * 1024).read_to_string(&mut input).map_err(|e| e.to_string())?; run(serde_json::from_str(&input).map_err(|e| e.to_string())?) })();
    match result { Ok(report) => println!("{report}"), Err(error) => { eprintln!("{error}"); std::process::exit(1); } }
}
