//! Sources captured by the controller; guest module callbacks never touch IO.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_void},
};
use trueos_qjs as q;

pub struct OfflineModules {
    sources: BTreeMap<String, String>,
    resolutions: BTreeMap<String, BTreeMap<String, String>>,
    imports: BTreeMap<String, String>,
    scopes: BTreeMap<String, BTreeMap<String, String>>,
    requests: usize,
    loaded: Vec<String>,
    missing: Vec<String>,
}
impl OfflineModules {
    pub fn from_input(input: &Value) -> Self {
        let mut sources = BTreeMap::new();
        if let Some(scripts) = input["scripts"].as_array() {
            for script in scripts {
                if script["module"].as_bool() == Some(true) {
                    if let (Some(url), Some(source)) =
                        (script["url"].as_str(), script["source"].as_str())
                    {
                        sources.insert(url.to_owned(), source.to_owned());
                    }
                }
            }
        }
        if let Some(modules) = input["modules"].as_object() {
            for (url, module) in modules {
                if let Some(source) = module["source"].as_str() {
                    sources.insert(url.to_owned(), source.to_owned());
                }
            }
        }
        Self {
            sources,
            resolutions: serde_json::from_value(input["module_resolutions"].clone())
                .unwrap_or_default(),
            imports: serde_json::from_value(input["import_map"]["imports"].clone())
                .unwrap_or_default(),
            scopes: serde_json::from_value(input["import_map"]["scopes"].clone())
                .unwrap_or_default(),
            requests: 0,
            loaded: Vec::new(),
            missing: Vec::new(),
        }
    }
    /// Keep this object at a stable address until after JS_FreeRuntime.
    pub unsafe fn install(&mut self, rt: *mut q::JSRuntime) {
        unsafe {
            q::JS_SetModuleLoaderFunc(rt, Some(normalize), Some(load), (self as *mut Self).cast());
        }
    }
    pub fn report(&self) -> Value {
        json!({"mode":"captured-offline", "captured_modules":self.sources.len(), "requests":self.requests, "loaded":self.loaded, "missing":self.missing})
    }
    fn resolve(&self, base: &str, name: &str) -> Option<String> {
        if let Some(mapped) = self.resolutions.get(base).and_then(|m| m.get(name)) {
            return Some(mapped.clone());
        }
        let mut scopes: Vec<_> = self
            .scopes
            .iter()
            .filter(|(s, _)| base.starts_with(s.as_str()))
            .collect();
        scopes.sort_by_key(|(scope, _)| std::cmp::Reverse(scope.len()));
        for (_, map) in scopes {
            if let Some(url) = map_specifier(map, name) {
                return Some(url);
            }
        }
        if let Some(url) = map_specifier(&self.imports, name) {
            return Some(url);
        }
        if name.starts_with("https://")
            || name.starts_with("http://")
            || self.sources.contains_key(name)
        {
            return Some(name.to_owned());
        }
        None
    }
}
fn map_specifier(map: &BTreeMap<String, String>, name: &str) -> Option<String> {
    if let Some(value) = map.get(name) {
        return Some(value.clone());
    }
    map.iter()
        .filter(|(key, _)| key.ends_with('/') && name.starts_with(key.as_str()))
        .max_by_key(|(key, _)| key.len())
        .map(|(key, value)| format!("{}{}", value, &name[key.len()..]))
}
unsafe extern "C" {
    fn JS_GetImportMeta(ctx: *mut q::JSContext, module: *mut q::JSModuleDef) -> q::JSValue;
}
unsafe fn set_import_meta(
    ctx: *mut q::JSContext,
    module: *mut q::JSModuleDef,
    name: &str,
    main: bool,
) -> bool {
    let meta = unsafe { JS_GetImportMeta(ctx, module) };
    if meta.is_exception() {
        return false;
    }
    let url = unsafe { q::JS_NewStringLen(ctx, name.as_ptr().cast(), name.len()) };
    let main = q::JSValue {
        u: q::JSValueUnion { int32: main as i32 },
        tag: q::JS_TAG_BOOL,
    };
    let ok = unsafe { q::JS_SetPropertyStr(ctx, meta, c"url".as_ptr(), url) } >= 0
        && unsafe { q::JS_SetPropertyStr(ctx, meta, c"main".as_ptr(), main) } >= 0;
    unsafe {
        q::js_free_value(ctx, meta);
    }
    ok
}
/// Compile, attach URL metadata, then evaluate an HTML module entry point.
/// No script source or module dependency is resolved through filesystem/network IO.
pub unsafe fn evaluate_entry(
    ctx: *mut q::JSContext,
    source: *const c_char,
    length: usize,
    name: *const c_char,
) -> q::JSValue {
    let value = unsafe { q::JS_Eval(ctx, source, length, name, 1 | 32) };
    if value.is_exception() {
        return value;
    }
    let filename = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    if !unsafe { set_import_meta(ctx, value.u.ptr.cast(), &filename, true) } {
        unsafe {
            q::js_free_value(ctx, value);
        }
        return q::JSValue::exception();
    }
    unsafe { q::JS_EvalFunction(ctx, value) }
}
unsafe fn fail(ctx: *mut q::JSContext, message: &str) {
    let value = unsafe { q::JS_NewStringLen(ctx, message.as_ptr().cast(), message.len()) };
    unsafe {
        q::JS_Throw(ctx, value);
    }
}
unsafe extern "C" fn normalize(
    ctx: *mut q::JSContext,
    base: *const c_char,
    name: *const c_char,
    opaque: *mut c_void,
) -> *mut c_char {
    let base = unsafe { CStr::from_ptr(base) }.to_string_lossy();
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let resolved = unsafe { &*(opaque as *const OfflineModules) }.resolve(&base, &name);
    let Some(resolved) = resolved else {
        let message = format!("offline module resolution unavailable: {name} imported by {base}");
        let state = unsafe { &mut *(opaque as *mut OfflineModules) };
        if state.missing.len() < 1000 {
            state.missing.push(message.clone());
        }
        unsafe {
            fail(ctx, &message);
        }
        return std::ptr::null_mut();
    };
    let Ok(resolved) = CString::new(resolved) else {
        unsafe {
            fail(ctx, "module URL contains NUL");
        }
        return std::ptr::null_mut();
    };
    let bytes = resolved.as_bytes_with_nul();
    let result = unsafe { q::js_malloc(ctx, bytes.len()) }.cast::<c_char>();
    if !result.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), result, bytes.len());
        }
    }
    result
}
unsafe extern "C" fn load(
    ctx: *mut q::JSContext,
    name: *const c_char,
    opaque: *mut c_void,
) -> *mut q::JSModuleDef {
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    // Never retain a mutable state borrow across JS_Eval, which can recurse here.
    let source = {
        let state = unsafe { &mut *(opaque as *mut OfflineModules) };
        state.requests += 1;
        let source = state.sources.get(&name).cloned();
        if source.is_none() && state.missing.len() < 1000 {
            state.missing.push(name.clone());
        }
        source
    };
    let Some(source) = source else {
        unsafe {
            fail(ctx, &format!("module not captured: {name}"));
        }
        return std::ptr::null_mut();
    };
    let Ok(source) = CString::new(source) else {
        unsafe {
            fail(ctx, "module source contains NUL");
        }
        return std::ptr::null_mut();
    };
    let Ok(filename) = CString::new(name.clone()) else {
        unsafe {
            fail(ctx, "module URL contains NUL");
        }
        return std::ptr::null_mut();
    };
    let value = unsafe {
        q::JS_Eval(
            ctx,
            source.as_ptr(),
            source.as_bytes().len(),
            filename.as_ptr(),
            1 | 32,
        )
    };
    if value.is_exception() {
        return std::ptr::null_mut();
    }
    let module = unsafe { value.u.ptr }.cast::<q::JSModuleDef>();
    if !unsafe { set_import_meta(ctx, module, &name, false) } {
        unsafe {
            q::js_free_value(ctx, value);
        }
        return std::ptr::null_mut();
    }
    // QuickJS owns the module record; release the compile-only result value.
    unsafe {
        q::js_free_value(ctx, value);
    }
    let state = unsafe { &mut *(opaque as *mut OfflineModules) };
    if state.loaded.len() < 1000 {
        state.loaded.push(name);
    }
    module
}
