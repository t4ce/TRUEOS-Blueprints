//! App-local C clock callback for native parser deadlines.
//! Uses the same infallible monotonic clock as Rust; no Unix clock IDs or
//! wall-clock fallback cross this interface.

#[unsafe(no_mangle)]
pub extern "C" fn trueos_blueprint_c_monotonic_nanos() -> u64 {
    crate::vclock::monotonic_nanos()
}
