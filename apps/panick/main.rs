use core::ffi::c_char;

use trueos::{
    logl,
    logl::level,
    platform,
    vfs,
};

const PROBE_DIR: &[u8] = b"/common";
const PROBE_PATH_BYTES: &[u8] = b"/common/panick-memory-probe.bin";
const PROBE_PATH_CSTR: &[u8] = b"/common/panick-memory-probe.bin\0";

const BAD_PTR_LOW: *mut u8 = 0x1usize as *mut u8;
const BAD_PTR_HIGH: *mut u8 = 0xFFFF_FFFF_FFFF_F000usize as *mut u8;

unsafe extern "C" {
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;

    fn open(path: *const c_char, flags: i32, mode: u32) -> i32;
    fn read(fd: i32, buf: *mut core::ffi::c_void, count: usize) -> isize;
    fn write(fd: i32, buf: *const core::ffi::c_void, count: usize) -> isize;
    fn close(fd: i32) -> i32;
}

fn log_stage(stage: &str) {
    logl::log(level::INFO, format_args!("panick: stage {}", stage));
}

fn run_safe_controls() -> Result<(), &'static str> {
    log_stage("prepare_probe_file");
    vfs::create_dir_all(PROBE_DIR).map_err(|_| "vfs.create_dir_all")?;
    vfs::write_file(PROBE_PATH_BYTES, b"panick-pointer-probe\n").map_err(|_| "vfs.write_file")?;

    log_stage("cabi_read_len_probe");
    let len = unsafe { trueos_cabi_fs_read_file(PROBE_PATH_BYTES.as_ptr(), PROBE_PATH_BYTES.len(), core::ptr::null_mut(), 0) };
    logl::log(level::INFO, format_args!("panick: cabi len probe rc={}", len));
    if len <= 0 {
        return Err("cabi_read_len_probe");
    }

    log_stage("libc_write_bad_fd_control");
    let rc = unsafe { write(-1, b"x".as_ptr().cast(), 1) };
    logl::log(level::INFO, format_args!("panick: libc write bad-fd rc={}", rc));

    Ok(())
}

fn run_dangerous_pointer_probe(path_ptr: *mut u8) {
    log_stage("dangerous_cabi_read_bad_pointer");
    logl::log(
        level::WARN,
        format_args!(
            "panick: about to call trueos_cabi_fs_read_file with out_ptr=0x{:016X}",
            path_ptr as usize
        ),
    );
    let rc = unsafe {
        trueos_cabi_fs_read_file(
            PROBE_PATH_BYTES.as_ptr(),
            PROBE_PATH_BYTES.len(),
            path_ptr,
            16,
        )
    };
    logl::log(
        level::WARN,
        format_args!("panick: dangerous probe returned rc={}", rc),
    );
}

fn run_dangerous_posix_probe(path_ptr: *mut u8) {
    const O_RDONLY: i32 = 0;

    log_stage("dangerous_posix_read_bad_pointer");
    let fd = unsafe { open(PROBE_PATH_CSTR.as_ptr().cast(), O_RDONLY, 0) };
    if fd < 0 {
        logl::log(level::ERROR, "panick: open failed for dangerous posix probe");
        return;
    }

    logl::log(
        level::WARN,
        format_args!(
            "panick: about to read into out_ptr=0x{:016X} via read()",
            path_ptr as usize
        ),
    );
    let rc = unsafe { read(fd, path_ptr.cast(), 16) };
    logl::log(level::WARN, format_args!("panick: dangerous posix probe rc={}", rc));
    let _ = unsafe { close(fd) };
}

fn main() {
    logl::log(level::INFO, "panick: blueprint start");

    match run_safe_controls() {
        Ok(()) => logl::log(level::INFO, "panick: safe controls ok"),
        Err(stage) => {
            logl::log(level::ERROR, format_args!("panick: failed stage={}", stage));
            platform::poll_once();
            return;
        }
    }

    run_dangerous_pointer_probe(BAD_PTR_LOW);
    run_dangerous_posix_probe(BAD_PTR_LOW);
    run_dangerous_pointer_probe(BAD_PTR_HIGH);
    run_dangerous_posix_probe(BAD_PTR_HIGH);

    logl::log(level::INFO, "panick: done");
    platform::poll_once();
}
