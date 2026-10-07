//! API-compatible resident sink. No queue or worker thread is created.
#![allow(missing_docs)]
use std::io::{self, Write};
use tracing_subscriber::fmt::writer::MakeWriter;

pub const DEFAULT_BUFFERED_LINES_LIMIT: usize = 128_000;
#[derive(Debug)]
#[must_use]
pub struct WorkerGuard;
impl Drop for WorkerGuard {
    fn drop(&mut self) {}
}
#[derive(Clone, Debug)]
pub struct NonBlocking;
#[derive(Clone, Debug)]
pub struct ErrorCounter;
impl ErrorCounter {
    pub fn dropped_lines(&self) -> usize {
        0
    }
}
impl NonBlocking {
    pub fn new<T: Write + Send + 'static>(writer: T) -> (Self, WorkerGuard) {
        NonBlockingBuilder::default().finish(writer)
    }
    pub fn error_counter(&self) -> ErrorCounter {
        ErrorCounter
    }
}
#[derive(Clone, Debug, Default)]
pub struct NonBlockingBuilder;
impl NonBlockingBuilder {
    pub fn buffered_lines_limit(self, _: usize) -> Self {
        self
    }
    pub fn lossy(self, _: bool) -> Self {
        self
    }
    pub fn thread_name(self, _: &str) -> Self {
        self
    }
    pub fn finish<T: Write + Send + 'static>(self, writer: T) -> (NonBlocking, WorkerGuard) {
        drop(writer);
        (NonBlocking, WorkerGuard)
    }
}
impl Write for NonBlocking {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        tracing_core::trueos::write_bytes(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl<'a> MakeWriter<'a> for NonBlocking {
    type Writer = NonBlocking;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
