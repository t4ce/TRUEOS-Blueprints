//! Compatibility helpers for the permanent resident tracing collector.
pub use tracing::resident::Resident as KernelSubscriber;

/// Executes with the already-resident collector; no subscriber setup is needed.
#[inline]
pub fn with_default<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// Number of events sent through this Blueprint instance's host log boundary.
#[inline]
pub fn emitted_events() -> usize {
    tracing::resident::emitted_events()
}
