/* Private text operations for the pinned native parser/regex providers.
 * This is a byte formatter and callback writer, not a FILE implementation.
 * The supported conversions are the integer/string conversions used by those
 * providers. Unsupported formats stop with a diagnostic instead of inventing
 * output. Host providers continue to use their original C runtime.
 */
#ifndef TRUEOS_BLUEPRINT_NATIVE_TEXT_H
#define TRUEOS_BLUEPRINT_NATIVE_TEXT_H
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>

extern void trueos_cabi_write(uint32_t stream, const unsigned char *bytes, size_t len);

typedef struct {
  int (*write)(void *context, const char *bytes, size_t len);
  void *context;
} TrueosCTextSink;

static int trueos_c_text_console(void *context, const char *bytes, size_t len) {
  if (len > INT_MAX) return -1;
  trueos_cabi_write((uint32_t)(uintptr_t)context, (const unsigned char *)bytes, len);
  return (int)len;
}
static TrueosCTextSink trueos_c_text_out = {trueos_c_text_console, (void *)(uintptr_t)1};
static TrueosCTextSink trueos_c_text_err = {trueos_c_text_console, (void *)(uintptr_t)2};

static void trueos_c_text_fail(void) {
  static const unsigned char message[] = "native C provider used an unsupported text format\n";
  trueos_cabi_write(2, message, sizeof(message) - 1);
  abort();
}

typedef struct {
  char *bytes;
  size_t capacity;
  size_t count;
} TrueosCTextBuffer;

static void trueos_c_text_byte(TrueosCTextBuffer *out, char byte) {
  if (out->count == INT_MAX) trueos_c_text_fail();
  if (out->capacity && out->count < out->capacity - 1) out->bytes[out->count] = byte;
  out->count++;
}

static void trueos_c_text_repeat(TrueosCTextBuffer *out, char byte, size_t count) {
  if (count > (size_t)INT_MAX - out->count) trueos_c_text_fail();
  size_t available = out->capacity && out->count < out->capacity - 1
    ? out->capacity - 1 - out->count : 0;
  size_t written = count < available ? count : available;
  for (size_t i = 0; i < written; i++) out->bytes[out->count + i] = byte;
  out->count += count;
}

static unsigned trueos_c_text_number(const char **format) {
  unsigned result = 0;
  while (**format >= '0' && **format <= '9') {
    unsigned digit = (unsigned)(*(*format)++ - '0');
    if (result > ((unsigned)INT_MAX - digit) / 10) trueos_c_text_fail();
    result = result * 10 + digit;
  }
  return result;
}

/* Return the untruncated byte count, terminate iff capacity is nonzero, and
 * permit a null buffer iff capacity is zero, matching snprintf's contract. */
static int trueos_c_text_vformat(char *bytes, size_t capacity, const char *format, va_list arguments) {
  TrueosCTextBuffer out = {bytes, capacity, 0};
  va_list args;
  va_copy(args, arguments);
  while (*format) {
    if (*format != '%') { trueos_c_text_byte(&out, *format++); continue; }
    format++;
    if (*format == '%') { trueos_c_text_byte(&out, *format++); continue; }
    int left = 0, plus = 0, space = 0, alternate = 0, zero = 0;
    for (;;) {
      if (*format == '-') left = 1;
      else if (*format == '+') plus = 1;
      else if (*format == ' ') space = 1;
      else if (*format == '#') alternate = 1;
      else if (*format == '0') zero = 1;
      else break;
      format++;
    }
    unsigned width;
    if (*format == '*') {
      format++;
      int value = va_arg(args, int);
      if (value == INT_MIN) trueos_c_text_fail();
      if (value < 0) { left = 1; value = -value; }
      width = (unsigned)value;
    } else width = trueos_c_text_number(&format);
    int precision = -1;
    if (*format == '.') {
      format++;
      if (*format == '*') { format++; precision = va_arg(args, int); }
      else precision = (int)trueos_c_text_number(&format);
    }
    enum { NORMAL, HH, H, L, LL, J, Z, T } length = NORMAL;
    if (*format == 'h') { format++; length = H; if (*format == 'h') {format++; length = HH;} }
    else if (*format == 'l') { format++; length = L; if (*format == 'l') {format++; length = LL;} }
    else if (*format == 'j') {format++; length = J;}
    else if (*format == 'z') {format++; length = Z;}
    else if (*format == 't') {format++; length = T;}
    char kind = *format;
    if (!kind) trueos_c_text_fail();
    format++;
    if (kind == 's' || kind == 'c') {
      if (length != NORMAL) trueos_c_text_fail();
      char single;
      const char *text;
      size_t count = 0;
      if (kind == 'c') { single = (char)va_arg(args, int); text = &single; count = 1; }
      else {
        text = va_arg(args, const char *);
        if (!text) text = "(null)";
        while ((precision < 0 || count < (unsigned)precision) && text[count]) count++;
      }
      size_t padding = width > count ? width - count : 0;
      if (!left) trueos_c_text_repeat(&out, ' ', padding);
      for (size_t i = 0; i < count; i++) trueos_c_text_byte(&out, text[i]);
      if (left) trueos_c_text_repeat(&out, ' ', padding);
      continue;
    }
    if (kind != 'd' && kind != 'i' && kind != 'u' && kind != 'x' && kind != 'X' && kind != 'o' && kind != 'p') trueos_c_text_fail();
    uintmax_t value;
    int negative = 0;
    int pointer = kind == 'p';
    if (pointer) {
      if (length != NORMAL) trueos_c_text_fail();
      value = (uintptr_t)va_arg(args, void *);
      /* Pointer spelling is implementation-defined. Match the build host's
       * conventional hexadecimal form so DOT identifiers stay familiar. */
      alternate = 1;
      if (!value) {
        const char *nil = "(nil)";
        size_t padding = width > 5 ? width - 5 : 0;
        if (!left) trueos_c_text_repeat(&out, ' ', padding);
        for (int i = 0; i < 5; i++) trueos_c_text_byte(&out, nil[i]);
        if (left) trueos_c_text_repeat(&out, ' ', padding);
        continue;
      }
    } else if (kind == 'd' || kind == 'i') {
      intmax_t signed_value;
      switch (length) {
      case HH: signed_value = (signed char)va_arg(args, int); break;
      case H: signed_value = (short)va_arg(args, int); break;
      case L: signed_value = va_arg(args, long); break;
      case LL: signed_value = va_arg(args, long long); break;
      case J: signed_value = va_arg(args, intmax_t); break;
      case Z: signed_value = va_arg(args, ptrdiff_t); break;
      case T: signed_value = va_arg(args, ptrdiff_t); break;
      default: signed_value = va_arg(args, int); break;
      }
      negative = signed_value < 0;
      value = negative ? (uintmax_t)0 - (uintmax_t)signed_value : (uintmax_t)signed_value;
    } else {
      switch (length) {
      case HH: value = (unsigned char)va_arg(args, unsigned int); break;
      case H: value = (unsigned short)va_arg(args, unsigned int); break;
      case L: value = va_arg(args, unsigned long); break;
      case LL: value = va_arg(args, unsigned long long); break;
      case J: value = va_arg(args, uintmax_t); break;
      case Z: value = va_arg(args, size_t); break;
      case T: value = va_arg(args, uintptr_t); break;
      default: value = va_arg(args, unsigned int); break;
      }
    }
    unsigned base = (kind == 'x' || kind == 'X' || pointer) ? 16 : kind == 'o' ? 8 : 10;
    const char *digits = kind == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
    char reversed[sizeof(uintmax_t) * CHAR_BIT];
    unsigned count = 0;
    uintmax_t original = value;
    if (value || precision != 0) do { reversed[count++] = digits[value % base]; value /= base; } while (value);
    char prefix[3];
    unsigned prefix_count = 0;
    if (kind == 'd' || kind == 'i') {
      if (negative) prefix[prefix_count++] = '-';
      else if (plus) prefix[prefix_count++] = '+';
      else if (space) prefix[prefix_count++] = ' ';
    }
    if (alternate && base == 16 && original) {prefix[prefix_count++] = '0'; prefix[prefix_count++] = kind == 'X' ? 'X' : 'x';}
    size_t zeros = precision >= 0 && (unsigned)precision > count ? (unsigned)precision - count : 0;
    if (alternate && base == 8 && !zeros && (!count || reversed[count - 1] != '0')) zeros = 1;
    size_t total = prefix_count + zeros + count;
    size_t padding = width > total ? width - total : 0;
    if (!left && !(zero && precision < 0)) trueos_c_text_repeat(&out, ' ', padding);
    for (unsigned i = 0; i < prefix_count; i++) trueos_c_text_byte(&out, prefix[i]);
    if (!left && zero && precision < 0) trueos_c_text_repeat(&out, '0', padding);
    trueos_c_text_repeat(&out, '0', zeros);
    while (count) trueos_c_text_byte(&out, reversed[--count]);
    if (left) trueos_c_text_repeat(&out, ' ', padding);
  }
  va_end(args);
  if (capacity) bytes[out.count < capacity ? out.count : capacity - 1] = '\0';
  return (int)out.count;
}

static int trueos_c_text_format(char *bytes, size_t capacity, const char *format, ...) {
  va_list args; va_start(args, format);
  int count = trueos_c_text_vformat(bytes, capacity, format, args);
  va_end(args); return count;
}

static int trueos_c_text_printf(TrueosCTextSink *sink, const char *format, ...) {
  va_list args; va_start(args, format);
  char local[512];
  int count = trueos_c_text_vformat(local, sizeof(local), format, args);
  char *bytes = local;
  if ((size_t)count >= sizeof(local)) {
    bytes = (char *)malloc((size_t)count + 1);
    if (!bytes) { va_end(args); return -1; }
    trueos_c_text_vformat(bytes, (size_t)count + 1, format, args);
  }
  va_end(args);
  int result = sink->write(sink->context, bytes, (size_t)count);
  if (bytes != local) free(bytes);
  return result < 0 || result != count ? -1 : count;
}

static int trueos_c_text_putc(int byte, TrueosCTextSink *sink) {
  char value = (char)byte;
  return sink->write(sink->context, &value, 1) == 1 ? (unsigned char)value : -1;
}
static int trueos_c_text_puts(const char *text, TrueosCTextSink *sink) {
  size_t len = 0; while (text[len]) len++;
  if (len > INT_MAX) return -1;
  return sink->write(sink->context, text, len) == (int)len ? 0 : -1;
}

/* The headers were included above, before the provider-private spellings. */
#define FILE TrueosCTextSink
#undef stdout
#undef stderr
#define stdout (&trueos_c_text_out)
#define stderr (&trueos_c_text_err)
#define snprintf trueos_c_text_format
#define vsnprintf trueos_c_text_vformat
#define fprintf trueos_c_text_printf
#define fputc trueos_c_text_putc
#define fputs trueos_c_text_puts
#endif
