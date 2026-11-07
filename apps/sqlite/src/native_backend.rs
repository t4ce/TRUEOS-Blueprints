//! SQLite's OS boundary for this service's in-memory database images.
//!
//! Persistence belongs to `storage`, not a second SQLite filesystem backend.
//! Keep SQLite serialized and supply real Rust mutexes and allocation callbacks;
//! the small VFS only supplies entropy, UTC time and sleep. File opens fail.

use rusqlite::ffi;
use std::{
    alloc::{Layout, alloc, dealloc, realloc},
    cell::UnsafeCell,
    ffi::{c_char, c_int, c_void},
    ptr,
    sync::{Condvar, Mutex, OnceLock},
    thread::ThreadId,
    time::Duration,
};

/// Must run before any SQLite call or worker starts. SQLite copies both tables.
pub(super) fn initialize() -> Result<(), String> {
    static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();
    INITIALIZED
        .get_or_init(|| unsafe {
            let allocator = ffi::sqlite3_mem_methods {
                xMalloc: Some(memory_alloc),
                xFree: Some(memory_free),
                xRealloc: Some(memory_realloc),
                xSize: Some(memory_size),
                xRoundup: Some(memory_roundup),
                xInit: Some(memory_init),
                xShutdown: Some(memory_end),
                pAppData: ptr::null_mut(),
            };
            let mutexes = ffi::sqlite3_mutex_methods {
                xMutexInit: Some(mutex_init),
                xMutexEnd: Some(mutex_end),
                xMutexAlloc: Some(mutex_alloc),
                xMutexFree: Some(mutex_free),
                xMutexEnter: Some(mutex_enter),
                xMutexTry: Some(mutex_try),
                xMutexLeave: Some(mutex_leave),
                xMutexHeld: Some(mutex_held),
                xMutexNotheld: Some(mutex_not_held),
            };
            fn check(operation: &str, code: c_int) -> Result<(), String> {
                if code == ffi::SQLITE_OK {
                    Ok(())
                } else {
                    Err(format!("native SQLite {operation}: error {code}"))
                }
            }
            check(
                "allocator",
                ffi::sqlite3_config(ffi::SQLITE_CONFIG_MALLOC, &allocator),
            )?;
            check(
                "mutexes",
                ffi::sqlite3_config(ffi::SQLITE_CONFIG_MUTEX, &mutexes),
            )?;
            check(
                "serialized mode",
                ffi::sqlite3_config(ffi::SQLITE_CONFIG_SERIALIZED),
            )?;
            check("initialization", ffi::sqlite3_initialize())?;
            Ok(())
        })
        .clone()
}

// A private header makes xSize and realloc independent of C malloc bookkeeping.
// SQLite requires at least eight-byte alignment; keep sixteen on this target.
#[repr(C, align(16))]
struct AllocationHeader {
    size: usize,
}

fn allocation_layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(size.checked_add(size_of::<AllocationHeader>())?, 16).ok()
}

unsafe extern "C" fn memory_alloc(size: c_int) -> *mut c_void {
    if size <= 0 {
        return ptr::null_mut();
    }
    let Some(layout) = allocation_layout(size as usize) else {
        return ptr::null_mut();
    };
    let header = unsafe { alloc(layout).cast::<AllocationHeader>() };
    if header.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        header.write(AllocationHeader {
            size: size as usize,
        });
        header.add(1).cast()
    }
}

unsafe extern "C" fn memory_free(pointer: *mut c_void) {
    if pointer.is_null() {
        return;
    }
    unsafe {
        let header = pointer.cast::<AllocationHeader>().sub(1);
        let layout = allocation_layout((*header).size).unwrap();
        dealloc(header.cast(), layout);
    }
}

unsafe extern "C" fn memory_realloc(pointer: *mut c_void, size: c_int) -> *mut c_void {
    if pointer.is_null() {
        return unsafe { memory_alloc(size) };
    }
    if size <= 0 {
        unsafe { memory_free(pointer) };
        return ptr::null_mut();
    }
    let Some(new_layout) = allocation_layout(size as usize) else {
        return ptr::null_mut();
    };
    unsafe {
        let old = pointer.cast::<AllocationHeader>().sub(1);
        let old_layout = allocation_layout((*old).size).unwrap();
        let new = realloc(old.cast(), old_layout, new_layout.size()).cast::<AllocationHeader>();
        if new.is_null() {
            return ptr::null_mut(); // The old allocation remains owned by SQLite.
        }
        new.write(AllocationHeader {
            size: size as usize,
        });
        new.add(1).cast()
    }
}

unsafe extern "C" fn memory_size(pointer: *mut c_void) -> c_int {
    if pointer.is_null() {
        return 0;
    }
    unsafe { (*pointer.cast::<AllocationHeader>().sub(1)).size as c_int }
}

unsafe extern "C" fn memory_roundup(size: c_int) -> c_int {
    if size <= 0 {
        0
    } else {
        size.checked_add(7).map_or(0, |n| n & !7)
    }
}

unsafe extern "C" fn memory_init(_: *mut c_void) -> c_int {
    ffi::SQLITE_OK
}
unsafe extern "C" fn memory_end(_: *mut c_void) {}

struct MutexState {
    owner: Option<ThreadId>,
    depth: usize,
}

struct NativeMutex {
    recursive: bool,
    dynamic: bool,
    state: Mutex<MutexState>,
    changed: Condvar,
}

impl NativeMutex {
    const fn new(recursive: bool, dynamic: bool) -> Self {
        Self {
            recursive,
            dynamic,
            state: Mutex::new(MutexState {
                owner: None,
                depth: 0,
            }),
            changed: Condvar::new(),
        }
    }
}

// IDs 2..=13 are the static mutexes in the pinned SQLite headers. Unlike a
// lazily allocated registry, this cannot recurse into SQLite's allocator.
static STATIC_MUTEXES: [NativeMutex; 12] = [const { NativeMutex::new(false, false) }; 12];

unsafe extern "C" fn mutex_init() -> c_int {
    ffi::SQLITE_OK
}
unsafe extern "C" fn mutex_end() -> c_int {
    ffi::SQLITE_OK
}

unsafe extern "C" fn mutex_alloc(kind: c_int) -> *mut ffi::sqlite3_mutex {
    match kind {
        ffi::SQLITE_MUTEX_FAST | ffi::SQLITE_MUTEX_RECURSIVE => unsafe {
            let pointer = alloc(Layout::new::<NativeMutex>()).cast::<NativeMutex>();
            if !pointer.is_null() {
                pointer.write(NativeMutex::new(kind == ffi::SQLITE_MUTEX_RECURSIVE, true));
            }
            pointer.cast()
        },
        ffi::SQLITE_MUTEX_STATIC_MAIN..=ffi::SQLITE_MUTEX_STATIC_VFS3 => {
            (&STATIC_MUTEXES[(kind - ffi::SQLITE_MUTEX_STATIC_MAIN) as usize] as *const NativeMutex)
                .cast_mut()
                .cast()
        }
        _ => ptr::null_mut(),
    }
}

unsafe extern "C" fn mutex_free(pointer: *mut ffi::sqlite3_mutex) {
    if pointer.is_null() {
        return;
    }
    unsafe {
        let mutex = pointer.cast::<NativeMutex>();
        if (*mutex).dynamic {
            ptr::drop_in_place(mutex);
            dealloc(mutex.cast(), Layout::new::<NativeMutex>());
        }
    }
}

unsafe extern "C" fn mutex_enter(pointer: *mut ffi::sqlite3_mutex) {
    if pointer.is_null() {
        return;
    }
    let mutex = unsafe { &*pointer.cast::<NativeMutex>() };
    let current = std::thread::current().id();
    let mut state = mutex.state.lock().unwrap_or_else(|e| e.into_inner());
    while state.owner.is_some() && !(mutex.recursive && state.owner == Some(current)) {
        state = mutex.changed.wait(state).unwrap_or_else(|e| e.into_inner());
    }
    state.owner = Some(current);
    state.depth += 1;
}

unsafe extern "C" fn mutex_try(pointer: *mut ffi::sqlite3_mutex) -> c_int {
    if pointer.is_null() {
        return ffi::SQLITE_OK;
    }
    let mutex = unsafe { &*pointer.cast::<NativeMutex>() };
    let current = std::thread::current().id();
    let mut state = mutex.state.lock().unwrap_or_else(|e| e.into_inner());
    if state.owner.is_some() && !(mutex.recursive && state.owner == Some(current)) {
        return ffi::SQLITE_BUSY;
    }
    state.owner = Some(current);
    state.depth += 1;
    ffi::SQLITE_OK
}

unsafe extern "C" fn mutex_leave(pointer: *mut ffi::sqlite3_mutex) {
    if pointer.is_null() {
        return;
    }
    let mutex = unsafe { &*pointer.cast::<NativeMutex>() };
    let mut state = mutex.state.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(state.owner, Some(std::thread::current().id()));
    assert!(state.depth != 0);
    state.depth -= 1;
    if state.depth == 0 {
        state.owner = None;
        mutex.changed.notify_one();
    }
}

unsafe extern "C" fn mutex_held(pointer: *mut ffi::sqlite3_mutex) -> c_int {
    if pointer.is_null() {
        return 1;
    }
    let mutex = unsafe { &*pointer.cast::<NativeMutex>() };
    let state = mutex.state.lock().unwrap_or_else(|e| e.into_inner());
    c_int::from(state.owner == Some(std::thread::current().id()))
}

unsafe extern "C" fn mutex_not_held(pointer: *mut ffi::sqlite3_mutex) -> c_int {
    if pointer.is_null() {
        1
    } else {
        1 - unsafe { mutex_held(pointer) }
    }
}

struct Vfs(UnsafeCell<ffi::sqlite3_vfs>);
// SQLite owns pNext while registered and serializes registry changes itself.
unsafe impl Sync for Vfs {}

static VFS: Vfs = Vfs(UnsafeCell::new(ffi::sqlite3_vfs {
    iVersion: 2,
    szOsFile: size_of::<ffi::sqlite3_file>() as c_int,
    mxPathname: 1024,
    pNext: ptr::null_mut(),
    zName: c"trueos-memory-image".as_ptr(),
    pAppData: ptr::null_mut(),
    xOpen: Some(vfs_open),
    xDelete: Some(vfs_delete),
    xAccess: Some(vfs_access),
    xFullPathname: Some(vfs_full_pathname),
    xDlOpen: None,
    xDlError: None,
    xDlSym: None,
    xDlClose: None,
    xRandomness: Some(vfs_randomness),
    xSleep: Some(vfs_sleep),
    xCurrentTime: Some(vfs_time),
    xGetLastError: Some(vfs_last_error),
    xCurrentTimeInt64: Some(vfs_time_int64),
    xSetSystemCall: None,
    xGetSystemCall: None,
    xNextSystemCall: None,
}));

#[unsafe(no_mangle)]
unsafe extern "C" fn sqlite3_os_init() -> c_int {
    unsafe { ffi::sqlite3_vfs_register(VFS.0.get(), 1) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sqlite3_os_end() -> c_int {
    unsafe { ffi::sqlite3_vfs_unregister(VFS.0.get()) }
}

unsafe extern "C" fn vfs_open(
    _: *mut ffi::sqlite3_vfs,
    _: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    _: c_int,
    flags: *mut c_int,
) -> c_int {
    unsafe {
        (*file).pMethods = ptr::null();
        if !flags.is_null() {
            flags.write(0);
        }
    }
    ffi::SQLITE_CANTOPEN
}

unsafe extern "C" fn vfs_delete(_: *mut ffi::sqlite3_vfs, _: *const c_char, _: c_int) -> c_int {
    ffi::SQLITE_IOERR_DELETE_NOENT
}

unsafe extern "C" fn vfs_access(
    _: *mut ffi::sqlite3_vfs,
    _: *const c_char,
    _: c_int,
    result: *mut c_int,
) -> c_int {
    unsafe { result.write(0) };
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_full_pathname(
    _: *mut ffi::sqlite3_vfs,
    _: *const c_char,
    capacity: c_int,
    out: *mut c_char,
) -> c_int {
    if capacity > 0 {
        unsafe { out.write(0) };
    }
    ffi::SQLITE_CANTOPEN
}

unsafe extern "C" fn vfs_randomness(
    _: *mut ffi::sqlite3_vfs,
    size: c_int,
    out: *mut c_char,
) -> c_int {
    if size <= 0 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(out.cast::<u8>(), size as usize) };
    if platform::fill_random(bytes) {
        size
    } else {
        0
    }
}

unsafe extern "C" fn vfs_sleep(_: *mut ffi::sqlite3_vfs, micros: c_int) -> c_int {
    if micros > 0 {
        std::thread::sleep(Duration::from_micros(micros as u64));
        micros
    } else {
        0
    }
}

unsafe extern "C" fn vfs_time_int64(_: *mut ffi::sqlite3_vfs, out: *mut i64) -> c_int {
    // Julian epoch in milliseconds. A missing UTC clock is an error, not 1970.
    let Some(unix_ms) = platform::unix_millis() else {
        return ffi::SQLITE_ERROR;
    };
    let Some(julian_ms) = unix_ms.checked_add(210_866_760_000_000) else {
        return ffi::SQLITE_ERROR;
    };
    unsafe { out.write(julian_ms) };
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_time(vfs: *mut ffi::sqlite3_vfs, out: *mut f64) -> c_int {
    let mut millis = 0;
    let code = unsafe { vfs_time_int64(vfs, &mut millis) };
    if code == ffi::SQLITE_OK {
        unsafe { out.write(millis as f64 / 86_400_000.0) };
    }
    code
}

unsafe extern "C" fn vfs_last_error(
    _: *mut ffi::sqlite3_vfs,
    capacity: c_int,
    out: *mut c_char,
) -> c_int {
    if capacity > 0 {
        unsafe { out.write(0) };
    }
    0
}

#[cfg(target_os = "trueos")]
mod platform {
    unsafe extern "C" {
        fn sys_rand(words: *mut u32, len: usize);
        fn trueos_time_unix_nanos() -> u64;
    }

    pub fn fill_random(bytes: &mut [u8]) -> bool {
        for chunk in bytes.chunks_mut(256) {
            let mut words = [0u32; 64];
            unsafe { sys_rand(words.as_mut_ptr(), chunk.len().div_ceil(4)) };
            for (out, word) in chunk.chunks_mut(4).zip(words) {
                out.copy_from_slice(&word.to_ne_bytes()[..out.len()]);
            }
        }
        true
    }

    pub fn unix_millis() -> Option<i64> {
        let nanos = unsafe { trueos_time_unix_nanos() };
        (nanos != 0).then_some((nanos / 1_000_000) as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_preserves_alignment_content_and_exact_sizes() {
        unsafe {
            assert!(memory_alloc(0).is_null());
            assert!(memory_alloc(-1).is_null());
            assert_eq!(memory_roundup(1), 8);
            assert_eq!(memory_roundup(17), 24);
            assert_eq!(memory_roundup(c_int::MAX), 0);
            let mut pointer = memory_alloc(17);
            assert!(!pointer.is_null());
            assert_eq!(pointer as usize % 16, 0);
            assert_eq!(memory_size(pointer), 17);
            ptr::write_bytes(pointer.cast::<u8>(), 0xa5, 17);
            pointer = memory_realloc(pointer, 4097);
            assert!(!pointer.is_null());
            assert_eq!(pointer as usize % 16, 0);
            assert_eq!(memory_size(pointer), 4097);
            assert_eq!(
                std::slice::from_raw_parts(pointer.cast::<u8>(), 17),
                &[0xa5; 17]
            );
            pointer = memory_realloc(pointer, 9);
            assert_eq!(memory_size(pointer), 9);
            assert_eq!(
                std::slice::from_raw_parts(pointer.cast::<u8>(), 9),
                &[0xa5; 9]
            );
            assert!(memory_realloc(pointer, 0).is_null());
            memory_free(ptr::null_mut());
        }
    }

    #[test]
    fn recursive_mutex_releases_only_after_the_last_leave() {
        unsafe {
            let pointer = mutex_alloc(ffi::SQLITE_MUTEX_RECURSIVE);
            assert!(!pointer.is_null());
            assert_eq!(mutex_not_held(pointer), 1);
            mutex_enter(pointer);
            assert_eq!(mutex_try(pointer), ffi::SQLITE_OK);
            assert_eq!(mutex_held(pointer), 1);
            let address = pointer as usize;
            let try_other = || {
                std::thread::spawn(move || mutex_try(address as *mut _))
                    .join()
                    .unwrap()
            };
            assert_eq!(try_other(), ffi::SQLITE_BUSY);
            mutex_leave(pointer);
            assert_eq!(try_other(), ffi::SQLITE_BUSY);
            mutex_leave(pointer);
            std::thread::spawn(move || {
                let pointer = address as *mut _;
                assert_eq!(mutex_try(pointer), ffi::SQLITE_OK);
                mutex_leave(pointer);
            })
            .join()
            .unwrap();
            assert_eq!(mutex_not_held(pointer), 1);
            mutex_free(pointer);
        }
    }

    #[test]
    fn a_waiting_thread_is_woken_on_unlock() {
        unsafe {
            let pointer = mutex_alloc(ffi::SQLITE_MUTEX_FAST);
            mutex_enter(pointer);
            assert_eq!(mutex_try(pointer), ffi::SQLITE_BUSY);
            let address = pointer as usize;
            let (started, ready) = std::sync::mpsc::channel();
            let (finished, done) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                let pointer = address as *mut _;
                started.send(()).unwrap();
                mutex_enter(pointer);
                finished.send(()).unwrap();
                mutex_leave(pointer);
            });
            ready.recv().unwrap();
            assert!(done.recv_timeout(Duration::from_millis(20)).is_err());
            mutex_leave(pointer);
            done.recv_timeout(Duration::from_secs(2)).unwrap();
            thread.join().unwrap();
            mutex_free(pointer);
        }
    }

    #[test]
    fn static_mutexes_have_stable_distinct_identity() {
        unsafe {
            let mut pointers = Vec::new();
            for kind in ffi::SQLITE_MUTEX_STATIC_MAIN..=ffi::SQLITE_MUTEX_STATIC_VFS3 {
                let pointer = mutex_alloc(kind);
                assert_eq!(pointer, mutex_alloc(kind));
                assert!(!pointers.contains(&pointer));
                pointers.push(pointer);
                mutex_free(pointer); // Static mutexes are never freed.
                mutex_enter(pointer);
                mutex_leave(pointer);
            }
            assert!(mutex_alloc(ffi::SQLITE_MUTEX_STATIC_VFS3 + 1).is_null());
            assert_eq!(mutex_held(ptr::null_mut()), 1);
            assert_eq!(mutex_not_held(ptr::null_mut()), 1);
        }
    }

    #[test]
    fn sqlite_serializes_real_concurrent_calls_on_one_connection() {
        initialize().unwrap();
        unsafe {
            assert_eq!(ffi::sqlite3_threadsafe(), 1);
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            assert!(!ffi::sqlite3_db_mutex(connection.handle()).is_null());
            connection
                .execute_batch("CREATE TABLE concurrent(value INTEGER)")
                .unwrap();
            let address = connection.handle() as usize;
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    std::thread::spawn(move || {
                        for _ in 0..100 {
                            assert_eq!(
                                ffi::sqlite3_exec(
                                    address as *mut _,
                                    c"INSERT INTO concurrent VALUES(1)".as_ptr(),
                                    None,
                                    ptr::null_mut(),
                                    ptr::null_mut()
                                ),
                                ffi::SQLITE_OK
                            );
                        }
                    })
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
            assert_eq!(
                connection
                    .query_row("SELECT count(*) FROM concurrent", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                800
            );
            assert!(rusqlite::Connection::open("/unsupported.sqlite").is_err());
        }
    }
}

// Only the opt-in host test variant selects this boundary. Normal host builds
// continue using SQLite's original bundled Unix backend.
#[cfg(not(target_os = "trueos"))]
mod platform {
    use std::{
        io::Read,
        time::{SystemTime, UNIX_EPOCH},
    };

    pub fn fill_random(bytes: &mut [u8]) -> bool {
        std::fs::File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(bytes))
            .is_ok()
    }

    pub fn unix_millis() -> Option<i64> {
        i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis(),
        )
        .ok()
    }
}
