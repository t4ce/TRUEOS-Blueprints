//! Resident replacement for rolling files. Paths are compatibility arguments;
//! construction, writes, and flushes never access the filesystem.
#![allow(missing_docs)]
use std::{
    fmt,
    io::{self, Write},
    marker::PhantomData,
    path::Path,
};
use tracing_subscriber::fmt::writer::MakeWriter;

#[derive(Debug)]
pub struct RollingFileAppender;
#[derive(Debug)]
pub struct RollingWriter<'a>(PhantomData<&'a RollingFileAppender>);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rotation(u8);
impl Rotation {
    pub const MINUTELY: Self = Self(1);
    pub const HOURLY: Self = Self(2);
    pub const DAILY: Self = Self(3);
    pub const WEEKLY: Self = Self(4);
    pub const NEVER: Self = Self(0);
}
#[derive(Debug)]
pub struct InitError;
impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("resident logging initialization")
    }
}
impl std::error::Error for InitError {}
#[derive(Debug, Default)]
pub struct Builder;
impl Builder {
    pub const fn new() -> Self {
        Self
    }
    pub fn rotation(self, _: Rotation) -> Self {
        self
    }
    pub fn filename_prefix(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn filename_suffix(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn latest_symlink(self, _: impl Into<String>) -> Self {
        self
    }
    pub fn max_log_files(self, _: usize) -> Self {
        self
    }
    pub fn build(&self, _: impl AsRef<Path>) -> Result<RollingFileAppender, InitError> {
        Ok(RollingFileAppender)
    }
}
impl RollingFileAppender {
    pub fn new(_: Rotation, _: impl AsRef<Path>, _: impl AsRef<Path>) -> Self {
        Self
    }
    pub fn builder() -> Builder {
        Builder
    }
}
impl Write for RollingFileAppender {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        tracing_core::trueos::write_bytes(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Write for RollingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        tracing_core::trueos::write_bytes(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl<'a> MakeWriter<'a> for RollingFileAppender {
    type Writer = RollingWriter<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        RollingWriter(PhantomData)
    }
}
pub fn minutely(directory: impl AsRef<Path>, filename: impl AsRef<Path>) -> RollingFileAppender {
    RollingFileAppender::new(Rotation::MINUTELY, directory, filename)
}
pub fn hourly(directory: impl AsRef<Path>, filename: impl AsRef<Path>) -> RollingFileAppender {
    RollingFileAppender::new(Rotation::HOURLY, directory, filename)
}
pub fn daily(directory: impl AsRef<Path>, filename: impl AsRef<Path>) -> RollingFileAppender {
    RollingFileAppender::new(Rotation::DAILY, directory, filename)
}
pub fn never(directory: impl AsRef<Path>, filename: impl AsRef<Path>) -> RollingFileAppender {
    RollingFileAppender::new(Rotation::NEVER, directory, filename)
}

pub fn weekly(directory: impl AsRef<Path>, filename: impl AsRef<Path>) -> RollingFileAppender {
    RollingFileAppender::new(Rotation::WEEKLY, directory, filename)
}
