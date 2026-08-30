//! Cooperative Blueprint checkpoint boundary.
//!
//! A host pause request is only a request to prepare. The Blueprint owns the
//! quiescence boundary: it stops new work, drains or cancels in-flight work,
//! drops host capability handles, and then calls [`ready`]. A successful
//! `ready` call is the checkpoint boundary and returns only after this instance
//! has been resumed.

use alloc::string::String;
use core::fmt::Write as _;

use v::bp_abi::{TrueosLifecycleIdentity, TrueosLifecyclePreparePause};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Pause,
    Replicate,
    Migrate,
}

impl Reason {
    const fn from_raw(raw: u32) -> Self {
        match raw {
            2 => Self::Replicate,
            3 => Self::Migrate,
            _ => Self::Pause,
        }
    }
}

/// One operation-scoped request to enter a quiescent checkpoint boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparePause {
    operation: u64,
    pub deadline_ms: u64,
    pub reason: Reason,
}

impl PreparePause {
    #[must_use]
    pub const fn operation(self) -> u64 {
        self.operation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub instance: [u8; 16],
    pub lineage: [u8; 16],
    pub generation: u64,
    pub is_clone: bool,
}

impl Identity {
    #[must_use]
    pub fn instance_guid(self) -> String {
        format_uuid(&self.instance)
    }

    #[must_use]
    pub fn lineage_guid(self) -> String {
        format_uuid(&self.lineage)
    }
}

/// The identity observed immediately after the Ready boundary resumes.
pub type Resume = Identity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    StaleOperation,
    IdentityUnavailable,
}

/// Poll for the host's current PreparePause request.
///
/// Repeated polls return the same operation until it is acknowledged or its
/// deadline expires. An acknowledgement for an old operation is rejected.
#[must_use]
pub fn poll_prepare_pause() -> Option<PreparePause> {
    let mut raw = TrueosLifecyclePreparePause::default();
    let status = unsafe { v::bp_abi::trueos_cabi_lifecycle_poll(&mut raw as *mut _) };
    if status != 1 || raw.operation == 0 {
        return None;
    }
    Some(PreparePause {
        operation: raw.operation,
        deadline_ms: raw.deadline_ms,
        reason: Reason::from_raw(raw.reason),
    })
}

/// Declare the app quiescent and checkpoint at this exact call boundary.
///
/// On success this function returns after Resume, never before the checkpoint.
/// The returned identity is host-issued and may differ from the pre-pause
/// identity when the checkpoint was resumed as a clone.
pub fn ready(prepare: PreparePause, checkpoint_version: u64) -> Result<Resume, Error> {
    let status =
        unsafe { v::bp_abi::trueos_cabi_lifecycle_ready(prepare.operation, checkpoint_version) };
    if status != 0 {
        return Err(Error::StaleOperation);
    }
    current_identity().ok_or(Error::IdentityUnavailable)
}

/// Declare the app quiescent and attach a compact, application-defined
/// checkpoint for fresh-Hull replication. Same-Hull pause/resume still returns
/// from this exact call; a live kernel replacement launches a fresh Blueprint
/// which retrieves the payload with [`restore_checkpoint`].
pub fn ready_with_checkpoint(
    prepare: PreparePause,
    checkpoint_version: u64,
    checkpoint: &[u8],
) -> Result<Resume, Error> {
    let status = unsafe {
        v::bp_abi::trueos_cabi_lifecycle_ready_with_checkpoint(
            prepare.operation,
            checkpoint_version,
            checkpoint.as_ptr(),
            checkpoint.len(),
        )
    };
    if status != 0 {
        return Err(Error::StaleOperation);
    }
    current_identity().ok_or(Error::IdentityUnavailable)
}

/// Consume the application checkpoint supplied to a fresh replicated launch.
/// Returns its application-defined version and exact byte length. A normal
/// launch has no payload and returns `None`.
pub fn restore_checkpoint(out: &mut [u8]) -> Option<(u64, usize)> {
    let mut version = 0u64;
    let len = unsafe {
        v::bp_abi::trueos_cabi_lifecycle_checkpoint_restore(
            out.as_mut_ptr(),
            out.len(),
            &mut version as *mut _,
        )
    };
    (len > 0).then_some((version, len as usize))
}

#[must_use]
pub fn current_identity() -> Option<Identity> {
    let mut raw = TrueosLifecycleIdentity::default();
    let status = unsafe { v::bp_abi::trueos_cabi_lifecycle_identity(&mut raw as *mut _) };
    (status == 0).then_some(Identity {
        instance: raw.instance,
        lineage: raw.lineage,
        generation: raw.generation,
        is_clone: raw.flags & 1 != 0,
    })
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().copied().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{byte:02x}");
    }
    out
}
