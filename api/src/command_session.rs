//! Backend-neutral command sessions for Shell3 and Blueprint launches.
//!
//! Native launch scripts, including Tab mode selection, are passed unchanged to
//! a dedicated Shell3 session. There is no executable/argv or Unix shell context.
//! Each returned transport owns a separate session lifetime;
//! its controls must never address a reused slot by its numeric ID alone.
//! This contract does not claim that the retired Shell2 frontend implements it.

use alloc::boxed::Box;
use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Clone, Debug)]
pub struct Launch {
    pub script: String,
    /// None requests captured command I/O without terminal formatting.
    pub terminal: Option<Geometry>,
    pub stdin_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    Closed,
    InvalidRequest,
    Busy,
    Backend(i32),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "command session: {self:?}")
    }
}
impl core::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    /// A backend reports this only after draining all captured output. A shell
    /// prompt, transport failure, or expired lease is never a successful exit.
    Exited(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Read {
    pub len: usize,
    pub stream: Stream,
    pub state: State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
}

/// Operations are nonblocking. A zero-byte Running read means try again later;
/// partial writes are permitted, and callers retain the unsent suffix.
/// Closing stdin is distinct from cancelling or closing the session.
pub trait Transport: Send {
    fn read(&mut self, out: &mut [u8]) -> Result<Read, Error>;
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Error>;
    fn close_stdin(&mut self) -> Result<(), Error>;
    fn resize(&mut self, geometry: Geometry) -> Result<(), Error>;
    fn signal(&mut self, signal: Signal) -> Result<(), Error>;
    /// Release this exact session lifetime, cancelling any unfinished launch.
    /// Must be idempotent and must not block waiting for a worker.
    fn close(&mut self);
}

pub trait Backend: Send + Sync {
    /// Admit a launch atomically, or fail without leaving an orphaned session.
    fn open(&self, launch: Launch) -> Result<Box<dyn Transport>, Error>;
}

/// Owns the session and guarantees release even on error or cancellation.
pub struct Session {
    transport: Box<dyn Transport>,
}
impl Session {
    pub fn open(backend: &dyn Backend, launch: Launch) -> Result<Self, Error> {
        if launch.script.contains('\0')
            || launch
                .terminal
                .is_some_and(|size| size.rows == 0 || size.cols == 0)
        {
            return Err(Error::InvalidRequest);
        }
        Ok(Self {
            transport: backend.open(launch)?,
        })
    }
    pub fn transport(&mut self) -> &mut dyn Transport {
        &mut *self.transport
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.transport.close();
    }
}
