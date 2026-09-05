use core::ffi::c_char;

use trueos::{async_fs, clock, logl, logl::level, platform};

const PROBE_DIR: &[u8] = b"/common";
const PROBE_PATH_BYTES: &[u8] = b"/common/panick-memory-probe.bin";
const PROBE_PATH_CSTR: &[u8] = b"/common/panick-memory-probe.bin\0";

const BAD_PTR_LOW: *mut u8 = 0x1usize as *mut u8;
const BAD_PTR_HIGH: *mut u8 = 0xFFFF_FFFF_FFFF_F000usize as *mut u8;

unsafe extern "C" {
    fn trueos_cabi_async_fs_read_start(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_async_fs_status(id: u32) -> i32;
    fn trueos_cabi_async_fs_result_len(id: u32) -> isize;
    fn trueos_cabi_async_fs_result_read(
        id: u32,
        offset: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_async_fs_discard(id: u32) -> i32;

    fn open(path: *const c_char, flags: i32, mode: u32) -> i32;
    fn read(fd: i32, buf: *mut core::ffi::c_void, count: usize) -> isize;
    fn close(fd: i32) -> i32;
}

struct ReadResult(u32);

impl Drop for ReadResult {
    fn drop(&mut self) {
        let _ = unsafe { trueos_cabi_async_fs_discard(self.0) };
    }
}

fn prepare_read() -> Result<ReadResult, &'static str> {
    let id = unsafe {
        trueos_cabi_async_fs_read_start(PROBE_PATH_BYTES.as_ptr(), PROBE_PATH_BYTES.len())
    };
    if id <= 0 { return Err("async.read.start"); }
    let operation = ReadResult(id as u32);
    let started = clock::monotonic_millis();
    loop {
        match unsafe { trueos_cabi_async_fs_status(operation.0) } {
            1 => break,
            0 if clock::monotonic_millis().saturating_sub(started) < 2_000 => {
                platform::poll_once();
                platform::sleep_ms(1);
            }
            0 => return Err("async.read.timeout"),
            _ => return Err("async.read.status"),
        }
    }
    if unsafe { trueos_cabi_async_fs_result_len(operation.0) } < 16 {
        return Err("async.read.empty-result");
    }
    Ok(operation)
}

fn log_stage(stage: &str) {
    logl::log(level::INFO, format_args!("panick: stage {}", stage));
}

fn run_dangerous_pointer_probe(path_ptr: *mut u8) {
    log_stage("dangerous_async_cabi_result_read_bad_pointer");
    let operation = match prepare_read() {
        Ok(operation) => operation,
        Err(stage) => {
            logl::log(level::ERROR, format_args!("panick: FAIL setup={stage}"));
            return;
        }
    };
    logl::log(
        level::WARN,
        format_args!(
            "panick: about to call trueos_cabi_async_fs_result_read with out_ptr=0x{:016X}",
            path_ptr as usize
        ),
    );
    let rc = unsafe {
        trueos_cabi_async_fs_result_read(
            operation.0,
            0,
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
        logl::log(
            level::ERROR,
            "panick: open failed for dangerous posix probe",
        );
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
    logl::log(
        level::WARN,
        format_args!("panick: dangerous posix probe rc={}", rc),
    );
    let _ = unsafe { close(fd) };
}

fn main() {
    logl::log(level::INFO, "panick: blueprint start");

    // Ensure a valid nonempty read result before deliberately supplying an
    // invalid destination. A missing file must not masquerade as containment.
    let setup = async_fs::block_on(async {
        async_fs::create_dir_all(PROBE_DIR).await?;
        async_fs::write_file(PROBE_PATH_BYTES, b"pointer-probe-16").await
    });
    if let Err(error) = setup {
        logl::log(level::ERROR, format_args!("panick: FAIL setup rc={error}"));
        return;
    }

    run_dangerous_pointer_probe(BAD_PTR_LOW);
    run_dangerous_posix_probe(BAD_PTR_LOW);
    run_dangerous_pointer_probe(BAD_PTR_HIGH);
    run_dangerous_posix_probe(BAD_PTR_HIGH);

    logl::log(level::INFO, "panick: done");
    platform::poll_once();
}
