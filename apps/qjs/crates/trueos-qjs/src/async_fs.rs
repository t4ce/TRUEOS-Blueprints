#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::ToString;
use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll, Waker};

use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::trueos_shims::{
    trueos_cabi_net_fetch_bytes_discard, trueos_cabi_net_fetch_bytes_read,
    trueos_cabi_net_fetch_bytes_result_len, trueos_cabi_net_fetch_bytes_start,
    trueos_cabi_net_fetch_bytes_wait, trueos_cabi_net_fetch_discard,
    trueos_cabi_net_fetch_post_json_bytes_start, trueos_cabi_net_fetch_post_json_start,
    trueos_cabi_net_fetch_result, trueos_cabi_net_fetch_start, trueos_cabi_net_fetch_wait,
};

include!("../../../../../../TRUEOS/src/r/cabi_codes.rs");

const ASYNC_FS_MAX_QUEUE: usize = 64;

static ASYNC_FS_SEQ: AtomicU32 = AtomicU32::new(1);
static ASYNC_FS_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);

#[inline]
fn async_fs_diag(msg: &str) {
    if ASYNC_FS_DIAG_LOGS.fetch_add(1, Ordering::Relaxed) < 64 {
        crate::trueos_shims::log_info(msg);
    }
}

#[derive(Debug)]
enum AsyncFsRequest {
    ReadFile {
        id: u32,
        path: String,
    },
    WriteFile {
        id: u32,
        path: String,
        data: Vec<u8>,
    },
}

#[derive(Clone, Debug)]
struct AsyncFsCompletion {
    id: u32,
    rc: i32,
    data: Vec<u8>,
}

type AsyncFsFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, i32>> + Send>>;

struct ActiveAsyncFsRequest {
    id: u32,
    future: AsyncFsFuture,
}

static ASYNC_FS_REQS: Mutex<VecDeque<AsyncFsRequest>> = Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE: Mutex<Vec<ActiveAsyncFsRequest>> = Mutex::new(Vec::new());
static ASYNC_FS_DONE: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());
static ASYNC_FS_RESULTS: Mutex<BTreeMap<u32, AsyncFsCompletion>> = Mutex::new(BTreeMap::new());
static ASYNC_NET_FILE_OPS: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());
static ASYNC_NET_BYTES_OPS: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

#[inline]
fn next_async_fs_id() -> u32 {
    ASYNC_FS_SEQ.fetch_add(1, Ordering::Relaxed)
}

fn is_net_file_op(id: u32) -> bool {
    ASYNC_NET_FILE_OPS.lock().contains(&id)
}

fn is_net_bytes_op(id: u32) -> bool {
    ASYNC_NET_BYTES_OPS.lock().contains(&id)
}

fn push_async_fs_req(req: AsyncFsRequest) -> Result<(), i32> {
    let mut q = ASYNC_FS_REQS.lock();
    if q.len() >= ASYNC_FS_MAX_QUEUE {
        return Err(FS_ERR_NO_SPACE);
    }
    q.push_back(req);
    Ok(())
}

fn take_async_fs_req() -> Option<AsyncFsRequest> {
    let mut q = ASYNC_FS_REQS.lock();
    q.pop_front()
}

fn activate_async_fs_req(req: AsyncFsRequest) {
    let (id, future): (u32, AsyncFsFuture) = match req {
        AsyncFsRequest::ReadFile { id, path } => (
            id,
            Box::pin(async move { v::vfs_async::read_file(path.as_bytes()).await }),
        ),
        AsyncFsRequest::WriteFile { id, path, data } => (
            id,
            Box::pin(async move {
                v::vfs_async::write_file(path.as_bytes(), data.as_slice())
                    .await
                    .map(|()| Vec::new())
            }),
        ),
    };
    ASYNC_FS_ACTIVE
        .lock()
        .push(ActiveAsyncFsRequest { id, future });
}

fn push_async_fs_completion(done: AsyncFsCompletion) {
    let id = done.id;
    let inserted = {
        let mut res = ASYNC_FS_RESULTS.lock();
        if res.contains_key(&id) {
            false
        } else {
            res.insert(id, done);
            true
        }
    };
    if inserted {
        ASYNC_FS_DONE.lock().push_back(id);
    }
}

fn completion_rc_len(id: u32) -> Option<(i32, usize)> {
    let res = ASYNC_FS_RESULTS.lock();
    res.get(&id).map(|c| (c.rc, c.data.len()))
}

fn remove_async_fs_completion(id: u32) -> Option<AsyncFsCompletion> {
    let mut res = ASYNC_FS_RESULTS.lock();
    res.remove(&id)
}

fn has_result(id: u32) -> bool {
    ASYNC_FS_RESULTS.lock().contains_key(&id)
}

pub fn has_completion_result(op_id: u32) -> bool {
    has_result(op_id)
}

fn remove_queued_reqs(id: u32) {
    let mut q = ASYNC_FS_REQS.lock();
    q.retain(|req| match req {
        AsyncFsRequest::ReadFile { id: rid, .. } | AsyncFsRequest::WriteFile { id: rid, .. } => {
            *rid != id
        }
    });
}

fn remove_active_req(id: u32) {
    ASYNC_FS_ACTIVE.lock().retain(|req| req.id != id);
}

fn remove_done_id(id: u32) {
    let mut done = ASYNC_FS_DONE.lock();
    if let Some(pos) = done.iter().position(|x| *x == id) {
        done.remove(pos);
    }
}

fn has_completion() -> bool {
    !ASYNC_FS_DONE.lock().is_empty()
}

fn start_net_fetch_to_file_via_cabi(url: &str, path: &str) -> Result<u32, i32> {
    let id =
        unsafe { trueos_cabi_net_fetch_start(url.as_ptr(), url.len(), path.as_ptr(), path.len()) };
    if id == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    Ok(id)
}

fn start_net_fetch_bytes_via_cabi(url: &str) -> Result<u32, i32> {
    let id = unsafe { trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if id == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    Ok(id)
}

fn start_net_post_json_to_file_via_cabi(
    url: &str,
    path: &str,
    body_json: &str,
    bearer: Option<&str>,
) -> Result<u32, i32> {
    let (bearer_ptr, bearer_len) = if let Some(v) = bearer {
        (v.as_ptr(), v.len())
    } else {
        (core::ptr::null(), 0)
    };

    let id = unsafe {
        trueos_cabi_net_fetch_post_json_start(
            url.as_ptr(),
            url.len(),
            path.as_ptr(),
            path.len(),
            body_json.as_ptr(),
            body_json.len(),
            bearer_ptr,
            bearer_len,
        )
    };
    if id == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    Ok(id)
}

fn start_net_post_json_bytes_via_cabi(
    url: &str,
    body_json: &str,
    bearer: Option<&str>,
) -> Result<u32, i32> {
    let (bearer_ptr, bearer_len) = if let Some(v) = bearer {
        (v.as_ptr(), v.len())
    } else {
        (core::ptr::null(), 0)
    };

    let id = unsafe {
        trueos_cabi_net_fetch_post_json_bytes_start(
            url.as_ptr(),
            url.len(),
            body_json.as_ptr(),
            body_json.len(),
            bearer_ptr,
            bearer_len,
        )
    };
    if id == 0 {
        return Err(FS_ERR_BAD_PARAM);
    }
    Ok(id)
}

/// Process at most one bounded queue batch from the VM's owning task.
///
/// Filesystem work enters the kernel through the native asynchronous TRUEOSFS
/// operation ABI. Futures stay active across VM pump ticks so QuickJS jobs,
/// timers, and worker messages continue to advance while storage is pending.
pub fn pump_service() -> bool {
    let mut progress = false;
    for _ in 0..ASYNC_FS_MAX_QUEUE {
        let Some(req) = take_async_fs_req() else {
            break;
        };
        activate_async_fs_req(req);
        progress = true;
    }

    let mut active = core::mem::take(&mut *ASYNC_FS_ACTIVE.lock());
    let mut pending = Vec::with_capacity(active.len());
    let mut context = Context::from_waker(Waker::noop());
    for mut req in active.drain(..) {
        match req.future.as_mut().poll(&mut context) {
            Poll::Ready(Ok(data)) => {
                async_fs_diag(
                    alloc::format!("qjs-async-fs: done id={} len={}\n", req.id, data.len())
                        .as_str(),
                );
                push_async_fs_completion(AsyncFsCompletion {
                    id: req.id,
                    rc: 0,
                    data,
                });
                progress = true;
            }
            Poll::Ready(Err(rc)) => {
                async_fs_diag(
                    alloc::format!("qjs-async-fs: error id={} rc={}\n", req.id, rc).as_str(),
                );
                push_async_fs_completion(AsyncFsCompletion {
                    id: req.id,
                    rc,
                    data: Vec::new(),
                });
                progress = true;
            }
            Poll::Pending => pending.push(req),
        }
    }
    ASYNC_FS_ACTIVE.lock().extend(pending);
    progress
}

pub fn start_net_fetch_to_file(url: &[u8], path: &[u8]) -> Result<u32, i32> {
    if url.is_empty() || path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(url_str) = core::str::from_utf8(url) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = start_net_fetch_to_file_via_cabi(url_str, path_str)?;
    ASYNC_NET_FILE_OPS.lock().insert(id);
    Ok(id)
}

pub fn start_net_fetch_bytes(url: &[u8]) -> Result<u32, i32> {
    if url.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    let Ok(url_str) = core::str::from_utf8(url) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = start_net_fetch_bytes_via_cabi(url_str)?;
    ASYNC_NET_BYTES_OPS.lock().insert(id);
    Ok(id)
}

pub fn start_net_post_json_to_file(
    url: &[u8],
    path: &[u8],
    body_json: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    if url.is_empty() || path.is_empty() || body_json.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(url_str) = core::str::from_utf8(url) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let Ok(body_str) = core::str::from_utf8(body_json) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let bearer_str = match bearer {
        Some(v) => {
            let Ok(s) = core::str::from_utf8(v) else {
                return Err(FS_ERR_BAD_UTF8);
            };
            Some(s)
        }
        None => None,
    };

    let id = start_net_post_json_to_file_via_cabi(url_str, path_str, body_str, bearer_str)?;
    ASYNC_NET_FILE_OPS.lock().insert(id);
    Ok(id)
}

pub fn start_net_post_json_bytes(
    url: &[u8],
    body_json: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    if url.is_empty() || body_json.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    let Ok(url_str) = core::str::from_utf8(url) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let Ok(body_str) = core::str::from_utf8(body_json) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let bearer_str = match bearer {
        Some(v) => Some(core::str::from_utf8(v).map_err(|_| FS_ERR_BAD_UTF8)?),
        None => None,
    };
    let id = start_net_post_json_bytes_via_cabi(url_str, body_str, bearer_str)?;
    ASYNC_NET_BYTES_OPS.lock().insert(id);
    Ok(id)
}

pub fn start_read_file(path: &[u8]) -> Result<u32, i32> {
    if path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = next_async_fs_id();
    let req = AsyncFsRequest::ReadFile {
        id,
        path: path_str.to_string(),
    };
    push_async_fs_req(req)?;
    async_fs_diag(
        alloc::format!(
            "qjs-async-fs: read queued id={} path_len={}\n",
            id,
            path.len()
        )
        .as_str(),
    );
    Ok(id)
}

pub fn start_write_file(path: &[u8], data: &[u8]) -> Result<u32, i32> {
    if path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > BLUEPRINT_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = next_async_fs_id();
    let path = path_str.to_string();
    push_async_fs_req(AsyncFsRequest::WriteFile {
        id,
        path,
        data: data.to_vec(),
    })?;
    Ok(id)
}

pub fn poll_completed(out_id: *mut u32) -> i32 {
    if out_id.is_null() {
        return 0;
    }
    let mut done = ASYNC_FS_DONE.lock();
    let Some(id) = done.pop_front() else {
        return 0;
    };
    unsafe { *out_id = id };
    1
}

pub fn result_len(op_id: u32) -> isize {
    if is_net_file_op(op_id) {
        let rc = unsafe { trueos_cabi_net_fetch_result(op_id) };
        if rc == FS_ERR_NOT_FOUND {
            return FS_ERR_NOT_FOUND as isize;
        }
        if rc != 0 {
            return rc as isize;
        }
        return 0;
    }
    if is_net_bytes_op(op_id) {
        return unsafe { trueos_cabi_net_fetch_bytes_result_len(op_id) };
    }
    let Some((rc, len)) = completion_rc_len(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        return rc as isize;
    }
    len as isize
}

pub fn wait_net_fetch(op_id: u32, timeout_ms: u64) -> i32 {
    if is_net_file_op(op_id) {
        return unsafe { trueos_cabi_net_fetch_wait(op_id, timeout_ms) };
    }
    if is_net_bytes_op(op_id) {
        return unsafe { trueos_cabi_net_fetch_bytes_wait(op_id, timeout_ms) };
    }
    FS_ERR_BAD_PARAM
}

pub fn read_result(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize {
    if is_net_file_op(op_id) {
        let rc = unsafe { trueos_cabi_net_fetch_result(op_id) };
        if rc == FS_ERR_NOT_FOUND {
            return FS_ERR_NOT_FOUND as isize;
        }
        let _ = unsafe { trueos_cabi_net_fetch_discard(op_id) };
        ASYNC_NET_FILE_OPS.lock().remove(&op_id);
        if rc != 0 {
            return rc as isize;
        }
        let _ = (out_ptr, out_cap);
        return 0;
    }
    if is_net_bytes_op(op_id) {
        let got = unsafe { trueos_cabi_net_fetch_bytes_read(op_id, out_ptr, out_cap) };
        if got != FS_ERR_NOT_FOUND as isize {
            ASYNC_NET_BYTES_OPS.lock().remove(&op_id);
        }
        return got;
    }
    let Some((rc, len)) = completion_rc_len(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        remove_async_fs_completion(op_id);
        return rc as isize;
    }

    if out_ptr.is_null() || out_cap == 0 {
        return len as isize;
    }
    if len > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }

    let Some(c) = remove_async_fs_completion(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    unsafe { core::ptr::copy_nonoverlapping(c.data.as_ptr(), out_ptr, c.data.len()) };
    let n = c.data.len() as isize;
    n
}

pub fn discard(op_id: u32) -> i32 {
    if is_net_file_op(op_id) {
        let _ = unsafe { trueos_cabi_net_fetch_discard(op_id) };
        ASYNC_NET_FILE_OPS.lock().remove(&op_id);
        return 0;
    }
    if is_net_bytes_op(op_id) {
        let _ = unsafe { trueos_cabi_net_fetch_bytes_discard(op_id) };
        ASYNC_NET_BYTES_OPS.lock().remove(&op_id);
        return 0;
    }
    remove_done_id(op_id);
    remove_async_fs_completion(op_id);
    remove_queued_reqs(op_id);
    remove_active_req(op_id);
    0
}

pub async fn wait_for_completion(timeout_ms: u64) -> bool {
    if timeout_ms == 0 {
        return has_completion();
    }
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        if has_completion() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}
