//! TRUEOS host backend for the medium Weave Windows CLI specimen.
//!
//! This remains a deliberately bounded vertical slice: one relocation-free
//! PE32+ console image, one kernel32 import descriptor, and fourteen shims.

use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use trueos::vsys;

const IMAGE_CAP: usize = 0x8000;
const PE32_PLUS_MAGIC: u16 = 0x20b;
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;
const IMAGE_ORDINAL_FLAG64: u64 = 1 << 63;
const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
const STDOUT_HANDLE: usize = 1;
const STDERR_HANDLE: usize = 2;
const IMPORT_COUNT: u32 = 14;
const COMPLETE_IMPORT_MASK: u32 = (1 << IMPORT_COUNT) - 1;
const COMMAND_LINE: &[u8] = b"hello_medium.exe --trueos-proof\0";
const MODULE_PATH: &[u8] = b"C:\\TRUEOS\\hello_medium.exe\0";
const ENVIRONMENT_NAME: &[u8] = b"WEAVE_MODE";
const ENVIRONMENT_VALUE: &[u8] = b"trueos\0";

static EXIT_CODE: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

fn important_stage(message: &str) {
    let _ = vsys::log_record(2, "weave-boot-probe", message);
}

// Boot-isolation policy: keep the kernel32 surface contract-shaped, but do not
// let this diagnostic specimen reach the ordinary TRUEOS console, clock, or
// thread helpers. The IMPORTANT receipts are intentional TODO markers. Restore
// one real helper at a time after the Blueprint/Win64 boundary is proven not to
// disturb unrelated kernel state.

#[repr(C, align(4096))]
struct ExecutablePeImage([u8; IMAGE_CAP]);

// The arena lives in the executable Blueprint REL allocation. TRUEOS grants
// execute permission only for the duration of the Blueprint entry call.
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
    important_stage("IMPORTANT stage=loader.validate action=begin");
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

    let data_directories = optional + 112;
    let relocation_rva = read_u32(file, data_directories + 5 * 8)?;
    let relocation_size = read_u32(file, data_directories + 5 * 8 + 4)?;
    if relocation_rva != 0 || relocation_size != 0 {
        return Err(Error::RelocationsUnsupported);
    }
    important_stage("IMPORTANT stage=loader.validate action=complete");

    important_stage("IMPORTANT stage=loader.map action=begin");
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
    important_stage("IMPORTANT stage=loader.map action=complete sections=3");

    important_stage("IMPORTANT stage=loader.bind action=begin dll=kernel32.dll");
    bind_imports(image, data_directories)?;
    important_stage("IMPORTANT stage=loader.bind action=complete imports=14");

    if entry_rva >= size_of_image {
        return Err(Error::EntryOutsideImage);
    }
    let entry = image.as_ptr().wrapping_add(entry_rva);
    EXIT_CODE.store(u32::MAX, Ordering::Release);
    LAST_ERROR.store(0, Ordering::Release);
    important_stage("IMPORTANT stage=loader.enter action=begin abi=win64");
    let entry_fn: extern "win64" fn() = unsafe { core::mem::transmute(entry) };
    entry_fn();

    let exit_code = EXIT_CODE.load(Ordering::Acquire);
    if exit_code == u32::MAX {
        Err(Error::ExitProcessNotCalled)
    } else {
        important_stage("IMPORTANT stage=loader.enter action=return exit_process=observed");
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
    let mut resolved = 0u32;
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

    if resolved != COMPLETE_IMPORT_MASK {
        return Err(Error::IncompleteContract);
    }
    Ok(())
}

fn resolve_kernel32(name: &[u8]) -> Result<(usize, u32), Error> {
    let resolved = match name {
        b"ExitProcess" => (exit_process as *const () as usize, 1 << 0),
        b"GetCommandLineA" => (get_command_line_a as *const () as usize, 1 << 1),
        b"GetCurrentProcessId" => (get_current_process_id as *const () as usize, 1 << 2),
        b"GetCurrentThreadId" => (get_current_thread_id as *const () as usize, 1 << 3),
        b"GetEnvironmentVariableA" => (get_environment_variable_a as *const () as usize, 1 << 4),
        b"GetLastError" => (get_last_error as *const () as usize, 1 << 5),
        b"GetModuleFileNameA" => (get_module_file_name_a as *const () as usize, 1 << 6),
        b"GetStdHandle" => (get_std_handle as *const () as usize, 1 << 7),
        b"GetTickCount64" => (get_tick_count64 as *const () as usize, 1 << 8),
        b"InterlockedIncrement" => (interlocked_increment as *const () as usize, 1 << 9),
        b"QueryPerformanceCounter" => (query_performance_counter as *const () as usize, 1 << 10),
        b"QueryPerformanceFrequency" => {
            (query_performance_frequency as *const () as usize, 1 << 11)
        }
        b"SetLastError" => (set_last_error as *const () as usize, 1 << 12),
        b"WriteFile" => (write_file as *const () as usize, 1 << 13),
        _ => return Err(Error::UnsupportedImport),
    };
    Ok(resolved)
}

unsafe extern "win64" fn get_std_handle(std_handle: u32) -> usize {
    important_stage("IMPORTANT stage=kernel32.GetStdHandle action=noop-contract");
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
    important_stage("IMPORTANT stage=kernel32.WriteFile action=noop-contract");
    if !bytes_written.is_null() {
        unsafe { bytes_written.write(0) };
    }
    if buffer.is_null() || !overlapped.is_null() {
        return 0;
    }
    match handle {
        STDOUT_HANDLE | STDERR_HANDLE => {}
        _ => return 0,
    }
    // Deliberately do not dereference or route the guest buffer during this
    // boot diagnostic. A successful receipt preserves the Win32 probe's
    // control flow while removing console IO as a possible corruption source.
    if !bytes_written.is_null() {
        unsafe { bytes_written.write(bytes_to_write) };
    }
    1
}

unsafe extern "win64" fn get_current_process_id() -> u32 {
    important_stage("IMPORTANT stage=kernel32.GetCurrentProcessId action=noop-contract");
    1
}

unsafe extern "win64" fn get_current_thread_id() -> u32 {
    important_stage("IMPORTANT stage=kernel32.GetCurrentThreadId action=noop-contract");
    1
}

unsafe extern "win64" fn get_tick_count64() -> u64 {
    important_stage("IMPORTANT stage=kernel32.GetTickCount64 action=noop-contract");
    1
}

unsafe extern "win64" fn query_performance_counter(value: *mut i64) -> i32 {
    important_stage("IMPORTANT stage=kernel32.QueryPerformanceCounter action=noop-contract");
    if value.is_null() {
        return 0;
    }
    unsafe { value.write(1) };
    1
}

unsafe extern "win64" fn query_performance_frequency(value: *mut i64) -> i32 {
    important_stage("IMPORTANT stage=kernel32.QueryPerformanceFrequency action=noop-contract");
    if value.is_null() {
        return 0;
    }
    unsafe { value.write(1_000_000_000) };
    1
}

unsafe extern "win64" fn get_command_line_a() -> *mut u8 {
    important_stage("IMPORTANT stage=kernel32.GetCommandLineA action=noop-contract");
    COMMAND_LINE.as_ptr() as *mut u8
}

unsafe extern "win64" fn get_environment_variable_a(
    name: *const u8,
    buffer: *mut u8,
    size: u32,
) -> u32 {
    important_stage("IMPORTANT stage=kernel32.GetEnvironmentVariableA action=noop-contract");
    if name.is_null() || !unsafe { c_ptr_eq(name, ENVIRONMENT_NAME) } {
        LAST_ERROR.store(203, Ordering::Release); // ERROR_ENVVAR_NOT_FOUND
        return 0;
    }
    unsafe { copy_windows_string(ENVIRONMENT_VALUE, buffer, size) }
}

unsafe extern "win64" fn get_module_file_name_a(module: usize, buffer: *mut u8, size: u32) -> u32 {
    important_stage("IMPORTANT stage=kernel32.GetModuleFileNameA action=noop-contract");
    if module != 0 {
        LAST_ERROR.store(126, Ordering::Release); // ERROR_MOD_NOT_FOUND
        return 0;
    }
    unsafe { copy_windows_string(MODULE_PATH, buffer, size) }
}

unsafe extern "win64" fn set_last_error(error: u32) {
    important_stage("IMPORTANT stage=kernel32.SetLastError action=noop-contract");
    LAST_ERROR.store(error, Ordering::Release);
}

unsafe extern "win64" fn get_last_error() -> u32 {
    important_stage("IMPORTANT stage=kernel32.GetLastError action=noop-contract");
    LAST_ERROR.load(Ordering::Acquire)
}

unsafe extern "win64" fn interlocked_increment(value: *mut i32) -> i32 {
    important_stage("IMPORTANT stage=kernel32.InterlockedIncrement action=noop-contract");
    if value.is_null() {
        return 0;
    }
    let atomic = unsafe { AtomicI32::from_ptr(value) };
    atomic.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "win64" fn exit_process(exit_code: u32) {
    important_stage("IMPORTANT stage=kernel32.ExitProcess action=noop-contract");
    EXIT_CODE.store(exit_code, Ordering::Release);
}

unsafe fn c_ptr_eq(mut input: *const u8, expected: &[u8]) -> bool {
    for expected_byte in expected {
        if unsafe { input.read() } != *expected_byte {
            return false;
        }
        input = unsafe { input.add(1) };
    }
    (unsafe { input.read() }) == 0
}

unsafe fn copy_windows_string(source: &[u8], destination: *mut u8, capacity: u32) -> u32 {
    let payload_len = source.len().saturating_sub(1);
    if destination.is_null() || capacity as usize <= payload_len {
        return source.len() as u32;
    }
    unsafe { ptr::copy_nonoverlapping(source.as_ptr(), destination, source.len()) };
    payload_len as u32
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, Error> {
    let raw: [u8; 2] = range(input, offset, 2)?
        .try_into()
        .map_err(|_| Error::Truncated)?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, Error> {
    let raw: [u8; 4] = range(input, offset, 4)?
        .try_into()
        .map_err(|_| Error::Truncated)?;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(input: &[u8], offset: usize) -> Result<u64, Error> {
    let raw: [u8; 8] = range(input, offset, 8)?
        .try_into()
        .map_err(|_| Error::Truncated)?;
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
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::Truncated)?;
    Ok(&tail[..end])
}

fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}
