/* Host regression tests for the native OpenSSL utility implementations.
 *
 * cc -std=c11 -D_GNU_SOURCE -DOPENSSL_SYS_TRUEOS -fno-builtin \
 *    -Wall -Wextra -Werror src/openssl_trueos_compat.c \
 *    src/openssl_trueos_compat_test.c -o /tmp/openssl-trueos-compat-test
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE 1
#endif
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <locale.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

char *ossl_trueos_strcpy(char *, const char *);
char *ossl_trueos_strdup(const char *);
char *ossl_trueos_strncpy(char *, const char *, size_t);
char *ossl_trueos_strpbrk(const char *, const char *);
char *ossl_trueos_strstr(const char *, const char *);
long ossl_trueos_strtol(const char *, char **, int);
unsigned long ossl_trueos_strtoul(const char *, char **, int);
struct tm *ossl_trueos_gmtime_r(const time_t *, struct tm *);
int ossl_trueos_tolower(int);
int ossl_trueos_sscanf(const char *, const char *, ...);
char *__strcat_chk(char *, const char *, size_t);

static unsigned long checks;
static uint64_t random_state = UINT64_C(0x91ac4076fe83d257);

static void fail(const char *format, ...)
{
    va_list args;
    fputs("FAIL: ", stderr);
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    exit(1);
}

#define CHECK(condition, ...) do { \
    ++checks; \
    if (!(condition)) fail(__VA_ARGS__); \
} while (0)

static uint64_t next_random(void)
{
    random_state ^= random_state << 13;
    random_state ^= random_state >> 7;
    random_state ^= random_state << 17;
    return random_state;
}

static long offset(const char *base, const char *found)
{
    return found == NULL ? -1 : (long)(found - base);
}

static void check_string(const char *text)
{
    unsigned char actual[256], expected[256];
    size_t length = strlen(text);
    size_t count;
    char *copy;
    CHECK(length + 4 < sizeof(actual), "string fixture too long");
    memset(actual, 0xa5, sizeof(actual));
    memset(expected, 0xa5, sizeof(expected));
    strcpy((char *)expected, text);
    CHECK(ossl_trueos_strcpy((char *)actual, text) == (char *)actual,
          "strcpy return pointer");
    CHECK(memcmp(actual, expected, sizeof(actual)) == 0,
          "strcpy bytes for '%s'", text);
    copy = ossl_trueos_strdup(text);
    CHECK(copy != NULL && copy != text && strcmp(copy, text) == 0,
          "strdup bytes/ownership for '%s'", text);
    free(copy);
    for (count = 0; count <= length + 3; ++count) {
        memset(actual, 0xa5, sizeof(actual));
        memset(expected, 0xa5, sizeof(expected));
        strncpy((char *)expected, text, count);
        CHECK(ossl_trueos_strncpy((char *)actual, text, count) == (char *)actual,
              "strncpy return pointer");
        CHECK(memcmp(actual, expected, sizeof(actual)) == 0,
              "strncpy bytes/padding for '%s', n=%zu", text, count);
    }
}

static void test_strings(void)
{
    static const char *const samples[] = {
        "", "a", "abc", "aaaaab", "needle", "prefix needle suffix",
        "http://127.0.0.1:8080/a?b=c", "Mixed CASE and\tspaces\n", "\200\377x"
    };
    size_t i, j;
    char text[100], needle[30];
    for (i = 0; i < sizeof(samples) / sizeof(samples[0]); ++i) {
        check_string(samples[i]);
        for (j = 0; j < sizeof(samples) / sizeof(samples[0]); ++j) {
            CHECK(offset(samples[i], ossl_trueos_strpbrk(samples[i], samples[j]))
                  == offset(samples[i], strpbrk(samples[i], samples[j])),
                  "strpbrk sample %zu/%zu", i, j);
            CHECK(offset(samples[i], ossl_trueos_strstr(samples[i], samples[j]))
                  == offset(samples[i], strstr(samples[i], samples[j])),
                  "strstr sample %zu/%zu", i, j);
        }
    }
    for (i = 0; i < 1000; ++i) {
        size_t size = (size_t)(next_random() % sizeof(text));
        size_t wanted = (size_t)(next_random() % sizeof(needle));
        for (j = 0; j < size; ++j) text[j] = (char)('a' + next_random() % 7);
        text[size] = 0;
        for (j = 0; j < wanted; ++j) needle[j] = (char)('a' + next_random() % 7);
        needle[wanted] = 0;
        check_string(text);
        CHECK(offset(text, ossl_trueos_strpbrk(text, needle))
              == offset(text, strpbrk(text, needle)), "random strpbrk %zu", i);
        CHECK(offset(text, ossl_trueos_strstr(text, needle))
              == offset(text, strstr(text, needle)), "random strstr %zu", i);
        CHECK(ossl_trueos_strstr(text, text) == text, "strstr full self-match");
    }
}

static void check_number_against_host(const char *text, int base)
{
    char *actual_end, *expected_end;
    long actual_signed, expected_signed;
    unsigned long actual_unsigned, expected_unsigned;
    int actual_errno, expected_errno;
    errno = E2BIG;
    expected_signed = strtol(text, &expected_end, base);
    expected_errno = errno;
    errno = E2BIG;
    actual_signed = ossl_trueos_strtol(text, &actual_end, base);
    actual_errno = errno;
    CHECK(actual_signed == expected_signed && actual_end == expected_end
          && actual_errno == expected_errno,
          "strtol '%s' base %d: actual=%ld/end%ld/errno%d expected=%ld/end%ld/errno%d",
          text, base, actual_signed, offset(text, actual_end), actual_errno,
          expected_signed, offset(text, expected_end), expected_errno);
    errno = E2BIG;
    expected_unsigned = strtoul(text, &expected_end, base);
    expected_errno = errno;
    errno = E2BIG;
    actual_unsigned = ossl_trueos_strtoul(text, &actual_end, base);
    actual_errno = errno;
    CHECK(actual_unsigned == expected_unsigned && actual_end == expected_end
          && actual_errno == expected_errno,
          "strtoul '%s' base %d: actual=%lu/end%ld/errno%d expected=%lu/end%ld/errno%d",
          text, base, actual_unsigned, offset(text, actual_end), actual_errno,
          expected_unsigned, offset(text, expected_end), expected_errno);
}

static void check_binary(const char *text, int base, long value, long consumed)
{
    char *end;
    long actual_signed;
    unsigned long actual_unsigned;
    errno = E2BIG;
    actual_signed = ossl_trueos_strtol(text, &end, base);
    CHECK(actual_signed == value && end == text + consumed && errno == E2BIG,
          "C23 binary strtol '%s' base %d", text, base);
    errno = E2BIG;
    actual_unsigned = ossl_trueos_strtoul(text, &end, base);
    CHECK(actual_unsigned == (unsigned long)value && end == text + consumed
          && errno == E2BIG, "C23 binary strtoul '%s' base %d", text, base);
}

/* Remove only the prefix so hosts predating C23 remain a valid range oracle. */
static void check_binary_range(const char *text, int base)
{
    char canonical[220], *actual_end, *expected_end;
    const char *prefix = strstr(text, "0b");
    size_t before;
    long actual_signed, expected_signed;
    unsigned long actual_unsigned, expected_unsigned;
    int expected_errno;
    CHECK(prefix != NULL && (prefix[2] == '0' || prefix[2] == '1'),
          "binary range fixture has valid prefix");
    before = (size_t)(prefix - text);
    memcpy(canonical, text, before);
    strcpy(canonical + before, prefix + 2);
    errno = E2BIG;
    expected_signed = strtol(canonical, &expected_end, 2);
    expected_errno = errno;
    errno = E2BIG;
    actual_signed = ossl_trueos_strtol(text, &actual_end, base);
    CHECK(actual_signed == expected_signed && errno == expected_errno
          && actual_end - text == expected_end - canonical + 2,
          "prefixed binary signed boundary '%s' base %d", text, base);
    errno = E2BIG;
    expected_unsigned = strtoul(canonical, &expected_end, 2);
    expected_errno = errno;
    errno = E2BIG;
    actual_unsigned = ossl_trueos_strtoul(text, &actual_end, base);
    CHECK(actual_unsigned == expected_unsigned && errno == expected_errno
          && actual_end - text == expected_end - canonical + 2,
          "prefixed binary unsigned boundary '%s' base %d", text, base);
}

static void test_numbers(void)
{
    static const char *const samples[] = {
        "", " ", "\t\n\v\f\r ", "+", "-", "+ 1", "- 1", "xyz",
        "0", "00", "08", "09", "-0", "+0", " 123xyz", "0x", "-0x",
        "+0X", "0xg", "0x0", "-0XfFrest", "123456789012345678901234567890",
        "-123456789012345678901234567890", "7fffffff", "zZZ", "101010!",
        "9223372036854775807", "9223372036854775808", "-9223372036854775808",
        "-9223372036854775809", "18446744073709551615", "18446744073709551616",
        "-18446744073709551615", "-18446744073709551616"
    };
    static const int bases[] = {0, 2, 3, 8, 10, 16, 35, 36};
    static const char digits[] = "0123456789abcdefghijklmnopqrstuvwxyz";
    char text[220], *end;
    size_t i, j;
    int base;
    for (i = 0; i < sizeof(samples) / sizeof(samples[0]); ++i)
        for (j = 0; j < sizeof(bases) / sizeof(bases[0]); ++j)
            check_number_against_host(samples[i], bases[j]);
    snprintf(text, sizeof(text), "%ld", LONG_MIN);
    check_number_against_host(text, 10);
    snprintf(text, sizeof(text), "%ld", LONG_MAX);
    check_number_against_host(text, 10);
    snprintf(text, sizeof(text), "%lu", ULONG_MAX);
    check_number_against_host(text, 10);
    snprintf(text, sizeof(text), "%ld0tail", LONG_MIN);
    check_number_against_host(text, 10);
    snprintf(text, sizeof(text), "%ld0tail", LONG_MAX);
    check_number_against_host(text, 10);
    snprintf(text, sizeof(text), "%lu0tail", ULONG_MAX);
    check_number_against_host(text, 10);
    for (base = 2; base <= 36; ++base) {
        for (i = 0; i < 160; ++i) {
            size_t size = (size_t)(next_random() % 180), pos = 0;
            if (next_random() & 1) text[pos++] = ' ';
            if (next_random() & 1) text[pos++] = (next_random() & 1) ? '+' : '-';
            /* Start with nonzero to avoid a host-dependent C23 0b prefix. */
            if (size != 0) text[pos++] = digits[1 + next_random() % (unsigned)(base - 1)];
            for (j = 1; j < size; ++j) text[pos++] = digits[next_random() % (unsigned)base];
            text[pos++] = '!';
            text[pos] = 0;
            check_number_against_host(text, base);
        }
    }
    for (i = 0; i < 600; ++i) {
        uint64_t value = next_random();
        switch (i % 3) {
        case 0: snprintf(text, sizeof(text), "  %s%llu?", i & 1 ? "-" : "+",
                         (unsigned long long)value); break;
        case 1: snprintf(text, sizeof(text), "%s0%llo?", i & 1 ? "-" : "+",
                         (unsigned long long)value); break;
        default: snprintf(text, sizeof(text), "%s0x%llx?", i & 1 ? "-" : "+",
                          (unsigned long long)value); break;
        }
        check_number_against_host(text, 0);
    }
    for (base = 0; base <= 2; base += 2) {
        check_binary("0b0", base, 0, 3);
        check_binary("0B101", base, 5, 5);
        check_binary("  -0b101tail", base, -5, 8);
        check_binary("+0B11!", base, 3, 5);
        check_binary("0b", base, 0, 1);
        check_binary("-0b", base, 0, 2);
        check_binary("0b2", base, 0, 1);
        check_binary("+0B+1", base, 0, 2);
        for (i = sizeof(long) * CHAR_BIT - 1; i <= sizeof(long) * CHAR_BIT + 1; ++i) {
            size_t sign;
            for (sign = 0; sign <= 1; ++sign) {
                size_t pos = 0;
                if (sign) text[pos++] = '-';
                text[pos++] = '0'; text[pos++] = 'b';
                for (j = 0; j < i; ++j) text[pos++] = '1';
                text[pos++] = '!'; text[pos] = 0;
                check_binary_range(text, base);
                /* A lone leading 1 also covers LONG_MIN's exact magnitude. */
                memset(text + (sign ? 4 : 3), '0', i - 1);
                check_binary_range(text, base);
            }
        }
        for (i = 0; i < 300; ++i) {
            unsigned long value = (unsigned long)(next_random() & UINT64_C(0x7fffffff));
            char reversed[64];
            size_t size = 0, pos = 0;
            unsigned long left = value;
            do { reversed[size++] = (char)('0' + (left & 1)); left >>= 1; } while (left);
            if (i & 1) text[pos++] = '-';
            text[pos++] = '0'; text[pos++] = i & 2 ? 'B' : 'b';
            while (size != 0) text[pos++] = reversed[--size];
            text[pos++] = '!'; text[pos] = 0;
            check_binary(text, base, i & 1 ? -(long)value : (long)value, (long)pos - 1);
        }
    }
    /* Prefix recognition is exclusive to base 0 or 2. */
    check_number_against_host("0b101", 16);
    check_number_against_host("0b101", 3);
    for (base = -1; base <= 37; ++base) {
        const char *invalid_base_input = "1";
        if (base == 0 || (base >= 2 && base <= 36)) continue;
        end = text;
        errno = 0;
        CHECK(ossl_trueos_strtol(invalid_base_input, &end, base) == 0
              && errno == EINVAL && end == invalid_base_input,
              "invalid strtol base %d", base);
        end = text;
        errno = 0;
        CHECK(ossl_trueos_strtoul(invalid_base_input, &end, base) == 0
              && errno == EINVAL && end == invalid_base_input,
              "invalid strtoul base %d", base);
    }
    errno = E2BIG;
    CHECK(ossl_trueos_strtol("42", NULL, 10) == 42 && errno == E2BIG,
          "strtol optional end pointer");
    errno = E2BIG;
    CHECK(ossl_trueos_strtoul("42", NULL, 10) == 42 && errno == E2BIG,
          "strtoul optional end pointer");
}

static int64_t civil_seconds(int64_t year, unsigned month, unsigned day)
{
    int64_t era, yoe, days;
    unsigned shifted_month, doy;
    year -= month <= 2;
    era = (year >= 0 ? year : year - 399) / 400;
    yoe = year - era * 400;
    shifted_month = month > 2 ? month - 3 : month + 9;
    doy = (153 * shifted_month + 2) / 5 + day - 1;
    days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    return days * INT64_C(86400);
}

static void check_utc(int64_t seconds)
{
    time_t timestamp = (time_t)seconds;
    struct tm actual, expected;
    struct tm *actual_result, *expected_result;
    int actual_errno, expected_errno;
    CHECK((int64_t)timestamp == seconds, "host time_t cannot represent fixture");
    memset(&actual, 0xa5, sizeof(actual));
    memset(&expected, 0xa5, sizeof(expected));
    errno = E2BIG;
    expected_result = gmtime_r(&timestamp, &expected);
    expected_errno = errno;
    errno = E2BIG;
    actual_result = ossl_trueos_gmtime_r(&timestamp, &actual);
    actual_errno = errno;
    CHECK((actual_result == NULL) == (expected_result == NULL),
          "gmtime_r representability at %lld", (long long)seconds);
    CHECK(actual_errno == expected_errno, "gmtime_r errno at %lld: %d != %d",
          (long long)seconds, actual_errno, expected_errno);
    if (expected_result == NULL) return;
    CHECK(actual_result == &actual, "gmtime_r output pointer");
    CHECK(actual.tm_sec == expected.tm_sec && actual.tm_min == expected.tm_min
          && actual.tm_hour == expected.tm_hour && actual.tm_mday == expected.tm_mday
          && actual.tm_mon == expected.tm_mon && actual.tm_year == expected.tm_year
          && actual.tm_wday == expected.tm_wday && actual.tm_yday == expected.tm_yday
          && actual.tm_isdst == expected.tm_isdst,
          "gmtime_r civil fields at %lld: actual %d/%d/%d %d:%d:%d w%d y%d dst%d; "
          "expected %d/%d/%d %d:%d:%d w%d y%d dst%d",
          (long long)seconds, actual.tm_year, actual.tm_mon, actual.tm_mday,
          actual.tm_hour, actual.tm_min, actual.tm_sec, actual.tm_wday,
          actual.tm_yday, actual.tm_isdst, expected.tm_year, expected.tm_mon,
          expected.tm_mday, expected.tm_hour, expected.tm_min, expected.tm_sec,
          expected.tm_wday, expected.tm_yday, expected.tm_isdst);
    CHECK(actual.tm_gmtoff == expected.tm_gmtoff && actual.tm_gmtoff == 0,
          "gmtime_r UTC offset");
    CHECK(actual.tm_zone != NULL
          && (strcmp(actual.tm_zone, "GMT") == 0 || strcmp(actual.tm_zone, "UTC") == 0),
          "gmtime_r timezone abbreviation");
}

static void test_utc(void)
{
    static const int64_t fixed[] = {
        INT64_MIN, INT64_MAX, -1, 0, 1, -86401, -86400, -86399, 86399, 86400,
        INT32_MIN, INT32_MAX, INT64_C(2147483648), INT64_C(-2147483649),
        INT64_C(-62167219200), INT64_C(-62135596800), INT64_C(-2208988800),
        INT64_C(951782400), INT64_C(4107542400), INT64_C(253402300799)
    };
    static const int64_t years[] = {
        -400, -100, -4, -1, 0, 1, 4, 100, 400, 1582, 1600, 1700, 1800,
        1900, 1969, 1970, 1999, 2000, 2001, 2038, 2100, 2400, 9999,
        (int64_t)INT_MIN + 1899, (int64_t)INT_MIN + 1900,
        (int64_t)INT_MAX + 1900, (int64_t)INT_MAX + 1901
    };
    size_t i;
    unsigned month;
    int delta;
    for (i = 0; i < sizeof(fixed) / sizeof(fixed[0]); ++i) check_utc(fixed[i]);
    for (i = 0; i < sizeof(years) / sizeof(years[0]); ++i) {
        for (month = 1; month <= 12; ++month) {
            int64_t start = civil_seconds(years[i], month, 1);
            for (delta = -1; delta <= 1; ++delta) check_utc(start + delta);
        }
    }
    for (i = 0; i < 2500; ++i) {
        int64_t magnitude = (int64_t)(next_random() % UINT64_C(70000000000000000));
        check_utc(i & 1 ? -magnitude : magnitude);
    }
}

static void test_tolower(void)
{
    int ch;
    CHECK(setlocale(LC_CTYPE, "C") != NULL, "select host C locale");
    CHECK(ossl_trueos_tolower(EOF) == tolower(EOF), "tolower EOF");
    for (ch = 0; ch <= UCHAR_MAX; ++ch)
        CHECK(ossl_trueos_tolower(ch) == tolower(ch), "tolower byte %d", ch);
}

static void test_scanner(void)
{
    static const char *const valid[] = {"0", "1", "65535", "  8080tail", "+80", "-0", "-1", "\t12\n"};
    static const char *const invalid[] = {"x", "+", "-", "+ 12", "- 12", "0x", ""};
    static const char *const unsupported[] = {"%ld", "%x", "%i", "%2d", " %d", "%d ", "%d%d", "%n", "port=%u", ""};
    size_t i;
    int actual, expected, result;
    unsigned actual_unsigned, expected_unsigned;
    char text[100];
    for (i = 0; i < sizeof(valid) / sizeof(valid[0]); ++i) {
        actual = expected = -777;
        result = sscanf(valid[i], "%d", &expected);
        CHECK(ossl_trueos_sscanf(valid[i], "%d", &actual) == result && actual == expected,
              "scanner signed '%s'", valid[i]);
        actual_unsigned = expected_unsigned = 777;
        result = sscanf(valid[i], "%u", &expected_unsigned);
        CHECK(ossl_trueos_sscanf(valid[i], "%u", &actual_unsigned) == result
              && actual_unsigned == expected_unsigned, "scanner unsigned '%s'", valid[i]);
    }
    for (i = 0; i < sizeof(invalid) / sizeof(invalid[0]); ++i) {
        actual = expected = -777;
        result = sscanf(invalid[i], "%d", &expected);
        CHECK(ossl_trueos_sscanf(invalid[i], "%d", &actual) == result && actual == expected,
              "scanner invalid signed '%s'", invalid[i]);
        actual_unsigned = expected_unsigned = 777;
        result = sscanf(invalid[i], "%u", &expected_unsigned);
        CHECK(ossl_trueos_sscanf(invalid[i], "%u", &actual_unsigned) == result
              && actual_unsigned == expected_unsigned, "scanner invalid unsigned '%s'", invalid[i]);
    }
    actual = -777;
    CHECK(ossl_trueos_sscanf(" \t\n", "%d", &actual) == EOF && actual == -777,
          "scanner whitespace EOF");
    snprintf(text, sizeof(text), "%d", INT_MIN);
    actual = 0;
    CHECK(ossl_trueos_sscanf(text, "%d", &actual) == 1 && actual == INT_MIN,
          "scanner INT_MIN");
    snprintf(text, sizeof(text), "%d", INT_MAX);
    CHECK(ossl_trueos_sscanf(text, "%d", &actual) == 1 && actual == INT_MAX,
          "scanner INT_MAX");
    snprintf(text, sizeof(text), "%u", UINT_MAX);
    actual_unsigned = 0;
    CHECK(ossl_trueos_sscanf(text, "%u", &actual_unsigned) == 1
          && actual_unsigned == UINT_MAX, "scanner UINT_MAX");
    for (i = 0; i < 2; ++i) {
        snprintf(text, sizeof(text), "%d0", i ? INT_MIN : INT_MAX);
        actual = -777;
        errno = 0;
        CHECK(ossl_trueos_sscanf(text, "%d", &actual) == 0 && actual == -777
              && errno == ERANGE,
              "scanner signed overflow '%s'", text);
    }
    snprintf(text, sizeof(text), "%u0", UINT_MAX);
    actual_unsigned = 777;
    errno = 0;
    CHECK(ossl_trueos_sscanf(text, "%u", &actual_unsigned) == 0
          && actual_unsigned == 777 && errno == ERANGE,
          "scanner unsigned overflow");
    actual = -777;
    CHECK(ossl_trueos_sscanf("999999999999999999999999999999", "%d", &actual) == 0
          && actual == -777, "scanner long overflow");
    for (i = 0; i < sizeof(unsupported) / sizeof(unsupported[0]); ++i) {
        actual = -777;
        errno = 0;
        CHECK(ossl_trueos_sscanf("42", unsupported[i], &actual) == 0
              && actual == -777 && errno == EINVAL,
              "scanner unsupported format '%s'", unsupported[i]);
    }
}

static void check_strcat_abort(int unterminated)
{
    pid_t child = fork();
    int status;
    CHECK(child >= 0, "fork strcat overflow test");
    if (child == 0) {
        struct rlimit no_core = {0, 0};
        char destination[5] = {'a', 'b', 'c', 'd', 'e'};
        if (setrlimit(RLIMIT_CORE, &no_core) != 0) _exit(90);
        signal(SIGABRT, SIG_DFL);
        if (unterminated)
            __strcat_chk(destination, "x", sizeof(destination));
        else {
            destination[3] = 0;
            __strcat_chk(destination, "de", sizeof(destination));
        }
        _exit(91);
    }
    CHECK(waitpid(child, &status, 0) == child, "wait strcat overflow child");
    CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGABRT,
          "strcat %s must abort, wait status=%d", unterminated ? "unterminated destination" : "overflow", status);
}

static void test_strcat_checked(void)
{
    char destination[32] = "abc";
    CHECK(__strcat_chk(destination, "def", sizeof(destination)) == destination
          && strcmp(destination, "abcdef") == 0, "checked strcat success");
    CHECK(__strcat_chk(destination, "", 7) == destination
          && strcmp(destination, "abcdef") == 0, "checked strcat empty source/exact capacity");
    destination[0] = 0;
    CHECK(__strcat_chk(destination, "abcd", 5) == destination
          && strcmp(destination, "abcd") == 0, "checked strcat exact fit");
    check_strcat_abort(0);
    check_strcat_abort(1);
}

int main(void)
{
    unsigned long previous = 0;
#define RUN(suite) do { \
    suite(); \
    printf("PASS %-22s %lu checks\n", #suite, checks - previous); \
    previous = checks; \
} while (0)
    RUN(test_strings);
    RUN(test_numbers);
    RUN(test_utc);
    RUN(test_tolower);
    RUN(test_scanner);
    RUN(test_strcat_checked);
    printf("PASS all native OpenSSL utility tests: %lu checks\n", checks);
    return 0;
}
