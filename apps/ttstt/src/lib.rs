//! On-demand local speech. Models, lexicalization and inference live in this
//! Blueprint; the OS supplies filesystem, scheduling and PCM playback only.
extern crate alloc;

// Keep startup/failure records visible in both the service slot and global log.
#[macro_export]
macro_rules! log_warn {
    (target: $target:literal; $($args:tt)*) => { $crate::diagnostics::record(true, format_args!($($args)*)); };
}
#[macro_export]
macro_rules! log_info {
    (target: $target:literal; $($args:tt)*) => { $crate::diagnostics::detail(false, format_args!($($args)*)); };
}

pub mod bundle;
pub mod diagnostics;
pub mod http;
pub mod playback;
mod ttstt_kokoro;
pub mod ttstt_service;
