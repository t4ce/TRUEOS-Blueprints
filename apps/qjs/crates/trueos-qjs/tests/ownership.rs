//! Ownership helpers must match the exact QuickJS header selected by build.rs.
use std::ffi::CStr;
use std::ffi::CString;
use trueos_qjs::*;

#[test]
fn duplicated_values_survive_original_release_and_runtime_teardown() {
    unsafe {
        let runtime = JS_NewRuntime();
        assert!(!runtime.is_null());
        let context = JS_NewContext(runtime);
        assert!(!context.is_null());
        let source = CString::new("({message:'retained 🌍', values:[1,2,3]})").unwrap();
        for _ in 0..1000 {
            let value = JS_Eval(context, source.as_ptr(), source.as_bytes().len(), c"ownership-test".as_ptr(), JS_EVAL_TYPE_GLOBAL);
            assert!(!value.is_exception());
            let retained = js_dup_value(context, value);
            let second = js_dup_value(context, retained);
            js_free_value(context, value);
            js_free_value(context, retained);
            let message = JS_GetPropertyStr(context, second, c"message".as_ptr());
            assert!(!message.is_exception());
            let text = js_to_cstring(context, message);
            assert!(!text.is_null());
            assert_eq!(CStr::from_ptr(text).to_str().unwrap(), "retained 🌍");
            JS_FreeCString(context, text);
            js_free_value(context, message);
            js_free_value(context, second);
        }
        JS_FreeContext(context);
        JS_FreeRuntime(runtime);
    }
}
