//! Small, locale-independent ANSI string primitives shared by the CRT shim.

pub const MAX_C_STRING: usize = 1_048_576;

pub const fn fold_ascii(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() { byte + 0x20 } else { byte }
}

pub fn compare(left: &[u8], right: &[u8], limit: Option<usize>, insensitive: bool) -> i32 {
    compare_with(left, right, limit, |byte| if insensitive { fold_ascii(byte) } else { byte })
}

pub fn compare_with(
    left: &[u8],
    right: &[u8],
    limit: Option<usize>,
    fold: impl Fn(u8) -> u8,
) -> i32 {
    let count = limit.unwrap_or(usize::MAX);
    for index in 0..count {
        let left_byte = *left.get(index).unwrap_or(&0);
        let right_byte = *right.get(index).unwrap_or(&0);
        let left_byte = fold(left_byte);
        let right_byte = fold(right_byte);
        if left_byte != right_byte || left_byte == 0 {
            return i32::from(left_byte) - i32::from(right_byte);
        }
    }
    0
}

pub fn pbrk(haystack: &[u8], accept: &[u8]) -> Option<usize> {
    haystack.iter().position(|byte| *byte != 0 && accept.contains(byte))
}

pub fn rchr(bytes: &[u8], needle: u8) -> Option<usize> {
    bytes.iter().take_while(|byte| **byte != 0).position(|byte| *byte == needle).and_then(|first| {
        bytes.iter().take_while(|byte| **byte != 0).enumerate().filter_map(|(index, byte)| (*byte == needle).then_some(index)).last().or(Some(first))
    }).or_else(|| (needle == 0).then_some(bytes.iter().position(|byte| *byte == 0)?))
}

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let haystack = haystack.split(|byte| *byte == 0).next().unwrap_or_default();
    let needle = needle.split(|byte| *byte == 0).next().unwrap_or_default();
    haystack.windows(needle.len()).position(|candidate| candidate == needle)
        .or_else(|| needle.is_empty().then_some(0))
}

pub fn lower_in_place(bytes: &mut [u8]) {
    for byte in bytes.iter_mut().take_while(|byte| **byte != 0) {
        *byte = fold_ascii(*byte);
    }
}

pub fn upper_in_place(bytes: &mut [u8]) {
    for byte in bytes.iter_mut().take_while(|byte| **byte != 0) {
        if byte.is_ascii_lowercase() {
            *byte -= 0x20;
        }
    }
}
