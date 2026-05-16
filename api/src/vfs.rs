extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::vcabi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FsNodeKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FsStat {
    pub kind: FsNodeKind,
    pub len: u64,
}

pub fn read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
    v::vfs::read_file(path)
}

pub fn read_file_utf8(path: &[u8]) -> Result<String, i32> {
    v::vfs::read_file_utf8(path)
}

pub fn write_file(path: &[u8], data: &[u8]) -> Result<(), i32> {
    v::vfs::write_file(path, data)
}

pub fn create_dir_all(path: &[u8]) -> Result<(), i32> {
    v::vfs::create_dir_all(path)
}

pub fn stat(path: &[u8]) -> Result<FsStat, i32> {
    let mut raw_kind = 0u32;
    let mut len = 0u64;
    let rc = unsafe {
        vcabi::trueos_cabi_fs_stat(
            path.as_ptr(),
            path.len(),
            &mut raw_kind as *mut u32,
            &mut len as *mut u64,
        )
    };
    if rc != 0 {
        return Err(rc);
    }

    let kind = match raw_kind {
        1 => FsNodeKind::File,
        2 => FsNodeKind::Directory,
        _ => return Err(-4),
    };
    Ok(FsStat { kind, len })
}
