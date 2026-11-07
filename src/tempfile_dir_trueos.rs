use crate::TempDir;
use crate::error::IoResultExt;
use std::path::PathBuf;
use std::{fs, io};

pub fn create(
    path: PathBuf,
    permissions: Option<&fs::Permissions>,
    disable_cleanup: bool,
) -> io::Result<TempDir> {
    if permissions.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "explicit Unix permissions have no native tempdir contract",
        ));
    }
    fs::create_dir(&path)
        .with_err_path(|| &path)
        .map(|_| TempDir {
            path: path.into_boxed_path(),
            disable_cleanup,
        })
}
