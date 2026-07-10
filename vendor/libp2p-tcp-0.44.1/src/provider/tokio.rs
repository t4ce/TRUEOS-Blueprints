// Copyright 2020 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::{
    io, net,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    future::{BoxFuture, FutureExt},
    prelude::*,
};

use super::{Incoming, Provider};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn tokio_error_kind(kind: tokio::io::ErrorKind) -> io::ErrorKind {
    match kind {
        tokio::io::ErrorKind::NotFound => io::ErrorKind::NotFound,
        tokio::io::ErrorKind::PermissionDenied => io::ErrorKind::PermissionDenied,
        tokio::io::ErrorKind::ConnectionRefused => io::ErrorKind::ConnectionRefused,
        tokio::io::ErrorKind::ConnectionReset => io::ErrorKind::ConnectionReset,
        tokio::io::ErrorKind::ConnectionAborted => io::ErrorKind::ConnectionAborted,
        tokio::io::ErrorKind::NotConnected => io::ErrorKind::NotConnected,
        tokio::io::ErrorKind::AddrInUse => io::ErrorKind::AddrInUse,
        tokio::io::ErrorKind::AddrNotAvailable => io::ErrorKind::AddrNotAvailable,
        tokio::io::ErrorKind::BrokenPipe => io::ErrorKind::BrokenPipe,
        tokio::io::ErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
        tokio::io::ErrorKind::WouldBlock => io::ErrorKind::WouldBlock,
        tokio::io::ErrorKind::InvalidInput => io::ErrorKind::InvalidInput,
        tokio::io::ErrorKind::InvalidData => io::ErrorKind::InvalidData,
        tokio::io::ErrorKind::TimedOut => io::ErrorKind::TimedOut,
        tokio::io::ErrorKind::WriteZero => io::ErrorKind::WriteZero,
        tokio::io::ErrorKind::Interrupted => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn tokio_to_std_error(error: tokio::io::Error) -> io::Error {
    io::Error::new(
        tokio_error_kind(error.kind()),
        "libp2p TCP TRUEOS Tokio I/O error",
    )
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn map_tokio_poll<T>(poll: Poll<tokio::io::Result<T>>) -> Poll<io::Result<T>> {
    match poll {
        Poll::Pending => Poll::Pending,
        Poll::Ready(Ok(value)) => Poll::Ready(Ok(value)),
        Poll::Ready(Err(error)) => Poll::Ready(Err(tokio_to_std_error(error))),
    }
}

/// A TCP [`Transport`](libp2p_core::Transport) that works with the `tokio` ecosystem.
///
/// # Example
///
/// ```rust
/// # use libp2p_tcp as tcp;
/// # use libp2p_core::{Transport, transport::ListenerId};
/// # use futures::future;
/// # use std::pin::Pin;
/// #
/// # #[tokio::main]
/// # async fn main() {
/// let mut transport = tcp::tokio::Transport::new(tcp::Config::default());
/// let id = transport
///     .listen_on(ListenerId::next(), "/ip4/127.0.0.1/tcp/0".parse().unwrap())
///     .unwrap();
///
/// let addr = future::poll_fn(|cx| Pin::new(&mut transport).poll(cx))
///     .await
///     .into_new_address()
///     .unwrap();
///
/// println!("Listening on {addr}");
/// # }
/// ```
pub type Transport = crate::Transport<Tcp>;

#[derive(Copy, Clone)]
#[doc(hidden)]
pub enum Tcp {}

impl Provider for Tcp {
    type Stream = TcpStream;
    type Listener = tokio::net::TcpListener;
    type IfWatcher = if_watch::tokio::IfWatcher;

    fn new_if_watcher() -> io::Result<Self::IfWatcher> {
        Self::IfWatcher::new()
    }

    fn addrs(if_watcher: &Self::IfWatcher) -> Vec<if_watch::IpNet> {
        if_watcher.iter().copied().collect()
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn new_listener(l: net::TcpListener) -> io::Result<Self::Listener> {
        tokio::net::TcpListener::try_from(l)
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn new_listener_addr(addr: net::SocketAddr) -> io::Result<Self::Listener> {
        tokio::net::TcpListener::bind_addr(addr).map_err(tokio_to_std_error)
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn listener_local_addr(listener: &Self::Listener) -> io::Result<net::SocketAddr> {
        listener.local_addr().map_err(tokio_to_std_error)
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn new_stream(s: net::TcpStream) -> BoxFuture<'static, io::Result<Self::Stream>> {
        async move {
            // Taken from [`tokio::net::TcpStream::connect_mio`].

            let stream = tokio::net::TcpStream::try_from(s)?;

            // Once we've connected, wait for the stream to be writable as
            // that's when the actual connection has been initiated. Once we're
            // writable we check for `take_socket_error` to see if the connect
            // actually hit an error or not.
            //
            // If all that succeeded then we ship everything on up.
            stream.writable().await?;

            if let Some(e) = stream.take_error()? {
                return Err(e);
            }

            Ok(TcpStream(stream))
        }
        .boxed()
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn new_stream_addr(addr: net::SocketAddr) -> BoxFuture<'static, io::Result<Self::Stream>> {
        async move {
            tokio::net::TcpStream::connect(addr)
                .await
                .map(TcpStream)
                .map_err(tokio_to_std_error)
        }
        .boxed()
    }

    fn poll_accept(
        l: &mut Self::Listener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Incoming<Self::Stream>>> {
        let (stream, remote_addr) = match l.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => {
                #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
                return Poll::Ready(Err(e));
                #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
                return Poll::Ready(Err(tokio_to_std_error(e)));
            }
            Poll::Ready(Ok((stream, remote_addr))) => (stream, remote_addr),
        };

        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        let local_addr = stream.local_addr()?;
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        let local_addr = stream.local_addr().map_err(tokio_to_std_error)?;
        let stream = TcpStream(stream);

        Poll::Ready(Ok(Incoming {
            stream,
            local_addr,
            remote_addr,
        }))
    }
}

/// A [`tokio::net::TcpStream`] that implements [`AsyncRead`] and [`AsyncWrite`].
#[derive(Debug)]
pub struct TcpStream(pub tokio::net::TcpStream);

impl From<TcpStream> for tokio::net::TcpStream {
    fn from(t: TcpStream) -> tokio::net::TcpStream {
        t.0
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut read_buf = tokio::io::ReadBuf::new(buf);
        let result = futures::ready!(tokio::io::AsyncRead::poll_read(
            Pin::new(&mut self.0),
            cx,
            &mut read_buf
        ));
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        result?;
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        result.map_err(tokio_to_std_error)?;
        Poll::Ready(Ok(read_buf.filled().len()))
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        return tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf);
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        map_tokio_poll(tokio::io::AsyncWrite::poll_write(
            Pin::new(&mut self.0),
            cx,
            buf,
        ))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        return tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.0), cx);
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        map_tokio_poll(tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.0), cx))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        return tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx);
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        map_tokio_poll(tokio::io::AsyncWrite::poll_shutdown(
            Pin::new(&mut self.0),
            cx,
        ))
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        return tokio::io::AsyncWrite::poll_write_vectored(Pin::new(&mut self.0), cx, bufs);
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        {
            let platform_bufs: Vec<tokio::io::IoSlice<'_>> = bufs
                .iter()
                .map(|buf| tokio::io::IoSlice::new(&**buf))
                .collect();
            map_tokio_poll(tokio::io::AsyncWrite::poll_write_vectored(
                Pin::new(&mut self.0),
                cx,
                &platform_bufs,
            ))
        }
    }
}
