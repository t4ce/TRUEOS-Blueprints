//! HTTP/1 over the native Tokio/Mio socket path.
//! Based on apps/sqlite/src/native_transport.rs, the existing service boundary.
//!
//! Axum still owns routing, extraction and responses. Keeping the two I/O
//! adapters here avoids hyper-util's explicit std-compat runtime boundary.
use axum::Router;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
#[cfg(target_os = "trueos")]
use hyper::io;
use hyper::service::service_fn;
#[cfg(not(target_os = "trueos"))]
use std::io;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpListener,
};
use tower::Service;

fn hyper_error(error: std::io::Error) -> io::Error {
    #[cfg(target_os = "trueos")]
    {
        let _ = error;
        io::Error::new(io::ErrorKind::Other, "native Lily socket I/O failed")
    }
    #[cfg(not(target_os = "trueos"))]
    {
        error
    }
}

pub(super) async fn serve(listener: TcpListener, router: Router) -> Result<(), String> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                super::announce(&format!("lily2vec-service: accept failed: {error}"));
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
                .await
            {
                super::announce(&format!(
                    "lily2vec-service: connection from {peer} failed: {error}"
                ));
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
                Poll::Ready(Err(error)) => return Poll::Ready(Err(hyper_error(error))),
                Poll::Pending => return Poll::Pending,
            }
        };
        unsafe { output.advance(count) };
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> hyper::rt::Write for HyperIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0)
            .poll_write(context, bytes)
            .map_err(hyper_error)
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0)
            .poll_flush(context)
            .map_err(hyper_error)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0)
            .poll_shutdown(context)
            .map_err(hyper_error)
    }
}
