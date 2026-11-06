/* Native OpenSSL utility routines. No Unix process, stdio, or locale service
 * is implied by these namespaced, C-locale implementations. The only scanf
 * formats used by the x86_64 library are the HTTP port's exact "%d"/"%u".
 * Every unsupported scan format fails without changing the destination.
 */
#if defined(OPENSSL_SYS_TRUEOS)

#ifndef _GNU_SOURCE
# define _GNU_SOURCE 1
#endif

#include <errno.h>
#include <limits.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

_Static_assert(CHAR_BIT == 8 && sizeof(long) == 8 && sizeof(time_t) == 8,
               "native OpenSSL utility ABI requires x86_64 LP64 time64");
_Static_assert((time_t)-1 < 0, "native OpenSSL requires signed time_t");

static int ascii_space(unsigned char c)
{
    return c == ' ' || c == '\t' || c == '\n' || c == '\r'
        || c == '\f' || c == '\v';
}

static int ascii_digit(unsigned char c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'z')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'Z')
        return c - 'A' + 10;
    return -1;
}

int ossl_trueos_tolower(int c)
{
    return c >= 'A' && c <= 'Z' ? c + ('a' - 'A') : c;
}

char *ossl_trueos_strcpy(char *dest, const char *src)
{
    char *out = dest;
    do {
        *out++ = *src;
    } while (*src++ != '\0');
    return dest;
}

char *ossl_trueos_strdup(const char *src)
{
    size_t len = 0;
    char *out;

    while (src[len] != '\0') {
        if (len == SIZE_MAX - 1) {
            errno = ENOMEM;
            return NULL;
        }
        ++len;
    }
    out = malloc(len + 1);
    if (out == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    for (size_t i = 0; i <= len; ++i)
        out[i] = src[i];
    return out;
}

char *ossl_trueos_strncpy(char *dest, const char *src, size_t n)
{
    size_t i = 0;
    while (i < n && src[i] != '\0') {
        dest[i] = src[i];
        ++i;
    }
    while (i < n)
        dest[i++] = '\0';
    return dest;
}

char *ossl_trueos_strpbrk(const char *text, const char *accept)
{
    for (; *text != '\0'; ++text) {
        for (const char *p = accept; *p != '\0'; ++p) {
            if (*text == *p)
                return (char *)text;
        }
    }
    return NULL;
}

char *ossl_trueos_strstr(const char *text, const char *needle)
{
    if (*needle == '\0')
        return (char *)text;
    for (; *text != '\0'; ++text) {
        size_t i = 0;
        while (needle[i] != '\0' && text[i] != '\0'
               && text[i] == needle[i])
            ++i;
        if (needle[i] == '\0')
            return (char *)text;
    }
    return NULL;
}

struct parsed_integer {
    unsigned long value;
    const char *end;
    int negative;
    int any;
    int overflow;
};

/* Keep scanning after overflow so endptr still names the end of the subject
 * sequence. Only multiply after checking against the applicable signed or
 * unsigned magnitude bound; no signed arithmetic can overflow here.
 */
static struct parsed_integer parse_integer(const char *text, int base,
                                           unsigned long positive_limit,
                                           unsigned long negative_limit)
{
    struct parsed_integer out = {0, text, 0, 0, 0};
    const char *p = text;
    unsigned long limit, cutoff, cutlim;

    while (ascii_space((unsigned char)*p))
        ++p;
    if (*p == '+' || *p == '-') {
        out.negative = *p == '-';
        ++p;
    }
    if (base == 0) {
        base = *p == '0' ? 8 : 10;
        if (p[0] == '0' && (p[1] == 'x' || p[1] == 'X')
            && ascii_digit((unsigned char)p[2]) >= 0
            && ascii_digit((unsigned char)p[2]) < 16) {
            base = 16;
            p += 2;
        } else if (p[0] == '0' && (p[1] == 'b' || p[1] == 'B')
                   && (p[2] == '0' || p[2] == '1')) {
            base = 2;
            p += 2;
        }
    } else if (base == 16 && p[0] == '0'
               && (p[1] == 'x' || p[1] == 'X')
               && ascii_digit((unsigned char)p[2]) >= 0
               && ascii_digit((unsigned char)p[2]) < 16) {
        p += 2;
    } else if (base == 2 && p[0] == '0'
               && (p[1] == 'b' || p[1] == 'B')
               && (p[2] == '0' || p[2] == '1')) {
        p += 2;
    }
    limit = out.negative ? negative_limit : positive_limit;
    cutoff = limit / (unsigned)base;
    cutlim = limit % (unsigned)base;
    for (;;) {
        int digit = ascii_digit((unsigned char)*p);
        if (digit < 0 || digit >= base)
            break;
        out.any = 1;
        if (out.value > cutoff
            || (out.value == cutoff && (unsigned)digit > cutlim)) {
            out.overflow = 1;
        } else if (!out.overflow) {
            out.value = out.value * (unsigned)base + (unsigned)digit;
        }
        ++p;
    }
    if (out.any)
        out.end = p;
    return out;
}

long ossl_trueos_strtol(const char *text, char **endptr, int base)
{
    struct parsed_integer value;
    if (endptr != NULL)
        *endptr = (char *)text;
    if (text == NULL || (base != 0 && (base < 2 || base > 36))) {
        errno = EINVAL;
        return 0;
    }
    value = parse_integer(text, base, LONG_MAX, (unsigned long)LONG_MAX + 1UL);
    if (endptr != NULL)
        *endptr = (char *)value.end;
    if (value.overflow) {
        errno = ERANGE;
        return value.negative ? LONG_MIN : LONG_MAX;
    }
    if (!value.negative)
        return (long)value.value;
    return value.value == (unsigned long)LONG_MAX + 1UL
        ? LONG_MIN : -(long)value.value;
}

unsigned long ossl_trueos_strtoul(const char *text, char **endptr, int base)
{
    struct parsed_integer value;
    if (endptr != NULL)
        *endptr = (char *)text;
    if (text == NULL || (base != 0 && (base < 2 || base > 36))) {
        errno = EINVAL;
        return 0;
    }
    value = parse_integer(text, base, ULONG_MAX, ULONG_MAX);
    if (endptr != NULL)
        *endptr = (char *)value.end;
    if (value.overflow) {
        errno = ERANGE;
        return ULONG_MAX;
    }
    return value.negative ? 0UL - value.value : value.value;
}

int ossl_trueos_sscanf(const char *text, const char *format, ...)
{
    const char *start = text;
    struct parsed_integer value;
    va_list args;

    if (format == NULL || format[0] != '%'
        || (format[1] != 'd' && format[1] != 'u') || format[2] != '\0') {
        errno = EINVAL;
        return 0;
    }
    if (text == NULL) {
        errno = EINVAL;
        return -1;
    }
    while (ascii_space((unsigned char)*start))
        ++start;
    if (*start == '\0')
        return -1; /* EOF: input failure before the first conversion. */
    value = format[1] == 'd'
        ? parse_integer(text, 10, INT_MAX, (unsigned long)INT_MAX + 1UL)
        : parse_integer(text, 10, UINT_MAX, UINT_MAX);
    if (!value.any)
        return 0;
    if (value.overflow) {
        errno = ERANGE;
        return 0;
    }
    va_start(args, format);
    if (format[1] == 'd') {
        int *out = va_arg(args, int *);
        if (out == NULL) {
            va_end(args);
            errno = EINVAL;
            return 0;
        }
        *out = value.negative
            ? (value.value == (unsigned long)INT_MAX + 1UL
               ? INT_MIN : -(int)value.value)
            : (int)value.value;
    } else {
        unsigned int *out = va_arg(args, unsigned int *);
        if (out == NULL) {
            va_end(args);
            errno = EINVAL;
            return 0;
        }
        *out = value.negative ? 0U - (unsigned int)value.value
                             : (unsigned int)value.value;
    }
    va_end(args);
    return 1;
}

static int leap_year(int64_t year)
{
    return year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
}

struct tm *ossl_trueos_gmtime_r(const time_t *timer, struct tm *result)
{
    static const int month_days[12] = {
        31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
    };
    int64_t seconds, days, rem, era, year, day_in_year, cycle_days;
    int month = 0;
    struct tm out = {0};

    if (timer == NULL || result == NULL) {
        errno = EINVAL;
        return NULL;
    }
    seconds = (int64_t)*timer;
    days = seconds / 86400;
    rem = seconds % 86400;
    if (rem < 0) {
        rem += 86400;
        --days;
    }
    /* January 1, 2000 is day 10957. Each Gregorian 400-year cycle has
     * 146097 days. Floor division gives a nonnegative day within the cycle
     * even before year zero; at most 399 small year steps are then needed.
     */
    cycle_days = days - 10957;
    era = cycle_days / 146097;
    if (cycle_days % 146097 < 0)
        --era;
    day_in_year = cycle_days - era * 146097;
    year = 2000 + era * 400;
    for (;;) {
        int n = 365 + leap_year(year);
        if (day_in_year < n)
            break;
        day_in_year -= n;
        ++year;
    }
    if (year - 1900 < INT_MIN || year - 1900 > INT_MAX) {
        errno = EOVERFLOW;
        return NULL;
    }
    out.tm_year = (int)(year - 1900);
    out.tm_yday = (int)day_in_year;
    for (;;) {
        int n = month_days[month] + (month == 1 && leap_year(year));
        if (day_in_year < n)
            break;
        day_in_year -= n;
        ++month;
    }
    out.tm_mon = month;
    out.tm_mday = (int)day_in_year + 1;
    out.tm_hour = (int)(rem / 3600);
    out.tm_min = (int)(rem / 60 % 60);
    out.tm_sec = (int)(rem % 60);
    out.tm_wday = (int)((days + 4) % 7);
    if (out.tm_wday < 0)
        out.tm_wday += 7;
    out.tm_isdst = 0;
    out.tm_gmtoff = 0;
    out.tm_zone = "UTC";
    *result = out;
    return result;
}

/* A compiler can emit the fortified symbol even when source calls were
 * namespaced. Validate both strings fit before writing, then include NUL.
 */
char *__strcat_chk(char *dest, const char *src, size_t capacity)
{
    size_t used = 0, len = 0, available;
    while (used < capacity && dest[used] != '\0')
        ++used;
    if (used == capacity)
        abort();
    available = capacity - used;
    while (len < available && src[len] != '\0')
        ++len;
    if (len == available)
        abort();
    for (size_t i = 0; i <= len; ++i)
        dest[used + i] = src[i];
    return dest;
}

#endif /* OPENSSL_SYS_TRUEOS */
