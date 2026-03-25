extern crate alloc;

use alloc::string::String;
use std::time::{SystemTime, UNIX_EPOCH};

#[inline]
pub fn ntp_current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[inline]
pub fn kernel_date_day_month_year() -> Option<String> {
    None
}