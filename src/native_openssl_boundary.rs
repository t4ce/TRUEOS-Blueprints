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

fn entropy_with(bytes: &mut [u8], mut fill: impl FnMut(&mut [u8]) -> bool) -> bool {
    for chunk in bytes.chunks_mut(256) {
        if !fill(chunk) {
            bytes.fill(0);
            return false;
        }
    }
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ossl_trueos_getentropy(out: *mut c_void, len: usize) -> c_int {
    if len == 0 {
        return 0;
    }
    if out.is_null() || len > isize::MAX as usize {
        return failure(EINVAL);
    }
    // C entropy buffers may be freshly allocated and uninitialized. Establish
    // initialized bytes before forming a Rust slice, including on failure.
    unsafe { ptr::write_bytes(out.cast::<u8>(), 0, len) };
    let bytes = unsafe { std::slice::from_raw_parts_mut(out.cast::<u8>(), len) };
    let ok = entropy_with(bytes, |chunk| {
        #[cfg(target_os = "trueos")]
        {
            unsafe extern "C" {
                fn trueos_cabi_random_bytes_v1(out: *mut u8, len: usize) -> c_int;
            }
            // The native bridge chooses guest transport or the hosted thread
            // path. Raw vmcall is invalid on a native stackful worker.
            unsafe { trueos_cabi_random_bytes_v1(chunk.as_mut_ptr(), chunk.len()) == 0 }
        }
        #[cfg(not(target_os = "trueos"))]
        {
            // Explicit host proof provider: the host OS's fallible strong
            // entropy, never a deterministic fixture or hardware fallback.
            unsafe extern "C" {
                #[link_name = "getentropy"]
                fn host_getentropy(out: *mut c_void, len: usize) -> c_int;
            }
            unsafe { host_getentropy(chunk.as_mut_ptr().cast(), chunk.len()) == 0 }
        }
    });
    if ok { 0 } else { failure(5) }
}

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
    let object = handle.object.load(Ordering::Acquire);
    if !object.is_null() && held(&unsafe { &*object }.state).owner.is_some() {
        return 16;
    }
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
                let scheduled: Vec<_> = unsafe { &*pointer }.borrow().keys().copied().collect();
                if scheduled.is_empty() {
                    break;
                }
                for key in scheduled {
                    // Clear only this key before its callback. Other keys must
                    // remain readable by OpenSSL's interdependent cleanups.
                    let value = unsafe { &*pointer }.borrow_mut().remove(&key);
                    let Some(value) = value else {
                        continue;
                    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicU32, AtomicUsize},
    };

    fn native_mutex() -> NativeMutex {
        NativeMutex {
            object: AtomicPtr::new(ptr::null_mut()),
        }
    }
    fn native_cond() -> NativeCond {
        NativeCond {
            object: AtomicPtr::new(ptr::null_mut()),
        }
    }
    #[test]
    fn entropy_failure_clears_partial_output_and_validates_request() {
        let mut bytes = [0xabu8; 300];
        let mut calls = 0;
        assert!(!entropy_with(&mut bytes, |chunk| {
            calls += 1;
            chunk.fill(7);
            calls < 2
        }));
        assert!(bytes.iter().all(|byte| *byte == 0));
        assert_eq!(unsafe { ossl_trueos_getentropy(ptr::null_mut(), 0) }, 0);
        assert_eq!(unsafe { ossl_trueos_getentropy(ptr::null_mut(), 1) }, -1);
        assert_eq!(
            unsafe { ossl_trueos_getentropy(bytes.as_mut_ptr().cast(), isize::MAX as usize + 1) },
            -1
        );
        assert!(bytes.iter().all(|byte| *byte == 0));
        let mut uninitialized = std::mem::MaybeUninit::<[u8; 257]>::uninit();
        assert_eq!(
            unsafe { ossl_trueos_getentropy(uninitialized.as_mut_ptr().cast(), 257) },
            0
        );
        let initialized = unsafe { uninitialized.assume_init() };
        assert!(initialized.iter().any(|byte| *byte != 0));
        assert_eq!(
            unsafe { ossl_trueos_getentropy(bytes.as_mut_ptr().cast(), 257) },
            0
        );
        assert_eq!(
            unsafe { ossl_trueos_getentropy(bytes.as_mut_ptr().cast(), 256) },
            0
        );
        assert!(bytes[..256].iter().any(|byte| *byte != 0));
    }
    struct Shared {
        lock: NativeMutex,
        cond: NativeCond,
        values: UnsafeCell<(usize, bool, usize)>,
    }
    unsafe impl Sync for Shared {}

    #[test]
    fn mutex_static_initialization_serializes_workers_and_checks_ownership() {
        let shared = Arc::new(Shared {
            lock: native_mutex(),
            cond: native_cond(),
            values: UnsafeCell::new((0, false, 0)),
        });
        let mut workers = Vec::new();
        for _ in 0..8 {
            let shared = shared.clone();
            workers.push(std::thread::spawn(move || unsafe {
                let lock = ptr::from_ref(&shared.lock).cast_mut();
                for _ in 0..1000 {
                    assert_eq!(ossl_trueos_mutex_lock(lock), 0);
                    (*shared.values.get()).0 += 1;
                    assert_eq!(ossl_trueos_mutex_unlock(lock), 0);
                }
                assert_eq!(ossl_trueos_mutex_unlock(lock), EPERM);
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        unsafe {
            assert_eq!((*shared.values.get()).0, 8000);
            let lock = ptr::from_ref(&shared.lock).cast_mut();
            assert_eq!(ossl_trueos_mutex_lock(lock), 0);
            assert_eq!(ossl_trueos_mutex_lock(lock), EDEADLK);
            assert_eq!(ossl_trueos_mutex_destroy(lock), 16);
            assert_eq!(ossl_trueos_mutex_unlock(lock), 0);
            assert_eq!(ossl_trueos_mutex_destroy(lock), 0);
        }
    }

    #[test]
    fn condition_wait_releases_and_reacquires_mutex_and_broadcast_wakes_all() {
        let shared = Arc::new(Shared {
            lock: native_mutex(),
            cond: native_cond(),
            values: UnsafeCell::new((0, false, 0)),
        });
        let (ready, arrivals) = std::sync::mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..4 {
            let shared = shared.clone();
            let ready = ready.clone();
            workers.push(std::thread::spawn(move || unsafe {
                let lock = ptr::from_ref(&shared.lock).cast_mut();
                let cond = ptr::from_ref(&shared.cond).cast_mut();
                assert_eq!(ossl_trueos_mutex_lock(lock), 0);
                (*shared.values.get()).0 += 1;
                ready.send(()).unwrap();
                while !(*shared.values.get()).1 {
                    assert_eq!(ossl_trueos_cond_wait(cond, lock), 0);
                }
                (*shared.values.get()).2 += 1;
                assert_eq!(ossl_trueos_mutex_unlock(lock), 0);
            }));
        }
        for _ in 0..4 {
            arrivals.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        unsafe {
            let lock = ptr::from_ref(&shared.lock).cast_mut();
            assert_eq!(ossl_trueos_mutex_lock(lock), 0);
            assert_eq!((*shared.values.get()).0, 4);
            (*shared.values.get()).1 = true;
            assert_eq!(
                ossl_trueos_cond_broadcast(ptr::from_ref(&shared.cond).cast_mut()),
                0
            );
            assert_eq!(ossl_trueos_mutex_unlock(lock), 0);
        }
        for worker in workers {
            worker.join().unwrap();
        }
        unsafe {
            assert_eq!((*shared.values.get()).2, 4);
            assert_eq!(
                ossl_trueos_cond_destroy(ptr::from_ref(&shared.cond).cast_mut()),
                0
            );
            assert_eq!(
                ossl_trueos_mutex_destroy(ptr::from_ref(&shared.lock).cast_mut()),
                0
            );
        }
    }

    struct Payload {
        counter: Arc<AtomicUsize>,
        key: u32,
        remaining: u32,
    }
    unsafe extern "C" fn cleanup(pointer: *mut c_void) {
        let payload = unsafe { Box::from_raw(pointer.cast::<Payload>()) };
        payload.counter.fetch_add(1, Ordering::SeqCst);
        if payload.remaining != 0 {
            let next = Box::new(Payload {
                counter: payload.counter.clone(),
                key: payload.key,
                remaining: payload.remaining - 1,
            });
            assert_eq!(
                unsafe { ossl_trueos_setspecific(payload.key, Box::into_raw(next).cast()) },
                0
            );
        }
    }
    #[test]
    fn tls_values_are_thread_local_and_destructors_can_reinstall_values() {
        static KEY: AtomicU32 = AtomicU32::new(0);
        let mut key = 0;
        assert_eq!(
            unsafe { ossl_trueos_key_create(&mut key, Some(cleanup)) },
            0
        );
        KEY.store(key, Ordering::SeqCst);
        let counter = Arc::new(AtomicUsize::new(0));
        let owned = counter.clone();
        std::thread::spawn(move || unsafe {
            assert!(ossl_trueos_getspecific(key).is_null());
            let value = Box::into_raw(Box::new(Payload {
                counter: owned,
                key,
                remaining: 2,
            }))
            .cast();
            assert_eq!(ossl_trueos_setspecific(key, value), 0);
            assert_eq!(ossl_trueos_getspecific(key), value);
        })
        .join()
        .unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        assert!(unsafe { ossl_trueos_getspecific(key) }.is_null());
        assert_eq!(unsafe { ossl_trueos_key_delete(key) }, 0);
        assert_eq!(unsafe { ossl_trueos_setspecific(key, ptr::null()) }, EINVAL);
    }
    #[test]
    fn deleting_tls_key_suppresses_other_threads_destructor_and_does_not_reuse_key() {
        let mut key = 0;
        assert_eq!(
            unsafe { ossl_trueos_key_create(&mut key, Some(cleanup)) },
            0
        );
        let counter = Arc::new(AtomicUsize::new(0));
        let owned = counter.clone();
        let barrier = Arc::new(Barrier::new(2));
        let child_barrier = barrier.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || unsafe {
            let value = Box::into_raw(Box::new(Payload {
                counter: owned,
                key,
                remaining: 0,
            }));
            assert_eq!(ossl_trueos_setspecific(key, value.cast()), 0);
            sent.send(value as usize).unwrap();
            child_barrier.wait();
            assert!(ossl_trueos_getspecific(key).is_null());
        });
        let value = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(unsafe { ossl_trueos_key_delete(key) }, 0);
        barrier.wait();
        worker.join().unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        unsafe {
            drop(Box::from_raw(value as *mut Payload));
        }
        let mut next = 0;
        assert_eq!(unsafe { ossl_trueos_key_create(&mut next, None) }, 0);
        assert_ne!(next, key);
        assert_eq!(unsafe { ossl_trueos_key_delete(next) }, 0);
    }
    struct LinkedCleanup {
        other: u32,
        expected: usize,
        observed: Arc<AtomicUsize>,
    }
    unsafe extern "C" fn linked_cleanup(pointer: *mut c_void) {
        let payload = unsafe { Box::from_raw(pointer.cast::<LinkedCleanup>()) };
        assert_eq!(
            unsafe { ossl_trueos_getspecific(payload.other) } as usize,
            payload.expected
        );
        payload.observed.fetch_add(1, Ordering::SeqCst);
    }
    #[test]
    fn tls_destructor_can_read_other_keys_before_their_cleanup() {
        let mut first = 0;
        let mut other = 0;
        assert_eq!(
            unsafe { ossl_trueos_key_create(&mut first, Some(linked_cleanup)) },
            0
        );
        assert_eq!(unsafe { ossl_trueos_key_create(&mut other, None) }, 0);
        let observed = Arc::new(AtomicUsize::new(0));
        let result = observed.clone();
        std::thread::spawn(move || unsafe {
            assert_eq!(
                ossl_trueos_setspecific(other, 0x1234usize as *const c_void),
                0
            );
            let value = Box::into_raw(Box::new(LinkedCleanup {
                other,
                expected: 0x1234,
                observed: result,
            }));
            assert_eq!(ossl_trueos_setspecific(first, value.cast()), 0);
        })
        .join()
        .unwrap();
        assert_eq!(observed.load(Ordering::SeqCst), 1);
        assert_eq!(unsafe { ossl_trueos_key_delete(first) }, 0);
        assert_eq!(unsafe { ossl_trueos_key_delete(other) }, 0);
    }
    #[test]
    fn errno_thread_identity_clocks_and_error_text_have_native_contracts() {
        unsafe {
            ossl_trueos_errno().write(71);
        }
        let id = ossl_trueos_thread_self();
        std::thread::spawn(move || unsafe {
            assert_ne!(ossl_trueos_thread_self(), id);
            assert_eq!(ossl_trueos_errno().read(), 0);
            ossl_trueos_errno().write(72);
        })
        .join()
        .unwrap();
        assert_eq!(unsafe { ossl_trueos_errno().read() }, 71);
        assert!(unsafe { ossl_trueos_time(ptr::null_mut()) } > 1_700_000_000);
        let mut time = Timespec {
            seconds: 0,
            nanos: 0,
        };
        assert_eq!(unsafe { ossl_trueos_clock_gettime(1, &mut time) }, 0);
        assert_eq!(unsafe { ossl_trueos_clock_gettime(99, &mut time) }, -1);
        assert_eq!(unsafe { ossl_trueos_errno().read() }, EINVAL);
        let mut bytes = [0u8; 64];
        assert_eq!(
            unsafe { ossl_trueos_strerror_r(22, bytes.as_mut_ptr().cast(), bytes.len()) },
            0
        );
        assert_eq!(
            CStr::from_bytes_until_nul(&bytes)
                .unwrap()
                .to_str()
                .unwrap(),
            "invalid argument"
        );
        let mut short = [0u8; 4];
        assert_eq!(
            unsafe { ossl_trueos_strerror_r(22, short.as_mut_ptr().cast(), short.len()) },
            34
        );
        assert_eq!(&short, b"inv\0");
    }
}
