/* Private formatting for QuickJS diagnostics and Date formatting. */
#include <stdarg.h>
#include <stddef.h>
#include <limits.h>

#define STB_SPRINTF_IMPLEMENTATION
#define STB_SPRINTF_STATIC
#include "stb_sprintf.h"

int native_engine_vsnprintf(char *buffer, size_t count, const char *format,
                           va_list arguments) {
    if (count > INT_MAX || !format || (!buffer && count != 0))
        return -1;
    /* stb's counting-only branch requires NULL for a zero-sized buffer. */
    if (count == 0)
        buffer = NULL;
    return stbsp_vsnprintf(buffer, (int)count, format, arguments);
}

int native_engine_snprintf(char *buffer, size_t count, const char *format, ...) {
    va_list arguments;
    va_start(arguments, format);
    int result = native_engine_vsnprintf(buffer, count, format, arguments);
    va_end(arguments);
    return result;
}
