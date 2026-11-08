//! Native UDP binding validates the full option contract before allocation.
use rama_core::error::{BoxError, ErrorContext as _};
use rama_net::{
    address::SocketAddress,
    socket::{
        Interface, SocketOptions,
        opts::{Domain, Protocol, Type},
    },
};
use std::{io, net::SocketAddr};
pub use tokio::net::UdpSocket;
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "native UDP socket options or socket ownership conversion are unavailable",
    )
}
pub async fn bind_udp_with_address<A: TryInto<SocketAddress, Error: Into<BoxError>>>(
    addr: A,
) -> Result<UdpSocket, BoxError> {
    let addr: SocketAddr = addr.try_into().map_err(Into::into)?.into();
    Ok(UdpSocket::bind(addr)
        .await
        .context("bind native udp socket")?)
}
pub async fn bind_udp_with_socket(
    _: rama_net::socket::core::Socket,
) -> Result<UdpSocket, BoxError> {
    Err(unsupported().into())
}
pub async fn bind_udp<I: TryInto<Interface, Error: Into<BoxError>>>(
    interface: I,
) -> Result<UdpSocket, BoxError> {
    #[allow(unreachable_patterns)]
    match interface.try_into().map_err(Into::into)? {
        Interface::Address(addr) => bind_udp_with_address(addr).await,
        Interface::Socket(options) => bind_options(&options).await,
        _ => Err(unsupported().into()),
    }
}
fn validated_address(options: &SocketOptions) -> Result<SocketAddr, BoxError> {
    if options.r#type != Type::Datagram
        || options.protocol.is_some_and(|p| p != Protocol::UDP)
        || !matches!(options.domain, Domain::IPv4 | Domain::IPv6)
    {
        return Err(unsupported().into());
    }
    // Compare every serialized public option, including future optional fields,
    // against an exact supported configuration. An explicitly set unsupported
    // option is rejected even if its value looks like a conventional default.
    let supported = SocketOptions {
        domain: options.domain,
        r#type: options.r#type,
        protocol: options.protocol,
        address: options.address.clone(),
        ttl: options.ttl,
        ..Default::default()
    };
    if serde_json::to_value(options)? != serde_json::to_value(&supported)? {
        return Err(unsupported().into());
    }
    if options.ttl.is_some_and(|ttl| !(1..=255).contains(&ttl)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid native UDP TTL").into());
    }
    let address = options
        .address
        .clone()
        .map(SocketAddr::from)
        .unwrap_or_else(|| match options.domain {
            Domain::IPv4 => SocketAddr::from(([0, 0, 0, 0], 0)),
            Domain::IPv6 => SocketAddr::from(([0; 8], 0)),
            Domain::Unix => unreachable!(),
        });
    if address.is_ipv4() != (options.domain == Domain::IPv4) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "UDP address does not match requested family",
        )
        .into());
    }
    Ok(address)
}
async fn bind_options(options: &SocketOptions) -> Result<UdpSocket, BoxError> {
    let address = validated_address(options)?;
    let socket = UdpSocket::bind(address)
        .await
        .context("bind native UDP options")?;
    if let Some(ttl) = options.ttl {
        socket.set_ttl(ttl).context("set native UDP TTL")?;
    }
    Ok(socket)
}
