extern crate alloc;

use alloc::{string::String, vec};
use core::fmt;
use core::num::FpCategory;
use core::ops::{Add, AddAssign, Sub, SubAssign};
use core::time::Duration as CoreDuration;

use crate::vcabi;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Duration {
    nanos: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Instant {
    nanos: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct UtcDateTime {
    pub year: u32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Duration {
    #[inline]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    #[inline]
    pub const fn from_millis(millis: u64) -> Self {
        Self {
            nanos: millis.saturating_mul(1_000_000),
        }
    }

    #[inline]
    pub const fn from_secs(seconds: u64) -> Self {
        Self {
            nanos: seconds.saturating_mul(1_000_000_000),
        }
    }

    #[inline]
    pub fn try_from_secs_f32(seconds: f32) -> Result<Self, TryFromSecsError> {
        match seconds.classify() {
            FpCategory::Nan | FpCategory::Infinite if seconds.is_sign_negative() => {
                return Err(TryFromSecsError);
            }
            FpCategory::Nan | FpCategory::Infinite => return Err(TryFromSecsError),
            _ => {}
        }
        if seconds < 0.0 {
            return Err(TryFromSecsError);
        }
        let nanos = (seconds as f64) * 1_000_000_000.0;
        if nanos > u64::MAX as f64 {
            return Err(TryFromSecsError);
        }
        Ok(Self {
            nanos: nanos as u64,
        })
    }

    #[inline]
    pub const fn as_nanos(self) -> u64 {
        self.nanos
    }

    #[inline]
    pub const fn as_secs(self) -> u64 {
        self.nanos / 1_000_000_000
    }

    #[inline]
    pub const fn as_millis(self) -> u64 {
        self.nanos / 1_000_000
    }

    #[inline]
    pub const fn as_micros(self) -> u128 {
        self.nanos as u128 / 1_000
    }

    #[inline]
    pub const fn as_secs_f64(self) -> f64 {
        self.nanos as f64 / 1_000_000_000.0
    }

    #[inline]
    pub const fn as_secs_f32(self) -> f32 {
        self.nanos as f32 / 1_000_000_000.0
    }

    #[inline]
    pub const fn as_core_duration(self) -> CoreDuration {
        CoreDuration::from_nanos(self.nanos)
    }
}

impl From<Duration> for CoreDuration {
    #[inline]
    fn from(value: Duration) -> Self {
        value.as_core_duration()
    }
}

impl From<CoreDuration> for Duration {
    #[inline]
    fn from(value: CoreDuration) -> Self {
        Self {
            nanos: duration_nanos_u64(value),
        }
    }
}

impl PartialEq<CoreDuration> for Duration {
    #[inline]
    fn eq(&self, other: &CoreDuration) -> bool {
        self.as_core_duration() == *other
    }
}

impl PartialEq<Duration> for CoreDuration {
    #[inline]
    fn eq(&self, other: &Duration) -> bool {
        *self == other.as_core_duration()
    }
}

impl PartialOrd<CoreDuration> for Duration {
    #[inline]
    fn partial_cmp(&self, other: &CoreDuration) -> Option<core::cmp::Ordering> {
        self.as_core_duration().partial_cmp(other)
    }
}

impl PartialOrd<Duration> for CoreDuration {
    #[inline]
    fn partial_cmp(&self, other: &Duration) -> Option<core::cmp::Ordering> {
        self.partial_cmp(&other.as_core_duration())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryFromSecsError;

impl fmt::Display for TryFromSecsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid duration")
    }
}

impl Instant {
    #[inline]
    pub fn now() -> Self {
        Self {
            nanos: monotonic_nanos(),
        }
    }

    #[inline]
    pub fn elapsed(&self) -> CoreDuration {
        Instant::now() - *self
    }

    #[inline]
    pub fn duration_since(&self, earlier: Instant) -> CoreDuration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    #[inline]
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<CoreDuration> {
        self.nanos
            .checked_sub(earlier.nanos)
            .map(CoreDuration::from_nanos)
    }

    #[inline]
    pub fn saturating_duration_since(&self, earlier: Instant) -> CoreDuration {
        self.duration_since(earlier)
    }

    #[inline]
    pub fn checked_add(&self, duration: CoreDuration) -> Option<Instant> {
        self.nanos
            .checked_add(duration_nanos_checked(duration)?)
            .map(|nanos| Instant { nanos })
    }

    #[inline]
    pub fn checked_sub(&self, duration: CoreDuration) -> Option<Instant> {
        self.nanos
            .checked_sub(duration_nanos_checked(duration)?)
            .map(|nanos| Instant { nanos })
    }
}

impl Add<CoreDuration> for Instant {
    type Output = Instant;

    #[track_caller]
    fn add(self, other: CoreDuration) -> Instant {
        self.checked_add(other)
            .expect("overflow when adding duration to instant")
    }
}

impl AddAssign<CoreDuration> for Instant {
    #[inline]
    fn add_assign(&mut self, other: CoreDuration) {
        *self = *self + other;
    }
}

impl Sub<CoreDuration> for Instant {
    type Output = Instant;

    #[track_caller]
    fn sub(self, other: CoreDuration) -> Instant {
        self.checked_sub(other)
            .expect("overflow when subtracting duration from instant")
    }
}

impl SubAssign<CoreDuration> for Instant {
    #[inline]
    fn sub_assign(&mut self, other: CoreDuration) {
        *self = *self - other;
    }
}

impl Sub<Instant> for Instant {
    type Output = CoreDuration;

    #[inline]
    fn sub(self, other: Instant) -> CoreDuration {
        self.duration_since(other)
    }
}

impl UtcDateTime {
    #[inline]
    pub fn from_unix_seconds(seconds: u64) -> Self {
        unix_seconds_to_utc_date_time(seconds)
    }
}

impl fmt::Display for UtcDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

#[inline]
pub fn monotonic_nanos() -> u64 {
    unsafe { vcabi::trueos_time_monotonic_nanos() }
}

#[inline]
pub fn monotonic_millis() -> u64 {
    monotonic_nanos() / 1_000_000
}

#[inline]
fn duration_nanos_checked(duration: CoreDuration) -> Option<u64> {
    u64::try_from(duration.as_nanos()).ok()
}

#[inline]
fn duration_nanos_u64(duration: CoreDuration) -> u64 {
    duration_nanos_checked(duration).unwrap_or(u64::MAX)
}

#[inline]
pub fn unix_seconds() -> Option<u64> {
    match unsafe { vcabi::trueos_time_unix_seconds() } {
        0 => None,
        seconds => Some(seconds),
    }
}

#[inline]
pub fn unix_nanos() -> Option<u64> {
    match unsafe { vcabi::trueos_time_unix_nanos() } {
        0 => None,
        nanos => Some(nanos),
    }
}

#[inline]
pub fn utc_date_time() -> Option<UtcDateTime> {
    unix_seconds().map(UtcDateTime::from_unix_seconds)
}

#[inline]
pub fn ntp_current_unix_seconds() -> u64 {
    unsafe { vcabi::trueos_cabi_ntp_current_unix_seconds() }
}

#[inline]
pub fn ntp_utc_date_time() -> Option<UtcDateTime> {
    match ntp_current_unix_seconds() {
        0 => None,
        seconds => Some(UtcDateTime::from_unix_seconds(seconds)),
    }
}

#[inline]
pub fn kernel_date_day_month_year() -> Option<String> {
    let len =
        unsafe { vcabi::trueos_cabi_ntp_kernel_date_day_month_year(core::ptr::null_mut(), 0) };
    if len == 0 {
        return None;
    }
    let mut bytes = vec![0u8; len];
    let got = unsafe {
        vcabi::trueos_cabi_ntp_kernel_date_day_month_year(bytes.as_mut_ptr(), bytes.len())
    };
    if got == 0 {
        return None;
    }
    bytes.truncate(got);
    String::from_utf8(bytes).ok()
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn unix_seconds_to_utc_date_time(seconds: u64) -> UtcDateTime {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = seconds / SECS_PER_DAY;
    let mut rem = seconds % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let lengths = month_lengths(year);
    let mut month_idx = 0usize;
    while month_idx < lengths.len() {
        let len = lengths[month_idx] as u64;
        if days < len {
            return UtcDateTime {
                year,
                month: (month_idx + 1) as u8,
                day: (days + 1) as u8,
                hour,
                minute,
                second,
            };
        }
        days -= len;
        month_idx += 1;
    }

    UtcDateTime {
        year,
        month: 12,
        day: 31,
        hour,
        minute,
        second,
    }
}
