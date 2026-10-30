//! UI4 interactions for the caller's currently leased local terminal.
//!
//! Works with forwarded VT output, including Crossterm. Claim the terminal
//! first (for example by enabling raw mode). Register again after terminal
//! reentry or attachment changes. Remote-only terminals return an error.
//! UI4 owns the collapse menu, compact icon, dragging, and click-to-restore.

/// Register tightly packed straight-alpha RGBA8 (1..=256 pixels per axis).
/// The kernel copies the bytes; the caller may immediately release them.
pub fn enable_collapse(width: u32, height: u32, rgba: &[u8]) -> Result<(), i32> {
    if !(1..=256).contains(&width)
        || !(1..=256).contains(&height)
        || rgba.len() != width as usize * height as usize * 4
    {
        return Err(-1);
    }
    let rc = unsafe {
        v::bp_abi::trueos_cabi_ui4_scene_enable_collapse_v1(
            0,
            width,
            height,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc == 0 { Ok(()) } else { Err(rc) }
}

/// Collapse the registered terminal frame, retaining application state.
/// Repeated calls leave an already collapsed frame collapsed.
pub fn collapse() -> Result<(), i32> {
    let rc = unsafe { v::bp_abi::trueos_cabi_ui4_scene_collapse_v1(0) };
    if rc == 0 { Ok(()) } else { Err(rc) }
}
