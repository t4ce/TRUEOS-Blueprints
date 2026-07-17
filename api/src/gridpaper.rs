//! Dedicated producer side of the GridPaper kernel snapshot transport.
//!
//! This intentionally exposes no generic UI or drawing calls. The Blueprint
//! publishes its fixed wire image; the kernel service owns presentation and
//! GPU residency.

pub const COLUMNS: usize = 21;
pub const ROWS: usize = 30;
pub const CELL_BYTES: usize = 20;
pub const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidSnapshot,
    InvalidScale,
    NotOwner,
    Transport,
    Unknown(i32),
}

/// Copy one immutable GridPaper generation into the kernel-owned back buffer.
///
/// Returning means the kernel has accepted its own stable copy. Rendering is
/// asynchronous and never runs in the Blueprint's call path.
pub fn submit_snapshot(
    generation: u64,
    scale_percent: u16,
    raw: &[u8; PAGE_BYTES],
) -> Result<(), Error> {
    status(unsafe {
        v::bp_abi::trueos_cabi_gridpaper_snapshot_submit(
            generation,
            u32::from(scale_percent),
            raw.as_ptr(),
            raw.len(),
        )
    })
}

/// Detach this Blueprint producer. The UI retains its last published frame.
pub fn close() -> Result<(), Error> {
    status(unsafe { v::bp_abi::trueos_cabi_gridpaper_close() })
}

fn status(code: i32) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        -1 => Err(Error::InvalidSnapshot),
        -2 => Err(Error::InvalidScale),
        -3 => Err(Error::NotOwner),
        -4 => Err(Error::Transport),
        other => Err(Error::Unknown(other)),
    }
}

