use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Interest, Token};

const READY_READABLE: u8 = 0b0000_0001;
const READY_WRITABLE: u8 = 0b0000_0010;
const READY_ERROR: u8 = 0b0000_0100;
const READY_READ_CLOSED: u8 = 0b0000_1000;
const READY_WRITE_CLOSED: u8 = 0b0001_0000;
const INTEREST_EDGE_MANAGED: u8 = 0b1000_0000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TrueosMioReadyEvent {
    token: usize,
    readiness: u8,
    reserved: [u8; 7],
}

const _: () = assert!(std::mem::size_of::<TrueosMioReadyEvent>() == 16);

extern "C" {
    fn trueos_mio_selector_register_fd(
        selector_id: usize,
        fd: RawFd,
        token: usize,
        interests: u8,
    ) -> i32;
    fn trueos_mio_selector_deregister_fd(selector_id: usize, fd: RawFd) -> i32;
    fn trueos_mio_selector_poll_fds(
        selector_id: usize,
        out_events: *mut TrueosMioReadyEvent,
        out_cap: usize,
        timeout_nanos: u64,
    ) -> usize;
    fn trueos_mio_selector_wake(selector_id: usize) -> i32;
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    token: Token,
    readiness: Ready,
}

pub type Events = Vec<Event>;

#[derive(Clone, Copy, Debug, Default)]
struct Ready {
    readable: bool,
    writable: bool,
    error: bool,
    read_closed: bool,
    write_closed: bool,
}

#[derive(Debug, Default)]
struct SelectorState {
    registrations: Mutex<HashMap<RawFd, bool>>,
    ready: Mutex<VecDeque<Event>>,
}

static NEXT_SELECTOR_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct Selector {
    id: usize,
    state: Arc<SelectorState>,
}

impl Selector {
    pub fn new() -> io::Result<Selector> {
        Ok(Selector {
            id: NEXT_SELECTOR_ID.fetch_add(1, Ordering::Relaxed),
            state: Arc::new(SelectorState::default()),
        })
    }

    pub fn try_clone(&self) -> io::Result<Selector> {
        Ok(Selector {
            id: self.id,
            state: Arc::clone(&self.state),
        })
    }

    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();
        if self.drain_waker_events(events) {
            return Ok(());
        }

        let remaining = events.capacity();
        if remaining == 0 {
            return Ok(());
        }
        let mut raw_events = vec![TrueosMioReadyEvent::default(); remaining];
        let timeout_nanos = timeout.map_or(u64::MAX, duration_to_nanos);
        let count = unsafe {
            trueos_mio_selector_poll_fds(
                self.id,
                raw_events.as_mut_ptr(),
                raw_events.len(),
                timeout_nanos,
            )
        }
        .min(raw_events.len());

        events.extend(raw_events[..count].iter().map(|event| Event {
            token: Token(event.token),
            readiness: Ready::from_bits(event.readiness),
        }));

        // An application Waker queues its token before waking the native wait.
        // If the wake won the race with a socket completion, preserve both up
        // to the caller-provided event capacity.
        self.drain_waker_events(events);
        Ok(())
    }

    pub fn register(&self, fd: RawFd, token: Token, interests: Interest) -> io::Result<()> {
        self.register_with_mode(fd, token, interests, false)
    }

    pub(crate) fn register_internal(
        &self,
        fd: RawFd,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        self.register_with_mode(fd, token, interests, true)
    }

    fn register_with_mode(
        &self,
        fd: RawFd,
        token: Token,
        interests: Interest,
        edge_managed: bool,
    ) -> io::Result<()> {
        let mut registrations = self.state.registrations.lock().unwrap();
        if registrations.contains_key(&fd) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "I/O source already registered with this TRUEOS selector",
            ));
        }
        status_result(unsafe {
            trueos_mio_selector_register_fd(
                self.id,
                fd,
                token.0,
                interest_bits(interests, edge_managed),
            )
        })?;
        registrations.insert(fd, edge_managed);
        Ok(())
    }

    pub fn reregister(&self, fd: RawFd, token: Token, interests: Interest) -> io::Result<()> {
        let edge_managed = self
            .state
            .registrations
            .lock()
            .unwrap()
            .get(&fd)
            .copied()
            .ok_or(io::ErrorKind::NotFound)?;
        self.reregister_with_mode(fd, token, interests, edge_managed)
    }

    pub(crate) fn reregister_internal(
        &self,
        fd: RawFd,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        self.reregister_with_mode(fd, token, interests, true)
    }

    fn reregister_with_mode(
        &self,
        fd: RawFd,
        token: Token,
        interests: Interest,
        edge_managed: bool,
    ) -> io::Result<()> {
        let mut registrations = self.state.registrations.lock().unwrap();
        let Some(mode) = registrations.get_mut(&fd) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        status_result(unsafe {
            trueos_mio_selector_register_fd(
                self.id,
                fd,
                token.0,
                interest_bits(interests, edge_managed),
            )
        })?;
        *mode = edge_managed;
        Ok(())
    }

    pub fn deregister(&self, fd: RawFd) -> io::Result<()> {
        let mut registrations = self.state.registrations.lock().unwrap();
        if !registrations.contains_key(&fd) {
            return Err(io::ErrorKind::NotFound.into());
        }
        status_result(unsafe { trueos_mio_selector_deregister_fd(self.id, fd) })?;
        registrations.remove(&fd);
        Ok(())
    }

    pub(crate) fn push_waker_event(&self, token: Token) {
        let mut ready = self.state.ready.lock().unwrap();
        if !ready.iter().any(|event| event.token == token) {
            ready.push_back(Event {
                token,
                readiness: Ready {
                    readable: true,
                    ..Ready::default()
                },
            });
        }
    }

    pub(crate) fn wake_native(&self) -> io::Result<()> {
        status_result(unsafe { trueos_mio_selector_wake(self.id) })
    }

    fn drain_waker_events(&self, events: &mut Events) -> bool {
        let mut ready = self.state.ready.lock().unwrap();
        while events.len() < events.capacity() {
            let Some(event) = ready.pop_front() else {
                break;
            };
            events.push(event);
        }
        !events.is_empty()
    }

    cfg_io_source! {
        #[cfg(debug_assertions)]
        pub fn id(&self) -> usize {
            self.id
        }
    }
}

fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

fn interest_bits(interests: Interest, edge_managed: bool) -> u8 {
    let mut bits = 0;
    if interests.is_readable() {
        bits |= READY_READABLE;
    }
    if interests.is_writable() {
        bits |= READY_WRITABLE;
    }
    if edge_managed {
        bits |= INTEREST_EDGE_MANAGED;
    }
    bits
}

fn status_result(status: i32) -> io::Result<()> {
    match status {
        0 => Ok(()),
        -1 => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "TRUEOS selector only accepts native network descriptors",
        )),
        -4 => Err(io::ErrorKind::InvalidInput.into()),
        -5 => Err(io::ErrorKind::NotFound.into()),
        -9 => Err(io::ErrorKind::OutOfMemory.into()),
        _ => Err(io::Error::new(
            io::ErrorKind::Other,
            "TRUEOS native selector operation failed",
        )),
    }
}

impl Ready {
    fn from_bits(bits: u8) -> Self {
        Self {
            readable: bits & READY_READABLE != 0,
            writable: bits & READY_WRITABLE != 0,
            error: bits & READY_ERROR != 0,
            read_closed: bits & READY_READ_CLOSED != 0,
            write_closed: bits & READY_WRITE_CLOSED != 0,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub mod event {
    use std::fmt;

    use crate::Token;
    use crate::sys::Event;

    pub fn token(event: &Event) -> Token {
        event.token
    }

    pub fn is_readable(event: &Event) -> bool {
        event.readiness.readable
    }

    pub fn is_writable(event: &Event) -> bool {
        event.readiness.writable
    }

    pub fn is_error(event: &Event) -> bool {
        event.readiness.error
    }

    pub fn is_read_closed(event: &Event) -> bool {
        event.readiness.read_closed
    }

    pub fn is_write_closed(event: &Event) -> bool {
        event.readiness.write_closed
    }

    pub fn is_priority(_: &Event) -> bool {
        false
    }

    pub fn is_aio(_: &Event) -> bool {
        false
    }

    pub fn is_lio(_: &Event) -> bool {
        false
    }

    pub fn debug_details(f: &mut fmt::Formatter<'_>, event: &Event) -> fmt::Result {
        f.debug_struct("trueos_event")
            .field("token", &event.token)
            .field("readable", &event.readiness.readable)
            .field("writable", &event.readiness.writable)
            .field("error", &event.readiness.error)
            .field("read_closed", &event.readiness.read_closed)
            .field("write_closed", &event.readiness.write_closed)
            .finish()
    }
}
