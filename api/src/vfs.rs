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
