//! Private VMX child-Hull entrypoint for `new Worker(source)`.
//!
//! The kernel starts the same qjs Blueprint with `--trueos-child-worker` and
//! places `source` into the first parent-to-child frame. Handle zero is the
//! child's endpoint for its parent. No terminal APIs are touched here.

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::ffi::c_char;

use crate as qjs;

pub const ARGUMENT: &str = "--trueos-child-worker";

const PARENT_HANDLE: u64 = v::child_hull::HANDLE_PARENT;
const CONTEXT_ID: u32 = 1;

/// Run the child-only QuickJS VM. The kernel owns the VMX lane; this function
/// owns only the QuickJS runtime/context living on that lane.
pub fn run() -> Result<(), String> {
    let startup = receive_required(PARENT_HANDLE)?;
    if startup.is_empty() {
        return Err("child worker startup frame is empty".into());
    }

    let vm = unsafe { qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Worker) }
        .ok_or_else(|| String::from("failed to create child QuickJS VM"))?;
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();
    qjs::workers::enter_child_context(ctx, CONTEXT_ID);

    let filename = b"<worker>\0";
    let value = unsafe {
        qjs::js_eval_bytes(
            ctx,
            startup.as_slice(),
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
        )
    };
    if value.is_exception() {
        unsafe { qjs::qjs_diag::dump_last_exception(ctx, "worker-startup-eval") };
        let _ = qjs::workers::post_to_parent(
            CONTEXT_ID,
            b"{\"ok\":0,\"dbg\":\"worker-startup-eval-exception\"}",
        );
        unsafe { qjs::js_free_value(ctx, value) };
        qjs::workers::leave_child_context(ctx);
        return Err("child worker startup evaluation failed".into());
    }
    unsafe { qjs::js_free_value(ctx, value) };
    let _ =
        qjs::workers::post_to_parent(CONTEXT_ID, b"{\"ok\":1,\"dbg\":\"worker-startup-eval-ok\"}");

    // A child remains alive even when it has no currently pending jobs: the
    // parent can post a message later. `poll_once` yields the VMX lane between
    // QuickJS pumps without owning an executor or a terminal.
    loop {
        if v::child_hull::status(PARENT_HANDLE) >= v::child_hull::STATUS_EXITED {
            break Ok(());
        }
        let _ = unsafe { qjs::vm::pump_runtime_once(rt, ctx, "qjs-child-worker") };
        unsafe { qjs::trueos_shims::trueos_cabi_poll_once() };
    }
}

fn receive_required(handle: u64) -> Result<Vec<u8>, String> {
    loop {
        if v::child_hull::status(handle) >= v::child_hull::STATUS_EXITED {
            return Err("child worker parent exited before startup frame arrived".into());
        }
        match receive_one(handle)? {
            Some(frame) => return Ok(frame),
            None => unsafe { qjs::trueos_shims::trueos_cabi_poll_once() },
        }
    }
}

/// `receive_v1` returns zero for no frame and a positive required byte count
/// when called with a null or too-small buffer. The kernel retains that frame
/// until this second, sized receive succeeds.
pub(crate) fn receive_one(handle: u64) -> Result<Option<Vec<u8>>, String> {
    let Some(len) = v::child_hull::receive_len(handle)
        .map_err(|rc| alloc::format!("child receive probe failed: {rc}"))?
    else {
        return Ok(None);
    };
    let mut frame = Vec::new();
    frame.resize(len, 0);
    let got = v::child_hull::receive_into(handle, frame.as_mut_slice())
        .map_err(|rc| alloc::format!("child receive failed: {rc}"))?
        .ok_or_else(|| String::from("child frame disappeared while retained"))?;
    if got > frame.len() {
        return Err("child receive overflow".into());
    }
    frame.truncate(got);
    Ok(Some(frame))
}
