//! Read a named, kernel-owned image source through the Blueprint ABI.
//!
//! Source bytes are copied in bounded chunks. A Blueprint never receives a
//! kernel pointer or a lease to a kernel-owned RGBA allocation.

use alloc::vec::Vec;

pub const FORMAT_JPEG: u32 = 1;
pub const FORMAT_RGBA8: u32 = 2;
pub const FORMAT_PNG: u32 = 3;
const READ_CHUNK_BYTES: usize = 16 * 1024;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Info {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub byte_len: u32,
}

pub fn read(name: &str) -> Result<(Info, Vec<u8>), i32> {
    if name.is_empty() {
        return Err(-3);
    }

    let mut raw_info = v::bp_abi::TrueosImageSourceInfo {
        format: 0,
        width: 0,
        height: 0,
        byte_len: 0,
    };
    let status = unsafe {
        v::bp_abi::trueos_cabi_image_source_info(name.as_ptr(), name.len(), &mut raw_info)
    };
    if status != 0 {
        return Err(status);
    }
    let info = Info {
        format: raw_info.format,
        width: raw_info.width,
        height: raw_info.height,
        byte_len: raw_info.byte_len,
    };
    let byte_len = info.byte_len as usize;
    if byte_len == 0 || byte_len > MAX_SOURCE_BYTES {
        return Err(-3);
    }

    let mut bytes = alloc::vec![0; byte_len];
    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(READ_CHUNK_BYTES), bytes.len());
        let copied = unsafe {
            v::bp_abi::trueos_cabi_image_source_read(
                name.as_ptr(),
                name.len(),
                offset,
                bytes[offset..end].as_mut_ptr(),
                end - offset,
            )
        };
        if copied <= 0 {
            return Err(if copied < 0 { copied as i32 } else { -7 });
        }
        let copied = copied as usize;
        if copied > end - offset {
            return Err(-7);
        }
        offset += copied;
    }
    Ok((info, bytes))
}
