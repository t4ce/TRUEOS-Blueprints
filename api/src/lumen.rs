//! Replicatable Lumen Blueprint capability.
//!
//! The guest owns policy and the portable session image. TRUEOS owns immutable
//! model assets and the CPU/IGC/GuC execution capability.

use alloc::vec::Vec;

pub use v::bp_abi::{
    LUMEN_PHASE_CHECKPOINT_READY, LUMEN_PHASE_CHECKPOINTING, LUMEN_PHASE_ERROR, LUMEN_PHASE_IDLE,
    LUMEN_PHASE_OPENING, LUMEN_PHASE_READY, LUMEN_PHASE_REPLY_READY, LUMEN_PHASE_RESTORE_UPLOAD,
    LUMEN_PHASE_RESTORING, LUMEN_PHASE_RUNNING, TrueosLumenStatus,
};

const TRANSFER_CHUNK: usize = 128 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Error(pub i32);

fn result(code: i32) -> Result<(), Error> {
    if code == 0 { Ok(()) } else { Err(Error(code)) }
}

pub fn open_template(system_prompt: &str) -> Result<(), Error> {
    result(unsafe {
        v::bp_abi::trueos_cabi_lumen_template_open(system_prompt.as_ptr(), system_prompt.len())
    })
}

pub fn submit_prompt(turn: u64, reply_tail: &[u32], prompt: &str) -> Result<(), Error> {
    result(unsafe {
        v::bp_abi::trueos_cabi_lumen_prompt_submit(
            turn,
            reply_tail.as_ptr(),
            reply_tail.len(),
            prompt.as_ptr(),
            prompt.len(),
        )
    })
}

pub fn status() -> Result<TrueosLumenStatus, Error> {
    let mut status = TrueosLumenStatus::default();
    result(unsafe { v::bp_abi::trueos_cabi_lumen_status(&mut status) })?;
    Ok(status)
}

pub fn take_reply(status: TrueosLumenStatus) -> Result<Vec<u8>, Error> {
    let mut reply = Vec::new();
    reply
        .try_reserve_exact(status.reply_len as usize)
        .map_err(|_| Error(-5))?;
    reply.resize(status.reply_len as usize, 0);
    let read = unsafe { v::bp_abi::trueos_cabi_lumen_reply_read(reply.as_mut_ptr(), reply.len()) };
    if read < 0 {
        return Err(Error(read as i32));
    }
    reply.truncate(read as usize);
    Ok(reply)
}

pub fn request_checkpoint() -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_lumen_checkpoint_request() })
}

pub fn read_checkpoint(status: TrueosLumenStatus) -> Result<Vec<u8>, Error> {
    let total = usize::try_from(status.checkpoint_len).map_err(|_| Error(-3))?;
    let mut image = Vec::new();
    image.try_reserve_exact(total).map_err(|_| Error(-5))?;
    image.resize(total, 0);
    let mut offset = 0usize;
    while offset < total {
        let end = core::cmp::min(offset.saturating_add(TRANSFER_CHUNK), total);
        let read = unsafe {
            v::bp_abi::trueos_cabi_lumen_checkpoint_read(
                offset,
                image[offset..end].as_mut_ptr(),
                end - offset,
            )
        };
        if read <= 0 || read as usize > end - offset {
            return Err(Error(read as i32));
        }
        offset += read as usize;
    }
    Ok(image)
}

pub fn restore(image: &[u8]) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_lumen_restore_begin(image.len()) })?;
    let mut offset = 0usize;
    while offset < image.len() {
        let end = core::cmp::min(offset.saturating_add(TRANSFER_CHUNK), image.len());
        result(unsafe {
            v::bp_abi::trueos_cabi_lumen_restore_write(
                offset,
                image[offset..end].as_ptr(),
                end - offset,
            )
        })?;
        offset = end;
    }
    result(unsafe { v::bp_abi::trueos_cabi_lumen_restore_commit() })
}

pub fn close() -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_lumen_close() })
}

pub fn play_emotion(idea: &str) -> Result<(), Error> {
    result(unsafe { v::bp_abi::trueos_cabi_spirit_emotion_play(idea.as_ptr(), idea.len()) })
}

pub fn present_reply(turn: u64, text: &str) -> Result<(), Error> {
    result(unsafe {
        v::bp_abi::trueos_cabi_spirit_response_present(turn, text.as_ptr(), text.len())
    })
}
