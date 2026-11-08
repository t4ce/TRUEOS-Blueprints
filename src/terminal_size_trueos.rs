//! Geometry of the actual attached native terminal, without claiming its lease.
use super::{Height, Width};
use std::io::IsTerminal;

unsafe extern "C" {
    fn trueos_cabi_konsole_size(columns: *mut u32, rows: *mut u32) -> i32;
}

pub fn terminal_size() -> Option<(Width, Height)> {
    // Keep the upstream stdout/stderr/stdin search semantics. Native std tests
    // each stream's live route; a pipe does not become a terminal descriptor.
    if !std::io::stdout().is_terminal()
        && !std::io::stderr().is_terminal()
        && !std::io::stdin().is_terminal()
    {
        return None;
    }
    let (mut columns, mut rows) = (0, 0);
    let status = unsafe { trueos_cabi_konsole_size(&mut columns, &mut rows) };
    if status != 0 || columns == 0 || rows == 0 {
        return None;
    }
    Some((
        Width(columns.min(u32::from(u16::MAX)) as u16),
        Height(rows.min(u32::from(u16::MAX)) as u16),
    ))
}
