/* Keep inline ownership operations coupled to the compiled QuickJS version.
 * JSRefCountHeader moved before the value pointer in newer QuickJS releases;
 * reproducing that private layout in Rust silently corrupts live values. */
#include "quickjs.h"

void native_engine_js_free_value(JSContext *ctx, JSValue value) {
    JS_FreeValue(ctx, value);
}

JSValue native_engine_js_dup_value(JSContext *ctx, JSValue value) {
    return JS_DupValue(ctx, value);
}
