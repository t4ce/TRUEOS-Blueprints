// Keep hf-hub's nonblocking exclusive lock and integer result contract while
// using the native file identity and lock implementation supplied by std.
pub(crate) fn lock(file: &std::fs::File) -> i32 {
    if file.try_lock().is_ok() { 0 } else { -1 }
}

pub(crate) fn unlock(file: &std::fs::File) -> i32 {
    if file.unlock().is_ok() { 0 } else { -1 }
}
