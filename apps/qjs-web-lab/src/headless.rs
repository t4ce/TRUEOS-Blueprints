use std::{ffi::{CStr, CString, c_int, c_void}, io::{self, Read}, time::{Duration, Instant}};
use serde_json::{Value, json};
use trueos_qjs as q;
mod offline_modules;

unsafe extern "C" {
    fn JS_SetMemoryLimit(rt: *mut q::JSRuntime, limit: usize);
    fn JS_SetMaxStackSize(rt: *mut q::JSRuntime, limit: usize);
    fn JS_SetInterruptHandler(rt: *mut q::JSRuntime,
        handler: Option<unsafe extern "C" fn(*mut q::JSRuntime, *mut c_void) -> c_int>, opaque: *mut c_void);
}
unsafe extern "C" fn host_now(ctx: *mut q::JSContext, _: q::JSValue, _: c_int, _: *const q::JSValue) -> q::JSValue {
    let start = unsafe { &*(q::JS_GetContextOpaque(ctx) as *const Instant) };
    unsafe { q::JS_NewFloat64(ctx, start.elapsed().as_secs_f64() * 1000.0) }
}
unsafe extern "C" fn host_random(ctx: *mut q::JSContext, _: q::JSValue, argc: c_int, argv: *const q::JSValue) -> q::JSValue {
    let count = if argc == 1 { unsafe { string(ctx, *argv) }.parse::<usize>().ok() } else { None };
    let Some(count) = count.filter(|n| *n <= 65536) else {
        return unsafe { q::JS_Throw(ctx, q::JS_NewStringLen(ctx, c"invalid random byte count".as_ptr(), 25)) };
    };
    let mut bytes = vec![0; count];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).is_err() {
        return unsafe { q::JS_Throw(ctx, q::JS_NewStringLen(ctx, c"host entropy unavailable".as_ptr(), 24)) };
    }
    unsafe { q::JS_NewArrayBufferCopy(ctx, bytes.as_ptr(), bytes.len()) }
}
unsafe fn trace_snapshot(ctx: *mut q::JSContext) -> Value {
    unsafe { eval(ctx, "JSON.stringify(__traceSnapshot())", "trace", false) }
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
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
    unsafe { eval_mode(ctx, source, name, module, true) }
}
unsafe fn eval_discard(ctx: *mut q::JSContext, source: &str, name: &str, module: bool) -> Result<(), String> {
    unsafe { eval_mode(ctx, source, name, module, false) }.map(|_| ())
}
unsafe fn eval_mode(ctx: *mut q::JSContext, source: &str, name: &str, module: bool, stringify_result: bool) -> Result<String, String> {
    let source = CString::new(source).map_err(|e| e.to_string())?;
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let value = unsafe { if module { offline_modules::evaluate_entry(ctx, source.as_ptr(), source.as_bytes().len(), name.as_ptr()) } else { q::JS_Eval(ctx, source.as_ptr(), source.as_bytes().len(), name.as_ptr(), 0) } };
    if value.is_exception() { return Err(unsafe { exception(ctx) }); }
    let result = if stringify_result { unsafe { string(ctx, value) } } else { String::new() };
    unsafe { q::js_free_value(ctx, value) }; Ok(result)
}
unsafe extern "C" fn rejection(ctx: *mut q::JSContext, promise: q::JSValue, reason: q::JSValue, handled: c_int, opaque: *mut c_void) {
    let pending = unsafe { &mut *(opaque as *mut std::collections::BTreeMap<usize, String>) };
    let id = unsafe { promise.u.ptr } as usize;
    if handled != 0 { pending.remove(&id); } else { pending.insert(id, unsafe { string(ctx, reason) }); }
}
unsafe fn drain_jobs(rt: *mut q::JSRuntime, ctx: *mut q::JSContext, deadline: Instant, errors: &mut Vec<Value>) {
    for _ in 0..1000 {
        if Instant::now() >= deadline { break; }
        let mut job_ctx = ctx;
        let rc = unsafe { q::JS_ExecutePendingJob(rt, &mut job_ctx) };
        if rc < 0 {
            errors.push(json!({"phase":"promise","error":unsafe {exception(job_ctx)},"api_trace":unsafe {trace_snapshot(ctx)}}));
            break;
        }
        if rc == 0 { break; }
    }
}
unsafe fn drain_timers(rt: *mut q::JSRuntime, ctx: *mut q::JSContext, deadline: Instant, errors: &mut Vec<Value>) {
    for _ in 0..1000 {
        if Instant::now() >= deadline { break; }
        let pending = unsafe {eval(ctx, "__hostReport().pendingTimers", "timer-count", false)}.unwrap_or_default();
        if pending == "0" { break; }
        if let Err(error) = unsafe {eval_discard(ctx, "__tracePhase('timer'); __pumpTimers(1)", "timers", false)} {
            errors.push(json!({"phase":"timers","error":error,"api_trace":unsafe {trace_snapshot(ctx)}}));
            break;
        }
        unsafe {drain_jobs(rt, ctx, deadline, errors)};
    }
}
const CHECKPOINT_SOURCE: &str = r#"JSON.stringify((() => {
    // Inspect stored node state rather than traced DOM getters. Sampling must
    // not manufacture an apparent API-call increase or create lazy nodes.
    let connected=0, characters=0;
    const stack=[document];
    while(stack.length) {
        const node=stack.pop();connected++;
        if(node.nodeType===3||node.nodeType===4)characters+=Array.from(node.nodeValue||'').length;
        for(const child of node.childNodes||[])stack.push(child);
        if(node.shadowRoot)stack.push(node.shadowRoot);
    }
    const host=__hostReport(), api=__traceSnapshot(), draws=host.text?.draws||[];
    return {
        api_calls:api.total,api_counts:api.counts,
        allocated_nodes:host.dom?.allocatedNodes||0,connected_nodes:connected,
        text_characters:characters,text_draw_calls:draws.length,
        text_draw_characters:draws.reduce((sum,draw)=>sum+Array.from(draw.text||'').length,0),
        host_errors:(host.errors||[]).length,pending_timers:host.pendingTimers||0,
        console_entries:(host.logs||[]).length
    };
})())"#;
unsafe fn checkpoint(ctx: *mut q::JSContext, rt: *mut q::JSRuntime, deadline: &mut Instant, name: &str, kind: &str, spec: Option<&Value>, errors: &[Value], rejection_count: usize, previous: Option<&Value>) -> Value {
    // Give diagnostic sampling its own bounded allowance. This never grants
    // another site-event turn after the page execution deadline has expired.
    let saved_deadline = *deadline;
    *deadline = Instant::now() + Duration::from_millis(200);
    let sample = unsafe {eval(ctx, CHECKPOINT_SOURCE, "checkpoint", false)}
        .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
    *deadline = saved_deadline;
    let mut sample = match sample {
        Ok(value) => value,
        Err(error) => json!({"sample_error":error}),
    };
    sample["name"]=json!(name);sample["kind"]=json!(kind);
    sample["errors"]=json!(errors.len());
    sample["unhandled_rejections"]=json!(rejection_count);
    sample["pending_jobs"]=json!(unsafe {q::JS_IsJobPending(rt)} != 0);
    sample["deadline_exceeded"]=json!(Instant::now() >= saved_deadline);
    if let Some(spec)=spec {sample["event"]=spec.clone();}
    if let Some(previous)=previous {
        let mut delta=serde_json::Map::new();
        for key in ["api_calls","allocated_nodes","connected_nodes","text_characters","text_draw_calls","text_draw_characters","errors","host_errors","unhandled_rejections","console_entries"] {
            if let (Some(now),Some(before))=(sample[key].as_i64(),previous[key].as_i64()) {delta.insert(key.to_owned(),json!(now-before));}
        }
        let progress=["allocated_nodes","connected_nodes","text_characters","text_draw_calls","text_draw_characters"]
            .iter().any(|key|delta.get(*key).and_then(Value::as_i64).unwrap_or(0)!=0);
        sample["delta"]=Value::Object(delta);sample["state_progress"]=json!(progress);
    } else {sample["state_progress"]=json!(false);}
    sample
}
fn event_dispatch_source(spec: &Value) -> Result<String,String> {
    let kind=spec["type"].as_str().filter(|s|!s.is_empty()).ok_or("event type must be a nonempty string")?;
    let target=spec.get("target").cloned().unwrap_or(json!("window"));
    if target.as_str().is_none() && target["selector"].as_str().is_none() {return Err("event target must be window, document or a selector".into());}
    if spec.get("init").is_some_and(|v|!v.is_object()) || spec.get("state").is_some_and(|v|!v.is_object()) {return Err("event init and state must be objects".into());}
    let _=kind;
    Ok(format!(r#"(() => {{
        const spec={spec};
        __tracePhase('event:'+(spec.name||spec.type));
        const state=spec.state||{{}},allowed=new Set(['visibilityState','online','innerWidth','innerHeight']);
        for(const key of Object.keys(state))if(!allowed.has(key))throw TypeError('Unsupported event state: '+key);
        if(state.visibilityState!==undefined){{if(!['visible','hidden'].includes(state.visibilityState))throw TypeError('Invalid visibilityState');document.visibilityState=state.visibilityState;document.hidden=state.visibilityState==='hidden';}}
        if(state.online!==undefined){{if(typeof state.online!=='boolean')throw TypeError('Invalid online state');globalThis.navigator=Object.freeze({{...navigator,onLine:state.online}});}}
        for(const key of ['innerWidth','innerHeight'])if(state[key]!==undefined){{if(typeof state[key]!=='number'||!Number.isFinite(state[key])||state[key]<0)throw TypeError('Invalid viewport distance');globalThis[key]=state[key];}}
        const requested=spec.target||'window';
        const target=requested==='window'?globalThis:requested==='document'?document:document.querySelectorAll(typeof requested==='string'?requested:requested.selector)[0];
        if(!target)throw Error('Synthetic event target does not exist');
        const init={{...(spec.init||{{}})}};
        for(const key of ['target','currentTarget','type','isTrusted','_stopped','_immediate'])delete init[key];
        const event=Object.prototype.hasOwnProperty.call(init,'detail')?new CustomEvent(spec.type,init):new Event(spec.type,init);
        event.isTrusted=false;
        __traceCall('SyntheticEvent.dispatch',[spec.type,typeof requested==='string'?requested:requested.selector]);
        target.dispatchEvent(event);
    }})()"#, spec=spec))
}
fn run(input: Value) -> Result<Value, String> {
    let scripts = input["scripts"].as_array().ok_or("scripts must be an array")?;
    let events = match input.get("events") {
        None => &[][..],
        Some(value) => value.as_array().ok_or("events must be an array")?.as_slice(),
    };
    if events.len() > 64 { return Err("at most 64 event scenarios are supported".into()); }
    let mut checkpoints=Vec::new();
    let lifecycle_mode = match input.get("lifecycle_mode") {
        None => "legacy",
        Some(value) => value.as_str().filter(|mode|matches!(*mode,"legacy"|"chrome"))
            .ok_or("lifecycle_mode must be legacy or chrome")?,
    };
    let mut attempted = 0;
    let mut modules = offline_modules::OfflineModules::from_input(&input);
    let mut errors = Vec::new();
    let mut rejections = std::collections::BTreeMap::<usize, String>::new();
    let mut deadline = Instant::now() + Duration::from_secs(8);
    unsafe {
        let rt = q::JS_NewRuntime(); if rt.is_null() { return Err("runtime allocation failed".into()); }
        modules.install(rt);
        JS_SetMemoryLimit(rt, 256 * 1024 * 1024); JS_SetMaxStackSize(rt, 1024 * 1024);
        JS_SetInterruptHandler(rt, Some(interrupt), (&mut deadline as *mut Instant).cast());
        q::JS_SetHostPromiseRejectionTracker(rt, Some(rejection), (&mut rejections as *mut std::collections::BTreeMap<usize, String>).cast());
        let ctx = q::JS_NewContext(rt);
        if ctx.is_null() { q::JS_FreeRuntime(rt); return Err("context allocation failed".into()); }
        let mut host_start = Instant::now();
        q::JS_SetContextOpaque(ctx, (&mut host_start as *mut Instant).cast());
        let global = q::JS_GetGlobalObject(ctx);
        for (name, callback) in [(c"__hostNow", host_now as q::JSCFunction), (c"__hostRandom", host_random as q::JSCFunction)] {
            let function = q::JS_NewCFunction2(ctx, Some(callback), name.as_ptr(), 1, 0, 0);
            q::JS_SetPropertyStr(ctx, global, name.as_ptr(), function);
        }
        q::js_free_value(ctx, global);
        let boot = format!("globalThis.__page = {};\n{}", input["page"], format!("{}\n{}\n{}\n{}\n{}", include_str!("dotm.js"), include_str!("api_trace.js"), include_str!("web_host.js"), include_str!("virtual_dom.js"), include_str!("web_compat.js")));
        if let Err(error) = eval_discard(ctx, &boot, "web_host.js", false) { errors.push(json!({"phase":"bootstrap","error":error})); }
        for script in scripts {
            if Instant::now() >= deadline { errors.push(json!({"phase":"budget","error":"execution deadline exceeded"})); break; }
            attempted += 1;
            let name = script["url"].as_str().unwrap_or("inline");
            let _ = eval_discard(ctx, &format!("__tracePhase({})", json!(name)), "phase", false);
            if let Err(error) = eval_discard(ctx, script["source"].as_str().unwrap_or(""), name, script["module"].as_bool().unwrap_or(false)) {
                errors.push(json!({"phase":"script","url":name,"error":error,"api_trace":trace_snapshot(ctx)}));
            }
            drain_jobs(rt, ctx, deadline, &mut errors);
        }
        if Instant::now() < deadline {
            if let Err(error) = eval_discard(ctx, &format!("__tracePhase('lifecycle'); __domReady({})",json!(lifecycle_mode)), "timers", false) { errors.push(json!({"phase":"timers","error":error,"api_trace":trace_snapshot(ctx)})); }
            drain_jobs(rt,ctx,deadline,&mut errors);
        }
        drain_timers(rt,ctx,deadline,&mut errors);
        checkpoints.push(checkpoint(ctx,rt,&mut deadline,"baseline","baseline",None,&errors,rejections.len(),None));
        for (index,spec) in events.iter().enumerate() {
            let generated_name=format!("event-{}:{}",index+1,spec["type"].as_str().unwrap_or("invalid"));
            let name=spec["name"].as_str().unwrap_or(&generated_name);
            let mut dispatched=false;
            if Instant::now() >= deadline {
                errors.push(json!({"phase":"event-budget","name":name,"error":"execution deadline exceeded before synthetic event"}));
            } else {
                match event_dispatch_source(spec).and_then(|source|eval_discard(ctx,&source,name,false)) {
                    Ok(())=>dispatched=true,
                    Err(error)=>errors.push(json!({"phase":"event","name":name,"error":error,"api_trace":trace_snapshot(ctx)})),
                }
                drain_jobs(rt,ctx,deadline,&mut errors);
                drain_timers(rt,ctx,deadline,&mut errors);
            }
            let mut next=checkpoint(ctx,rt,&mut deadline,name,"event",Some(spec),&errors,rejections.len(),checkpoints.last());
            next["dispatched"]=json!(dispatched);next["is_trusted"]=json!(false);
            checkpoints.push(next);
        }
        // Allow reporting even if the site exhausted its execution budget.
        deadline = Instant::now() + Duration::from_millis(500);
        std::hint::black_box(&deadline);
        let host = eval(ctx, "JSON.stringify(__hostReport())", "report", false)
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()));
        let pending_jobs = q::JS_IsJobPending(rt) != 0;
        q::JS_FreeContext(ctx); q::JS_FreeRuntime(rt);
        Ok(json!({"errors":errors,"unhandled_rejections":rejections.values().collect::<Vec<_>>(),"host":host?,"pending_jobs":pending_jobs,"scripts_attempted":attempted,"scripts_captured":scripts.len(),"modules":modules.report(),"checkpoints":checkpoints,"lifecycle_mode":lifecycle_mode}))
    }
}
fn main() {
    let result = (|| { let mut input = String::new(); io::stdin().take(160 * 1024 * 1024).read_to_string(&mut input).map_err(|e| e.to_string())?; run(serde_json::from_str(&input).map_err(|e| e.to_string())?) })();
    match result { Ok(report) => println!("{report}"), Err(error) => { eprintln!("{error}"); std::process::exit(1); } }
}
