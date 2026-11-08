//! TRUEOS temporary files own native file objects, never Unix descriptors.
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

pub fn create_named(
    path: &Path,
    options: &mut OpenOptions,
    permissions: Option<&fs::Permissions>,
) -> io::Result<File> {
    if permissions.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "explicit Unix permissions have no native tempfile contract",
        ));
    }
    options.read(true).write(true).create_new(true).open(path)
}

pub fn create(dir: &Path) -> io::Result<File> {
    crate::util::create_helper(
        dir,
        OsStr::new(".tmp"),
        OsStr::new(""),
        crate::NUM_RAND_CHARS,
        |path| {
            let file = create_named(&path, &mut OpenOptions::new(), None)?;
            // Native objects retain their bytes after their final name is removed.
            // Propagate failure instead of returning a supposedly anonymous file.
            fs::remove_file(&path)?;
            Ok(file)
        },
    )
}

pub fn reopen(file: &File, path: &Path) -> io::Result<File> {
    // Opening again gives an independent cursor. try_clone shares the cursor.
    let before = file.metadata()?;
    let before_id = stable_id(&before)?;
    let reopened = OpenOptions::new().read(true).write(true).open(path)?;
    let after = reopened.metadata()?;
    let after_id = stable_id(&after)?;
    if before.root_id() != after.root_id() || before_id != after_id {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "original tempfile has been replaced",
        ));
    }
    Ok(reopened)
}

fn stable_id(metadata: &fs::Metadata) -> io::Result<u64> {
    metadata.file_id().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "native tempfile metadata has no stable filesystem object identity",
        )
    })
}

pub fn persist(old: &Path, new: &Path, overwrite: bool) -> io::Result<()> {
    if overwrite {
        fs::rename(old, new)
    } else {
        rename_without_replace(old, new)
    }
}

pub fn keep(_: &Path) -> io::Result<()> {
    Ok(())
}

unsafe extern "C" {
    fn trueos_cabi_async_fs_rename_start_v2(
        old: *const u8,
        old_len: usize,
        new: *const u8,
        new_len: usize,
        flags: u32,
    ) -> i32;
    fn trueos_cabi_async_fs_status(id: u32) -> i32;
    fn trueos_cabi_async_fs_discard(id: u32) -> i32;
}

fn native_error(code: i32) -> io::Error {
    let kind = match code {
        -1 | -4 | -6 => io::ErrorKind::InvalidInput,
        -3 => io::ErrorKind::OutOfMemory,
        -7 => io::ErrorKind::FileTooLarge,
        -8 => io::ErrorKind::NotFound,
        -9 => io::ErrorKind::AlreadyExists,
        -14 => io::ErrorKind::TimedOut,
        -17 | -18 => io::ErrorKind::WouldBlock,
        -19 => io::ErrorKind::PermissionDenied,
        -20 => io::ErrorKind::Unsupported,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("native tempfile rename failed ({code})"))
}

fn path_bytes(path: &Path) -> io::Result<&[u8]> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.is_empty()
        || bytes.len() > 1024
        || bytes.contains(&0)
        || std::str::from_utf8(bytes).is_err()
    {
        return Err(native_error(-4));
    }
    Ok(bytes)
}

fn rename_without_replace(old: &Path, new: &Path) -> io::Result<()> {
    let old = path_bytes(old)?;
    let new = path_bytes(new)?;
    // flags=0 is the native atomic no-replace contract. Never replace this with
    // exists()+rename(), nor a hard-link emulation unsupported by TRUEOS.
    let id = unsafe {
        trueos_cabi_async_fs_rename_start_v2(old.as_ptr(), old.len(), new.as_ptr(), new.len(), 0)
    };
    if id <= 0 {
        return Err(native_error(id));
    }
    struct Operation(u32);
    impl Drop for Operation {
        fn drop(&mut self) {
            unsafe { trueos_cabi_async_fs_discard(self.0) };
        }
    }
    let operation = Operation(id as u32);
    loop {
        match unsafe { trueos_cabi_async_fs_status(operation.0) } {
            0 => std::thread::yield_now(),
            1 => return Ok(()),
            code => return Err(native_error(code)),
        }
    }
}
