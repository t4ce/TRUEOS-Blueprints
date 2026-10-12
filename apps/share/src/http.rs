//! Direct HTTP/1 driver, matching the working TRUEOS websys socket path.
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use dav_server::DavHandler;
use hyper::service::service_fn;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;

pub async fn serve(listener: TcpListener, handler: DavHandler) -> io::Result<()> {
    // Native file handles retain file contents; bound simultaneous connections
    // as well as the per-file limit in the filesystem backend.
    let slots = Arc::new(tokio::sync::Semaphore::new(4));
    loop {
        let slot = slots
            .clone()
            .acquire_owned()
            .await
            .map_err(io::Error::other)?;
        let (stream, peer) = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let service = service_fn(move |request| {
                let handler = handler.clone();
                async move { Ok::<_, std::convert::Infallible>(handler.handle(request).await) }
            });
            if let Err(error) = hyper::server::conn::http1::Builder::new()
                .serve_connection(HyperIo(stream), service)
                .await
            {
                eprintln!("share: HTTP connection from {peer} failed: {error:?}");
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
    ) -> Poll<Result<(), io::Error>> {
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
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.0).poll_write(context, bytes)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.0).poll_flush(context)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}
