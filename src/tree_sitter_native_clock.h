// Selected only by the pinned TRUEOS Tree-sitter C build.
// The SDK supplies the native VM monotonic clock, in nanoseconds.
#ifndef TRUEOS_BLUEPRINT_TREE_SITTER_CLOCK_H_
#define TRUEOS_BLUEPRINT_TREE_SITTER_CLOCK_H_

extern uint64_t trueos_blueprint_c_monotonic_nanos(void);
typedef struct { uint64_t seconds, nanos; } TSClock;

static inline TSDuration duration_from_micros(uint64_t micros) { return micros; }
static inline uint64_t duration_to_micros(TSDuration duration) { return duration; }
static inline TSClock clock_null(void) { return (TSClock) {0, 0}; }
static inline TSClock clock_now(void) {
  uint64_t nanos = trueos_blueprint_c_monotonic_nanos();
  return (TSClock) {nanos / 1000000000, nanos % 1000000000};
}
static inline TSClock clock_after(TSClock base, TSDuration duration) {
  base.seconds += duration / 1000000;
  base.nanos += (duration % 1000000) * 1000;
  if (base.nanos >= 1000000000) {
    base.nanos -= 1000000000;
    ++base.seconds;
  }
  return base;
}
static inline bool clock_is_null(TSClock clock) { return !clock.seconds && !clock.nanos; }
static inline bool clock_is_gt(TSClock clock, TSClock other) {
  return clock.seconds > other.seconds ||
    (clock.seconds == other.seconds && clock.nanos > other.nanos);
}

#endif
