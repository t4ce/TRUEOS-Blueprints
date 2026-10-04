//! Cooperative Blueprint stop and cleanup before host thread/realm teardown.
use core::marker::PhantomData;

/// The Hull's sole cleanup owner. Declare this guard before the runtime and
/// other process resources so Rust drops it last. A host stop retains guest
/// execution and native-job admission until this guard acknowledges cleanup
/// through the existing Blueprint shutdown boundary.
///
/// Poll `requested` at application safe points and return from the main loop.
/// All guest threads and runtimes must be shut down before dropping the guard.
/// There is no timeout that frees a still-running guest thread's storage.
#[must_use]
pub struct ShutdownGuard {
    _hull_only: PhantomData<*mut ()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownError;

impl ShutdownGuard {
    pub fn register() -> Result<Self, ShutdownError> {
        if control(0) == 0 {
            Ok(Self {
                _hull_only: PhantomData,
            })
        } else {
            Err(ShutdownError)
        }
    }
    pub fn requested(&self) -> Result<bool, ShutdownError> {
        requested()
    }
}

/// Poll from either the Hull owner or one of its native std-thread workers.
pub fn requested() -> Result<bool, ShutdownError> {
    match control(1) {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ShutdownError),
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "trueos")]
        unsafe {
            const REASON: &[u8] = b"cooperative cleanup complete";
            let _ = v::bp_abi::trueos_cabi_blueprint_shutdown(REASON.as_ptr(), REASON.len());
        }
    }
}

fn control(operation: u32) -> i32 {
    #[cfg(target_os = "trueos")]
    {
        unsafe { v::bp_abi::trueos_cabi_blueprint_stop_control_v1(operation) }
    }
    #[cfg(not(target_os = "trueos"))]
    {
        let _ = operation;
        -1
    }
}
