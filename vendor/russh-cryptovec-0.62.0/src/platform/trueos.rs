use std::convert::Infallible;

// Blueprint address spaces are backed by resident guest pages and TRUEOS has
// no swap-backed pager.  There is therefore nothing analogous to Unix mlock
// to request: these allocations cannot be paged to disk.
pub fn mlock(_ptr: *const u8, _len: usize) -> Result<(), Infallible> {
    Ok(())
}

pub fn munlock(_ptr: *const u8, _len: usize) -> Result<(), Infallible> {
    Ok(())
}
