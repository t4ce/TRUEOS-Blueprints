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

    /// Execute on a reusable native lane, retaining ordinary exit boundaries.
    pub async fn run_on(&mut self, carrier: &ExecutionCarrier) -> Result<Exit, Error> {
        carrier.execute(self.handle, false).await
    }

    pub async fn resume_on(&mut self, carrier: &ExecutionCarrier) -> Result<Exit, Error> {
        carrier.execute(self.handle, true).await
    }

    /// Time request delivery, native execution, and reply delivery separately.
    pub async fn execute_on_measured(
        &mut self,
        carrier: &ExecutionCarrier,
        resume: bool,
    ) -> Result<(Exit, CarrierTiming), Error> {
        carrier.execute_measured(self.handle, resume).await
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

#[derive(Clone, Copy, Debug, Default)]
pub struct CarrierTiming {
    pub request_ns: u64,
    /// Includes kernel setup/cleanup as well as guest execution.
    pub native_ns: u64,
    pub reply_ns: u64,
}

struct CarrierReply {
    result: Result<v::vx86::Exit, i32>,
    timing: Option<(tokio::time::Instant, tokio::time::Instant)>,
}

struct CarrierRequest {
    handle: u64,
    resume: bool,
    measured: bool,
    generation: u64,
}

// One-time startup markers distinguish a worker startup stall from guest code.
fn carrier_boot(_message: &'static str) {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    crate::logl::log_record(crate::logl::level::IMPORTANT, "xpapp-carrier", _message);
}

struct CompletionState {
    generation: u64,
    waiting: bool,
    reply: Option<CarrierReply>,
    waker: Option<core::task::Waker>,
    closed: bool,
}

struct Completion {
    state: std::sync::Mutex<CompletionState>,
}

impl Completion {
    fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(CompletionState {
                generation: 0,
                waiting: false,
                reply: None,
                waker: None,
                closed: false,
            }),
        }
    }

    fn start(&self) -> Result<u64, Error> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err(Error::CarrierLost);
        }
        let generation = state.generation.checked_add(1).ok_or(Error::CarrierLost)?;
        state.generation = generation;
        state.waiting = true;
        state.reply = None;
        Ok(generation)
    }

    fn cancel(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        if state.generation == generation {
            state.waiting = false;
            state.reply = None;
            let waker = state.waker.take();
            drop(state);
            drop(waker);
        }
    }

    fn is_waiting(&self, generation: u64) -> bool {
        let state = self.state.lock().unwrap();
        !state.closed && state.waiting && state.generation == generation
    }

    fn publish(&self, generation: u64, reply: CarrierReply) {
        let mut state = self.state.lock().unwrap();
        let waker = if state.waiting && state.generation == generation {
            state.reply = Some(reply);
            state.waker.take()
        } else {
            None
        };
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn poll_reply(
        &self,
        generation: u64,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<CarrierReply, Error>> {
        use core::task::Poll;
        let mut state = self.state.lock().unwrap();
        if state.generation == generation {
            if let Some(reply) = state.reply.take() {
                return Poll::Ready(Ok(reply));
            }
        }
        if state.closed {
            return Poll::Ready(Err(Error::CarrierLost));
        }
        // Only the Waker crosses to the native carrier, as it did with the
        // old oneshot. Tokio Notify instead links its Notified future into
        // an intrusive list: under block_on that node can live on a Hull
        // stack which is not mapped into the carrier's execution realm.
        let old = if state
            .waker
            .as_ref()
            .is_some_and(|w| w.will_wake(cx.waker()))
        {
            None
        } else {
            state.waker.replace(cx.waker().clone())
        };
        drop(state);
        drop(old);
        Poll::Pending
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        let waker = state.waker.take();
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct CompletionWait<'a> {
    completion: &'a Completion,
    generation: u64,
}

impl Drop for CompletionWait<'_> {
    fn drop(&mut self) {
        self.completion.cancel(self.generation);
    }
}

struct CarrierEndpoint {
    requests: tokio::sync::mpsc::Receiver<CarrierRequest>,
    completion: Arc<Completion>,
}

impl Drop for CarrierEndpoint {
    fn drop(&mut self) {
        self.completion.close();
    }
}

/// One native worker shared by serially scheduled logical x86 contexts.
/// Dropping the last sender retires the worker; forced Blueprint teardown is
/// checked between bounded guest slices and while the worker is idle.
pub struct ExecutionCarrier {
    requests: tokio::sync::mpsc::Sender<CarrierRequest>,
    submit_lock: tokio::sync::Mutex<()>,
    completion: Arc<Completion>,
}

impl ExecutionCarrier {
    pub fn new() -> Result<Self, Error> {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let job = crate::worker::spawn(move || {
            carrier_boot("XPAPP CARRIER worker-enter");
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            else {
                return;
            };
            carrier_boot("XPAPP CARRIER runtime-ready");
            runtime.block_on(carrier_requests(
                endpoint,
                |handle, resume| {
                    if resume {
                        v::vx86::context_resume(handle)
                    } else {
                        v::vx86::context_run(handle)
                    }
                },
                tokio::time::Instant::now,
            ));
        })
        .map_err(|_| Error::CarrierUnavailable)?;
        // The request channel owns the worker lifetime, not an individual exit.
        drop(job);
        Ok(Self {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        })
    }

    async fn execute(&self, handle: u64, resume: bool) -> Result<Exit, Error> {
        self.submit(handle, resume, false)
            .await?
            .result
            .map(Exit::from)
            .map_err(Error::from_kernel)
    }

    async fn execute_measured(
        &self,
        handle: u64,
        resume: bool,
    ) -> Result<(Exit, CarrierTiming), Error> {
        let started = tokio::time::Instant::now();
        let response = self.submit(handle, resume, true).await?;
        let received = tokio::time::Instant::now();
        let (accepted, finished) = response
            .timing
            .expect("measured carrier reply lacks timestamps");
        let timing = CarrierTiming {
            request_ns: accepted.saturating_duration_since(started).as_nanos() as u64,
            native_ns: finished.saturating_duration_since(accepted).as_nanos() as u64,
            reply_ns: received.saturating_duration_since(finished).as_nanos() as u64,
        };
        response
            .result
            .map(|raw| (Exit::from(raw), timing))
            .map_err(Error::from_kernel)
    }

    async fn submit(
        &self,
        handle: u64,
        resume: bool,
        measured: bool,
    ) -> Result<CarrierReply, Error> {
        let _serial = match self.submit_lock.try_lock() {
            Ok(guard) => guard,
            // The contended mutex future also has an intrusive waiter. Put
            // that rare waiter in shared heap memory, never a realm's stack.
            Err(_) => alloc::boxed::Box::pin(self.submit_lock.lock()).await,
        };
        let generation = self.completion.start()?;
        let _wait = CompletionWait {
            completion: &self.completion,
            generation,
        };
        let request = CarrierRequest {
            handle,
            resume,
            measured,
            generation,
        };
        match self.requests.try_send(request) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(request)) => {
                // A cancelled queued call can temporarily occupy the channel.
                // Its send-permit waiter must also stay in shared heap memory.
                alloc::boxed::Box::pin(self.requests.send(request))
                    .await
                    .map_err(|_| Error::CarrierLost)?;
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                return Err(Error::CarrierLost);
            }
        }
        if generation == 1 {
            carrier_boot("XPAPP CARRIER first-request-sent");
        }
        let reply = core::future::poll_fn(|cx| self.completion.poll_reply(generation, cx)).await;
        if generation == 1 {
            carrier_boot("XPAPP CARRIER first-reply-received");
        }
        reply
    }
}

async fn carrier_requests(
    mut endpoint: CarrierEndpoint,
    mut execute: impl FnMut(u64, bool) -> Result<v::vx86::Exit, i32>,
    mut now: impl FnMut() -> tokio::time::Instant,
) {
    loop {
        if crate::worker::cancellation_requested() {
            break;
        }
        let request = tokio::select! {
            request = endpoint.requests.recv() => match request { Some(request) => request, None => break },
            _ = tokio::time::sleep(core::time::Duration::from_millis(8)) => continue,
        };
        // A cancelled caller must not start a queued context after its owner
        // has dropped it. Already executing slices still retire normally.
        if request.generation == 1 {
            carrier_boot("XPAPP CARRIER first-request-accepted");
        }
        if endpoint.completion.is_waiting(request.generation) {
            let accepted = request.measured.then(&mut now);
            let result = execute(request.handle, request.resume);
            let timing = accepted.map(|accepted| (accepted, now()));
            if request.generation == 1 {
                carrier_boot("XPAPP CARRIER first-native-returned");
            }
            endpoint
                .completion
                .publish(request.generation, CarrierReply { result, timing });
            if request.generation == 1 {
                carrier_boot("XPAPP CARRIER first-reply-published");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    // The API's abort export references these kernel services even in a host
    // test binary. Carrier fixtures must never need either service.
    #[cfg(not(target_os = "trueos"))]
    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

    #[cfg(not(target_os = "trueos"))]
    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_blueprint_shutdown(_bytes: *const u8, _len: usize) -> i32 {
        std::process::exit(1)
    }

    #[tokio::test]
    async fn reusable_carrier_orders_contexts_propagates_errors_and_retires() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let task = tokio::spawn(carrier_requests(
            endpoint,
            move |handle, resume| {
                observed.lock().unwrap().push((handle, resume));
                if handle == 3 {
                    let mut exit = v::vx86::Exit::default();
                    exit.kind = v::vx86::EXIT_VMCALL;
                    exit.registers.eax = 0x12345678;
                    return Ok(exit);
                }
                Err(if handle == 2 { -125 } else { -16 })
            },
            || panic!("ordinary execution sampled its clock"),
        ));
        let carrier = ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        };
        for handle in [1, 2, 1] {
            let error = carrier.execute(handle, handle != 2).await.unwrap_err();
            assert_eq!(
                error,
                if handle == 2 {
                    Error::Cancelled
                } else {
                    Error::Busy
                }
            );
        }
        let exit = carrier.execute(3, false).await.unwrap();
        assert_eq!(exit.kind, ExitKind::VmCall);
        assert_eq!(exit.registers.eax, 0x12345678);
        drop(carrier);
        tokio::time::timeout(core::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [(1, true), (2, false), (1, true), (3, false)]
        );
    }

    #[tokio::test]
    async fn reusable_carrier_measures_only_requested_executions() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let samples = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&samples);
        let epoch = tokio::time::Instant::now();
        let task = tokio::spawn(carrier_requests(
            endpoint,
            |handle, resume| {
                assert_eq!(resume, handle != 1);
                if handle == 3 {
                    return Err(-125);
                }
                let mut exit = v::vx86::Exit::default();
                exit.kind = v::vx86::EXIT_VMCALL;
                exit.registers.eax = handle as u32;
                Ok(exit)
            },
            move || {
                let sample = observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                epoch + core::time::Duration::from_nanos(sample as u64 * 250)
            },
        ));
        let carrier = ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        };
        assert_eq!(carrier.execute(1, false).await.unwrap().registers.eax, 1);
        assert_eq!(samples.load(std::sync::atomic::Ordering::Relaxed), 0);
        let (exit, timing) = carrier.execute_measured(2, true).await.unwrap();
        assert_eq!(exit.registers.eax, 2);
        assert_eq!(timing.native_ns, 250);
        assert_eq!(samples.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(
            carrier.execute_measured(3, true).await.unwrap_err(),
            Error::Cancelled
        );
        assert_eq!(samples.load(std::sync::atomic::Ordering::Relaxed), 4);
        assert_eq!(carrier.execute(4, true).await.unwrap().registers.eax, 4);
        assert_eq!(samples.load(std::sync::atomic::Ordering::Relaxed), 4);
        drop(carrier);
        tokio::time::timeout(core::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn reusable_carrier_does_not_enter_abandoned_queued_context() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let generation = completion.start().unwrap();
        requests
            .send(CarrierRequest {
                handle: 1,
                resume: true,
                measured: true,
                generation,
            })
            .await
            .unwrap();
        completion.cancel(generation);
        drop(requests);
        carrier_requests(
            CarrierEndpoint {
                requests: receiver,
                completion,
            },
            |_, _| panic!("abandoned context entered"),
            || panic!("abandoned context sampled its clock"),
        )
        .await;
    }

    #[tokio::test]
    async fn reusable_carrier_recovers_after_queued_caller_is_abandoned() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let carrier = Arc::new(ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        });
        let abandoned = {
            let carrier = Arc::clone(&carrier);
            tokio::spawn(async move { carrier.execute(1, false).await })
        };
        tokio::time::timeout(core::time::Duration::from_secs(1), async {
            while carrier.requests.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        abandoned.abort();
        assert!(abandoned.await.is_err());

        let next = {
            let carrier = Arc::clone(&carrier);
            tokio::spawn(async move { carrier.execute(2, false).await })
        };
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let worker = tokio::spawn(carrier_requests(
            endpoint,
            move |handle, _| {
                observed.lock().unwrap().push(handle);
                let mut exit = v::vx86::Exit::default();
                exit.registers.eax = handle as u32;
                Ok(exit)
            },
            tokio::time::Instant::now,
        ));
        assert_eq!(next.await.unwrap().unwrap().registers.eax, 2);
        drop(carrier);
        tokio::time::timeout(core::time::Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), [2]);
    }

    #[tokio::test]
    async fn reusable_carrier_discards_late_reply_after_in_flight_abandonment() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let carrier = Arc::new(ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        });
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            runtime.block_on(carrier_requests(
                endpoint,
                move |handle, _| {
                    if handle == 1 {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }
                    let mut exit = v::vx86::Exit::default();
                    exit.registers.eax = handle as u32;
                    Ok(exit)
                },
                tokio::time::Instant::now,
            ));
        });
        let abandoned = {
            let carrier = Arc::clone(&carrier);
            tokio::spawn(async move { carrier.execute(1, false).await })
        };
        tokio::time::timeout(
            core::time::Duration::from_secs(1),
            tokio::task::spawn_blocking(move || entered_rx.recv().unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        abandoned.abort();
        assert!(abandoned.await.is_err());
        let next = {
            let carrier = Arc::clone(&carrier);
            tokio::spawn(async move { carrier.execute(2, false).await })
        };
        tokio::time::timeout(core::time::Duration::from_secs(1), async {
            while carrier.requests.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(core::time::Duration::from_secs(1), next)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .registers
                .eax,
            2
        );
        drop(carrier);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn reusable_carrier_serializes_concurrent_cross_thread_callers() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let carrier = Arc::new(ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        });
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            runtime.block_on(carrier_requests(
                endpoint,
                |handle, _| {
                    let mut exit = v::vx86::Exit::default();
                    exit.registers.eax = handle as u32;
                    Ok(exit)
                },
                tokio::time::Instant::now,
            ));
        });
        let mut callers = Vec::new();
        for handle in 0..32 {
            let carrier = Arc::clone(&carrier);
            callers.push(tokio::spawn(
                async move { carrier.execute(handle, false).await },
            ));
        }
        let mut results = Vec::new();
        for caller in callers {
            results.push(
                tokio::time::timeout(core::time::Duration::from_secs(1), caller)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .registers
                    .eax,
            );
        }
        results.sort_unstable();
        assert_eq!(results, (0..32).collect::<Vec<_>>());
        drop(carrier);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn reusable_carrier_reports_dropped_worker_to_pending_caller() {
        let (requests, receiver) = tokio::sync::mpsc::channel(1);
        let completion = Arc::new(Completion::new());
        let endpoint = CarrierEndpoint {
            requests: receiver,
            completion: Arc::clone(&completion),
        };
        let worker = tokio::spawn(async move {
            let _endpoint = endpoint;
            core::future::pending::<()>().await;
        });
        let carrier = Arc::new(ExecutionCarrier {
            requests,
            submit_lock: tokio::sync::Mutex::new(()),
            completion,
        });
        let pending = {
            let carrier = Arc::clone(&carrier);
            tokio::spawn(async move { carrier.execute(1, false).await })
        };
        tokio::time::timeout(core::time::Duration::from_secs(1), async {
            while carrier.requests.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        worker.abort();
        assert!(worker.await.is_err());
        assert_eq!(
            tokio::time::timeout(core::time::Duration::from_secs(1), pending)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            Error::CarrierLost
        );
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
