//! Native TCP ownership for the pinned Rama provider. This module is selected
//! only by the TRUEOS overlay; ordinary host providers retain their source.
//! No native service ID is represented as a Unix descriptor.
use rama_net::socket::{
    SocketOptions,
    opts::{Domain, Protocol, Type},
};
use std::{io, net::SocketAddr};
use tokio::net::{TcpListener, TcpSocket, TcpStream};

fn socket(options: &SocketOptions) -> io::Result<TcpSocket> {
    if !matches!(options.r#type, Type::Stream)
        || !matches!(options.protocol, None | Some(Protocol::TCP))
    {
        return Err(unsupported("native TCP requires stream/TCP socket options"));
    }
    // Check every optional setting in this pinned SocketOptions definition.
    // Explicit unsupported settings fail before allocation or endpoint bind.
    macro_rules! reject {
        ($field:ident) => {
            if options.$field.is_some() {
                return Err(unsupported(concat!(
                    "native TCP does not support SocketOptions::",
                    stringify!($field)
                )));
            }
        };
    }
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(device);
    reject!(broadcast);
    reject!(keep_alive);
    reject!(linger);
    #[cfg(not(target_os = "redox"))]
    reject!(out_of_band_inline);
    #[cfg(all(target_family = "unix", target_os = "linux"))]
    reject!(passcred);
    reject!(recv_buffer_size);
    reject!(read_timeout);
    reject!(reuse_address);
    reject!(send_buffer_size);
    reject!(write_timeout);
    #[cfg(not(any(target_os = "redox", target_os = "espidf")))]
    reject!(header_included);
    #[cfg(not(any(
        target_os = "redox",
        target_os = "espidf",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd"
    )))]
    reject!(header_included_v6);
    #[cfg(target_os = "linux")]
    reject!(ip_transparent);
    reject!(ttl);
    #[cfg(not(any(
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku",
    )))]
    reject!(tos);
    #[cfg(not(any(
        target_os = "aix",
        target_os = "dragonfly",
        target_os = "fuchsia",
        target_os = "hurd",
        target_os = "illumos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "solaris",
        target_os = "haiku",
        target_os = "nto",
        target_os = "espidf",
        target_os = "vita",
    )))]
    reject!(recv_tos);
    reject!(multicast_hops_v6);
    #[cfg(target_os = "linux")]
    reject!(multicast_all_v4);
    #[cfg(target_os = "linux")]
    reject!(multicast_all_v6);
    reject!(multicast_interface_v4);
    reject!(multicast_interface_v6);
    reject!(multicast_loop_v4);
    reject!(multicast_loop_v6);
    reject!(multicast_ttl_v4);
    reject!(unicast_hops_v6);
    reject!(only_v6);
    #[cfg(not(any(
        target_os = "dragonfly",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "solaris",
        target_os = "haiku",
        target_os = "hurd",
        target_os = "espidf",
        target_os = "vita",
    )))]
    reject!(recv_tclass_v6);
    #[cfg(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "fuchsia",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    reject!(tclass_v6);
    #[cfg(not(any(
        target_os = "windows",
        target_os = "dragonfly",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "redox",
        target_os = "solaris",
        target_os = "haiku",
        target_os = "hurd",
        target_os = "espidf",
        target_os = "vita",
    )))]
    reject!(recv_hoplimit_v6);
    reject!(tcp_keep_alive);
    #[cfg(all(target_family = "unix", not(target_os = "redox")))]
    reject!(tcp_max_segments);
    #[cfg(any(target_os = "freebsd", target_os = "linux"))]
    reject!(tcp_congestion);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(mark);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(tcp_cork);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(tcp_quick_ack);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(tcp_thin_linear_timeouts);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(tcp_user_timeout);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(freebind);
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    reject!(freebind_ipv6);
    #[cfg(target_os = "linux")]
    reject!(cpu_affinity);
    #[cfg(all(
        target_family = "unix",
        not(any(target_os = "solaris", target_os = "illumos"))
    ))]
    reject!(reuse_port);
    #[cfg(target_os = "linux")]
    reject!(dccp_service);
    #[cfg(target_os = "linux")]
    reject!(dccp_ccid);
    #[cfg(target_os = "linux")]
    reject!(dccp_server_timewait);
    #[cfg(target_os = "linux")]
    reject!(dccp_send_cscov);
    #[cfg(target_os = "linux")]
    reject!(dccp_recv_cscov);
    #[cfg(target_os = "linux")]
    reject!(dccp_qpolicy_txqlen);
    let socket = match options.domain {
        Domain::IPv4 => TcpSocket::new_v4()?,
        Domain::IPv6 => TcpSocket::new_v6()?,
        Domain::Unix => {
            return Err(unsupported(
                "native TCP does not accept Unix socket domains",
            ));
        }
    };
    if let Some(value) = options.tcp_no_delay {
        socket.set_nodelay(value)?;
    }
    if let Some(address) = options.address {
        socket.bind(address.into())?;
    }
    Ok(socket)
}

pub(crate) async fn connect(options: &SocketOptions, address: SocketAddr) -> io::Result<TcpStream> {
    socket(options)?.connect(address).await
}

pub(crate) fn listener(options: &SocketOptions) -> io::Result<TcpListener> {
    let socket = socket(options)?;
    if options.address.is_none() {
        socket.bind(match options.domain {
            Domain::IPv6 => "[::]:0"
                .parse()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
            _ => SocketAddr::from(([0, 0, 0, 0], 0)),
        })?;
    }
    // Rama's public listener builder has no backlog option. Use the existing
    // native service's maximum pending-connection capacity for this provider.
    socket.listen(32)
}

pub(crate) fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
