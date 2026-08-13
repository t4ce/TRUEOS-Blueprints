use core::ffi::{c_char, c_int, c_void};

use trueos::{async_fs, logl, logl::level, platform};

const PROBE_DIR: &[u8] = b"/common";
const PROBE_PATH: &[u8] = b"/common/posix-file-world-probe.bin";
const PROBE_PATH_C: &[u8] = b"/common/posix-file-world-probe.bin\0";

const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 0o2;
const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;
const SEEK_END: c_int = 2;

const BASELINE: &[u8] = b"page-0: ORIGINAL | page-1: ORIGINAL | end\n";
const PATCH_OFFSET: i64 = 8;
const PATCH: &[u8] = b"MUTATED!";
const TAIL_OFFSET: i64 = 8192;
const TAIL: &[u8] = b"materialized-tail";
const FINAL_LEN: usize = TAIL_OFFSET as usize + TAIL.len();

unsafe extern "C" {
    fn open(path: *const c_char, flags: c_int, mode: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn lseek(fd: c_int, offset: isize, whence: c_int) -> isize;
    fn pread(fd: c_int, buf: *mut c_void, count: usize, offset: i64) -> isize;
    fn pwrite(fd: c_int, buf: *const c_void, count: usize, offset: i64) -> isize;
    fn fsync(fd: c_int) -> c_int;
}

fn check(label: &str, condition: bool) -> bool {
    logl::log(
        if condition { level::INFO } else { level::ERROR },
        format_args!(
            "posix-file-world-probe: {} check={}",
            label,
            if condition { "PASS" } else { "FAIL" }
        ),
    );
    condition
}

fn check_io(label: &str, got: isize, expected: usize) -> bool {
    logl::log(
        if got == expected as isize {
            level::INFO
        } else {
            level::ERROR
        },
        format_args!(
            "posix-file-world-probe: {} got={} expected={}",
            label, got, expected
        ),
    );
    got == expected as isize
}

fn check_seek(label: &str, got: isize, expected: isize) -> bool {
    logl::log(
        if got == expected {
            level::INFO
        } else {
            level::ERROR
        },
        format_args!(
            "posix-file-world-probe: {} got={} expected={}",
            label, got, expected
        ),
    );
    got == expected
}

fn read_backing(label: &str) -> Option<trueos::platform::Vec<u8>> {
    match async_fs::block_on(async_fs::read_file(PROBE_PATH)) {
        Ok(bytes) => {
            logl::log(
                level::INFO,
                format_args!(
                    "posix-file-world-probe: backing-read label={} len={}",
                    label,
                    bytes.len()
                ),
            );
            Some(bytes)
        }
        Err(rc) => {
            logl::log(
                level::ERROR,
                format_args!(
                    "posix-file-world-probe: backing-read label={} rc={}",
                    label, rc
                ),
            );
            None
        }
    }
}

fn run_probe() -> bool {
    let mut ok = true;

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=seed backing=TRUEOSFS",
    );
    ok &= check(
        "create-dir",
        async_fs::block_on(async_fs::create_dir_all(PROBE_DIR)).is_ok(),
    );
    ok &= check(
        "seed-write",
        async_fs::block_on(async_fs::write_file(PROBE_PATH, BASELINE)).is_ok(),
    );
    ok &= check(
        "seed-visible-on-backing",
        read_backing("seed").is_some_and(|bytes| bytes == BASELINE),
    );

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=open expected=whole-file-materialization",
    );
    let fd = unsafe { open(PROBE_PATH_C.as_ptr().cast(), O_RDWR, 0) };
    if !check("open-writer", fd >= 0) {
        return false;
    }

    let mut initial = [0u8; BASELINE.len()];
    ok &= check_io(
        "pread-initial-world",
        unsafe { pread(fd, initial.as_mut_ptr().cast(), initial.len(), 0) },
        initial.len(),
    );
    ok &= check("initial-world-equals-backing", initial == BASELINE);

    ok &= check_seek(
        "lseek-jump-inside-world",
        unsafe { lseek(fd, 16, SEEK_SET) },
        16,
    );
    let mut cursor_sample = [0u8; 7];
    ok &= check_io(
        "read-at-cursor-without-backing-io",
        unsafe { read(fd, cursor_sample.as_mut_ptr().cast(), cursor_sample.len()) },
        cursor_sample.len(),
    );
    ok &= check_seek("cursor-after-read", unsafe { lseek(fd, 0, SEEK_CUR) }, 23);

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=mutate expected=in-memory-dirty-world",
    );
    ok &= check_io(
        "pwrite-patch",
        unsafe { pwrite(fd, PATCH.as_ptr().cast(), PATCH.len(), PATCH_OFFSET) },
        PATCH.len(),
    );
    ok &= check_io(
        "pwrite-eager-sparse-tail",
        unsafe { pwrite(fd, TAIL.as_ptr().cast(), TAIL.len(), TAIL_OFFSET) },
        TAIL.len(),
    );
    ok &= check_seek(
        "pwrite-preserves-cursor",
        unsafe { lseek(fd, 0, SEEK_CUR) },
        23,
    );

    let mut patch = [0u8; PATCH.len()];
    ok &= check_io(
        "pread-sees-dirty-patch",
        unsafe { pread(fd, patch.as_mut_ptr().cast(), patch.len(), PATCH_OFFSET) },
        patch.len(),
    );
    ok &= check("dirty-patch-visible-in-world", patch == PATCH);

    let mut tail = [0u8; TAIL.len()];
    ok &= check_io(
        "pread-sees-materialized-tail",
        unsafe { pread(fd, tail.as_mut_ptr().cast(), tail.len(), TAIL_OFFSET) },
        tail.len(),
    );
    ok &= check("tail-visible-in-world", tail == TAIL);
    ok &= check_seek(
        "world-length-after-eager-growth",
        unsafe { lseek(fd, 0, SEEK_END) },
        FINAL_LEN as isize,
    );

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=independent-open expected=shared-dirty-world",
    );
    let shared_fd = unsafe { open(PROBE_PATH_C.as_ptr().cast(), O_RDONLY, 0) };
    ok &= check("open-independent-before-fsync", shared_fd >= 0);
    if shared_fd >= 0 {
        let mut shared_patch = [0u8; PATCH.len()];
        ok &= check_io(
            "independent-pread-dirty-patch",
            unsafe {
                pread(
                    shared_fd,
                    shared_patch.as_mut_ptr().cast(),
                    shared_patch.len(),
                    PATCH_OFFSET,
                )
            },
            shared_patch.len(),
        );
        ok &= check("independent-open-shares-dirty-world", shared_patch == PATCH);
        ok &= check_seek(
            "independent-world-length",
            unsafe { lseek(shared_fd, 0, SEEK_END) },
            FINAL_LEN as isize,
        );
    }

    ok &= check(
        "backing-still-baseline-before-fsync",
        read_backing("before-fsync").is_some_and(|bytes| bytes == BASELINE),
    );

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=fsync expected=whole-world-writeback",
    );
    ok &= check("fsync-writes-shared-world", unsafe { fsync(fd) } == 0);

    let fsynced = read_backing("after-fsync");
    ok &= check(
        "fsync-persisted-grown-world",
        fsynced
            .as_ref()
            .is_some_and(|bytes| bytes.len() == FINAL_LEN),
    );
    ok &= check(
        "fsync-persisted-patch",
        fsynced.as_ref().is_some_and(|bytes| {
            bytes.get(PATCH_OFFSET as usize..PATCH_OFFSET as usize + PATCH.len()) == Some(PATCH)
        }),
    );
    ok &= check(
        "fsync-persisted-zero-fill",
        fsynced.as_ref().is_some_and(|bytes| {
            bytes
                .get(BASELINE.len()..TAIL_OFFSET as usize)
                .is_some_and(|gap| gap.iter().all(|byte| *byte == 0))
        }),
    );
    ok &= check(
        "fsync-persisted-tail",
        fsynced.as_ref().is_some_and(|bytes| bytes.ends_with(TAIL)),
    );

    if shared_fd >= 0 {
        let mut after_fsync = [0u8; PATCH.len()];
        ok &= check_io(
            "independent-pread-after-fsync",
            unsafe {
                pread(
                    shared_fd,
                    after_fsync.as_mut_ptr().cast(),
                    after_fsync.len(),
                    PATCH_OFFSET,
                )
            },
            after_fsync.len(),
        );
        ok &= check("shared-world-survives-fsync", after_fsync == PATCH);
        ok &= check(
            "close-independent-shared-world",
            unsafe { close(shared_fd) } == 0,
        );
    }

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=close expected=clean-world-release",
    );
    ok &= check("close-writer", unsafe { close(fd) } == 0);

    let persisted = read_backing("after-close");
    ok &= check(
        "close-preserved-fsynced-world",
        persisted
            .as_ref()
            .is_some_and(|bytes| bytes.len() == FINAL_LEN),
    );

    logl::log(
        level::INFO,
        "posix-file-world-probe: phase=reopen expected=rematerialized-persisted-world",
    );
    let reopened = unsafe { open(PROBE_PATH_C.as_ptr().cast(), O_RDONLY, 0) };
    ok &= check("reopen-persisted-world", reopened >= 0);
    if reopened >= 0 {
        let mut reopened_patch = [0u8; PATCH.len()];
        ok &= check_io(
            "reopen-pread-patch",
            unsafe {
                pread(
                    reopened,
                    reopened_patch.as_mut_ptr().cast(),
                    reopened_patch.len(),
                    PATCH_OFFSET,
                )
            },
            reopened_patch.len(),
        );
        ok &= check("reopened-world-has-patch", reopened_patch == PATCH);
        ok &= check_seek(
            "reopened-world-length",
            unsafe { lseek(reopened, 0, SEEK_END) },
            FINAL_LEN as isize,
        );
        ok &= check("close-reopened", unsafe { close(reopened) } == 0);
    }

    ok
}

fn main() {
    logl::log(level::INFO, "posix-file-world-probe: blueprint start");
    let ok = run_probe();
    logl::log(
        if ok { level::INFO } else { level::ERROR },
        format_args!(
            "posix-file-world-probe: result={} current_model=eager-shared-world+fsync-writeback",
            if ok { "PASS" } else { "FAIL" }
        ),
    );
    platform::poll_once();
}
