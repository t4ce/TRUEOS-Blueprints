//! Shared visual contract for Shell3 companion TUIs.
//!
//! Rows 0–2 are app-owned controls on the Shell3 control background.
//! Content starts at row 3 and uses the terminal’s default background.
//! Paint/erase each complete control row with its background selected; keep
//! selection and foreground changes from resetting that background. Restore
//! terminal defaults before returning the lease. Text and controls remain
//! app-specific; this contract does not reserve additional rows.

/// Number of control rows before the content area (zero-based row 3).
pub const CONTROL_ROWS: u16 = 3;
/// Shell3’s opaque control-strip RGB color.
pub const CONTROL_BACKGROUND_RGB: [u8; 3] = [16, 16, 16];
