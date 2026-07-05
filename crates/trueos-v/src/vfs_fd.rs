use crate::vcabi;

pub use crate::bp_abi::{TrueosCabiFdLock, TrueosCabiFdStat};

pub const O_RDONLY: i32 = 0;
pub const O_RDWR: i32 = 0o2;
pub const O_CREAT: i32 = 0o100;
pub const O_TRUNC: i32 = 0o1000;

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

pub const F_GETLK: i32 = 5;
pub const F_SETLK: i32 = 6;
pub const F_RDLCK: i16 = 0;
pub const F_WRLCK: i16 = 1;
pub const F_UNLCK: i16 = 2;

#[inline]
pub fn open(path: &[u8], flags: i32, mode: u32) -> Result<i32, i32> {
    let fd = unsafe { vcabi::trueos_cabi_fs_fd_open(path.as_ptr(), path.len(), flags, mode) };
    if fd < 0 { Err(fd) } else { Ok(fd) }
}

#[inline]
pub fn close(fd: i32) -> Result<(), i32> {
    rc_unit(unsafe { vcabi::trueos_cabi_fs_fd_close(fd) })
}

#[inline]
pub fn read(fd: i32, out: &mut [u8]) -> Result<usize, i32> {
    ssize(unsafe { vcabi::trueos_cabi_fs_fd_read(fd, out.as_mut_ptr(), out.len()) })
}

#[inline]
pub fn write(fd: i32, data: &[u8]) -> Result<usize, i32> {
    ssize(unsafe { vcabi::trueos_cabi_fs_fd_write(fd, data.as_ptr(), data.len()) })
}

#[inline]
pub fn lseek(fd: i32, offset: i64, whence: i32) -> Result<i64, i32> {
    let pos = unsafe { vcabi::trueos_cabi_fs_fd_lseek(fd, offset, whence) };
    if pos < 0 { Err(pos as i32) } else { Ok(pos) }
}

#[inline]
pub fn pread(fd: i32, out: &mut [u8], offset: i64) -> Result<usize, i32> {
    ssize(unsafe { vcabi::trueos_cabi_fs_fd_pread(fd, out.as_mut_ptr(), out.len(), offset) })
}

#[inline]
pub fn pwrite(fd: i32, data: &[u8], offset: i64) -> Result<usize, i32> {
    ssize(unsafe { vcabi::trueos_cabi_fs_fd_pwrite(fd, data.as_ptr(), data.len(), offset) })
}

#[inline]
pub fn fstat(fd: i32) -> Result<TrueosCabiFdStat, i32> {
    let mut stat = TrueosCabiFdStat::default();
    let rc = unsafe { vcabi::trueos_cabi_fs_fd_fstat(fd, &mut stat) };
    if rc != 0 { Err(rc) } else { Ok(stat) }
}

#[inline]
pub fn ftruncate(fd: i32, len: i64) -> Result<(), i32> {
    rc_unit(unsafe { vcabi::trueos_cabi_fs_fd_ftruncate(fd, len) })
}

#[inline]
pub fn fsync(fd: i32) -> Result<(), i32> {
    rc_unit(unsafe { vcabi::trueos_cabi_fs_fd_fsync(fd) })
}

#[inline]
pub fn fdatasync(fd: i32) -> Result<(), i32> {
    rc_unit(unsafe { vcabi::trueos_cabi_fs_fd_fdatasync(fd) })
}

#[inline]
pub fn fcntl(fd: i32, cmd: i32, lock: &mut TrueosCabiFdLock) -> Result<(), i32> {
    rc_unit(unsafe { vcabi::trueos_cabi_fs_fd_fcntl(fd, cmd, lock) })
}

#[inline]
fn rc_unit(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

#[inline]
fn ssize(value: isize) -> Result<usize, i32> {
    if value < 0 {
        Err(value as i32)
    } else {
        Ok(value as usize)
    }
}
