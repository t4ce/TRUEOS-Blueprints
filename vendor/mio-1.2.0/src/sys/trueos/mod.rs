//! Native TRUEOS Mio backend.
//!
//! Network objects retain Mio's standard Unix facade, but registration and
//! waiting go directly to the TRUEOS socket readiness registry. No `poll(2)`
//! compatibility scan is involved in the reactor path.

mod selector;
pub(crate) use self::selector::{Event, Events, Selector, event};

mod waker;
pub(crate) use self::waker::Waker;

cfg_net! {
    pub(crate) use super::unix::{tcp, udp, uds};
}

#[cfg(feature = "os-ext")]
pub use super::unix::SourceFd;
#[cfg(feature = "os-ext")]
pub(crate) use super::unix::pipe;

cfg_io_source! {
    use std::io;
    use std::os::fd::RawFd;

    use crate::{Interest, Registry, Token};

    struct InternalState {
        selector: Selector,
        token: Token,
        interests: Interest,
        fd: RawFd,
    }

    pub(crate) struct IoSourceState {
        inner: Option<Box<InternalState>>,
    }

    impl IoSourceState {
        pub fn new() -> IoSourceState {
            IoSourceState { inner: None }
        }

        pub fn do_io<T, F, R>(&self, f: F, io: &T) -> io::Result<R>
        where
            F: FnOnce(&T) -> io::Result<R>,
        {
            let result = f(io);
            if matches!(&result, Err(error) if error.kind() == io::ErrorKind::WouldBlock) {
                if let Some(state) = &self.inner {
                    // Mio-managed sources use edge delivery. Reaching
                    // WouldBlock is the explicit rearm point.
                    state.selector.reregister_internal(
                        state.fd,
                        state.token,
                        state.interests,
                    )?;
                }
            }
            result
        }

        pub fn register(
            &mut self,
            registry: &Registry,
            token: Token,
            interests: Interest,
            fd: RawFd,
        ) -> io::Result<()> {
            if self.inner.is_some() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }

            let selector = registry.selector().try_clone()?;
            selector.register_internal(fd, token, interests)?;
            self.inner = Some(Box::new(InternalState {
                selector,
                token,
                interests,
                fd,
            }));
            Ok(())
        }

        pub fn reregister(
            &mut self,
            registry: &Registry,
            token: Token,
            interests: Interest,
            fd: RawFd,
        ) -> io::Result<()> {
            let Some(state) = self.inner.as_mut() else {
                return Err(io::ErrorKind::NotFound.into());
            };
            registry
                .selector()
                .reregister_internal(fd, token, interests)?;
            state.token = token;
            state.interests = interests;
            state.fd = fd;
            Ok(())
        }

        pub fn deregister(&mut self, registry: &Registry, fd: RawFd) -> io::Result<()> {
            let Some(_state) = self.inner.take() else {
                return Err(io::ErrorKind::NotFound.into());
            };
            registry.selector().deregister(fd)
        }
    }

    impl Drop for IoSourceState {
        fn drop(&mut self) {
            if let Some(state) = self.inner.take() {
                let _ = state.selector.deregister(state.fd);
            }
        }
    }
}
