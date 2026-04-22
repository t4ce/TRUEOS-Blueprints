extern crate alloc;

use alloc::string::{String, ToString};

use crate::vcabi;

#[inline]
pub fn ntp_current_unix_seconds() -> Option<u64> {
    match unsafe { vcabi::trueos_cabi_ntp_current_unix_seconds() } {
        0 => None,
        secs => Some(secs),
    }
}

pub fn ntp_kernel_date_day_month_year() -> Option<String> {
    let mut bytes = [0u8; 64];
    let len = unsafe {
        vcabi::trueos_cabi_ntp_kernel_date_day_month_year(bytes.as_mut_ptr(), bytes.len())
    };
    if len == 0 || len > bytes.len() {
        return None;
    }
    core::str::from_utf8(&bytes[..len]).ok().map(ToString::to_string)
}