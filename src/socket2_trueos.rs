//! Socket2 vocabulary backed by owned native TCP capabilities.
//! UDP creation uses the fallible native bind API in its owning consumer.
//! Unix descriptors, Unix sockets and unavailable options are not emulated.
#![allow(missing_docs)]
use std::{
    io::{self, Read, Write},
    marker::PhantomData,
    mem::MaybeUninit,
    net::{
        self, Ipv4Addr, Ipv6Addr, NativeSocket, Shutdown, SocketAddr, SocketAddrV4, SocketAddrV6,
    },
    ops::Deref,
    time::Duration,
};
fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native socket capability does not provide this Socket2 operation",
    ))
}
macro_rules! number {($name:ident {$($key:ident=$value:expr),*})=>{
 #[derive(Clone,Copy,Debug,Eq,PartialEq,Hash)]pub struct $name(i32);
 impl $name{$(pub const $key:Self=Self($value);)*}
 impl From<i32> for $name{fn from(v:i32)->Self{Self(v)}}
 impl From<$name> for i32{fn from(v:$name)->i32{v.0}}
};}
number!(Domain{IPV4=2,IPV6=10,UNIX=1});
impl Domain {
    pub fn for_address(a: SocketAddr) -> Self {
        if a.is_ipv4() { Self::IPV4 } else { Self::IPV6 }
    }
}
number!(Type{STREAM=1,DGRAM=2,RAW=3,SEQPACKET=5});
number!(Protocol{TCP=6,UDP=17,ICMPV4=1,ICMPV6=58});
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SockAddr(SocketAddr);
impl SockAddr {
    pub fn as_socket(&self) -> Option<SocketAddr> {
        Some(self.0)
    }
    pub fn as_socket_ipv4(&self) -> Option<SocketAddrV4> {
        match self.0 {
            SocketAddr::V4(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_socket_ipv6(&self) -> Option<SocketAddrV6> {
        match self.0 {
            SocketAddr::V6(v) => Some(v),
            _ => None,
        }
    }
    pub fn domain(&self) -> Domain {
        Domain::for_address(self.0)
    }
    pub fn family(&self) -> u16 {
        self.domain().0 as u16
    }
    pub fn is_ipv4(&self) -> bool {
        self.0.is_ipv4()
    }
    pub fn is_ipv6(&self) -> bool {
        self.0.is_ipv6()
    }
    pub fn is_unix(&self) -> bool {
        false
    }
    pub fn unix(_: impl AsRef<std::path::Path>) -> io::Result<Self> {
        unsupported()
    }
}
impl From<SocketAddr> for SockAddr {
    fn from(v: SocketAddr) -> Self {
        Self(v)
    }
}
impl From<SocketAddrV4> for SockAddr {
    fn from(v: SocketAddrV4) -> Self {
        Self(v.into())
    }
}
impl From<SocketAddrV6> for SockAddr {
    fn from(v: SocketAddrV6) -> Self {
        Self(v.into())
    }
}
#[derive(Clone, Debug, Default)]
pub struct TcpKeepalive {
    time: Option<Duration>,
    interval: Option<Duration>,
    retries: Option<u32>,
}
impl TcpKeepalive {
    pub const fn new() -> Self {
        Self {
            time: None,
            interval: None,
            retries: None,
        }
    }
    pub const fn with_time(mut self, v: Duration) -> Self {
        self.time = Some(v);
        self
    }
    pub const fn with_interval(mut self, v: Duration) -> Self {
        self.interval = Some(v);
        self
    }
    pub const fn with_retries(mut self, v: u32) -> Self {
        self.retries = Some(v);
        self
    }
}
#[derive(Clone, Debug)]
pub struct Socket {
    inner: NativeSocket,
    domain: Option<Domain>,
    kind: Type,
}
impl Socket {
    pub fn new(domain: Domain, kind: Type, protocol: Option<Protocol>) -> io::Result<Self> {
        // Validate the complete creation contract before allocating a native ID.
        if kind != Type::STREAM
            || !matches!(domain, Domain::IPV4 | Domain::IPV6)
            || protocol.is_some_and(|p| p != Protocol::TCP)
        {
            return unsupported();
        }
        let inner = NativeSocket::new_tcp(if domain == Domain::IPV4 { 4 } else { 6 })?;
        Ok(Self {
            inner,
            domain: Some(domain),
            kind,
        })
    }
    fn from_native(inner: NativeSocket, kind: Type) -> Self {
        Self {
            inner,
            domain: None,
            kind,
        }
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }
    pub fn bind(&self, a: &SockAddr) -> io::Result<()> {
        if self.kind != Type::STREAM {
            return unsupported();
        }
        self.inner.bind(a.0)
    }
    pub fn connect(&self, a: &SockAddr) -> io::Result<()> {
        if self.kind == Type::DGRAM {
            self.inner.udp_connect(a.0)
        } else {
            self.inner.connect(a.0)?;
            if self.inner.nonblocking() {
                // The native nonblocking primitive confirms submission, not
                // handshake completion. A peer address is already available
                // at this point; only writable readiness certifies connection.
                Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "native TCP connection submitted; await writable readiness",
                ))
            } else {
                Ok(())
            }
        }
    }
    pub fn connect_timeout(&self, a: &SockAddr, t: Duration) -> io::Result<()> {
        if t.is_zero() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if self.kind == Type::DGRAM {
            // Native UDP peer selection completes synchronously; it is not a
            // TCP handshake and must not run the TCP completion poll.
            return self.inner.udp_connect(a.0);
        }
        let previous = self.inner.nonblocking();
        self.inner.set_nonblocking(true)?;
        let result = self.connect(a).or_else(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                self.inner.wait_connected(t)
            } else {
                Err(error)
            }
        });
        let restore = self.inner.set_nonblocking(previous);
        result.and(restore)
    }
    pub fn listen(&self, backlog: i32) -> io::Result<()> {
        if self.kind != Type::STREAM {
            return unsupported();
        }
        self.inner
            .listen(u32::try_from(backlog).map_err(|_| io::ErrorKind::InvalidInput)?)
    }
    pub fn accept(&self) -> io::Result<(Self, SockAddr)> {
        if self.kind != Type::STREAM {
            return unsupported();
        }
        self.inner
            .accept()
            .map(|(s, a)| (Self::from_native(s, Type::STREAM), a.into()))
    }
    pub fn local_addr(&self) -> io::Result<SockAddr> {
        self.inner.local_addr().map(Into::into)
    }
    pub fn peer_addr(&self) -> io::Result<SockAddr> {
        self.inner.peer_addr().map(Into::into)
    }
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.inner.take_error()
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.inner.shutdown(how)
    }
    pub fn set_nonblocking(&self, v: bool) -> io::Result<()> {
        self.inner.set_nonblocking(v)
    }
    pub fn nonblocking(&self) -> io::Result<bool> {
        Ok(self.inner.nonblocking())
    }
    pub fn domain(&self) -> io::Result<Domain> {
        match self.domain {
            Some(v) => Ok(v),
            None => self.inner.local_addr().map(Domain::for_address),
        }
    }
    pub fn r#type(&self) -> io::Result<Type> {
        Ok(self.kind)
    }
    pub fn protocol(&self) -> io::Result<Option<Protocol>> {
        Ok(Some(if self.kind == Type::DGRAM {
            Protocol::UDP
        } else {
            Protocol::TCP
        }))
    }
    pub fn set_tcp_nodelay(&self, v: bool) -> io::Result<()> {
        self.inner.option(1, Some(u32::from(v))).map(|_| ())
    }
    pub fn tcp_nodelay(&self) -> io::Result<bool> {
        self.inner.option(1, None).map(|v| v != 0)
    }
    pub fn set_nodelay(&self, v: bool) -> io::Result<()> {
        self.set_tcp_nodelay(v)
    }
    pub fn nodelay(&self) -> io::Result<bool> {
        self.tcp_nodelay()
    }
    pub fn set_ttl_v4(&self, v: u32) -> io::Result<()> {
        self.inner.option(2, Some(v)).map(|_| ())
    }
    pub fn ttl_v4(&self) -> io::Result<u32> {
        self.inner.option(2, None)
    }
    pub fn set_ttl(&self, v: u32) -> io::Result<()> {
        self.set_ttl_v4(v)
    }
    pub fn ttl(&self) -> io::Result<u32> {
        self.ttl_v4()
    }
    pub fn set_read_timeout(&self, v: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(v)
    }
    pub fn set_write_timeout(&self, v: Option<Duration>) -> io::Result<()> {
        self.inner.set_write_timeout(v)
    }
    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.inner.read_timeout())
    }
    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.inner.write_timeout())
    }
    pub fn recv(&self, out: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
        self.recv_with_flags(out, 0)
    }
    pub fn peek(&self, out: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
        self.recv_with_flags(out, 2)
    }
    pub fn recv_with_flags(&self, out: &mut [MaybeUninit<u8>], flags: i32) -> io::Result<usize> {
        if flags != 0 && flags != 2 {
            return unsupported();
        }
        for v in out.iter_mut() {
            v.write(0);
        }
        // Every element was initialized above; no uninitialized u8 is referenced.
        let out =
            unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr().cast::<u8>(), out.len()) };
        if self.kind == Type::DGRAM {
            self.inner.udp_recv(out, flags == 2).map(|(n, _)| n)
        } else {
            self.inner.read(out, flags == 2)
        }
    }
    pub fn send(&self, data: &[u8]) -> io::Result<usize> {
        self.send_with_flags(data, 0)
    }
    pub fn send_with_flags(&self, data: &[u8], flags: i32) -> io::Result<usize> {
        if flags != 0 {
            return unsupported();
        }
        if self.kind == Type::DGRAM {
            self.inner.udp_send(data, self.inner.peer_addr()?)
        } else {
            self.inner.write(data)
        }
    }
    pub fn send_to(&self, data: &[u8], addr: &SockAddr) -> io::Result<usize> {
        if self.kind != Type::DGRAM {
            return unsupported();
        }
        self.inner.udp_send(data, addr.0)
    }
}
macro_rules! unavailable{($($name:ident($($arg:ident:$ty:ty),*)->$result:ty),*$(,)?)=>{impl Socket{$(pub fn $name(&self,$($arg:$ty),*)->io::Result<$result>{$(let _=$arg;)*unsupported()})*}};}
unavailable!(
 set_reuse_address(v:bool)->(),reuse_address()->bool,set_reuse_port(v:bool)->(),reuse_port()->bool,
 set_keepalive(v:bool)->(),keepalive()->bool,set_tcp_keepalive(v:&TcpKeepalive)->(),
 set_linger(v:Option<Duration>)->(),linger()->Option<Duration>,set_only_v6(v:bool)->(),only_v6()->bool,
 set_recv_buffer_size(v:usize)->(),recv_buffer_size()->usize,set_send_buffer_size(v:usize)->(),send_buffer_size()->usize,
 set_broadcast(v:bool)->(),broadcast()->bool,set_out_of_band_inline(v:bool)->(),out_of_band_inline()->bool,
 set_tos_v4(v:u32)->(),set_tos(v:u32)->(),set_recv_tos_v4(v:bool)->(),set_tclass_v6(v:u32)->(),set_recv_tclass_v6(v:bool)->(),
 set_unicast_hops_v6(v:u32)->(),set_recv_hoplimit_v6(v:bool)->(),set_header_included_v4(v:bool)->(),set_header_included_v6(v:bool)->(),
 set_multicast_loop_v4(v:bool)->(),set_multicast_ttl_v4(v:u32)->(),set_multicast_if_v4(v:&Ipv4Addr)->(),
 set_multicast_loop_v6(v:bool)->(),set_multicast_hops_v6(v:u32)->(),set_multicast_if_v6(v:u32)->(),
 join_multicast_v4(a:&Ipv4Addr,b:&Ipv4Addr)->(),join_multicast_v6(a:&Ipv6Addr,b:u32)->(),
 leave_multicast_v4(a:&Ipv4Addr,b:&Ipv4Addr)->(),leave_multicast_v6(a:&Ipv6Addr,b:u32)->(),
 set_tcp_user_timeout(v:Option<Duration>)->(),set_tcp_mss(v:u32)->(),
 bind_device(v:Option<&[u8]>)->(),set_mark(v:u32)->(),set_passcred(v:bool)->()
);
impl Read for Socket {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.kind == Type::DGRAM {
            self.inner.udp_recv(out, false).map(|(count, _)| count)
        } else {
            self.inner.read(out, false)
        }
    }
}
impl Read for &Socket {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.kind == Type::DGRAM {
            self.inner.udp_recv(out, false).map(|(count, _)| count)
        } else {
            self.inner.read(out, false)
        }
    }
}
impl Write for Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.send(data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Write for &Socket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.send(data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
macro_rules! conversions {
    ($ty:ident,$kind:ident) => {
        impl From<net::$ty> for Socket {
            fn from(s: net::$ty) -> Self {
                Self::from_native(s.into_native_socket(), Type::$kind)
            }
        }
        impl From<Socket> for net::$ty {
            fn from(s: Socket) -> Self {
                Self::from_native_socket(s.inner)
            }
        }
        impl<'a> From<&'a net::$ty> for SockRef<'a> {
            fn from(s: &'a net::$ty) -> Self {
                Self {
                    socket: Socket::from_native(s.native_socket().clone(), Type::$kind),
                    borrow: PhantomData,
                }
            }
        }
    };
}
conversions!(TcpStream, STREAM);
conversions!(TcpListener, STREAM);
conversions!(UdpSocket, DGRAM);
#[derive(Debug)]
pub struct SockRef<'a> {
    socket: Socket,
    borrow: PhantomData<&'a ()>,
}
impl<'a> From<&'a Socket> for SockRef<'a> {
    fn from(socket: &'a Socket) -> Self {
        Self {
            socket: socket.clone(),
            borrow: PhantomData,
        }
    }
}
impl Deref for SockRef<'_> {
    type Target = Socket;
    fn deref(&self) -> &Socket {
        &self.socket
    }
}
