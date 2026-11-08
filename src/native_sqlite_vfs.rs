//! Native SQLite file I/O. A main database holds a real, exclusive File lock
//! for its connection lifetime. This deliberately serializes connections across
//! Hulls; the private WAL index is rebuilt from the WAL under that same lock.
//! No Unix descriptors, mmap, pthreads or process-local pretend file locks.
use crate as ffi;
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char, c_int, c_void},
    fs::{File, OpenOptions, TryLockError},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, PathBuf},
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering, fence},
    },
};

#[repr(C)]
struct NativeFile {
    base: ffi::sqlite3_file,
    state: *mut Mutex<State>,
}
struct State {
    file: File,
    path: PathBuf,
    delete: bool,
    level: c_int,
    shm: BTreeMap<c_int, Region>,
}
struct Region {
    // SQLite reads atomic words and WalIndexHdr through the returned pointer.
    // Guarantee eight-byte alignment independently of the global allocator.
    words: Vec<u64>,
    size: usize,
}

pub(super) const fn file_size() -> c_int {
    size_of::<NativeFile>() as c_int
}
fn path(name: *const c_char) -> Option<PathBuf> {
    if name.is_null() {
        return None;
    }
    // SQLite's filename argument is NUL terminated and valid for this callback.
    unsafe { CStr::from_ptr(name) }
        .to_str()
        .ok()
        .map(PathBuf::from)
}
unsafe fn state<'a>(file: *mut ffi::sqlite3_file) -> std::sync::MutexGuard<'a, State> {
    unsafe { &*(*file.cast::<NativeFile>()).state }
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
pub(super) unsafe extern "C" fn open(
    _: *mut ffi::sqlite3_vfs,
    name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe {
        (*file).pMethods = ptr::null();
    }
    let temporary = name.is_null();
    let path = if temporary {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return ffi::SQLITE_CANTOPEN;
        }
        dir.join(format!(
            "sqlite-{}-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    } else {
        let Some(path) = path(name) else {
            return ffi::SQLITE_CANTOPEN;
        };
        path
    };
    let write = flags & ffi::SQLITE_OPEN_READWRITE != 0;
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    if temporary || flags & ffi::SQLITE_OPEN_EXCLUSIVE != 0 {
        options.create_new(true);
    } else if flags & ffi::SQLITE_OPEN_CREATE != 0 {
        options.create(true);
    }
    let handle = match options.open(&path) {
        Ok(file) => file,
        Err(_) => return ffi::SQLITE_CANTOPEN,
    };
    if flags & ffi::SQLITE_OPEN_MAIN_DB != 0 {
        match handle.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return ffi::SQLITE_BUSY,
            Err(TryLockError::Error(_)) => return ffi::SQLITE_IOERR_LOCK,
        }
    }
    let native = NativeFile {
        base: ffi::sqlite3_file { pMethods: &METHODS },
        state: Box::into_raw(Box::new(Mutex::new(State {
            file: handle,
            path,
            delete: temporary || flags & ffi::SQLITE_OPEN_DELETEONCLOSE != 0,
            level: ffi::SQLITE_LOCK_NONE,
            shm: BTreeMap::new(),
        }))),
    };
    unsafe {
        file.cast::<NativeFile>().write(native);
        if !out_flags.is_null() {
            out_flags.write(flags);
        }
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn close(file: *mut ffi::sqlite3_file) -> c_int {
    let native = unsafe { &mut *file.cast::<NativeFile>() };
    let state = unsafe { Box::from_raw(native.state) }
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    native.state = ptr::null_mut();
    native.base.pMethods = ptr::null();
    drop(state.file);
    if state.delete && std::fs::remove_file(state.path).is_err() {
        return ffi::SQLITE_IOERR_DELETE;
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn read(
    file: *mut ffi::sqlite3_file,
    out: *mut c_void,
    size: c_int,
    offset: i64,
) -> c_int {
    if size < 0 || offset < 0 {
        return ffi::SQLITE_IOERR_READ;
    }
    let mut state = unsafe { state(file) };
    if state.file.seek(SeekFrom::Start(offset as u64)).is_err() {
        return ffi::SQLITE_IOERR_SEEK;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(out.cast::<u8>(), size as usize) };
    let mut count = 0;
    while count < out.len() {
        match state.file.read(&mut out[count..]) {
            Ok(0) => {
                out[count..].fill(0);
                return ffi::SQLITE_IOERR_SHORT_READ;
            }
            Ok(n) => count += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return ffi::SQLITE_IOERR_READ,
        }
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn write(
    file: *mut ffi::sqlite3_file,
    bytes: *const c_void,
    size: c_int,
    offset: i64,
) -> c_int {
    if size < 0 || offset < 0 {
        return ffi::SQLITE_IOERR_WRITE;
    }
    let mut state = unsafe { state(file) };
    if state.file.seek(SeekFrom::Start(offset as u64)).is_err() {
        return ffi::SQLITE_IOERR_SEEK;
    }
    match state
        .file
        .write_all(unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), size as usize) })
    {
        Ok(()) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_WRITE,
    }
}
unsafe extern "C" fn truncate(file: *mut ffi::sqlite3_file, size: i64) -> c_int {
    if size < 0 {
        return ffi::SQLITE_IOERR_TRUNCATE;
    }
    match unsafe { state(file) }.file.set_len(size as u64) {
        Ok(()) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_TRUNCATE,
    }
}
unsafe extern "C" fn sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    let state = unsafe { state(file) };
    let result = if flags & ffi::SQLITE_SYNC_DATAONLY != 0 {
        state.file.sync_data()
    } else {
        state.file.sync_all()
    };
    match result {
        Ok(()) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_FSYNC,
    }
}
unsafe extern "C" fn size(file: *mut ffi::sqlite3_file, out: *mut i64) -> c_int {
    match unsafe { state(file) }
        .file
        .metadata()
        .and_then(|m| i64::try_from(m.len()).map_err(std::io::Error::other))
    {
        Ok(size) => {
            unsafe { out.write(size) };
            ffi::SQLITE_OK
        }
        Err(_) => ffi::SQLITE_IOERR_FSTAT,
    }
}
unsafe extern "C" fn lock(file: *mut ffi::sqlite3_file, level: c_int) -> c_int {
    unsafe { state(file) }.level = level;
    ffi::SQLITE_OK
}
unsafe extern "C" fn reserved(file: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    unsafe { out.write(c_int::from(state(file).level >= ffi::SQLITE_LOCK_RESERVED)) };
    ffi::SQLITE_OK
}
unsafe extern "C" fn control(
    file: *mut ffi::sqlite3_file,
    operation: c_int,
    out: *mut c_void,
) -> c_int {
    if operation == ffi::SQLITE_FCNTL_LOCKSTATE {
        unsafe { out.cast::<c_int>().write(state(file).level) };
        ffi::SQLITE_OK
    } else {
        ffi::SQLITE_NOTFOUND
    }
}
unsafe extern "C" fn sector(_: *mut ffi::sqlite3_file) -> c_int {
    4096
}
unsafe extern "C" fn characteristics(_: *mut ffi::sqlite3_file) -> c_int {
    0
}
unsafe extern "C" fn shm_map(
    file: *mut ffi::sqlite3_file,
    region: c_int,
    size: c_int,
    _: c_int,
    out: *mut *mut c_void,
) -> c_int {
    if region < 0 || size <= 0 {
        return ffi::SQLITE_IOERR_SHMMAP;
    }
    let mut state = unsafe { state(file) };
    if let std::collections::btree_map::Entry::Vacant(entry) = state.shm.entry(region) {
        let mut words = Vec::new();
        let count = (size as usize).div_ceil(size_of::<u64>());
        if words.try_reserve_exact(count).is_err() {
            return ffi::SQLITE_NOMEM;
        }
        words.resize(count, 0u64);
        entry.insert(Region { words, size: size as usize });
    }
    let region = state.shm.get_mut(&region).unwrap();
    if region.size != size as usize {
        return ffi::SQLITE_IOERR_SHMSIZE;
    }
    unsafe { out.write(region.words.as_mut_ptr().cast()) };
    ffi::SQLITE_OK
}
unsafe extern "C" fn shm_lock(_: *mut ffi::sqlite3_file, _: c_int, _: c_int, _: c_int) -> c_int {
    ffi::SQLITE_OK
}
unsafe extern "C" fn shm_barrier(_: *mut ffi::sqlite3_file) {
    fence(Ordering::SeqCst);
}
unsafe extern "C" fn shm_unmap(file: *mut ffi::sqlite3_file, _: c_int) -> c_int {
    unsafe { state(file) }.shm.clear();
    ffi::SQLITE_OK
}
static METHODS: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    iVersion: 2,
    xClose: Some(close),
    xRead: Some(read),
    xWrite: Some(write),
    xTruncate: Some(truncate),
    xSync: Some(sync),
    xFileSize: Some(size),
    xLock: Some(lock),
    xUnlock: Some(lock),
    xCheckReservedLock: Some(reserved),
    xFileControl: Some(control),
    xSectorSize: Some(sector),
    xDeviceCharacteristics: Some(characteristics),
    xShmMap: Some(shm_map),
    xShmLock: Some(shm_lock),
    xShmBarrier: Some(shm_barrier),
    xShmUnmap: Some(shm_unmap),
    xFetch: None,
    xUnfetch: None,
};
pub(super) unsafe extern "C" fn delete(
    _: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    _: c_int,
) -> c_int {
    let Some(path) = path(name) else {
        return ffi::SQLITE_IOERR_DELETE;
    };
    match std::fs::remove_file(path) {
        Ok(()) => ffi::SQLITE_OK,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ffi::SQLITE_IOERR_DELETE_NOENT,
        Err(_) => ffi::SQLITE_IOERR_DELETE,
    }
}
pub(super) unsafe extern "C" fn access(
    _: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    let Some(path) = path(name) else {
        return ffi::SQLITE_IOERR_ACCESS;
    };
    let exists = if flags == ffi::SQLITE_ACCESS_READWRITE {
        OpenOptions::new().read(true).write(true).open(path).is_ok()
    } else if flags == ffi::SQLITE_ACCESS_READ {
        File::open(path).is_ok()
    } else {
        path.try_exists().unwrap_or(false)
    };
    unsafe { out.write(c_int::from(exists)) };
    ffi::SQLITE_OK
}
pub(super) unsafe extern "C" fn full_path(
    _: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    capacity: c_int,
    out: *mut c_char,
) -> c_int {
    let Some(path) = path(name) else {
        return ffi::SQLITE_CANTOPEN;
    };
    let path = if path.is_absolute() {
        path
    } else {
        let Ok(current) = std::env::current_dir() else {
            return ffi::SQLITE_CANTOPEN;
        };
        current.join(path)
    };
    // Resolve existing aliases, then normalize a new file's absolute path.
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            item => normalized.push(item.as_os_str()),
        }
    }
    let Some(text) = normalized.to_str() else {
        return ffi::SQLITE_CANTOPEN;
    };
    if capacity <= 0 || text.len() >= capacity as usize {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe {
        ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), text.len());
        out.add(text.len()).write(0);
    }
    ffi::SQLITE_OK
}
