//! Generic 32-bit x86 execution owned by a Blueprint.
//!
//! Running a context occupies a native TRUEOS carrier while this Blueprint's
//! Tokio task awaits its completion. No Windows or application-specific policy
//! crosses this boundary. For [`ExitKind::Exception`], `Exit::detail` is the
//! VM-exit interruption-information field (vector in bits 0..7, interruption
//! type in bits 8..10, error-code-valid in bit 11, valid in bit 31), and
//! `Exit::qualification` low 32 bits are the exception error code only when
//! bit 11 is set. For a valid page fault its high 32 bits are the fault linear
//! address from VM-exit qualification.
//! For [`ExitKind::MemoryViolation`], qualification remains the EPT value.

use alloc::sync::Arc;
use core::fmt;
use core::ops::{BitOr, BitOrAssign};

pub use v::vx86::{DebugRegisters, ExtendedState, Registers, X86_EXTENDED_STATE_BYTES};

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
    Cpuid,
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
            v::vx86::EXIT_OTHER if raw.detail == 10 => ExitKind::Cpuid,
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

fn transfer_chunk_len(guest_va: u32, remaining: usize) -> Result<usize, Error> {
    if remaining == 0 {
        return Ok(0);
    }
    let page_offset = usize::try_from(guest_va).map_err(|_| Error::Invalid)? % v::vx86::PAGE_BYTES;
    let page_remaining = v::vx86::PAGE_BYTES
        .checked_sub(page_offset)
        .ok_or(Error::Invalid)?;
    Ok(remaining.min(v::vx86::TRANSFER_BYTES).min(page_remaining))
}

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
        while transferred < out.len() {
            let address = guest_va
                .checked_add(u32::try_from(transferred).map_err(|_| Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            let remaining = out.len().checked_sub(transferred).ok_or(Error::Invalid)?;
            let len = transfer_chunk_len(address, remaining)?;
            if len == 0 {
                return Err(Error::Invalid);
            }
            let end = transferred.checked_add(len).ok_or(Error::Invalid)?;
            let read =
                v::vx86::address_space_read(self.inner.handle, address, &mut out[transferred..end])
                    .map_err(Error::from_kernel)?;
            transferred = transferred.checked_add(read).ok_or(Error::Invalid)?;
            if read != len {
                break;
            }
        }
        Ok(transferred)
    }

    pub fn write(&self, guest_va: u32, data: &[u8]) -> Result<usize, Error> {
        let mut transferred = 0usize;
        while transferred < data.len() {
            let address = guest_va
                .checked_add(u32::try_from(transferred).map_err(|_| Error::Invalid)?)
                .ok_or(Error::Invalid)?;
            let remaining = data.len().checked_sub(transferred).ok_or(Error::Invalid)?;
            let len = transfer_chunk_len(address, remaining)?;
            if len == 0 {
                return Err(Error::Invalid);
            }
            let end = transferred.checked_add(len).ok_or(Error::Invalid)?;
            let written =
                v::vx86::address_space_write(self.inner.handle, address, &data[transferred..end])
                    .map_err(Error::from_kernel)?;
            transferred = transferred.checked_add(written).ok_or(Error::Invalid)?;
            if written != len {
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

    /// Return the logical context's architectural debug registers.
    pub fn debug_registers(&self) -> Result<DebugRegisters, Error> {
        v::vx86::context_debug_registers(self.handle).map_err(Error::from_kernel)
    }

    /// Replace the logical context's architectural debug registers.
    pub fn set_debug_registers(&mut self, registers: DebugRegisters) -> Result<(), Error> {
        v::vx86::context_set_debug_registers(self.handle, &registers).map_err(Error::from_kernel)
    }

    /// Return the logical context's complete x87/SSE/AVX state.
    pub fn extended_state(&self) -> Result<ExtendedState, Error> {
        v::vx86::context_extended_state(self.handle).map_err(Error::from_kernel)
    }

    /// Replace the logical context's complete x87/SSE/AVX state.
    pub fn set_extended_state(&mut self, state: &ExtendedState) -> Result<(), Error> {
        v::vx86::context_set_extended_state(self.handle, state).map_err(Error::from_kernel)
    }

    /// Admit this context to a native carrier and await its first exit.
    pub async fn run(&mut self) -> Result<Exit, Error> {
        execute_on_carrier(self.handle, false).await
    }

    /// Resume a previously exited context and await its next exit.
    pub async fn resume(&mut self) -> Result<Exit, Error> {
        execute_on_carrier(self.handle, true).await
    }

    /// Synchronous entry for a coordinator already running on its dedicated AP.
    pub fn run_on_current_carrier(&mut self) -> Result<Exit, Error> {
        v::vx86::context_run(self.handle).map(Exit::from).map_err(Error::from_kernel)
    }
    pub fn resume_on_current_carrier(&mut self) -> Result<Exit, Error> {
        v::vx86::context_resume(self.handle).map(Exit::from).map_err(Error::from_kernel)
    }

    /// Execute on a reusable native lane, retaining ordinary exit boundaries.
    pub async fn run_on(&mut self, carrier: &ExecutionCarrier) -> Result<Exit, Error> {
        carrier.execute(self.handle, false).await
    }

    pub async fn resume_on(&mut self, carrier: &ExecutionCarrier) -> Result<Exit, Error> {
        carrier.execute(self.handle, true).await
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

struct CarrierRequest {
    handle: u64,
    resume: bool,
    reply: tokio::sync::oneshot::Sender<Result<v::vx86::Exit, i32>>,
}

/// One native worker shared by serially scheduled logical x86 contexts.
/// Dropping the last sender retires the worker; forced Blueprint teardown is
/// checked between bounded guest slices and while the worker is idle.
pub struct ExecutionCarrier {
    requests: tokio::sync::mpsc::Sender<CarrierRequest>,
}

impl ExecutionCarrier {
    pub fn new() -> Result<Self, Error> {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let job = crate::worker::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_time().build() else {
                return;
            };
            runtime.block_on(carrier_requests(receiver, |handle, resume| {
                if resume { v::vx86::context_resume(handle) }
                else { v::vx86::context_run(handle) }
            }));
        }).map_err(|_| Error::CarrierUnavailable)?;
        // The request channel owns the worker lifetime, not an individual exit.
        drop(job);
        Ok(Self { requests })
    }

    async fn execute(&self, handle: u64, resume: bool) -> Result<Exit, Error> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.requests.send(CarrierRequest { handle, resume, reply }).await
            .map_err(|_| Error::CarrierLost)?;
        response.await.map_err(|_| Error::CarrierLost)?
            .map(Exit::from).map_err(Error::from_kernel)
    }
}

async fn carrier_requests(
    mut requests: tokio::sync::mpsc::Receiver<CarrierRequest>,
    mut execute: impl FnMut(u64, bool) -> Result<v::vx86::Exit, i32>,
) {
    loop {
        if crate::worker::cancellation_requested() { break; }
        let request = tokio::select! {
            request = requests.recv() => match request { Some(request) => request, None => break },
            _ = tokio::time::sleep(core::time::Duration::from_millis(8)) => continue,
        };
        // A cancelled caller must not start a queued context after its owner
        // has dropped it. Already executing slices still retire normally.
        if !request.reply.is_closed() {
            let _ = request.reply.send(execute(request.handle, request.resume));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[tokio::test]
    async fn reusable_carrier_orders_contexts_propagates_errors_and_retires() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let task = tokio::spawn(carrier_requests(receiver, move |handle, resume| {
            observed.lock().unwrap().push((handle, resume));
            if handle == 3 {
                let mut exit = v::vx86::Exit::default();
                exit.kind = v::vx86::EXIT_VMCALL;
                exit.registers.eax = 0x12345678;
                return Ok(exit);
            }
            Err(if handle == 2 { -125 } else { -16 })
        }));
        let carrier = ExecutionCarrier { requests };
        for handle in [1, 2, 1] {
            let error = carrier.execute(handle, handle != 2).await.unwrap_err();
            assert_eq!(error, if handle == 2 { Error::Cancelled } else { Error::Busy });
        }
        let exit = carrier.execute(3, false).await.unwrap();
        assert_eq!(exit.kind, ExitKind::VmCall);
        assert_eq!(exit.registers.eax, 0x12345678);
        drop(carrier);
        tokio::time::timeout(core::time::Duration::from_secs(1), task).await.unwrap().unwrap();
        assert_eq!(*calls.lock().unwrap(), [(1, true), (2, false), (1, true), (3, false)]);
    }

    #[tokio::test]
    async fn reusable_carrier_does_not_enter_abandoned_queued_context() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let (reply, response) = tokio::sync::oneshot::channel();
        requests.send(CarrierRequest { handle: 1, resume: true, reply }).await.unwrap();
        drop(response);
        drop(requests);
        carrier_requests(receiver, |_, _| panic!("abandoned context entered")).await;
    }

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
        assert_eq!(v::vx86::PAGE_BYTES, 4096);
    }

    #[test]
    fn transfer_chunk_stays_within_current_guest_page() {
        assert_eq!(transfer_chunk_len(0x1400_0a80, 2048).unwrap(), 0x580);
    }

    #[test]
    fn transfer_chunk_uses_transport_limit_when_page_allows_it() {
        assert_eq!(
            transfer_chunk_len(0x1400_0000, v::vx86::TRANSFER_BYTES).unwrap(),
            v::vx86::TRANSFER_BYTES
        );
        assert_eq!(
            transfer_chunk_len(0x1400_0000, usize::MAX).unwrap(),
            v::vx86::TRANSFER_BYTES
        );
    }

    #[test]
    fn transfer_chunk_can_shrink_to_one_byte_at_page_end() {
        assert_eq!(transfer_chunk_len(0x1400_0fff, 32).unwrap(), 1);
    }

    #[test]
    fn transfer_chunks_model_the_observed_heap_zero_fill() {
        let mut address = 0x1400_0a80u32;
        let mut remaining = 2048usize;
        let mut chunks = Vec::new();
        while remaining != 0 {
            let len = transfer_chunk_len(address, remaining).unwrap();
            chunks.push((address, len));
            address = address.checked_add(len as u32).unwrap();
            remaining -= len;
        }
        assert_eq!(chunks, [(0x1400_0a80, 0x580), (0x1400_1000, 640)]);
    }
}
