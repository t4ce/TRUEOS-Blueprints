use std::{collections::VecDeque, io, time::Duration};

use mio::{unix::SourceFd, Events, Interest, Poll, Token};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use signal_hook_mio::v1_0::Signals;

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::Event;
use crate::event::{
    source::EventSource, sys::unix::parse::parse_event, timeout::PollTimeout, InternalEvent,
};
use crate::terminal::sys::file_descriptor::{tty_fd, FileDesc};

// Tokens to identify file descriptor
const TTY_TOKEN: Token = Token(0);
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
const SIGNAL_TOKEN: Token = Token(1);
#[cfg(feature = "event-stream")]
const WAKE_TOKEN: Token = Token(2);

// I (@zrzka) wasn't able to read more than 1_022 bytes when testing
// reading on macOS/Linux -> we don't need bigger buffer and 1k of bytes
// is enough.
const TTY_BUFFER_SIZE: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalSurfaceSnapshot {
    generation: u64,
    columns: u32,
    rows: u32,
}

fn terminal_surface_changed(
    previous: TerminalSurfaceSnapshot,
    current: TerminalSurfaceSnapshot,
) -> bool {
    previous.generation != current.generation
        || previous.columns != current.columns
        || previous.rows != current.rows
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_blueprint_terminal_surface_snapshot_v1(
        out_generation: *mut u64,
        out_cols: *mut u32,
        out_rows: *mut u32,
    ) -> i32;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn trueos_event_probe(message: &[u8]) {
    unsafe { trueos_cabi_write(1, message.as_ptr(), message.len()) };
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn trueos_terminal_surface_snapshot() -> io::Result<Option<TerminalSurfaceSnapshot>> {
    let mut generation = 0;
    let mut columns = 0;
    let mut rows = 0;
    let rc = unsafe {
        trueos_cabi_blueprint_terminal_surface_snapshot_v1(&mut generation, &mut columns, &mut rows)
    };
    match rc {
        0 if generation != 0 && columns != 0 && rows != 0 => Ok(Some(TerminalSurfaceSnapshot {
            generation,
            columns,
            rows,
        })),
        // An event source may outlive a parked or detached terminal lease.
        // Keep its last good snapshot so reentry can be compared against it.
        -5..=-1 => Ok(None),
        0 => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid TrueOS terminal surface snapshot",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::Other,
            "TrueOS terminal surface snapshot failed",
        )),
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn trueos_poll_timeout(timeout: &PollTimeout) -> Option<Duration> {
    const SURFACE_POLL_SLICE: Duration = Duration::from_millis(16);
    Some(match timeout.leftover() {
        Some(left) if left < SURFACE_POLL_SLICE => left,
        _ => SURFACE_POLL_SLICE,
    })
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn trueos_event_probe(_message: &[u8]) {}

fn mio_to_io_error(error: io::Error) -> io::Error {
    let kind = match error.kind() {
        io::ErrorKind::WouldBlock => io::ErrorKind::WouldBlock,
        io::ErrorKind::Interrupted => io::ErrorKind::Interrupted,
        io::ErrorKind::InvalidInput => io::ErrorKind::InvalidInput,
        io::ErrorKind::NotFound => io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => io::ErrorKind::PermissionDenied,
        io::ErrorKind::BrokenPipe => io::ErrorKind::BrokenPipe,
        io::ErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
        io::ErrorKind::TimedOut => io::ErrorKind::TimedOut,
        io::ErrorKind::ConnectionRefused => io::ErrorKind::ConnectionRefused,
        io::ErrorKind::ConnectionReset => io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted => io::ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected => io::ErrorKind::NotConnected,
        io::ErrorKind::AddrInUse => io::ErrorKind::AddrInUse,
        io::ErrorKind::AddrNotAvailable => io::ErrorKind::AddrNotAvailable,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, "mio trueos error")
}

pub(crate) struct UnixInternalEventSource {
    poll: Poll,
    events: Events,
    parser: Parser,
    tty_buffer: [u8; TTY_BUFFER_SIZE],
    tty_fd: FileDesc<'static>,
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    terminal_surface: Option<TerminalSurfaceSnapshot>,
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    signals: Signals,
    #[cfg(feature = "event-stream")]
    waker: Waker,
}

impl UnixInternalEventSource {
    pub fn new() -> io::Result<Self> {
        UnixInternalEventSource::from_file_descriptor(tty_fd()?)
    }

    pub(crate) fn from_file_descriptor(input_fd: FileDesc<'static>) -> io::Result<Self> {
        trueos_event_probe(b"[crossterm-resize-probe:INFO] event-source init\n");
        let poll = Poll::new().map_err(mio_to_io_error)?;
        let registry = poll.registry();

        let tty_raw_fd = input_fd.raw_fd();
        let mut tty_ev = SourceFd(&tty_raw_fd);
        registry
            .register(&mut tty_ev, TTY_TOKEN, Interest::READABLE)
            .map_err(mio_to_io_error)?;

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        let signals = {
            trueos_event_probe(b"[crossterm-resize-probe:INFO] sigwinch register begin\n");
            let mut signals =
                Signals::new([signal_hook::consts::SIGWINCH]).map_err(mio_to_io_error)?;
            registry
                .register(&mut signals, SIGNAL_TOKEN, Interest::READABLE)
                .map_err(mio_to_io_error)?;
            trueos_event_probe(b"[crossterm-resize-probe:INFO] sigwinch register ready\n");
            signals
        };
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        trueos_event_probe(
            b"[crossterm-resize-probe:INFO] sigwinch unavailable; tty polling active\n",
        );
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        let terminal_surface = trueos_terminal_surface_snapshot()?;

        #[cfg(feature = "event-stream")]
        let waker = Waker::new(registry, WAKE_TOKEN).map_err(mio_to_io_error)?;

        Ok(UnixInternalEventSource {
            poll,
            events: Events::with_capacity(3),
            parser: Parser::default(),
            tty_buffer: [0u8; TTY_BUFFER_SIZE],
            tty_fd: input_fd,
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            terminal_surface,
            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            signals,
            #[cfg(feature = "event-stream")]
            waker,
        })
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn terminal_surface_event(&mut self) -> io::Result<Option<InternalEvent>> {
        let Some(current) = trueos_terminal_surface_snapshot()? else {
            return Ok(None);
        };
        let previous = self.terminal_surface.replace(current);
        if previous.is_some_and(|previous| terminal_surface_changed(previous, current)) {
            if previous.is_some_and(|previous| previous.generation != current.generation) {
                // A reconnect or ownership boundary invalidates both partial
                // escape sequences and already-parsed bytes from the prior
                // transport incarnation.
                self.parser = Parser::default();
            }
            let columns = current.columns.min(u32::from(u16::MAX)) as u16;
            let rows = current.rows.min(u32::from(u16::MAX)) as u16;
            trueos_event_probe(
                b"[crossterm-resize-probe:INFO] typed surface change -> resize event\n",
            );
            return Ok(Some(InternalEvent::Event(Event::Resize(columns, rows))));
        }
        Ok(None)
    }
}

impl EventSource for UnixInternalEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>> {
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        if let Some(event) = self.terminal_surface_event()? {
            return Ok(Some(event));
        }
        if let Some(event) = self.parser.next() {
            return Ok(Some(event));
        }

        let timeout = PollTimeout::new(timeout);

        loop {
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            let poll_timeout = trueos_poll_timeout(&timeout);
            #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
            let poll_timeout = timeout.leftover();
            if let Err(e) = self.poll.poll(&mut self.events, poll_timeout) {
                // Mio will throw an interrupted error in case of cursor position retrieval. We need to retry until it succeeds.
                // Previous versions of Mio (< 0.7) would automatically retry the poll call if it was interrupted (if EINTR was returned).
                // https://docs.rs/mio/0.7.0/mio/struct.Poll.html#notes
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                } else {
                    return Err(mio_to_io_error(e));
                }
            };

            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            if let Some(event) = self.terminal_surface_event()? {
                return Ok(Some(event));
            }

            if self.events.is_empty() {
                // TrueOS has no SIGWINCH delivery into a Blueprint. Its poll
                // is sliced so both finite poll() and indefinite read() can
                // observe an out-of-band surface-generation change.
                #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                {
                    if timeout.elapsed() {
                        return Ok(None);
                    }
                    continue;
                }
                #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
                return Ok(None);
            }

            for token in self.events.iter().map(|x| x.token()) {
                match token {
                    TTY_TOKEN => {
                        loop {
                            match self.tty_fd.read(&mut self.tty_buffer) {
                                Ok(read_count) => {
                                    if read_count > 0 {
                                        self.parser.advance(
                                            &self.tty_buffer[..read_count],
                                            read_count == TTY_BUFFER_SIZE,
                                        );
                                    }
                                }
                                Err(e) => {
                                    // No more data to read at the moment. We will receive another event
                                    if e.kind() == io::ErrorKind::WouldBlock {
                                        break;
                                    }
                                    // once more data is available to read.
                                    else if e.kind() == io::ErrorKind::Interrupted {
                                        continue;
                                    }
                                }
                            };

                            if let Some(event) = self.parser.next() {
                                return Ok(Some(event));
                            }
                        }
                    }
                    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
                    SIGNAL_TOKEN => {
                        trueos_event_probe(b"[crossterm-resize-probe:INFO] signal token ready\n");
                        if self.signals.pending().next() == Some(signal_hook::consts::SIGWINCH) {
                            trueos_event_probe(
                                b"[crossterm-resize-probe:INFO] sigwinch -> resize event\n",
                            );
                            // TODO Should we remove tput?
                            //
                            // This can take a really long time, because terminal::size can
                            // launch new process (tput) and then it parses its output. It's
                            // not a really long time from the absolute time point of view, but
                            // it's a really long time from the mio, async-std/tokio executor, ...
                            // point of view.
                            let new_size = crate::terminal::size()?;
                            return Ok(Some(InternalEvent::Event(Event::Resize(
                                new_size.0, new_size.1,
                            ))));
                        }
                    }
                    #[cfg(feature = "event-stream")]
                    WAKE_TOKEN => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Poll operation was woken up by `Waker::wake`",
                        ));
                    }
                    _ => unreachable!("Synchronize Evented handle registration & token handling"),
                }
            }

            // Processing above can take some time, check if timeout expired
            if timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.waker.clone()
    }
}

//
// Following `Parser` structure exists for two reasons:
//
//  * mimic anes Parser interface
//  * move the advancing, parsing, ... stuff out of the `try_read` method
//
#[derive(Debug)]
struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
        }
    }
}

impl Parser {
    fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more;

            self.buffer.push(*byte);

            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    // Event can't be parsed, because we don't have enough bytes for
                    // the current sequence. Keep the buffer and process next bytes.
                }
                Err(_) => {
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::{terminal_surface_changed, TerminalSurfaceSnapshot};

    #[test]
    fn surface_generation_detects_same_size_reconnect() {
        let before = TerminalSurfaceSnapshot {
            generation: 7,
            columns: 120,
            rows: 40,
        };
        let after = TerminalSurfaceSnapshot {
            generation: 8,
            ..before
        };
        assert!(terminal_surface_changed(before, after));
    }

    #[test]
    fn surface_geometry_change_is_detected_without_generation_change() {
        let before = TerminalSurfaceSnapshot {
            generation: 7,
            columns: 120,
            rows: 40,
        };
        let after = TerminalSurfaceSnapshot {
            columns: 121,
            ..before
        };
        assert!(terminal_surface_changed(before, after));
        assert!(!terminal_surface_changed(before, before));
    }
}
