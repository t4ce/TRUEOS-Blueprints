//! A Rust port of the [Reticulum Python reference implementation](https://github.com/markqvist/reticulum),
//! the cryptography-based networking stack for building unstoppable
//! networks with LoRa, Packet Radio, WiFi and everything in between.
//!
//! Reticulum is the cryptography-based networking stack for building local
//! and wide-area networks with readily available hardware. It can operate
//! even with very high latency and extremely low bandwidth. Reticulum allows
//! you to build wide-area networks with off-the-shelf tools, and offers end-to-end
//! encryption and connectivity, initiator anonymity, autoconfiguring
//! cryptographically backed multi-hop transport, efficient addressing,
//! unforgeable delivery acknowledgements and more.
//!
//! More resources:
//!
//! * [Homepage](https://reticulum.network/)
//! * [Manual](https://reticulum.network/manual/index.html)
//! * [unsigned.io](https://unsigned.io/software/index.html)
//!
//! # A tour of this Reticulum implementation
//!
//! Reticulum consists of one main [`transport::Transport`] object that can connect to other
//! Reticulum instances via different kinds of interfaces by creating them with the
//! [`iface::InterfaceManager`]:
//! * [`iface::tcp_client::TcpClient`]
//! * [`iface::tcp_server::TcpServer`]
//! * [`iface::udp::UdpInterface`]
//! * Kaonic
//!
//! The main instance can be used to send messages to [`destination::Destination`]s directly
//! or over [`destination::link::Link`]s.
//!
//! [`hash::AddressHash`] is used for adressing destinations and [`destination::link::LinkId`] for
//! links.
//!
//! `Resources` can be used to send arbitrary amounts of data using a simple interface.
//!
//! ## Creating a Transport instance
//!
//! ```
//! use reticulum::transport::{Transport, TransportConfig};
//! #[tokio::main]
//! async fn main() {
//!     let transport = Transport::new(TransportConfig::default());
//! }
//! ```
//!
//! ## Creating interfaces
//!
//! ```
//! # use reticulum::transport::{Transport, TransportConfig};
//! # use reticulum::iface::tcp_client::TcpClient;
//! # #[tokio::main]
//! # async fn main() {
//!     # let transport = Transport::new(TransportConfig::default());
//!     let client_addr = transport.iface_manager().lock().await
//!         .spawn(TcpClient::new("127.0.0.1:4242"), TcpClient::spawn);
//! # }
//! ```
//!
//! ## Set up and announce destinations
//!
//! Destinations are used as targets for messages or links.
//!
//! Destinations need to be announced to the network.
//!
//! ```
//! # use rand_core::OsRng;
//! # use reticulum::transport::{Transport, TransportConfig};
//! # use reticulum::identity::PrivateIdentity;
//! # use reticulum::destination::{SingleInputDestination, DestinationName};
//! # use reticulum::hash::AddressHash;
//! # #[tokio::main]
//! # async fn main() {
//!     # let mut transport = Transport::new(TransportConfig::default());
//!     let id = PrivateIdentity::new_from_rand(OsRng);
//!     let destination = transport
//!         .add_destination(id, DestinationName::new("example", "app"))
//!         .await;
//!     transport.send_announce(&destination, None).await;
//! # }
//! ```
//!
//! ## Setting up links
//!
//! Links should be used for prolonged bidirectional communication.
//!
//! Links are established by sending a link request to the target destination. After the response
//! from the target the link can be used.
//!
//! ```no_run
// Don't run because it will run forever because it will never receive announces
//! # use rand_core::OsRng;
//! # use tokio::sync::Mutex;
//! # use std::sync::Arc;
//! # use reticulum::transport::{Transport, TransportConfig};
//! # use reticulum::hash::AddressHash;
//! # use reticulum::destination::link::{Link, LinkEvent};
//! # #[tokio::main]
//! # async fn main() {
//!     # let transport = Transport::new(TransportConfig::default());
//!     # let target_destination = AddressHash::new_from_rand(OsRng);
//!     let mut link: Option<Arc<Mutex<Link>>> = None;
//!     let mut announce_receiver = transport.recv_announces().await;
//!     while let Ok(announce) = announce_receiver.recv().await {
//!         let destination = announce.destination.lock().await.desc;
//!         if destination.address_hash == target_destination {
//!             // Send link request to target destination
//!             link = Some(transport.link(destination).await);
//!             break
//!         }
//!     }
//!     let link_id = link.unwrap().lock().await.id().clone();
//!
//!     // Handle link events for this link ID
//!     let mut link_event_receiver = transport.in_link_events();
//!     loop {
//!         let link_event_data = link_event_receiver.recv().await.unwrap();
//!         if link_event_data.id == link_id {
//!             match link_event_data.event {
//!                 LinkEvent::Activated => {
//!                     // Now this link can be used to send data
//!                     let link = transport.find_in_link(&link_id).await.unwrap();
//!                     let packet = link.lock().await.data_packet(b"hello world").unwrap();
//!                     transport.send_packet(packet).await;
//!                 }
//!                 LinkEvent::Data(_payload) => {
//!                     // Handle incoming messages
//!                 }
//!                 LinkEvent::Closed => {
//!                     // Notification of link close
//!                 }
//!                 _ => {}
//!             }
//!         }
//!     }
//! # }
//! ```

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod buffer;
pub mod crypt;
pub mod destination;
pub mod error;
pub mod hash;
pub mod identity;
pub mod iface;
pub mod packet;
pub mod serde;
pub mod transport;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn main() {
    trueos_log(
        log::Level::Info,
        format_args!("reticulum: public dual tcp probe start"),
    );
    install_trueos_logger();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            trueos_log(
                log::Level::Error,
                format_args!("reticulum: failed to build tokio runtime: {}", err),
            );
            return;
        }
    };

    runtime.block_on(async {
        run_trueos_public_dual_probe().await;
    });
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn run_trueos_public_dual_probe() {
    use crate::destination::DestinationName;
    use crate::identity::PrivateIdentity;
    use crate::iface::tcp_client::TcpClient;
    use crate::transport::TransportConfig;
    use rand_core::OsRng;
    use std::time::Duration;

    const PUBLIC_TARGETS: [&str; 2] = ["173.255.213.18:7822", "45.59.114.96:7822"];
    const ANNOUNCE_APP_DATA: &[u8] = b"trueos-reticulum-public-dual";

    let transport_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut transport = TransportConfig::new("trueos-reticulum", &transport_identity, false)
        .set_restart_outlinks(true)
        .build();

    let destination_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = transport
        .add_destination(
            destination_identity,
            DestinationName::new("trueos", "reticulum.probe"),
        )
        .await;
    let destination_address = destination.lock().await.desc.address_hash;
    trueos_log(
        log::Level::Info,
        format_args!("reticulum: local destination {}", destination_address),
    );

    let mut announces = transport.recv_announces().await;
    tokio::spawn(async move {
        loop {
            match announces.recv().await {
                Ok(announce) => {
                    let desc = announce.destination.lock().await.desc;
                    trueos_log(
                        log::Level::Info,
                        format_args!(
                            "reticulum: announce destination={} app_data_len={}",
                            desc.address_hash,
                            announce.app_data.len()
                        ),
                    );
                }
                Err(err) => {
                    trueos_log(
                        log::Level::Warn,
                        format_args!("reticulum: announce receiver error: {}", err),
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });

    let iface_manager = transport.iface_manager();
    for target in PUBLIC_TARGETS {
        let iface_addr = {
            let mut ifaces = iface_manager.lock().await;
            ifaces.spawn(TcpClient::new(target), TcpClient::spawn)
        };
        trueos_log(
            log::Level::Info,
            format_args!(
                "reticulum: public tcp client target={} iface={}",
                target, iface_addr
            ),
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    tokio::time::sleep(Duration::from_secs(5)).await;
    transport
        .send_announce(&destination, Some(ANNOUNCE_APP_DATA))
        .await;
    trueos_log(
        log::Level::Info,
        format_args!(
            "reticulum: sent announce destination={} app_data_len={}",
            destination_address,
            ANNOUNCE_APP_DATA.len()
        ),
    );

    let mut tick = tokio::time::interval(Duration::from_secs(30));
    loop {
        tick.tick().await;
        trueos_log(
            log::Level::Info,
            format_args!(
                "reticulum: status targets={},{} local_destination={} running=1",
                PUBLIC_TARGETS[0], PUBLIC_TARGETS[1], destination_address
            ),
        );
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
#[allow(dead_code)]
async fn run_trueos_localhost_dual_probe() {
    use crate::destination::DestinationName;
    use crate::identity::PrivateIdentity;
    use crate::iface::tcp_client::TcpClient;
    use crate::iface::tcp_server::TcpServer;
    use crate::transport::TransportConfig;
    use rand_core::OsRng;
    use std::time::Duration;

    const LOCAL_SERVER_BIND: &str = "0.0.0.0:4242";
    const LOCAL_CLIENT_TARGET: &str = "127.0.0.1:4242";
    const ANNOUNCE_APP_DATA: &[u8] = b"trueos-reticulum-localhost-dual";

    let transport_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut transport = TransportConfig::new("trueos-reticulum", &transport_identity, false)
        .set_restart_outlinks(true)
        .build();

    let destination_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = transport
        .add_destination(
            destination_identity,
            DestinationName::new("trueos", "reticulum.probe"),
        )
        .await;
    let destination_address = destination.lock().await.desc.address_hash;
    trueos_log(
        log::Level::Info,
        format_args!("reticulum: local destination {}", destination_address),
    );

    let mut announces = transport.recv_announces().await;
    tokio::spawn(async move {
        loop {
            match announces.recv().await {
                Ok(announce) => {
                    let desc = announce.destination.lock().await.desc;
                    trueos_log(
                        log::Level::Info,
                        format_args!(
                            "reticulum: announce destination={} app_data_len={}",
                            desc.address_hash,
                            announce.app_data.len()
                        ),
                    );
                }
                Err(err) => {
                    trueos_log(
                        log::Level::Warn,
                        format_args!("reticulum: announce receiver error: {}", err),
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });

    let iface_manager = transport.iface_manager();
    let server_iface_addr = {
        let server = TcpServer::new(LOCAL_SERVER_BIND, iface_manager.clone());
        let mut ifaces = iface_manager.lock().await;
        ifaces.spawn(server, TcpServer::spawn)
    };
    trueos_log(
        log::Level::Info,
        format_args!(
            "reticulum: local tcp server bind={} iface={}",
            LOCAL_SERVER_BIND, server_iface_addr
        ),
    );

    tokio::time::sleep(Duration::from_millis(250)).await;

    let client_iface_addr = {
        let mut ifaces = iface_manager.lock().await;
        ifaces.spawn(TcpClient::new(LOCAL_CLIENT_TARGET), TcpClient::spawn)
    };
    trueos_log(
        log::Level::Info,
        format_args!(
            "reticulum: local tcp client target={} iface={}",
            LOCAL_CLIENT_TARGET, client_iface_addr
        ),
    );

    tokio::time::sleep(Duration::from_secs(3)).await;
    transport
        .send_announce(&destination, Some(ANNOUNCE_APP_DATA))
        .await;
    trueos_log(
        log::Level::Info,
        format_args!(
            "reticulum: sent announce destination={} app_data_len={}",
            destination_address,
            ANNOUNCE_APP_DATA.len()
        ),
    );

    let mut tick = tokio::time::interval(Duration::from_secs(30));
    loop {
        tick.tick().await;
        trueos_log(
            log::Level::Info,
            format_args!(
                "reticulum: status local_server={} local_client={} local_destination={} running=1",
                LOCAL_SERVER_BIND, LOCAL_CLIENT_TARGET, destination_address
            ),
        );
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn install_trueos_logger() {
    static LOGGER: TrueosLogger = TrueosLogger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
struct TrueosLogger;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl log::Log for TrueosLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            trueos_log(record.level(), *record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn trueos_log(level: log::Level, args: core::fmt::Arguments<'_>) {
    let trueos_level = match level {
        log::Level::Error => trueos::logl::level::ERROR,
        log::Level::Warn => trueos::logl::level::WARN,
        log::Level::Info => trueos::logl::level::INFO,
        log::Level::Debug => trueos::logl::level::DEBUG,
        log::Level::Trace => trueos::logl::level::TRACE,
    };
    trueos::logl::log(trueos_level, args);
}
