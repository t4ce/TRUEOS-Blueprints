//! App-owned OS primitives for the pinned native OpenSSL sources.
//! The C adapter uses private opaque objects, never a kernel pthread ABI.
use std::{
    cell::{Cell, RefCell, UnsafeCell},
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    sync::{
        Condvar, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
    thread::ThreadId,
    time::Duration,
};

const EINVAL: c_int = 22;
const ENOMEM: c_int = 12;
const EDEADLK: c_int = 35;
const EPERM: c_int = 1;

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|error| error.into_inner())
}

#[repr(C)]
pub struct NativeMutex {
    object: AtomicPtr<OwnedMutex>,
}
#[repr(C)]
pub struct NativeCond {
    object: AtomicPtr<OwnedCond>,
}
#[repr(C)]
pub struct MutexAttr {
    kind: c_int,
}
struct OwnedMutex {
    state: Mutex<LockState>,
    changed: Condvar,
    recursive: bool,
}
struct LockState {
    owner: Option<ThreadId>,
    depth: usize,
}
struct OwnedCond {
    sequence: Mutex<u64>,
    changed: Condvar,
}

impl OwnedMutex {
    fn new(recursive: bool) -> Self {
        Self {
            state: Mutex::new(LockState {
                owner: None,
                depth: 0,
            }),
            changed: Condvar::new(),
            recursive,
        }
    }
    fn acquire(&self) -> c_int {
        let current = std::thread::current().id();
        let mut state = held(&self.state);
        while state.owner.is_some() {
            if state.owner == Some(current) {
                if !self.recursive {
                    return EDEADLK;
                }
                let Some(depth) = state.depth.checked_add(1) else {
                    return EINVAL;
                };
                state.depth = depth;
                return 0;
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.owner = Some(current);
        state.depth = 1;
        0
    }
    fn release(&self) -> c_int {
        let mut state = held(&self.state);
        if state.owner != Some(std::thread::current().id()) || state.depth == 0 {
            return EPERM;
        }
        state.depth -= 1;
        if state.depth == 0 {
            state.owner = None;
            self.changed.notify_one();
        }
        0
    }
}

// Zero-initialized static C objects publish one Rust owner. Losers destroy
// their unpublished allocation. Callers must not destroy an object in use.
unsafe fn mutex<'a>(handle: *mut NativeMutex) -> Option<&'a OwnedMutex> {
    let handle = unsafe { handle.as_ref() }?;
    let mut object = handle.object.load(Ordering::Acquire);
    if object.is_null() {
        let created = Box::into_raw(Box::new(OwnedMutex::new(false)));
        match handle.object.compare_exchange(
            ptr::null_mut(),
            created,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => object = created,
            Err(existing) => {
                unsafe { drop(Box::from_raw(created)) };
                object = existing;
            }
        }
    }
    unsafe { object.as_ref() }
}
unsafe fn condition<'a>(handle: *mut NativeCond) -> Option<&'a OwnedCond> {
    let handle = unsafe { handle.as_ref() }?;
    let mut object = handle.object.load(Ordering::Acquire);
    if object.is_null() {
        let created = Box::into_raw(Box::new(OwnedCond {
            sequence: Mutex::new(0),
            changed: Condvar::new(),
        }));
        match handle.object.compare_exchange(
            ptr::null_mut(),
            created,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => object = created,
            Err(existing) => {
                unsafe { drop(Box::from_raw(created)) };
                object = existing;
            }
        }
    }
    unsafe { object.as_ref() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutexattr_init(attr: *mut MutexAttr) -> c_int {
    let Some(attr) = (unsafe { attr.as_mut() }) else {
        return EINVAL;
    };
    attr.kind = 0;
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutexattr_destroy(_: *mut MutexAttr) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutexattr_settype(attr: *mut MutexAttr, kind: c_int) -> c_int {
    let Some(attr) = (unsafe { attr.as_mut() }) else {
        return EINVAL;
    };
    if !matches!(kind, 0..=2) {
        return EINVAL;
    }
    attr.kind = kind;
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutex_init(
    handle: *mut NativeMutex,
    attr: *const MutexAttr,
) -> c_int {
    if handle.is_null() {
        return EINVAL;
    }
    let recursive = unsafe { attr.as_ref() }.is_some_and(|attr| attr.kind == 1);
    unsafe {
        handle.write(NativeMutex {
            object: AtomicPtr::new(Box::into_raw(Box::new(OwnedMutex::new(recursive)))),
        })
    };
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutex_destroy(handle: *mut NativeMutex) -> c_int {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return EINVAL;
    };
    let object = handle.object.swap(ptr::null_mut(), Ordering::AcqRel);
    if !object.is_null() {
        unsafe { drop(Box::from_raw(object)) };
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutex_lock(handle: *mut NativeMutex) -> c_int {
    unsafe { mutex(handle) }.map_or(EINVAL, OwnedMutex::acquire)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_mutex_unlock(handle: *mut NativeMutex) -> c_int {
    unsafe { mutex(handle) }.map_or(EINVAL, OwnedMutex::release)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_cond_init(handle: *mut NativeCond, _: *const c_void) -> c_int {
    if handle.is_null() {
        return EINVAL;
    }
    unsafe {
        handle.write(NativeCond {
            object: AtomicPtr::new(Box::into_raw(Box::new(OwnedCond {
                sequence: Mutex::new(0),
                changed: Condvar::new(),
            }))),
        })
    };
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_cond_destroy(handle: *mut NativeCond) -> c_int {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return EINVAL;
    };
    let object = handle.object.swap(ptr::null_mut(), Ordering::AcqRel);
    if !object.is_null() {
        unsafe { drop(Box::from_raw(object)) };
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_cond_wait(
    cond: *mut NativeCond,
    lock: *mut NativeMutex,
) -> c_int {
    let (Some(cond), Some(lock)) = (unsafe { condition(cond) }, unsafe { mutex(lock) }) else {
        return EINVAL;
    };
    // Hold the notification gate until Condvar atomically releases it and
    // waits. A signal cannot slip between releasing the C lock and sleeping.
    let sequence = held(&cond.sequence);
    {
        let mut state = held(&lock.state);
        if state.owner != Some(std::thread::current().id()) || state.depth != 1 {
            return EPERM;
        }
        state.owner = None;
        state.depth = 0;
        lock.changed.notify_one();
    }
    drop(
        cond.changed
            .wait(sequence)
            .unwrap_or_else(|error| error.into_inner()),
    );
    lock.acquire()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_cond_signal(handle: *mut NativeCond) -> c_int {
    let Some(cond) = (unsafe { condition(handle) }) else {
        return EINVAL;
    };
    let mut sequence = held(&cond.sequence);
    *sequence = sequence.wrapping_add(1);
    cond.changed.notify_one();
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_cond_broadcast(handle: *mut NativeCond) -> c_int {
    let Some(cond) = (unsafe { condition(handle) }) else {
        return EINVAL;
    };
    let mut sequence = held(&cond.sequence);
    *sequence = sequence.wrapping_add(1);
    cond.changed.notify_all();
    0
}

type Destructor = unsafe extern "C" fn(*mut c_void);
struct Keys {
    next: u32,
    keys: BTreeMap<u32, Option<Destructor>>,
}
fn keys() -> &'static Mutex<Keys> {
    static KEYS: OnceLock<Mutex<Keys>> = OnceLock::new();
    KEYS.get_or_init(|| {
        Mutex::new(Keys {
            next: 1,
            keys: BTreeMap::new(),
        })
    })
}
thread_local! {
    // The pointer Cell has no destructor and remains accessible while Cleanup
    // invokes C destructors which may call get/set-specific again.
    static VALUES: Cell<*mut RefCell<BTreeMap<u32,*mut c_void>>> = const {Cell::new(ptr::null_mut())};
    static CLEANUP: Cleanup = const {Cleanup};
    static ERRNO: UnsafeCell<c_int> = const {UnsafeCell::new(0)};
}
struct Cleanup;
impl Drop for Cleanup {
    fn drop(&mut self) {
        VALUES.with(|slot| {
            let pointer = slot.get();
            if pointer.is_null() {
                return;
            }
            for _ in 0..4 {
                let values = std::mem::take(&mut *unsafe { &*pointer }.borrow_mut());
                if values.is_empty() {
                    break;
                }
                for (key, value) in values {
                    let destructor = held(keys()).keys.get(&key).copied().flatten();
                    if let Some(destructor) = destructor.filter(|_| !value.is_null()) {
                        unsafe { destructor(value) };
                    }
                }
            }
            slot.set(ptr::null_mut());
            unsafe { drop(Box::from_raw(pointer)) };
        });
    }
}
fn with_values<T>(action: impl FnOnce(&RefCell<BTreeMap<u32, *mut c_void>>) -> T) -> T {
    VALUES.with(|slot| {
        let mut pointer = slot.get();
        if pointer.is_null() {
            pointer = Box::into_raw(Box::new(RefCell::new(BTreeMap::new())));
            slot.set(pointer);
            CLEANUP.with(|_| {});
        }
        action(unsafe { &*pointer })
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_key_create(
    out: *mut u32,
    destructor: Option<Destructor>,
) -> c_int {
    if out.is_null() {
        return EINVAL;
    }
    let mut keys = held(keys());
    let key = keys.next;
    let Some(next) = key.checked_add(1) else {
        return ENOMEM;
    };
    keys.next = next;
    keys.keys.insert(key, destructor);
    unsafe { out.write(key) };
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_key_delete(key: u32) -> c_int {
    if held(keys()).keys.remove(&key).is_some() {
        0
    } else {
        EINVAL
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_getspecific(key: u32) -> *mut c_void {
    if !held(keys()).keys.contains_key(&key) {
        return ptr::null_mut();
    }
    with_values(|values| {
        values
            .borrow()
            .get(&key)
            .copied()
            .unwrap_or(ptr::null_mut())
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_setspecific(key: u32, value: *const c_void) -> c_int {
    if !held(keys()).keys.contains_key(&key) {
        return EINVAL;
    }
    with_values(|values| {
        values.borrow_mut().insert(key, value.cast_mut());
    });
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn ossl_trueos_thread_self() -> usize {
    #[cfg(target_os = "trueos")]
    {
        unsafe extern "C" {
            fn trueos_cabi_thread_current_id() -> usize;
        }
        unsafe { trueos_cabi_thread_current_id() }
    }
    #[cfg(not(target_os = "trueos"))]
    {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        thread_local! {static ID: usize = NEXT.fetch_add(1,Ordering::Relaxed);}
        ID.with(|id| *id)
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ossl_trueos_errno() -> *mut c_int {
    ERRNO.with(UnsafeCell::get)
}
#[unsafe(no_mangle)]
pub extern "C" fn ossl_trueos_yield() -> c_int {
    std::thread::yield_now();
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn ossl_trueos_getpid() -> c_int {
    std::process::id() as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_getenv(name: *const c_char) -> *mut c_char {
    if name.is_null() {
        return ptr::null_mut();
    }
    let Ok(name) = (unsafe { CStr::from_ptr(name) }).to_str() else {
        return ptr::null_mut();
    };
    let Some(value) = std::env::var_os(name) else {
        return ptr::null_mut();
    };
    let Ok(value) = CString::new(value.as_encoded_bytes()) else {
        return ptr::null_mut();
    };
    static CACHE: OnceLock<Mutex<BTreeMap<(String, Vec<u8>), CString>>> = OnceLock::new();
    // Preserve returned pointers across subsequent environment updates. These
    // app-local strings live for the Hull, like OpenSSL's own option caches.
    held(CACHE.get_or_init(|| Mutex::new(BTreeMap::new())))
        .entry((name.to_owned(), value.as_bytes().to_vec()))
        .or_insert(value)
        .as_ptr()
        .cast_mut()
}
fn unix_nanos() -> Option<u64> {
    #[cfg(target_os = "trueos")]
    {
        unsafe extern "C" {
            fn trueos_time_unix_nanos() -> u64;
        }
        let value = unsafe { trueos_time_unix_nanos() };
        (value != 0).then_some(value)
    }
    #[cfg(not(target_os = "trueos"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos()
            .try_into()
            .ok()
    }
}
fn monotonic_nanos() -> u64 {
    #[cfg(target_os = "trueos")]
    {
        unsafe extern "C" {
            fn trueos_time_monotonic_nanos() -> u64;
        }
        unsafe { trueos_time_monotonic_nanos() }
    }
    #[cfg(not(target_os = "trueos"))]
    {
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}
#[repr(C)]
pub struct Timespec {
    seconds: i64,
    nanos: i64,
}
#[repr(C)]
pub struct Timeval {
    seconds: i64,
    micros: i64,
}
fn failure(code: c_int) -> c_int {
    unsafe { ossl_trueos_errno().write(code) };
    -1
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_time(out: *mut i64) -> i64 {
    let Some(nanos) = unix_nanos() else {
        return failure(5) as i64;
    };
    let seconds = (nanos / 1_000_000_000) as i64;
    if !out.is_null() {
        unsafe { out.write(seconds) };
    }
    seconds
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_gettimeofday(
    out: *mut Timeval,
    timezone: *mut c_void,
) -> c_int {
    if out.is_null() || !timezone.is_null() {
        return failure(EINVAL);
    }
    let Some(nanos) = unix_nanos() else {
        return failure(5);
    };
    unsafe {
        out.write(Timeval {
            seconds: (nanos / 1_000_000_000) as i64,
            micros: (nanos % 1_000_000_000 / 1000) as i64,
        })
    };
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_clock_gettime(clock: c_int, out: *mut Timespec) -> c_int {
    if out.is_null() {
        return failure(EINVAL);
    }
    let nanos = match clock {
        0 => match unix_nanos() {
            Some(nanos) => nanos,
            None => return failure(5),
        },
        1 => monotonic_nanos(),
        _ => return failure(EINVAL),
    };
    unsafe {
        out.write(Timespec {
            seconds: (nanos / 1_000_000_000) as i64,
            nanos: (nanos % 1_000_000_000) as i64,
        })
    };
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_nanosleep(
    request: *const Timespec,
    remaining: *mut Timespec,
) -> c_int {
    let Some(request) = (unsafe { request.as_ref() }) else {
        return failure(EINVAL);
    };
    if request.seconds < 0 || !(0..1_000_000_000).contains(&request.nanos) {
        return failure(EINVAL);
    }
    std::thread::sleep(Duration::new(request.seconds as u64, request.nanos as u32));
    if !remaining.is_null() {
        unsafe {
            remaining.write(Timespec {
                seconds: 0,
                nanos: 0,
            })
        };
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_strerror_r(
    code: c_int,
    out: *mut c_char,
    capacity: usize,
) -> c_int {
    if out.is_null() || capacity == 0 {
        return EINVAL;
    }
    let message = match code {
        0 => "success".to_owned(),
        1 => "operation not permitted".to_owned(),
        2 => "file not found".to_owned(),
        5 => "native I/O or clock unavailable".to_owned(),
        12 => "out of memory".to_owned(),
        22 => "invalid argument".to_owned(),
        34 => "value out of range".to_owned(),
        _ => format!("native OpenSSL error {code}"),
    };
    let count = message.len().min(capacity - 1);
    unsafe {
        ptr::copy_nonoverlapping(message.as_ptr(), out.cast(), count);
        out.add(count).write(0)
    };
    if count == message.len() { 0 } else { 34 }
}
