use std::ffi::{CString, c_char, c_int, c_void};
use std::io;
use std::path::Path;
use std::time::Instant;

const AT_FDCWD: c_int = -100;
const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
const O_DIRECTORY: c_int = 0x10000;

#[repr(C)]
#[derive(Clone, Copy)]
struct Stat {
    dev: u64,
    ino: u64,
    nlink: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    pad: u32,
    rdev: u64,
    size: i64,
    blksize: i64,
    blocks: i64,
    atime: i64,
    atime_nsec: i64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
    unused: [i64; 3],
}
const _: () = assert!(std::mem::size_of::<Stat>() == 144);

#[repr(C, align(8))]
struct Output<const N: usize>([u8; N]);

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

unsafe extern "C" {
    fn __ctype_b_loc() -> *const *const u16;
    fn iswspace(value: u32) -> c_int;
    fn iswalpha(value: u32) -> c_int;
    fn iswdigit(value: u32) -> c_int;
    fn iswalnum(value: u32) -> c_int;
    fn __errno_location() -> *mut c_int;
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn fstatat(fd: c_int, path: *const c_char, out: *mut Stat, flags: c_int) -> c_int;
    fn mkdirat(fd: c_int, path: *const c_char, mode: u32) -> c_int;
    fn futimens(fd: c_int, times: *const Timespec) -> c_int;
    fn statvfs(path: *const c_char, out: *mut c_void) -> c_int;
}

fn cleanup(root: &Path) -> io::Result<()> {
    #[cfg(target_os = "trueos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = root.as_os_str().as_bytes();
        let runtime = trueos::runtime::current_thread().build()?;
        let result = runtime.block_on(async {
            trueos::time::timeout(
                std::time::Duration::from_secs(2),
                trueos::async_fs::remove(bytes),
            )
            .await
        });
        runtime.shutdown_background();
        require(
            matches!(result, Ok(Ok(()))),
            "typed filesystem cleanup failed",
        )?;
        require(
            !root.try_exists()?,
            "typed filesystem cleanup left probe directory",
        )
    }
    #[cfg(not(target_os = "trueos"))]
    std::fs::remove_dir_all(root)
}

struct Fd(c_int);
impl Drop for Fd {
    fn drop(&mut self) {
        unsafe { close(self.0) };
    }
}

fn require(condition: bool, message: &'static str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

fn errno() -> c_int {
    unsafe { *__errno_location() }
}

fn path_cstr(path: &Path) -> CString {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).unwrap()
}

fn record(start: Instant, state: &str, case: &str, error: Option<i32>) {
    let message = format!(
        "libc_contract: {state} case={case} elapsed_ms={} errno={}",
        start.elapsed().as_millis(),
        error.unwrap_or(0)
    );
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO, format_args!("{message}"));
    #[cfg(not(target_os = "trueos"))]
    println!("{message}");
}

fn classifications() -> io::Result<()> {
    let location = unsafe { __ctype_b_loc() };
    require(!location.is_null(), "classification accessor returned null")?;
    let table = unsafe { *location };
    require(!table.is_null(), "classification table returned null")?;
    for value in -128isize..=255 {
        let actual = unsafe { *table.offset(value) };
        if !(0..=127).contains(&value) {
            require(actual == 0, "C locale signed/extended byte was classified")?;
            continue;
        }
        let byte = value as u8;
        let expected = [
            byte.is_ascii_uppercase(),
            byte.is_ascii_lowercase(),
            byte.is_ascii_alphabetic(),
            byte.is_ascii_digit(),
            byte.is_ascii_hexdigit(),
            matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
            (b' '..=b'~').contains(&byte),
            byte.is_ascii_graphic(),
            matches!(byte, b' ' | b'\t'),
            byte.is_ascii_control(),
            byte.is_ascii_punctuation(),
            byte.is_ascii_alphanumeric(),
        ];
        for (bit, expected) in expected.into_iter().enumerate() {
            require(
                (actual & (1u16 << bit).to_be() != 0) == expected,
                "classification mask/layout differs from C locale",
            )?;
        }
    }
    for value in [
        0,
        9,
        10,
        13,
        32,
        33,
        48,
        57,
        65,
        90,
        97,
        122,
        127,
        0x85,
        0xa0,
        0x2003,
        u32::MAX,
    ] {
        let byte = u8::try_from(value).ok().filter(u8::is_ascii);
        let space = matches!(value, 9..=13 | 32);
        let alpha = byte.is_some_and(|b| b.is_ascii_alphabetic());
        let digit = byte.is_some_and(|b| b.is_ascii_digit());
        require(
            (unsafe { iswspace(value) } != 0) == space,
            "wide space classification",
        )?;
        require(
            (unsafe { iswalpha(value) } != 0) == alpha,
            "wide alpha classification",
        )?;
        require(
            (unsafe { iswdigit(value) } != 0) == digit,
            "wide digit classification",
        )?;
        require(
            (unsafe { iswalnum(value) } != 0) == (alpha || digit),
            "wide alnum classification",
        )?;
    }
    require(
        unsafe { __ctype_b_loc() } == location,
        "classification slot changed",
    )
}

fn metadata(fd: c_int, path: &CString, flags: c_int) -> io::Result<Stat> {
    let mut output = Output([0xa5; 144]);
    let rc = unsafe { fstatat(fd, path.as_ptr(), output.0.as_mut_ptr().cast(), flags) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    require(
        output.0 != [0xa5; 144],
        "successful metadata call left output uninitialized",
    )?;
    Ok(unsafe { output.0.as_ptr().cast::<Stat>().read() })
}

fn metadata_error(fd: c_int, path: &CString, flags: c_int, expected: c_int) -> io::Result<()> {
    let mut output = Output([0xa5; 144]);
    let rc = unsafe { fstatat(fd, path.as_ptr(), output.0.as_mut_ptr().cast(), flags) };
    require(
        rc == -1 && errno() == expected,
        "metadata error did not return expected errno",
    )?;
    require(
        output.0 == [0xa5; 144],
        "failed metadata call changed output",
    )
}

fn relative_fs(root: &Path) -> io::Result<()> {
    std::fs::write(root.join("data"), b"probe")?;
    let root_c = path_cstr(root);
    let dir = Fd(unsafe { open(root_c.as_ptr(), O_DIRECTORY, 0u32) });
    require(dir.0 >= 0, "opening directory description failed")?;
    let data = CString::new("data").unwrap();
    require(
        metadata(dir.0, &data, 0)?.size == 5,
        "relative metadata size",
    )?;
    require(
        metadata(dir.0, &data, AT_SYMLINK_NOFOLLOW)?.mode & 0xf000 == 0x8000,
        "regular file mode",
    )?;
    let absolute = path_cstr(&root.join("data"));
    require(
        metadata(-12345, &absolute, 0)?.size == 5,
        "absolute path should ignore dirfd",
    )?;
    metadata_error(-12345, &data, 0, 9)?;
    metadata_error(dir.0, &data, 1 << 30, 22)?;
    metadata_error(dir.0, &CString::new("").unwrap(), 0, 2)?;
    metadata_error(dir.0, &CString::new("missing").unwrap(), 0, 2)?;
    let file = Fd(unsafe { openat(dir.0, data.as_ptr(), 0, 0u32) });
    require(file.0 >= 0, "openat relative to directory failed")?;
    metadata_error(file.0, &data, 0, 20)?;
    let made = CString::new("created").unwrap();
    require(
        unsafe { mkdirat(dir.0, made.as_ptr(), 0o700) } == 0,
        "mkdirat failed",
    )?;
    require(
        std::fs::metadata(root.join("created"))?.is_dir(),
        "mkdirat did not create directory",
    )?;
    require(
        unsafe { mkdirat(dir.0, made.as_ptr(), 0o700) } == -1 && errno() == 17,
        "mkdirat existing target",
    )?;
    let missing_parent = CString::new("missing/child").unwrap();
    require(
        unsafe { mkdirat(dir.0, missing_parent.as_ptr(), 0o700) } == -1 && errno() == 2,
        "mkdirat missing parent",
    )?;
    require(
        !root.join("missing").try_exists()?,
        "mkdirat created absent parents",
    )?;
    let file_parent = CString::new("data/child").unwrap();
    require(
        unsafe { mkdirat(dir.0, file_parent.as_ptr(), 0o700) } == -1 && errno() == 20,
        "mkdirat non-directory parent",
    )?;
    let times = [
        Timespec {
            sec: 1700000000,
            nsec: 0,
        },
        Timespec {
            sec: 1700000001,
            nsec: 123000000,
        },
    ];
    let rc = unsafe { futimens(file.0, times.as_ptr()) };
    if rc == 0 {
        let state = metadata(dir.0, &data, 0)?;
        require(
            state.mtime == times[1].sec && state.mtime_nsec == times[1].nsec,
            "timestamp success did not update metadata",
        )?;
    } else {
        require(
            rc == -1 && errno() == 38,
            "timestamp unsupported error contract",
        )?;
    }
    require(
        unsafe { futimens(-12345, times.as_ptr()) } == -1 && errno() == 9,
        "timestamp invalid descriptor",
    )?;
    require(
        metadata(AT_FDCWD, &absolute, 0)?.size == 5,
        "AT_FDCWD metadata",
    )
}

fn capacity(root: &Path) -> io::Result<()> {
    let mut output = Output([0xa5; 112]);
    let path = path_cstr(root);
    let rc = unsafe { statvfs(path.as_ptr(), output.0.as_mut_ptr().cast()) };
    if rc == 0 {
        let word = |index: usize| {
            u64::from_ne_bytes(output.0[index * 8..index * 8 + 8].try_into().unwrap())
        };
        require(
            output.0 != [0xa5; 112] && word(0) > 0 && word(1) > 0 && word(2) >= word(3),
            "filesystem capacity success left invalid output",
        )
    } else {
        require(
            rc == -1 && errno() == 38 && output.0 == [0xa5; 112],
            "filesystem capacity unsupported must preserve output and return ENOSYS",
        )
    }
}

fn main() {
    let start = Instant::now();
    let root = std::env::current_dir()
        .unwrap()
        .join(format!(".libc-contract-{}", std::process::id()));
    let mut created = false;
    let result = (|| {
        classifications()?;
        record(start, "PASS", "classification", None);
        std::fs::create_dir(&root)?;
        created = true;
        relative_fs(&root)?;
        record(start, "PASS", "relative_filesystem", None);
        capacity(&root)?;
        record(start, "PASS", "capacity_output_contract", None);
        Ok::<_, io::Error>(())
    })();
    let cleanup = if created { cleanup(&root) } else { Ok(()) };
    match result.and(cleanup) {
        Ok(()) => record(start, "PASS", "all", None),
        Err(error) => {
            record(start, "FAIL", "all", error.raw_os_error());
            eprintln!("libc_contract: contract failed: {error}");
            std::process::exit(1);
        }
    }
}
