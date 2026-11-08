/* Additional immutable C-locale categories used by the pinned scanners.
 * Native TRUEOS already defines its four wide categories with C-locale ASCII
 * semantics. Keep these scanner-private categories in the same locale without
 * adding a mutable locale service or a new kernel C ABI.
 */
#ifndef TRUEOS_BLUEPRINT_NATIVE_GRAMMAR_H
#define TRUEOS_BLUEPRINT_NATIVE_GRAMMAR_H
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <wctype.h>
static inline int trueos_grammar_iswupper(wint_t value) {
  return value >= 'A' && value <= 'Z';
}
static inline int trueos_grammar_iswlower(wint_t value) {
  return value >= 'a' && value <= 'z';
}
static inline int trueos_grammar_iswblank(wint_t value) {
  return value == ' ' || value == '\t';
}
static inline int trueos_grammar_iswxdigit(wint_t value) {
  return (value >= '0' && value <= '9') ||
         (value >= 'A' && value <= 'F') ||
         (value >= 'a' && value <= 'f');
}
static inline wint_t trueos_grammar_towupper(wint_t value) {
  return trueos_grammar_iswlower(value) ? value - 'a' + 'A' : value;
}
static inline wint_t trueos_grammar_towlower(wint_t value) {
  return trueos_grammar_iswupper(value) ? value - 'A' + 'a' : value;
}
/* Perl uses bounded byte-string operations absent from the kernel's small
 * freestanding helper set. Keep their C contracts local, including known
 * destination object bounds instead of introducing checked-libc imports.
 */
static inline char *trueos_grammar_strncpy(char *out, const char *in, size_t count, size_t capacity) {
  if (capacity != (size_t)-1 && count > capacity) abort();
  size_t i = 0;
  for (; i < count && in[i]; i++) out[i] = in[i];
  for (; i < count; i++) out[i] = '\0';
  return out;
}
static inline char *trueos_grammar_strncat(char *out, const char *in, size_t count, size_t capacity) {
  size_t end = 0;
  while (end < capacity && out[end]) end++;
  if (end == capacity) abort();
  for (size_t i = 0; i < count && in[i]; i++) {
    if (capacity != (size_t)-1 && end >= capacity - 1) abort();
    out[end++] = in[i];
  }
  out[end] = '\0';
  return out;
}
#undef iswupper
#undef iswlower
#undef iswblank
#undef iswxdigit
#undef towupper
#undef towlower
#define iswupper trueos_grammar_iswupper
#define iswlower trueos_grammar_iswlower
#define iswblank trueos_grammar_iswblank
#define iswxdigit trueos_grammar_iswxdigit
#define towupper trueos_grammar_towupper
#define towlower trueos_grammar_towlower
#undef strncpy
#undef strncat
#define strncpy(out, in, count) trueos_grammar_strncpy((out), (in), (count), __builtin_object_size((out), 0))
#define strncat(out, in, count) trueos_grammar_strncat((out), (in), (count), __builtin_object_size((out), 0))
#endif
