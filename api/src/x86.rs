//! Generic 32-bit x86 execution owned by a Blueprint.
//!
//! Running a context occupies a native TRUEOS carrier while this Blueprint's
//! Tokio task awaits its completion. No Windows or application-specific policy
//! crosses this boundary. For [`ExitKind::Exception`], `Exit::detail` is the
//! VM-exit interruption-information field (vector in bits 0..7, interruption
//! type in bits 8..10, error-code-valid in bit 11, valid in bit 31), and
//! `Exit::qualification` is the exception error code only when bit 11 is set.
//! For [`ExitKind::MemoryViolation`], qualification remains the EPT value.

use alloc::sync::Arc;
use core::fmt;
use core::ops::{BitOr, BitOrAssign};

pub use v::vx86::Registers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Permissions(u32);

impl Permissions {
    pub const READ: Self = Self(v::vx86::PERMISSION_READ);
    pub const WRITE: Self = Self(v::vx86::PERMISSION_WRITE);
    pub const EXECUTE: Self = Self(v::vx86::PERMISSION_EXECUTE);

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl BitOr for Permissions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitKind {
    VmCall,
    Exception,
    MemoryViolation,
    Halted,
    Cancelled,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exit {
    pub kind: ExitKind,
    pub detail: u32,
    pub qualification: u64,
    pub registers: Registers,
}

impl From<v::vx86::Exit> for Exit {
    fn from(raw: v::vx86::Exit) -> Self {
        let kind = match raw.kind {
            v::vx86::EXIT_VMCALL => ExitKind::VmCall,
            v::vx86::EXIT_EXCEPTION => ExitKind::Exception,
            v::vx86::EXIT_EPT_VIOLATION => ExitKind::MemoryViolation,
            v::vx86::EXIT_HLT => ExitKind::Halted,
            v::vx86::EXIT_CANCELLED => ExitKind::Cancelled,
            _ => ExitKind::Other,
        };
        Self {
            kind,
            detail: raw.detail,
            qualification: raw.qualification,
            registers: raw.registers,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotFound,
    Busy,
    Invalid,
    Unsupported,
    Cancelled,
    CarrierUnavailable,
    CarrierLost,
    Kernel(i32),
}

impl Error {
    const fn from_kernel(code: i32) -> Self {
        match code {
            -2 => Self::NotFound,
            -16 => Self::Busy,
            -22 => Self::Invalid,
            -95 => Self::Unsupported,
            -125 => Self::Cancelled,
            other => Self::Kernel(other),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "x86 execution error: {self:?}")
    }
}

impl core::error::Error for Error {}

struct AddressSpaceInner {
    handle: u64,
}

impl Drop for AddressSpaceInner {
    fn drop(&mut self) {
        let _ = v::vx86::address_space_destroy(self.handle);
    }
}

/// An isolated 32-bit guest-physical address space.
#[derive(Clone)]
pub struct AddressSpace {
    inner: Arc<AddressSpaceInner>,
}

impl AddressSpace {
    pub fn create() -> Result<Self, Error> {
        let handle = v::vx86::address_space_create().map_err(Error::from_kernel)?;
        Ok(Self {
            inner: Arc::new(AddressSpaceInner { handle }),
        })
    }

    pub fn map(&self, guest_va: u32, len: usize, permissions: Permissions) -> Result<(), Error> {
        let len = u32::try_from(len).map_err(|_| Error::Invalid)?;
        v::vx86::address_space_map(self.inner.handle, guest_va, len, permissions.bits())
            .map_err(Error::from_kernel)
    }

    pub fn unmap(&self, guest_va: u32, len: usize) -> Result<(), Error> {
        let len = u32::try_from(len).map_err(|_| Error::Invalid)?;
        v::vx86::address_space_unmap(self.inner.handle, guest_va, len).map_err(Error::from_kernel)
    }

    pub fn read(&self, guest_va: u32, out: &mut [u8]) -> Result<usize, Error> {
        let mut transferred = 0usize;
        for chunk in out.chunks_mut(v::vx86::TRANSFER_BYTES) {
            let address = guest_va
                .checked_add(u32::try_from(transferred).map_err(|_| Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            let read = v::vx86::address_space_read(self.inner.handle, address, chunk)
                .map_err(Error::from_kernel)?;
            transferred = transferred.checked_add(read).ok_or(Error::Invalid)?;
            if read != chunk.len() {
                break;
            }
        }
        Ok(transferred)
    }

    pub fn write(&self, guest_va: u32, data: &[u8]) -> Result<usize, Error> {
        let mut transferred = 0usize;
        for chunk in data.chunks(v::vx86::TRANSFER_BYTES) {
            let address = guest_va
                .checked_add(u32::try_from(transferred).map_err(|_| Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            let written = v::vx86::address_space_write(self.inner.handle, address, chunk)
                .map_err(Error::from_kernel)?;
            transferred = transferred.checked_add(written).ok_or(Error::Invalid)?;
            if written != chunk.len() {
                break;
            }
        }
        Ok(transferred)
    }
}

/// One stable logical x86 register context associated with an address space.
pub struct Context {
    handle: u64,
    _address_space: AddressSpace,
}

impl Context {
    pub fn create(address_space: &AddressSpace, registers: Registers) -> Result<Self, Error> {
        let handle = v::vx86::context_create(address_space.inner.handle, &registers)
            .map_err(Error::from_kernel)?;
        Ok(Self {
            handle,
            _address_space: address_space.clone(),
        })
    }

    pub fn registers(&self) -> Result<Registers, Error> {
        v::vx86::context_registers(self.handle).map_err(Error::from_kernel)
    }

    pub fn set_registers(&mut self, registers: Registers) -> Result<(), Error> {
        v::vx86::context_set_registers(self.handle, &registers).map_err(Error::from_kernel)
    }

    /// Admit this context to a native carrier and await its first exit.
    pub async fn run(&mut self) -> Result<Exit, Error> {
        execute_on_carrier(self.handle, false).await
    }

    /// Resume a previously exited context and await its next exit.
    pub async fn resume(&mut self) -> Result<Exit, Error> {
        execute_on_carrier(self.handle, true).await
    }

    pub fn park(&self) -> Result<(), Error> {
        v::vx86::context_park(self.handle).map_err(Error::from_kernel)
    }

    pub fn cancel(&self) -> Result<(), Error> {
        v::vx86::context_cancel(self.handle).map_err(Error::from_kernel)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let _ = v::vx86::context_cancel(self.handle);
        let _ = v::vx86::context_destroy(self.handle);
    }
}

async fn execute_on_carrier(handle: u64, resume: bool) -> Result<Exit, Error> {
    let completion = crate::worker::spawn(move || {
        if resume {
            v::vx86::context_resume(handle)
        } else {
            v::vx86::context_run(handle)
        }
    })
    .map_err(|_| Error::CarrierUnavailable)?;
    let raw = completion.await.map_err(|_| Error::CarrierLost)?;
    raw.map(Exit::from).map_err(Error::from_kernel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_compose_without_application_policy() {
        let permissions = Permissions::READ | Permissions::WRITE | Permissions::EXECUTE;
        assert_eq!(permissions.bits(), 0b111);
    }

    #[test]
    fn exit_kind_decode_is_stable() {
        let raw = v::vx86::Exit {
            kind: v::vx86::EXIT_EPT_VIOLATION,
            detail: 14,
            qualification: 0x1234,
            registers: Registers::default(),
        };
        let exit = Exit::from(raw);
        assert_eq!(exit.kind, ExitKind::MemoryViolation);
        assert_eq!(exit.detail, 14);
        assert_eq!(exit.qualification, 0x1234);
    }

    #[test]
    fn raw_transfer_limit_matches_vmcall_communication_page() {
        assert_eq!(v::vx86::TRANSFER_BYTES, 4040);
    }
}
