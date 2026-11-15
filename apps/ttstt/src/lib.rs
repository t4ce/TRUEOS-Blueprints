//! On-demand local speech. Models, lexicalization and inference live in this
//! Blueprint; the OS supplies filesystem, scheduling and PCM playback only.
extern crate alloc;

// Preserve the native backend's structured diagnostics on stderr.
#[macro_export]
macro_rules! log_warn {
    (target: $target:literal; $($args:tt)*) => { eprint!($($args)*); };
}
#[macro_export]
macro_rules! log_info {
    (target: $target:literal; $($args:tt)*) => { eprint!($($args)*); };
}

pub mod http;
pub mod playback;
mod ttstt_kokoro;
pub mod ttstt_service;
