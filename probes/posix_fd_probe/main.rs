use core::ffi::{c_char, c_int, c_uint, c_void};

use trueos::{logl, logl::level, platform};

const PROBE_DIR: &[u8] = b"/common";
const PROBE_PATH: &[u8] = b"/common/posix-fd-probe.bin\0";

const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 0o2;
const O_CREAT: c_int = 0o100;
const O_TRUNC: c_int = 0o1000;

const SEEK_SET: c_int = 0;
const F_GETLK: c_int = 5;
const F_SETLK: c_int = 6;
const F_WRLCK: i16 = 1;
const F_UNLCK: i16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct Flock {
    l_type: i16,
    l_whence: i16,
    l_start: i64,
    l_len: i64,
    l_pid: c_int,
}

unsafe extern "C" {
    fn open(path: *const c_char, flags: c_int, mode: c_uint) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn fstat(fd: c_int, statbuf: *mut c_void) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, lock: *mut Flock) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    fn lseek(fd: c_int, offset: i64, whence: c_int) -> i64;
    fn pread(fd: c_int, buf: *mut c_void, count: usize, offset: i64) -> isize;
    fn pwrite(fd: c_int, buf: *const c_void, count: usize, offset: i64) -> isize;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn fsync(fd: c_int) -> c_int;
    fn fdatasync(fd: c_int) -> c_int;
}

fn log_stage(stage: &str) {
    logl::log(level::INFO, format_args!("posix-fd-probe: stage {}", stage));
}

fn log_rc(stage: &str, rc: c_int) -> bool {
    if rc == 0 {
        logl::log(
            level::INFO,
            format_args!("posix-fd-probe: success {} rc={}", stage, rc),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!("posix-fd-probe: failed {} rc={}", stage, rc),
        );
        false
    }
}

fn log_fd(stage: &str, fd: c_int) -> bool {
    if fd >= 0 {
        logl::log(
            level::INFO,
            format_args!("posix-fd-probe: success {} fd={}", stage, fd),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!("posix-fd-probe: failed {} fd={}", stage, fd),
        );
        false
    }
}

fn log_io(stage: &str, got: isize, expected: usize) -> bool {
    if got == expected as isize {
        logl::log(
            level::INFO,
            format_args!(
                "posix-fd-probe: success {} bytes={} expected={}",
                stage, got, expected
            ),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "posix-fd-probe: failed {} bytes={} expected={}",
                stage, got, expected
            ),
        );
        false
    }
}

fn log_seek(stage: &str, got: i64, expected: i64) -> bool {
    if got == expected {
        logl::log(
            level::INFO,
            format_args!(
                "posix-fd-probe: success {} offset={} expected={}",
                stage, got, expected
            ),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!(
                "posix-fd-probe: failed {} offset={} expected={}",
                stage, got, expected
            ),
        );
        false
    }
}

fn log_match(stage: &str, matched: bool) -> bool {
    if matched {
        logl::log(
            level::INFO,
            format_args!("posix-fd-probe: success {}", stage),
        );
        true
    } else {
        logl::log(
            level::ERROR,
            format_args!("posix-fd-probe: failed {}", stage),
        );
        false
    }
}

fn run_probe() -> Result<(), &'static str> {

    log_stage("open.O_RDWR_O_CREAT_O_TRUNC");
    let fd = unsafe {
        open(
            PROBE_PATH.as_ptr().cast(),
            O_RDWR | O_CREAT | O_TRUNC,
            0o644,
        )
    };
    if !log_fd("open.O_RDWR_O_CREAT_O_TRUNC", fd) {
        return Err("open");
    }

    let mut ok = true;

    log_stage("fstat.initial");
    let mut statbuf = [0u8; 256];
    ok &= log_rc("fstat.initial", unsafe {
        fstat(fd, statbuf.as_mut_ptr().cast())
    });

    let block_seq = b"TRUEOS sequential fd write/read\n";
    log_stage("lseek.start0");
    ok &= log_seek("lseek.start0", unsafe { lseek(fd, 0, SEEK_SET) }, 0);

    log_stage("write.sequential");
    ok &= log_io(
        "write.sequential",
        unsafe { write(fd, block_seq.as_ptr().cast(), block_seq.len()) },
        block_seq.len(),
    );

    log_stage("fsync.after_sequential_write");
    ok &= log_rc("fsync.after_sequential_write", unsafe { fsync(fd) });

    log_stage("lseek.readback0");
    ok &= log_seek("lseek.readback0", unsafe { lseek(fd, 0, SEEK_SET) }, 0);

    log_stage("read.sequential");
    let mut read_seq = [0u8; 64];
    ok &= log_io(
        "read.sequential",
        unsafe { read(fd, read_seq.as_mut_ptr().cast(), block_seq.len()) },
        block_seq.len(),
    );

    let block_zero = b"TRUEOS sqlite unix-vfs fd probe block zero\n";
    log_stage("pwrite.offset0");
    ok &= log_io(
        "pwrite.offset0",
        unsafe { pwrite(fd, block_zero.as_ptr().cast(), block_zero.len(), 0) },
        block_zero.len(),
    );

    let block_sparse = b"TRUEOS offset 4096";
    log_stage("pwrite.offset4096");
    ok &= log_io(
        "pwrite.offset4096",
        unsafe { pwrite(fd, block_sparse.as_ptr().cast(), block_sparse.len(), 4096) },
        block_sparse.len(),
    );

    log_stage("pread.offset0");
    let mut read_zero = [0u8; 64];
    ok &= log_io(
        "pread.offset0",
        unsafe { pread(fd, read_zero.as_mut_ptr().cast(), block_zero.len(), 0) },
        block_zero.len(),
    );

    log_stage("pread.offset4096");
    let mut read_sparse = [0u8; 64];
    ok &= log_io(
        "pread.offset4096",
        unsafe {
            pread(
                fd,
                read_sparse.as_mut_ptr().cast(),
                block_sparse.len(),
                4096,
            )
        },
        block_sparse.len(),
    );

    log_stage("ftruncate.grow8192");
    ok &= log_rc("ftruncate.grow8192", unsafe { ftruncate(fd, 8192) });

    log_stage("ftruncate.shrink4114");
    ok &= log_rc("ftruncate.shrink4114", unsafe { ftruncate(fd, 4114) });

    log_stage("fsync");
    ok &= log_rc("fsync", unsafe { fsync(fd) });

    log_stage("fdatasync");
    ok &= log_rc("fdatasync", unsafe { fdatasync(fd) });

    log_stage("fcntl.F_GETLK");
    let mut get_lock = Flock {
        l_type: F_WRLCK,
        l_whence: SEEK_SET as i16,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    let getlk_ok = log_rc("fcntl.F_GETLK", unsafe {
        fcntl(fd, F_GETLK, &mut get_lock)
    });
    ok &= getlk_ok;
    if getlk_ok {
        logl::log(
            level::INFO,
            format_args!(
                "posix-fd-probe: fcntl.F_GETLK result l_type={} l_pid={}",
                get_lock.l_type, get_lock.l_pid
            ),
        );
        if get_lock.l_type != F_UNLCK {
            logl::log(
                level::ERROR,
                format_args!(
                    "posix-fd-probe: failed fcntl.F_GETLK.semantic l_type={} expected={}",
                    get_lock.l_type, F_UNLCK
                ),
            );
            ok = false;
        }
    }

    log_stage("fcntl.F_SETLK.write_lock");
    let mut write_lock = Flock {
        l_type: F_WRLCK,
        l_whence: SEEK_SET as i16,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    ok &= log_rc("fcntl.F_SETLK.write_lock", unsafe {
        fcntl(fd, F_SETLK, &mut write_lock)
    });

    log_stage("fcntl.F_SETLK.unlock");
    let mut unlock = Flock {
        l_type: F_UNLCK,
        l_whence: SEEK_SET as i16,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    ok &= log_rc("fcntl.F_SETLK.unlock", unsafe {
        fcntl(fd, F_SETLK, &mut unlock)
    });

    log_stage("fstat.final");
    ok &= log_rc("fstat.final", unsafe {
        fstat(fd, statbuf.as_mut_ptr().cast())
    });

    log_stage("close");
    ok &= log_rc("close", unsafe { close(fd) });

    log_stage("open.O_RDWR_existing");
    let rw_fd = unsafe { open(PROBE_PATH.as_ptr().cast(), O_RDWR, 0) };
    if log_fd("open.O_RDWR_existing", rw_fd) {
        log_stage("lseek.rw_existing0");
        ok &= log_seek(
            "lseek.rw_existing0",
            unsafe { lseek(rw_fd, 0, SEEK_SET) },
            0,
        );

        log_stage("read.rw_existing");
        let mut rw_buf = [0u8; 64];
        ok &= log_io(
            "read.rw_existing",
            unsafe { read(rw_fd, rw_buf.as_mut_ptr().cast(), block_seq.len()) },
            block_seq.len(),
        );

        log_stage("close.rw_existing");
        ok &= log_rc("close.rw_existing", unsafe { close(rw_fd) });
    } else {
        ok = false;
    }

    log_stage("open.O_RDONLY");
    let read_fd = unsafe { open(PROBE_PATH.as_ptr().cast(), O_RDONLY, 0) };
    if log_fd("open.O_RDONLY", read_fd) {
        log_stage("lseek.readonly0");
        ok &= log_seek("lseek.readonly0", unsafe { lseek(read_fd, 0, SEEK_SET) }, 0);

        log_stage("read.readonly");
        let mut readonly_buf = [0u8; 64];
        ok &= log_io(
            "read.readonly",
            unsafe { read(read_fd, readonly_buf.as_mut_ptr().cast(), block_seq.len()) },
            block_seq.len(),
        );

        log_stage("close.readonly");
        ok &= log_rc("close.readonly", unsafe { close(read_fd) });
    } else {
        ok = false;
    }

    if ok {
        Ok(())
    } else {
        Err("one_or_more_posix_fd_stages")
    }
}

fn main() {
    logl::log(level::INFO, "posix-fd-probe: blueprint start");
    match run_probe() {
        Ok(()) => logl::log(level::INFO, "posix-fd-probe: ok"),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("posix-fd-probe: failed stage={}", stage),
        ),
    }
    platform::poll_once();
}
