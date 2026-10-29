//! Backend-neutral command sessions for Shell3 and Blueprint launches.
//!
//! Captured sessions accept native command lines without terminal presentation;
//! explicit terminal sessions retain Shell3's keyboard and mode controls.
//! There is no executable/argv or Unix shell context.
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

/// Live backend for the versioned Blueprint-owned Shell3 session ABI.
/// Other platforms can type-check the contract but cannot open kernel sessions.
pub struct KernelBackend;

#[cfg(not(target_os = "trueos"))]
impl Backend for KernelBackend {
    fn open(&self, _launch: Launch) -> Result<Box<dyn Transport>, Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(target_os = "trueos")]
mod kernel {
    use super::*;
    use v::vcabi;

    struct KernelTransport {
        id: u64,
        closed: bool,
    }
    fn error(code: i64) -> Error {
        match code {
            -1 => Error::InvalidRequest,
            -3 => Error::Closed,
            -4 => Error::Busy,
            code => Error::Backend(code as i32),
        }
    }
    impl Backend for KernelBackend {
        fn open(&self, launch: Launch) -> Result<Box<dyn Transport>, Error> {
            // The v1 ABI reserves (0, 0) for captured command I/O. Do not
            // silently turn an absent terminal into an interactive 80x24 view.
            let size = launch.terminal.unwrap_or(Geometry { rows: 0, cols: 0 });
            let id = unsafe {
                vcabi::trueos_cabi_shell3_session_open_v1(
                    launch.script.as_ptr(),
                    launch.script.len(),
                    size.cols.into(),
                    size.rows.into(),
                    u32::from(launch.stdin_open),
                )
            };
            if id <= 0 {
                return Err(error(id));
            }
            Ok(Box::new(KernelTransport {
                id: id as u64,
                closed: false,
            }))
        }
    }
    impl KernelTransport {
        fn control(&mut self, op: u32, size: Geometry) -> Result<(), Error> {
            if self.closed {
                return Err(Error::Closed);
            }
            let rc = unsafe {
                vcabi::trueos_cabi_shell3_session_control_v1(
                    self.id,
                    op,
                    size.cols.into(),
                    size.rows.into(),
                )
            };
            if rc < 0 {
                Err(error(rc.into()))
            } else {
                Ok(())
            }
        }
    }
    impl Transport for KernelTransport {
        fn read(&mut self, out: &mut [u8]) -> Result<Read, Error> {
            if self.closed {
                return Err(Error::Closed);
            }
            let mut payload = [0u8; 4096];
            let cap = payload.len().min(out.len().saturating_add(8));
            let rc = unsafe {
                vcabi::trueos_cabi_shell3_session_read_v1(self.id, payload.as_mut_ptr(), cap)
            };
            if rc < 0 {
                return Err(error(rc as i64));
            }
            let len = rc as usize;
            if len < 8 || len > cap {
                return Err(Error::Backend(-1));
            }
            let flag = u32::from_le_bytes(payload[..4].try_into().unwrap());
            let code = i32::from_le_bytes(payload[4..8].try_into().unwrap());
            if flag > 1 {
                return Err(Error::Backend(-1));
            }
            out[..len - 8].copy_from_slice(&payload[8..len]);
            Ok(Read {
                len: len - 8,
                stream: Stream::Stdout,
                state: if flag == 1 {
                    State::Exited(code)
                } else {
                    State::Running
                },
            })
        }
        fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
            if self.closed {
                return Err(Error::Closed);
            }
            let rc = unsafe {
                vcabi::trueos_cabi_shell3_session_write_v1(self.id, bytes.as_ptr(), bytes.len())
            };
            if rc < 0 {
                return Err(error(rc as i64));
            }
            if rc as usize > bytes.len() {
                return Err(Error::Backend(-1));
            }
            Ok(rc as usize)
        }
        fn close_stdin(&mut self) -> Result<(), Error> {
            self.control(2, Geometry { rows: 0, cols: 0 })
        }
        fn resize(&mut self, size: Geometry) -> Result<(), Error> {
            self.control(1, size)
        }
        fn signal(&mut self, signal: Signal) -> Result<(), Error> {
            self.control(
                match signal {
                    Signal::Interrupt => 3,
                    Signal::Terminate => 4,
                },
                Geometry { rows: 0, cols: 0 },
            )
        }
        fn close(&mut self) {
            if !self.closed {
                let _ = self.control(5, Geometry { rows: 0, cols: 0 });
                self.closed = true;
            }
        }
    }
}
