//! Minimal TRUEOS host backend for the first Weave Windows CLI specimen.
//!
//! This is intentionally a vertical slice, not a general PE loader. It accepts
//! a PE32+ x86-64 console image with no base relocations and binds exactly the
//! three kernel32 calls used by `weave-cli-hello.exe`.

use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use trueos::vsys;

const IMAGE_CAP: usize = 0x4000;
const PE32_PLUS_MAGIC: u16 = 0x20b;
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;
const IMAGE_ORDINAL_FLAG64: u64 = 1 << 63;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
const STDOUT_HANDLE: usize = 1;
const STDERR_HANDLE: usize = 2;

static EXIT_CODE: AtomicU32 = AtomicU32::new(u32::MAX);

#[repr(C, align(4096))]
struct ExecutablePeImage([u8; IMAGE_CAP]);

// This arena is part of the Blueprint REL allocation. TRUEOS's trusted
// Blueprint loader makes that allocation executable only for the duration of
// the Blueprint entry call, which lets the mapped PE execute without granting
// executable permission to an unrelated heap allocation.
static mut PE_IMAGE: ExecutablePeImage = ExecutablePeImage([0; IMAGE_CAP]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Truncated,
    BadDosMagic,
    BadPeSignature,
    UnsupportedMachine,
    UnsupportedOptionalHeader,
    NotConsole,
    ImageTooLarge,
    SectionOutsideFile,
    SectionOutsideImage,
    RelocationsUnsupported,
    MissingImports,
    UnsupportedDll,
    OrdinalImportUnsupported,
    UnsupportedImport,
    DuplicateImport,
    IncompleteContract,
    EntryOutsideImage,
    ExitProcessNotCalled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub fn run(file: &[u8]) -> Result<u32, Error> {
    let pe_offset = read_u32(file, 0x3c)? as usize;
    if read_u16(file, 0)? != 0x5a4d {
        return Err(Error::BadDosMagic);
    }
    if range(file, pe_offset, 4)? != b"PE\0\0" {
        return Err(Error::BadPeSignature);
    }

    let coff = pe_offset.checked_add(4).ok_or(Error::Truncated)?;
    if read_u16(file, coff)? != IMAGE_FILE_MACHINE_AMD64 {
        return Err(Error::UnsupportedMachine);
    }
    let section_count = read_u16(file, coff + 2)? as usize;
    let optional_size = read_u16(file, coff + 16)? as usize;
    let optional = coff.checked_add(20).ok_or(Error::Truncated)?;
    if read_u16(file, optional)? != PE32_PLUS_MAGIC || optional_size < 128 {
        return Err(Error::UnsupportedOptionalHeader);
    }
    if read_u16(file, optional + 68)? != IMAGE_SUBSYSTEM_WINDOWS_CUI {
        return Err(Error::NotConsole);
    }

    let entry_rva = read_u32(file, optional + 16)? as usize;
    let size_of_image = read_u32(file, optional + 56)? as usize;
    let size_of_headers = read_u32(file, optional + 60)? as usize;
    if size_of_image == 0 || size_of_image > IMAGE_CAP {
        return Err(Error::ImageTooLarge);
    }

    // Base relocations are intentionally outside this first slice. The tiny
    // specimen is linked entirely RIP-relative and has an empty relocation
    // directory, so it can execute from this Blueprint-owned arena.
    let data_directories = optional + 112;
    let relocation_rva = read_u32(file, data_directories + 5 * 8)?;
    let relocation_size = read_u32(file, data_directories + 5 * 8 + 4)?;
    if relocation_rva != 0 || relocation_size != 0 {
        return Err(Error::RelocationsUnsupported);
    }

    let image = unsafe { &mut *ptr::addr_of_mut!(PE_IMAGE.0) };
    image.fill(0);
    let header_bytes = size_of_headers.min(file.len());
    image
        .get_mut(..header_bytes)
        .ok_or(Error::ImageTooLarge)?
        .copy_from_slice(&file[..header_bytes]);

    let section_table = optional
        .checked_add(optional_size)
        .ok_or(Error::Truncated)?;
    for index in 0..section_count {
        let section = section_table
            .checked_add(index.checked_mul(40).ok_or(Error::Truncated)?)
            .ok_or(Error::Truncated)?;
        range(file, section, 40)?;
        let virtual_address = read_u32(file, section + 12)? as usize;
        let raw_size = read_u32(file, section + 16)? as usize;
        let raw_offset = read_u32(file, section + 20)? as usize;
        let source = range(file, raw_offset, raw_size).map_err(|_| Error::SectionOutsideFile)?;
        let destination = image
            .get_mut(
                virtual_address
                    ..virtual_address
                        .checked_add(raw_size)
                        .ok_or(Error::SectionOutsideImage)?,
            )
            .ok_or(Error::SectionOutsideImage)?;
        destination.copy_from_slice(source);
    }

    bind_imports(image, data_directories)?;
    let entry = image
        .as_ptr()
        .wrapping_add(entry_rva);
    if entry_rva >= size_of_image {
        return Err(Error::EntryOutsideImage);
    }

    EXIT_CODE.store(u32::MAX, Ordering::Release);
    let entry_fn: extern "win64" fn() = unsafe { core::mem::transmute(entry) };
    entry_fn();
    let exit_code = EXIT_CODE.load(Ordering::Acquire);
    if exit_code == u32::MAX {
        Err(Error::ExitProcessNotCalled)
    } else {
        Ok(exit_code)
    }
}

fn bind_imports(image: &mut [u8; IMAGE_CAP], directories: usize) -> Result<(), Error> {
    let import_rva = read_u32(image, directories + 8)? as usize;
    let import_size = read_u32(image, directories + 12)? as usize;
    if import_rva == 0 || import_size < 20 {
        return Err(Error::MissingImports);
    }
    let import_end = import_rva
        .checked_add(import_size)
        .filter(|end| *end <= image.len())
        .ok_or(Error::MissingImports)?;

    let mut descriptor = import_rva;
    let mut resolved = 0u8;
    while descriptor + 20 <= import_end {
        let original_thunk = read_u32(image, descriptor)? as usize;
        let name_rva = read_u32(image, descriptor + 12)? as usize;
        let first_thunk = read_u32(image, descriptor + 16)? as usize;
        if original_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        if !ascii_eq_ignore_case(c_string(image, name_rva)?, b"kernel32.dll") {
            return Err(Error::UnsupportedDll);
        }

        let lookup_base = if original_thunk == 0 {
            first_thunk
        } else {
            original_thunk
        };
        let mut index = 0usize;
        loop {
            let lookup = lookup_base
                .checked_add(index.checked_mul(8).ok_or(Error::Truncated)?)
                .ok_or(Error::Truncated)?;
            let iat = first_thunk
                .checked_add(index.checked_mul(8).ok_or(Error::Truncated)?)
                .ok_or(Error::Truncated)?;
            let thunk = read_u64(image, lookup)?;
            if thunk == 0 {
                break;
            }
            if thunk & IMAGE_ORDINAL_FLAG64 != 0 {
                return Err(Error::OrdinalImportUnsupported);
            }
            let name = c_string(image, thunk as usize + 2)?;
            let (address, bit) = resolve_kernel32(name)?;
            if resolved & bit != 0 {
                return Err(Error::DuplicateImport);
            }
            resolved |= bit;
            write_u64(image, iat, address as u64)?;
            index += 1;
        }
        descriptor += 20;
    }

    if resolved != 0b111 {
        return Err(Error::IncompleteContract);
    }
    Ok(())
}

fn resolve_kernel32(name: &[u8]) -> Result<(usize, u8), Error> {
    match name {
        b"GetStdHandle" => Ok((get_std_handle as *const () as usize, 0b001)),
        b"WriteFile" => Ok((write_file as *const () as usize, 0b010)),
        b"ExitProcess" => Ok((exit_process as *const () as usize, 0b100)),
        _ => Err(Error::UnsupportedImport),
    }
}

unsafe extern "win64" fn get_std_handle(std_handle: u32) -> usize {
    match std_handle {
        STD_ERROR_HANDLE => STDERR_HANDLE,
        STD_OUTPUT_HANDLE => STDOUT_HANDLE,
        _ => 0,
    }
}

unsafe extern "win64" fn write_file(
    handle: usize,
    buffer: *const u8,
    bytes_to_write: u32,
    bytes_written: *mut u32,
    overlapped: *mut u8,
) -> i32 {
    if !bytes_written.is_null() {
        unsafe { bytes_written.write(0) };
    }
    if buffer.is_null() || !overlapped.is_null() {
        return 0;
    }
    let stream = match handle {
        STDOUT_HANDLE => 1,
        STDERR_HANDLE => 2,
        _ => return 0,
    };
    let bytes = unsafe { core::slice::from_raw_parts(buffer, bytes_to_write as usize) };
    vsys::write_stream(stream, bytes);
    if !bytes_written.is_null() {
        unsafe { bytes_written.write(bytes_to_write) };
    }
    1
}

unsafe extern "win64" fn exit_process(exit_code: u32) {
    EXIT_CODE.store(exit_code, Ordering::Release);
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, Error> {
    let raw: [u8; 2] = range(input, offset, 2)?.try_into().map_err(|_| Error::Truncated)?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, Error> {
    let raw: [u8; 4] = range(input, offset, 4)?.try_into().map_err(|_| Error::Truncated)?;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(input: &[u8], offset: usize) -> Result<u64, Error> {
    let raw: [u8; 8] = range(input, offset, 8)?.try_into().map_err(|_| Error::Truncated)?;
    Ok(u64::from_le_bytes(raw))
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) -> Result<(), Error> {
    bytes
        .get_mut(offset..offset.checked_add(8).ok_or(Error::Truncated)?)
        .ok_or(Error::Truncated)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn range(input: &[u8], offset: usize, len: usize) -> Result<&[u8], Error> {
    input
        .get(offset..offset.checked_add(len).ok_or(Error::Truncated)?)
        .ok_or(Error::Truncated)
}

fn c_string(bytes: &[u8], offset: usize) -> Result<&[u8], Error> {
    let tail = bytes.get(offset..).ok_or(Error::Truncated)?;
    let end = tail.iter().position(|byte| *byte == 0).ok_or(Error::Truncated)?;
    Ok(&tail[..end])
}

fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}
