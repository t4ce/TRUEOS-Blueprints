extern crate alloc;

use alloc::string::String;

use crate::vsys;

#[inline]
pub fn uart1_shell_write(bytes: &[u8]) -> usize {
    vsys::write_stream(1, bytes);
    bytes.len()
}

#[inline]
pub fn shell2_print_line(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    vsys::write_stream(1, bytes);
    if !bytes.ends_with(b"\n") {
        vsys::write_stream(1, b"\n");
    }
    bytes.len()
}

#[inline]
pub fn shell2_print_targeted_line(_target_mask: u32, bytes: &[u8]) -> usize {
    shell2_print_line(bytes)
}

#[inline]
pub fn shell1_submit_input(_bytes: &[u8]) -> usize {
    0
}

#[inline]
pub fn shell_command_registry_json() -> Option<String> {
    Some(String::from("[]"))
}

#[inline]
pub fn shell1_history_total_lines() -> usize {
    0
}

#[inline]
pub fn shell1_history_text_since(_start_line: usize, _max_lines: usize) -> Option<String> {
    None
}

#[inline]
pub fn shell_qjs_init() {}

#[inline]
pub fn shell_qjs_write(bytes: &[u8]) -> usize {
    bytes.len()
}

#[inline]
pub fn shell_qjs_write_byte(_byte: u8) -> bool {
    false
}

#[inline]
pub fn shell_qjs_read(_out: &mut [u8]) -> usize {
    0
}

#[inline]
pub fn shell_qjs_read_byte() -> Option<u8> {
    None
}