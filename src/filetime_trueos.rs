//! No synthetic native filesystem timestamps and no descriptor conversions.
use crate::FileTime;
use std::{
    fs::{self, File},
    io,
    path::Path,
};
fn unavailable() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native files have no mutable wall-clock timestamp contract",
    ))
}
pub fn set_file_times(_: &Path, _: FileTime, _: FileTime) -> io::Result<()> {
    unavailable()
}
pub fn set_symlink_file_times(_: &Path, _: FileTime, _: FileTime) -> io::Result<()> {
    unavailable()
}
pub fn set_file_mtime(_: &Path, _: FileTime) -> io::Result<()> {
    unavailable()
}
pub fn set_file_atime(_: &Path, _: FileTime) -> io::Result<()> {
    unavailable()
}
pub fn set_file_handle_times(
    _: &File,
    atime: Option<FileTime>,
    mtime: Option<FileTime>,
) -> io::Result<()> {
    if atime.is_none() && mtime.is_none() {
        Ok(())
    } else {
        unavailable()
    }
}
// These upstream APIs have no error channel. Use a real timestamp when the
// filesystem supplies one, otherwise fail explicitly rather than invent zero.
pub fn from_last_modification_time(meta: &fs::Metadata) -> FileTime {
    meta.modified().expect("native metadata has no wall-clock modification time; use Metadata::modified to check support").into()
}
pub fn from_last_access_time(meta: &fs::Metadata) -> FileTime {
    meta.accessed().expect("native metadata has no wall-clock access time; use Metadata::accessed to check support").into()
}
pub fn from_creation_time(meta: &fs::Metadata) -> Option<FileTime> {
    meta.created().ok().map(Into::into)
}
