/* Compare actual engine-private formatting with the host libc, including the
 * formats used by QuickJS errors, escape generation and Date.toISOString. */
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stddef.h>

int native_engine_snprintf(char *, size_t, const char *, ...);

#define CHECK(format, ...) do { \
    for (size_t capacity = 0; capacity != 100; ++capacity) { \
        char expected[128], actual[128]; \
        memset(expected, 0x5a, sizeof expected); \
        memset(actual, 0x5a, sizeof actual); \
        int a = snprintf(expected, capacity, format, __VA_ARGS__); \
        int b = native_engine_snprintf(actual, capacity, format, __VA_ARGS__); \
        assert(a == b); \
        assert(memcmp(expected, actual, sizeof actual) == 0); \
    } \
    assert(snprintf(NULL, 0, format, __VA_ARGS__) == \
           native_engine_snprintf(NULL, 0, format, __VA_ARGS__)); \
} while (0)

int main(void) {
    CHECK("%u", 4294967295U);
    CHECK("get %s", "property");
    CHECK("%.*s", 5, "truncated");
    CHECK("\\x%02x \\u%04x", 10, 0x2665);
    CHECK("%.3s, %02d %.3s %0*d ", "Sunday", 5, "October", 4, 2026);
    CHECK("%+07d %04d", -12345, 2026);
    CHECK("%02d:%02d:%02d.%03dZ", 12, 34, 56, 789);
    CHECK("%02d:%02d:%02d %cM", 9, 30, 59, 'A');
    CHECK("%zu %lld %016llx", (size_t)123, -9223372036854775807LL, 0xabcdef1234567890ULL);
    CHECK("%-10s %.3f %.6g", "text", 1.25, 0.000125);
    puts("engine-private formatting: libc oracle passed");
}
