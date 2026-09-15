use core::num::{NonZeroU32, NonZeroU64};

use super::DisplayHandle;

/// Raw display handle for the TRUEOS UI4 service.
///
/// `connection` is a host-issued, Blueprint-scoped graphics connection. It
/// identifies the UI4 service connection; it is not an output identifier.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrueosDisplayHandle {
    /// Host-issued UI4 graphics connection capability.
    pub connection: NonZeroU64,
}

impl TrueosDisplayHandle {
    /// Create a TRUEOS UI4 display handle from a live graphics connection.
    pub fn new(connection: NonZeroU64) -> Self {
        Self { connection }
    }
}

/// Raw window handle for a TRUEOS UI4 visual frame.
///
/// The host validates that this frame belongs to the display connection before
/// it admits a graphics operation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrueosWindowHandle {
    /// Generation-bearing UI4 visual frame identifier.
    pub window: NonZeroU32,
}

impl TrueosWindowHandle {
    /// Create a TRUEOS UI4 window handle from a live visual frame identifier.
    pub fn new(window: NonZeroU32) -> Self {
        Self { window }
    }
}

impl DisplayHandle<'static> {
    /// Create a TRUEOS UI4 display handle.
    ///
    /// The connection is an owned integer capability, so this handle borrows
    /// no storage and is safe to construct.
    pub fn trueos(connection: NonZeroU64) -> Self {
        // SAFETY: the display handle borrows no memory.
        unsafe { Self::borrow_raw(TrueosDisplayHandle::new(connection).into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawDisplayHandle, RawWindowHandle};

    #[test]
    fn trueos_handles_preserve_the_host_issued_identifiers() {
        let display = TrueosDisplayHandle::new(NonZeroU64::new(7).unwrap());
        let window = TrueosWindowHandle::new(NonZeroU32::new(9).unwrap());
        assert_eq!(RawDisplayHandle::Trueos(display), display.into());
        assert_eq!(RawWindowHandle::Trueos(window), window.into());
    }
}
