//! HTTP/1 and upgraded SQL sessions over the native Tokio/Mio socket path.
//!
//! Axum still owns routing, extraction and responses. Keeping the two I/O
//! adapters here avoids hyper-util's explicit std-compat runtime boundary.
use core::{pin::Pin, task::{Context, Poll}};
use axum::Router;
use hyper::service::service_fn;
use tokio::{io::{self, AsyncRead, AsyncWrite, ReadBuf}, net::TcpListener};
use tower::Service;

pub(super) async fn serve(listener: TcpListener, router: Router) -> io::Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                super::announce(&format!("sqlite-service: accept failed: {error}"));
                tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                continue;
            }
        };
        let router = router.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request| {
                let mut router = router.clone();
                async move { router.call(request.map(axum::body::Body::new)).await }
            });
            if let Err(error) = hyper::server::conn::http1::Builder::new()
                .serve_connection(HyperIo(stream), service)
                .with_upgrades()
                .await
            {
                super::announce(&format!("sqlite-service: connection from {peer} failed: {error}"));
            }
        });
    }
}

struct HyperIo<T>(T);

impl<T: AsyncRead + Unpin> hyper::rt::Read for HyperIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        mut output: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        // The inner reader initializes exactly its filled portion. Advance
        // Hyper's cursor only after it reports those bytes as initialized.
        let count = unsafe {
            let mut input = ReadBuf::uninit(output.as_mut());
            match Pin::new(&mut self.0).poll_read(context, &mut input) {
                Poll::Ready(Ok(())) => input.filled().len(),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        };
        unsafe { output.advance(count) };
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
    fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}

/// The inverse adapter lets the unchanged length-prefixed SQL protocol use
/// Tokio's read_exact/write_all helpers after Hyper transfers socket ownership.
pub(super) struct TokioIo<T>(T);

impl<T> TokioIo<T> {
    pub(super) fn new(inner: T) -> Self { Self(inner) }
}

impl<T: hyper::rt::Read + Unpin> AsyncRead for TokioIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let count = unsafe {
            let mut input = hyper::rt::ReadBuf::uninit(output.unfilled_mut());
            match Pin::new(&mut self.0).poll_read(context, input.unfilled()) {
                Poll::Ready(Ok(())) => input.filled().len(),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        };
        // Hyper filled this prefix of Tokio's unfilled portion; preserve any
        // bytes that the caller had already filled before this poll.
        unsafe { output.assume_init(count) };
        output.advance(count);
        Poll::Ready(Ok(()))
    }
}

impl<T: hyper::rt::Write + Unpin> AsyncWrite for TokioIo<T> {
    fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}
