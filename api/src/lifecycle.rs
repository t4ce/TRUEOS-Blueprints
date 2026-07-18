//! Opt-in recovery boundaries for Blueprint-owned host capabilities.
//!
//! A VM snapshot can retain logical Rust state, but it cannot retain a live
//! host socket. [`RebindableTcpListener`] periodically verifies its listener
//! lease and reacquires one when TRUEOS revokes the old lease during pause.

use core::{
    net::{IpAddr, SocketAddr},
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};

use tokio::{
    io,
    net::{TcpListener, TcpStream},
    time,
};

use crate::logl::{self, level};

const DEFAULT_INCREMENT_ATTEMPTS: u16 = 32;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_LEASE_PROBE_INTERVAL: Duration = Duration::from_millis(500);

/// How a resumed or replicated server reacquires its listening port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortPolicy {
    /// Keep retrying one externally stable address.
    Fixed(SocketAddr),
    /// Prefer one address and step upward when another instance owns it.
    Increment {
        preferred: SocketAddr,
        attempts: u16,
    },
    /// Ask TRUEOS for any unused port on the selected interface.
    Ephemeral(IpAddr),
}

impl PortPolicy {
    #[must_use]
    pub const fn fixed(address: SocketAddr) -> Self {
        Self::Fixed(address)
    }

    #[must_use]
    pub const fn incrementing(preferred: SocketAddr) -> Self {
        Self::Increment {
            preferred,
            attempts: DEFAULT_INCREMENT_ATTEMPTS,
        }
    }

    #[must_use]
    pub const fn incrementing_with(preferred: SocketAddr, attempts: u16) -> Self {
        Self::Increment {
            preferred,
            attempts,
        }
    }

    #[must_use]
    pub const fn ephemeral(interface: IpAddr) -> Self {
        Self::Ephemeral(interface)
    }

    fn preferred(self) -> SocketAddr {
        match self {
            Self::Fixed(address)
            | Self::Increment {
                preferred: address, ..
            } => address,
            Self::Ephemeral(interface) => SocketAddr::new(interface, 0),
        }
    }
}

/// Tuning for a rebindable server boundary.
#[derive(Clone, Copy, Debug)]
pub struct ServerConfig {
    pub service: &'static str,
    pub ports: PortPolicy,
    pub retry_delay: Duration,
    pub lease_probe_interval: Duration,
}

impl ServerConfig {
    /// The default Blueprint server policy: prefer the declared port, then try
    /// the next 31 ports so a second instance can start without special code.
    #[must_use]
    pub const fn new(service: &'static str, preferred: SocketAddr) -> Self {
        Self {
            service,
            ports: PortPolicy::incrementing(preferred),
            retry_delay: DEFAULT_RETRY_DELAY,
            lease_probe_interval: DEFAULT_LEASE_PROBE_INTERVAL,
        }
    }

    #[must_use]
    pub const fn with_ports(mut self, ports: PortPolicy) -> Self {
        self.ports = ports;
        self
    }

    #[must_use]
    pub const fn with_retry_delay(mut self, retry_delay: Duration) -> Self {
        self.retry_delay = retry_delay;
        self
    }

    #[must_use]
    pub const fn with_lease_probe_interval(mut self, lease_probe_interval: Duration) -> Self {
        self.lease_probe_interval = lease_probe_interval;
        self
    }
}

/// A TCP listener whose host lease is reconstructed after VM pause/resume.
///
/// The router and all ordinary in-VM state remain untouched. Only the revoked
/// external listener is dropped and rebound. `published_port` is updated after
/// every bind so status endpoints can advertise an incremented or ephemeral
/// port correctly.
pub struct RebindableTcpListener {
    config: ServerConfig,
    listener: Option<TcpListener>,
    local_addr: SocketAddr,
    published_port: &'static AtomicU16,
}

impl RebindableTcpListener {
    /// Wait until a listener is available and publish its actual port.
    pub async fn bind(config: ServerConfig, published_port: &'static AtomicU16) -> Self {
        let (listener, local_addr) = acquire(config).await;
        published_port.store(local_addr.port(), Ordering::Release);
        log_bound(config, local_addr, false);
        Self {
            config,
            listener: Some(listener),
            local_addr,
            published_port,
        }
    }

    #[must_use]
    pub const fn bound_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Accept forever, rebuilding only the listener when its host lease was
    /// revoked. The periodic local-address probe guarantees recovery even when
    /// a paused Tokio readiness registration cannot wake itself.
    pub async fn accept(&mut self) -> (TcpStream, SocketAddr) {
        loop {
            let accept_result = {
                let listener = self.listener.as_ref().expect("listener is installed");
                time::timeout(self.config.lease_probe_interval, listener.accept()).await
            };

            match accept_result {
                Ok(Ok(connection)) => return connection,
                Ok(Err(error)) if listener_was_revoked(&error) => {
                    self.rebind(Some(error)).await;
                }
                Ok(Err(error)) => {
                    logl::log(
                        level::WARN,
                        format_args!(
                            "lifecycle-net: {} accept failed kind={:?} error={}",
                            self.config.service,
                            error.kind(),
                            error
                        ),
                    );
                    time::sleep(self.config.retry_delay).await;
                }
                Err(_) => {
                    let health = self
                        .listener
                        .as_ref()
                        .expect("listener is installed")
                        .local_addr();
                    if let Err(error) = health {
                        if listener_was_revoked(&error) {
                            self.rebind(Some(error)).await;
                        } else {
                            logl::log(
                                level::WARN,
                                format_args!(
                                    "lifecycle-net: {} lease probe failed kind={:?} error={}",
                                    self.config.service,
                                    error.kind(),
                                    error
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    async fn rebind(&mut self, cause: Option<io::Error>) {
        self.published_port.store(0, Ordering::Release);
        self.listener.take();
        if let Some(error) = cause {
            logl::log(
                level::INFO,
                format_args!(
                    "lifecycle-net: {} listener lease revoked kind={:?}; rebinding",
                    self.config.service,
                    error.kind()
                ),
            );
        }

        let (listener, local_addr) = acquire(self.config).await;
        self.local_addr = local_addr;
        self.published_port
            .store(local_addr.port(), Ordering::Release);
        self.listener = Some(listener);
        log_bound(self.config, local_addr, true);
    }
}

impl Drop for RebindableTcpListener {
    fn drop(&mut self) {
        self.published_port.store(0, Ordering::Release);
    }
}

async fn acquire(config: ServerConfig) -> (TcpListener, SocketAddr) {
    loop {
        match try_acquire(config.ports).await {
            Ok(bound) => return bound,
            Err(error) => {
                logl::log(
                    level::WARN,
                    format_args!(
                        "lifecycle-net: {} bind cycle failed near {} kind={:?} error={}; retrying",
                        config.service,
                        config.ports.preferred(),
                        error.kind(),
                        error
                    ),
                );
                time::sleep(config.retry_delay).await;
            }
        }
    }
}

async fn try_acquire(policy: PortPolicy) -> io::Result<(TcpListener, SocketAddr)> {
    match policy {
        PortPolicy::Fixed(address) => bind_one(address).await,
        PortPolicy::Ephemeral(interface) => bind_one(SocketAddr::new(interface, 0)).await,
        PortPolicy::Increment {
            preferred,
            attempts,
        } => {
            let mut last_error = None;
            for offset in 0..attempts.max(1) {
                let Some(port) = preferred.port().checked_add(offset) else {
                    break;
                };
                let candidate = SocketAddr::new(preferred.ip(), port);
                match bind_one(candidate).await {
                    Ok(bound) => return Ok(bound),
                    Err(error) => last_error = Some(error),
                }
            }
            Err(last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "no listener port candidate",
                )
            }))
        }
    }
}

async fn bind_one(address: SocketAddr) -> io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind(address).await?;
    let local_addr = listener.local_addr()?;
    Ok((listener, local_addr))
}

fn listener_was_revoked(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::InvalidInput
    )
}

fn log_bound(config: ServerConfig, local_addr: SocketAddr, rebound: bool) {
    let preferred = config.ports.preferred();
    logl::log(
        level::INFO,
        format_args!(
            "lifecycle-net: {} {} listen={} preferred={}",
            config.service,
            if rebound { "rebound" } else { "ready" },
            local_addr,
            preferred
        ),
    );
}

/// Build the compact Axum adapter in the consuming Blueprint.
///
/// Keeping this adapter at the call site avoids making the `trueos` facade
/// depend on Axum, which also keeps the Blueprint packer's staged dependency
/// graph unambiguous.
#[macro_export]
macro_rules! lifecycle_axum_listener {
    ($service:expr, $preferred:expr, $published_port:expr) => {{
        $crate::lifecycle_axum_listener!(
            @config $crate::lifecycle::ServerConfig::new($service, $preferred),
            $published_port
        )
    }};
    (@config $config:expr, $published_port:expr) => {{
        struct TrueosLifecycleAxumListener($crate::lifecycle::RebindableTcpListener);

        impl ::axum::serve::Listener for TrueosLifecycleAxumListener {
            type Io = $crate::tokio::net::TcpStream;
            type Addr = ::core::net::SocketAddr;

            async fn accept(&mut self) -> (Self::Io, Self::Addr) {
                self.0.accept().await
            }

            fn local_addr(&self) -> $crate::tokio::io::Result<Self::Addr> {
                Ok(self.0.bound_addr())
            }
        }

        async move {
            TrueosLifecycleAxumListener(
                $crate::lifecycle::RebindableTcpListener::bind(
                    $config,
                    $published_port,
                )
                .await,
            )
        }
    }};
}
