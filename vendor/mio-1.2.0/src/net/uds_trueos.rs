#![allow(missing_docs)]

use crate::io::{self, Read, Write};
use crate::{event, Interest, Registry, Token};
use ::core::fmt;

const AF_UNIX: i32 = 1;
const SOCK_STREAM: i32 = 1;
const SOCK_NONBLOCK: i32 = 0o4000;
const SOCK_CLOEXEC: i32 = 0o2000000;

unsafe extern "C" {
    fn socketpair(domain: i32, socket_type: i32, protocol: i32, sv: *mut i32) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
    fn write(fd: i32, buf: *const u8, count: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn __errno_location() -> *mut i32;
}

/// A non-blocking Unix stream socket backed by TRUEOS fd syscalls.
pub struct UnixStream {
    fd: i32,
}

impl UnixStream {
    /// Creates an unnamed pair of connected sockets.
    pub fn pair() -> io::Result<(UnixStream, UnixStream)> {
        let mut fds = [-1i32; 2];
        let rc = unsafe {
            socketpair(
                AF_UNIX,
                SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        };
        if rc != 0 || fds[0] < 0 || fds[1] < 0 {
            return Err(last_os_error());
        }
        Ok((UnixStream { fd: fds[0] }, UnixStream { fd: fds[1] }))
    }

    /// Execute an I/O operation against this non-blocking fd.
    pub fn try_io<F, T>(&self, f: F) -> io::Result<T>
    where
        F: FnOnce() -> io::Result<T>,
    {
        f()
    }

    pub(crate) fn raw_fd(&self) -> i32 {
        self.fd
    }
}

impl Drop for UnixStream {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = unsafe { close(self.fd) };
            self.fd = -1;
        }
    }
}

impl Read for UnixStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        fd_read(self.fd, buf)
    }
}

impl Read for &'_ UnixStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        fd_read(self.fd, buf)
    }
}

impl Write for UnixStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        fd_write(self.fd, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &'_ UnixStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        fd_write(self.fd, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl event::Source for UnixStream {
    fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        registry
            .selector()
            .register_fd_source(self.raw_fd(), token, interests)
    }

    fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        registry
            .selector()
            .reregister_fd_source(self.raw_fd(), token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        registry.selector().deregister_fd_source(self.raw_fd())
    }
}

impl fmt::Debug for UnixStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnixStream").field("fd", &self.fd).finish()
    }
}

impl crate::real_std::os::fd::AsRawFd for UnixStream {
    fn as_raw_fd(&self) -> crate::real_std::os::fd::RawFd {
        self.fd
    }
}

fn fd_read(fd: i32, buf: &mut [u8]) -> io::Result<usize> {
    let rc = unsafe { read(fd, buf.as_mut_ptr(), buf.len()) };
    if rc >= 0 {
        Ok(rc as usize)
    } else {
        Err(last_os_error())
    }
}

fn fd_write(fd: i32, buf: &[u8]) -> io::Result<usize> {
    let rc = unsafe { write(fd, buf.as_ptr(), buf.len()) };
    if rc >= 0 {
        Ok(rc as usize)
    } else {
        Err(last_os_error())
    }
}

fn last_os_error() -> io::Error {
    unsafe {
        let errno = __errno_location();
        if errno.is_null() {
            io::Error::new(io::ErrorKind::Other, "trueos unix fd error")
        } else {
            io::errno_error(*errno)
        }
    }
}
