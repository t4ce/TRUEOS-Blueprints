//! Portable Windows XP semantics for the launcher.
//!
//! This module deliberately knows nothing about VMX, VM ids, physical memory,
//! or carrier selection. It consumes an x86 trap frame plus generic guest
//! memory and returns the register value with which execution should resume.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[cfg(target_os = "trueos")]
use trueos::clock;
use trueos::clock::UtcDateTime;
use trueos::logl::{self, level};
use trueos::vgpu::{Capabilities, Device, Queue, QueueClass};

use crate::{
    ThisToThat::{decode_cp1252, encode_cp1252},
    child_loader::{ChildProvider, ProviderImport, ProviderOp, ProviderSymbol, provider_op},
    imports::{LauncherImport, WinCall},
    pe32,
    session::{
        CreateEventRequest, CreateMutexRequest, CreateProcessRequest, CreateWindowRequest,
        DuplicateHandleRequest,
        GetExitCodeProcessRequest, LoadImageRequest, OpenFileRequest, PersonalityAction, SessionRequest,
        SetWindowPosRequest, ThreadKey, WaitRequest, WindowBlitRequest, WindowFillRectRequest, WindowTextRequest,
    },
    staticstr,
    thunk32,
};

#[cfg(test)]
use crate::child_loader::provider_thunk_kind;

pub const ENTRY_VA: u32 = pe32::IMAGE_BASE + pe32::ENTRY_RVA;
pub const TEB_VA: u32 = 0x0020_1000;
pub const HEAP_VA: u32 = 0x0021_0000;
pub const CHILD_CRT_HEAP_BASE: u32 = 0x0100_0000;
pub const CHILD_CRT_HEAP_LIMIT: u32 = 0x0400_0000;
pub const XP_ALLOCATION_GRANULARITY: u32 = 0x0001_0000;
pub const XP_PAGE_SIZE: u32 = 0x1000;
pub const CHILD_VIRTUAL_ALLOC_BASE: u32 = 0x0600_0000;
pub const CHILD_VIRTUAL_ALLOC_LIMIT: u32 = 0x1400_0000;
pub const CHILD_WIN_HEAP_BASE: u32 = 0x1400_0000;
pub const CHILD_WIN_HEAP_LIMIT: u32 = 0x1500_0000;
pub const PROVIDER_MODULE_HANDLE_BASE: u32 = 0x5743_a001;
pub const CURRENT_PROCESS_PSEUDO_HANDLE: u32 = u32::MAX;
pub const CURRENT_THREAD_PSEUDO_HANDLE: u32 = 0xffff_fffe;
pub const D3D_OK: u32 = 0;
pub const D3DERR_INVALIDCALL: u32 = 0x8876_086c;
pub const D3DENUM_NO_WHQL_LEVEL: u32 = 0x0000_0002;
pub const D3DADAPTER_IDENTIFIER8_BYTES: usize = 0x42c;
pub const TRUEOS_D3D8_VENDOR_ID: u32 = 0x8086;
pub const TRUEOS_D3D8_DEVICE_ID: u32 = 0xa780;
pub const TRUEOS_DISPLAY_ADAPTER_DESCRIPTION: &str = "Intel(R) UHD Graphics 770";
pub const ENUM_CURRENT_SETTINGS: u32 = 0xffff_ffff;
pub const ENUM_REGISTRY_SETTINGS: u32 = 0xffff_fffe;
pub const DM_BITSPERPEL: u32 = 0x0004_0000;
pub const DM_PELSWIDTH: u32 = 0x0008_0000;
pub const DM_PELSHEIGHT: u32 = 0x0010_0000;
pub const DM_DISPLAYFLAGS: u32 = 0x0020_0000;
pub const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
pub const TRUEOS_DISPLAY_FIELDS: u32 = DM_BITSPERPEL
    | DM_PELSWIDTH
    | DM_PELSHEIGHT
    | DM_DISPLAYFLAGS
    | DM_DISPLAYFREQUENCY;
pub const XP_CXICON: u32 = 32;
pub const XP_CYICON: u32 = 32;
pub const XP_CXCURSOR: u32 = 32;
pub const XP_CYCURSOR: u32 = 32;
const USER_IMAGE_HANDLE_BASE: u32 = 0x5743_d001;
const HGLRC_HANDLE_BASE: u32 = 0x5743_e001;
pub const TRUEOS_D3D8_SUBSYSTEM_ID: u32 = 0;
pub const TRUEOS_D3D8_REVISION: u32 = 0x04;
pub const TRUEOS_D3D8_ADAPTER_GUID: [u8; 16] = [
    b'T', b'R', b'U', b'E', b'O', b'S', b'D', b'3', b'D', b'8', 0x80, 0xa7, 0x86, 0x80, 0x04, 0,
];
// SetUnhandledExceptionFilter callbacks return EXCEPTION_* filter results,
// distinct from the DISPOSITION_* values used by frame-based SEH handlers.
pub const EXCEPTION_FILTER_CONTINUE_EXECUTION: u32 = u32::MAX;
pub const EXCEPTION_FILTER_CONTINUE_SEARCH: u32 = 0;
pub const EXCEPTION_FILTER_EXECUTE_HANDLER: u32 = 1;
pub const CRT_UNKNOWN_APP: u32 = 0;
pub const CRT_CONSOLE_APP: u32 = 1;
pub const CRT_GUI_APP: u32 = 2;
pub const OSVERSIONINFOA_SIZE: u32 = 0x94;
pub const OSVERSIONINFOEXA_SIZE: u32 = 0x9c;
pub const VER_NT_WORKSTATION: u8 = 1;
pub const DRIVE_NO_ROOT_DIR: u32 = 1;
pub const DRIVE_FIXED: u32 = 3;
pub const FILE_CASE_PRESERVED_NAMES: u32 = 0x0000_0002;
pub const XP_C_VOLUME_SERIAL: u32 = 0x5743_C001;
pub const XP_C_MAX_COMPONENT: u32 = 255;
pub const XP_C_FS_FLAGS: u32 = FILE_CASE_PRESERVED_NAMES;
pub const XP_C_VOLUME_NAME: &str = "";
pub const XP_C_FILE_SYSTEM_NAME: &str = "TRUEOSFS";
pub const XP_C_DISK_BYTES: u64 = 10 * 1024 * 1024 * 1024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct XpDiskGeometry {
    pub sectors_per_cluster: u32,
    pub bytes_per_sector: u32,
    pub free_clusters: u32,
    pub total_clusters: u32,
}

pub const XP_C_DISK_GEOMETRY: XpDiskGeometry = XpDiskGeometry {
    sectors_per_cluster: 8,
    bytes_per_sector: 512,
    free_clusters: 2_621_440,
    total_clusters: 2_621_440,
};
const ERROR_MOD_NOT_FOUND: u32 = 126;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_FILE_EXISTS: u32 = 80;
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const GMEM_FIXED: u32 = 0x0000;
const GMEM_MOVEABLE: u32 = 0x0002;
const GMEM_ZEROINIT: u32 = 0x0040;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_RESOURCE_TYPE_NOT_FOUND: u32 = 1813;
const ERROR_RESOURCE_LANG_NOT_FOUND: u32 = 1815;
const ERROR_MR_MID_NOT_FOUND: u32 = 317;
const FORMAT_MESSAGE_FROM_HMODULE: u32 = 0x0000_0800;
const RT_MESSAGETABLE: u32 = 11;
const LANG_USER_DEFAULT: u32 = 0x0400;
const LANG_SYSTEM_DEFAULT: u32 = 0x0800;
const XP_USER_DEFAULT_LANGID: u32 = 0x0409;
const XP_SYSTEM_DEFAULT_LANGID: u32 = 0x0409;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageResourceEncoding {
    Ansi,
    Unicode,
}

pub fn format_message_language_resolution(requested: u32) -> (u32, &'static str) {
    match requested {
        LANG_USER_DEFAULT => (XP_USER_DEFAULT_LANGID, "user-default"),
        LANG_SYSTEM_DEFAULT => (XP_SYSTEM_DEFAULT_LANGID, "system-default"),
        other => (other, "explicit"),
    }
}

pub fn xp_drive_type(root: Option<&str>) -> u32 {
    let Some(root) = root else {
        // The modeled current directory is C:\\Warcraft III.
        return DRIVE_FIXED;
    };
    let normalized = root.replace('/', "\\");
    if normalized.eq_ignore_ascii_case("C:\\") {
        DRIVE_FIXED
    } else {
        DRIVE_NO_ROOT_DIR
    }
}

pub fn xp_volume_exists(root: Option<&str>) -> bool {
    match root {
        None => true,
        Some(root) => root.replace('/', "\\").eq_ignore_ascii_case("C:\\"),
    }
}

pub fn xp_disk_geometry(root: Option<&str>) -> Option<XpDiskGeometry> {
    xp_volume_exists(root).then_some(XP_C_DISK_GEOMETRY)
}

fn write_optional_ansi(
    memory: &mut impl GuestMemory,
    pointer: u32,
    capacity: u32,
    text: &str,
    api: &'static str,
) -> Result<(), ProviderDispatchError> {
    if pointer == 0 {
        return Ok(());
    }
    let required = text.len() + 1;
    if (capacity as usize) < required {
        return Err(ProviderDispatchError::Frontier {
            api,
            detail: format!("buffer too small capacity={capacity} required={required}"),
        });
    }
    memory.write(pointer, text.as_bytes())?;
    memory.write(
        pointer
            .checked_add(text.len() as u32)
            .ok_or(ProviderDispatchError::Fault("volume string overflow"))?,
        &[0],
    )?;
    Ok(())
}
const ERROR_NO_TOKEN: u32 = 1008;
const TOKEN_QUERY: u32 = 0x0000_0008;
const TOKEN_HANDLE_BASE: u32 = 0x5743_9001;
const FILE_HANDLE_BASE: u32 = 0x5743_b001;
const FIND_HANDLE_BASE: u32 = 0x5743_c001;
pub const PROCESS_HEAP_HANDLE: u32 = 0x5743_0000;
const PRIVATE_HEAP_HANDLE_BASE: u32 = 0x5743_0001;
const FILE_WRITE_ACCESS_MASK: u32 = 0x5000_0116;
const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;
const CREATE_NEW: u32 = 1;
const CREATE_ALWAYS: u32 = 2;
const OPEN_EXISTING: u32 = 3;
const OPEN_ALWAYS: u32 = 4;
const TRUNCATE_EXISTING: u32 = 5;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_BEGIN: u32 = 0;
const FILE_CURRENT: u32 = 1;
const FILE_END: u32 = 2;
pub const HEAP_NO_SERIALIZE: u32 = 0x0000_0001;
pub const HEAP_GENERATE_EXCEPTIONS: u32 = 0x0000_0004;
pub const HEAP_ZERO_MEMORY: u32 = 0x0000_0008;
pub const HEAP_ALLOC_ALLOWED_FLAGS: u32 =
    HEAP_NO_SERIALIZE | HEAP_GENERATE_EXCEPTIONS | HEAP_ZERO_MEMORY;
const TOKEN_GROUPS_CLASS: u32 = 2;
const SE_GROUP_MANDATORY: u32 = 0x0000_0001;
const SE_GROUP_ENABLED_BY_DEFAULT: u32 = 0x0000_0002;
const SE_GROUP_ENABLED: u32 = 0x0000_0004;
const XP_TOKEN_GROUP_ATTRIBUTES: u32 =
    SE_GROUP_MANDATORY | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED;
const XP_EVERYONE_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
const XP_TOKEN_GROUPS_REQUIRED: u32 = 4 + 8 + XP_EVERYONE_SID.len() as u32;
pub const PROCESS_DATA_VA: u32 = 0x0021_1000;
pub const CRT_FMODE_VA: u32 = PROCESS_DATA_VA + 0x40;
pub const CRT_COMMODE_VA: u32 = PROCESS_DATA_VA + 0x44;
pub const CRT_ACMDLN_VA: u32 = PROCESS_DATA_VA + 0x48;
pub const CRT_ARGV_VA: u32 = PROCESS_DATA_VA + 0x60;
pub const CRT_ENVP_VA: u32 = PROCESS_DATA_VA + 0x78;
pub const CRT_ARG0_VA: u32 = PROCESS_DATA_VA + 0x80;
pub const CRT_ARG1_VA: u32 = PROCESS_DATA_VA + 0x89;
pub const CRT_ARG2_VA: u32 = PROCESS_DATA_VA + 0x91;
pub const CRT_ARG3_VA: u32 = PROCESS_DATA_VA + 0x9a;
pub const CRT_ARGC: u32 = 4;
const XP_MIN_APPLICATION_ADDRESS: u32 = 0x0001_0000;
const XP_MAX_APPLICATION_ADDRESS: u32 = 0x7ffe_ffff;
pub const XP_MEMORY_LOAD: u32 = 25;
pub const XP_TOTAL_PHYS: u32 = 512 * 1024 * 1024;
pub const XP_AVAIL_PHYS: u32 = 384 * 1024 * 1024;
pub const XP_TOTAL_PAGEFILE: u32 = 1024 * 1024 * 1024;
pub const XP_AVAIL_PAGEFILE: u32 = 768 * 1024 * 1024;
pub const XP_TOTAL_VIRTUAL: u32 = 0x7ffe_0000;
pub const XP_AVAIL_VIRTUAL: u32 = 0x6000_0000;
const PROCESSOR_ARCHITECTURE_INTEL: u16 = 0;
const PROCESSOR_INTEL_PENTIUM: u32 = 586;
const XP_PROCESSOR_LEVEL: u16 = 6;
const PROCESS_SID_ARENA_BASE: u32 = PROCESS_DATA_VA + 0x200;
const PROCESS_SID_ARENA_LIMIT: u32 = PROCESS_DATA_VA + 0x1000;
const SID_HEADER_BYTES: usize = 8;
const SID_MAX_SUB_AUTHORITIES: usize = 15;
const ALLOCATE_AND_INITIALIZE_SID_MAX_SUB_AUTHORITIES: usize = 8;
/// Historical launcher stack: 0x0430_0000..0x0440_0000.
pub const STACK_BASE: u32 = 0x0430_0000;
pub const STACK_BYTES: usize = 0x10_0000;
pub const STACK_TOP: u32 = STACK_BASE + STACK_BYTES as u32;
pub const THUNK_PAGE_BYTES: usize = 0x1000;
/// Process state returned by GetCommandLineA.  The launcher constructs its
/// separate `"war3.exe" -opengl -nosound -swtnl` child command line on its
/// native stack.
pub const COMMAND_LINE: &[u8] = b"\"Warcraft III.exe\"\0";
pub const CHILD_COMMAND_LINE: &[u8] = b"\"war3.exe\" -opengl -nosound -swtnl\0";
pub const LAUNCHER_IMAGE_FILENAME: &[u8] = b"C:\\Warcraft III\\Warcraft III.exe\0";
pub const CHILD_IMAGE_FILENAME: &[u8] = b"C:\\Warcraft III\\War3.exe\0";
const CHILD_WORKING_DIRECTORY: &str = "C:\\Warcraft III";
pub const XP_WINDOWS_DIRECTORY: &[u8] = b"C:\\WINDOWS\0";
pub const XP_SYSTEM_DIRECTORY: &[u8] = b"C:\\WINDOWS\\system32\0";
const XP_TEMP_DIRECTORY: &[u8] = b"C:\\WINDOWS\\\0";
const XP_PERFORMANCE_COUNTER_FREQUENCY: u64 = 1_000_000_000;
const TIME_ZONE_ID_UNKNOWN: u32 = 0;
const XP_TIME_ZONE_INFORMATION_BYTES: usize = 172;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
const SECONDS_PER_DAY: u64 = 86_400;

fn xp_system_time_from_unix_nanos(nanos: u64) -> [u16; 8] {
    let seconds = nanos / NANOS_PER_SECOND;
    let now = UtcDateTime::from_unix_seconds(seconds);
    let day_of_week = ((seconds / SECONDS_PER_DAY + 4) % 7) as u16;
    let milliseconds = ((nanos / 1_000_000) % 1_000) as u16;

    [
        now.year as u16,
        now.month as u16,
        day_of_week,
        now.day as u16,
        now.hour as u16,
        now.minute as u16,
        now.second as u16,
        milliseconds,
    ]
}

fn xp_is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn xp_days_in_month(year: u32, month: u32) -> Option<u32> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if xp_is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

fn xp_filetime_from_system_time(st: [u16; 8]) -> Option<u64> {
    let year = u32::from(st[0]);
    let month = u32::from(st[1]);
    // st[2] is wDayOfWeek, which SystemTimeToFileTime ignores.
    let day = u32::from(st[3]);
    let hour = u32::from(st[4]);
    let minute = u32::from(st[5]);
    let second = u32::from(st[6]);
    let millis = u32::from(st[7]);

    if !(1601..=30827).contains(&year)
        || hour > 23
        || minute > 59
        || second > 59
        || millis > 999
    {
        return None;
    }

    let days_this_month = xp_days_in_month(year, month)?;
    if day == 0 || day > days_this_month {
        return None;
    }

    let mut days = 0u64;
    for current_year in 1601..year {
        days += if xp_is_leap_year(current_year) { 366 } else { 365 };
    }
    for current_month in 1..month {
        days += u64::from(xp_days_in_month(year, current_month)?);
    }
    days += u64::from(day - 1);

    let seconds = days
        .checked_mul(SECONDS_PER_DAY)?
        .checked_add(u64::from(hour) * 3_600)?
        .checked_add(u64::from(minute) * 60)?
        .checked_add(u64::from(second))?;

    seconds
        .checked_mul(10_000_000)?
        .checked_add(u64::from(millis) * 10_000)
}

/// A fixed UTC `TIME_ZONE_INFORMATION`: zero bias, no daylight-saving
/// transitions, and UTF-16 UTC names.  The exposed WC3 wall clock is UTC.
fn xp_utc_time_zone_information() -> [u8; XP_TIME_ZONE_INFORMATION_BYTES] {
    let mut bytes = [0; XP_TIME_ZONE_INFORMATION_BYTES];
    for name_start in [4, 88] {
        for (index, character) in "UTC".encode_utf16().enumerate() {
            let offset = name_start + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&character.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
#[test]
fn xp_system_time_epoch_layout() {
    assert_eq!(
        xp_system_time_from_unix_nanos(123_000_000),
        [1970, 1, 4, 1, 0, 0, 0, 123],
    );
}

#[cfg(test)]
#[test]
fn xp_filetime_from_system_time_converts_valid_calendar_times() {
    assert_eq!(
        xp_filetime_from_system_time([1601, 1, 0, 1, 0, 0, 0, 0]),
        Some(0),
    );
    assert_eq!(
        xp_filetime_from_system_time([1970, 1, 4, 1, 0, 0, 0, 0]),
        Some(116_444_736_000_000_000),
    );
    assert!(xp_filetime_from_system_time([2000, 2, 0, 29, 0, 0, 0, 0]).is_some());
    assert!(xp_filetime_from_system_time([1900, 2, 0, 29, 0, 0, 0, 0]).is_none());
}

#[cfg(test)]
#[test]
fn xp_utc_time_zone_information_layout() {
    let bytes = xp_utc_time_zone_information();

    assert_eq!(bytes.len(), 172);
    assert_eq!(&bytes[..4], &0i32.to_le_bytes());
    assert_eq!(&bytes[4..12], &[b'U', 0, b'T', 0, b'C', 0, 0, 0]);
    assert_eq!(&bytes[68..88], &[0; 20]);
    assert_eq!(&bytes[88..96], &[b'U', 0, b'T', 0, b'C', 0, 0, 0]);
    assert_eq!(&bytes[152..172], &[0; 20]);
}

#[cfg(target_os = "trueos")]
fn wall_clock_unix_nanos() -> Option<u64> {
    clock::unix_nanos()
}

#[cfg(not(target_os = "trueos"))]
fn wall_clock_unix_nanos() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}

#[cfg(target_os = "trueos")]
fn monotonic_counter_nanos() -> u64 {
    clock::monotonic_nanos()
}

#[cfg(not(target_os = "trueos"))]
fn monotonic_counter_nanos() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn monotonic_counter_millis() -> u32 {
    #[cfg(target_os = "trueos")]
    {
        return clock::monotonic_millis() as u32;
    }
    #[cfg(not(target_os = "trueos"))]
    {
        (monotonic_counter_nanos() / 1_000_000) as u32
    }
}
pub const XP_ANSI_CODE_PAGE: u32 = 1252;
const CT_CTYPE1: u32 = 1;
const C1_UPPER: u16 = 0x0001;
const C1_LOWER: u16 = 0x0002;
const C1_DIGIT: u16 = 0x0004;
const C1_SPACE: u16 = 0x0008;
const C1_PUNCT: u16 = 0x0010;
const C1_CNTRL: u16 = 0x0020;
const C1_BLANK: u16 = 0x0040;
const C1_XDIGIT: u16 = 0x0080;
const C1_ALPHA: u16 = 0x0100;
const LCMAP_LOWERCASE: u32 = 0x0000_0100;
const LCMAP_UPPERCASE: u32 = 0x0000_0200;
const LCMAP_LINGUISTIC_CASING: u32 = 0x0100_0000;
const WINDOWS_XP_GET_VERSION: u32 = 0x0a28_0105;
const CREATE_SUSPENDED: u32 = 4;
const THREAD_HANDLE_BASE: u32 = 0x5743_5001;
const DESKTOP_HWND: u32 = 0x5743_3000;
const GDI_HANDLE_BASE: u32 = 0x5743_7001;
const STOCK_MONO_BITMAP: u32 = 0x5743_7f01;
const STOCK_DEFAULT_PALETTE: u32 = 0x5743_7f02;
const STOCK_BLACK_BRUSH: u32 = 0x5743_7f04;
const TRANSPARENT: u32 = 1;
const OPAQUE: u32 = 2;
const GDI_DIB_BASE: u32 = 0x0500_0000;
pub const ENVIRONMENT_BLOCK_VA: u32 = PROCESS_DATA_VA + 0x100;

fn identity_gamma_ramp() -> [u16; 3 * 256] {
    let mut ramp = [0u16; 3 * 256];
    for channel in 0..3 {
        for index in 0..256 {
            let byte = index as u16;
            ramp[channel * 256 + index] = (byte << 8) | byte;
        }
    }
    ramp
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LcMapMode {
    Lower,
    Upper,
}

fn lc_map_mode(flags: u32) -> Option<LcMapMode> {
    let allowed = LCMAP_LOWERCASE | LCMAP_UPPERCASE | LCMAP_LINGUISTIC_CASING;
    if flags & !allowed != 0 {
        return None;
    }
    match flags & (LCMAP_LOWERCASE | LCMAP_UPPERCASE) {
        LCMAP_LOWERCASE => Some(LcMapMode::Lower),
        LCMAP_UPPERCASE => Some(LcMapMode::Upper),
        _ => None,
    }
}

pub trait GuestMemory {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str>;
    fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str>;
}

/// Fetch an API argument frame in one checked transfer across the host boundary.
pub fn read_guest_words(memory: &impl GuestMemory, address: u32, count: usize) -> Result<Vec<u32>, String> {
    let len = count.checked_mul(4).ok_or("guest frame size overflow")?;
    if len == 0 { return Ok(Vec::new()); }
    let last = u32::try_from(len - 1).map_err(|_| "guest frame size overflow")?;
    address.checked_add(last).ok_or("guest frame address overflow")?;
    let mut bytes = vec![0; len];
    memory.read(address, &mut bytes).map_err(str::to_owned)?;
    Ok(bytes.chunks_exact(4).map(|word| u32::from_le_bytes(word.try_into().unwrap())).collect())
}

fn read_u32(memory: &impl GuestMemory, address: u32) -> Result<u32, &'static str> {
    let mut bytes = [0; 4];
    memory.read(address, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_u32(memory: &mut impl GuestMemory, address: u32, value: u32) -> Result<(), &'static str> {
    memory.write(address, &value.to_le_bytes())
}

fn ascii_ctype1(value: u16) -> u16 {
    let Ok(byte) = u8::try_from(value) else {
        return 0;
    };
    match byte {
        b'A'..=b'Z' => {
            let mut class = C1_ALPHA | C1_UPPER;
            if matches!(byte, b'A'..=b'F') {
                class |= C1_XDIGIT;
            }
            class
        }
        b'a'..=b'z' => {
            let mut class = C1_ALPHA | C1_LOWER;
            if matches!(byte, b'a'..=b'f') {
                class |= C1_XDIGIT;
            }
            class
        }
        b'0'..=b'9' => C1_DIGIT | C1_XDIGIT,
        b' ' => C1_SPACE | C1_BLANK,
        b'\t' => C1_SPACE | C1_BLANK | C1_CNTRL,
        b'\n' | b'\r' | 0x0b | 0x0c => C1_SPACE | C1_CNTRL,
        0x00..=0x1f | 0x7f => C1_CNTRL,
        0x21..=0x2f | 0x3a..=0x40 | 0x5b..=0x60 | 0x7b..=0x7e => C1_PUNCT,
        _ => 0,
    }
}

fn arguments<const N: usize>(
    memory: &impl GuestMemory,
    esp: u32,
) -> Result<[u32; N], &'static str> {
    if N == 0 { return Ok([0; N]); }
    let len = N.checked_mul(4).ok_or("stack overflow")?;
    let last = u32::try_from(len - 1).map_err(|_| "stack overflow")?;
    esp.checked_add(last).ok_or("stack overflow")?;
    let mut bytes = [[0u8; 4]; N];
    memory.read(esp, bytes.as_flattened_mut())?;
    Ok(bytes.map(u32::from_le_bytes))
}

fn canonical_sid(
    memory: &impl GuestMemory,
    pointer: u32,
) -> Result<Vec<u8>, ProviderDispatchError> {
    let mut header = [0; SID_HEADER_BYTES];
    memory.read(pointer, &mut header)?;
    let subauthority_count = usize::from(header[1]);
    if subauthority_count > SID_MAX_SUB_AUTHORITIES {
        return Err(ProviderDispatchError::Fault("EqualSid subauthority count"));
    }
    let length = SID_HEADER_BYTES
        .checked_add(
            subauthority_count
                .checked_mul(4)
                .ok_or(ProviderDispatchError::Fault("EqualSid length"))?,
        )
        .ok_or(ProviderDispatchError::Fault("EqualSid length"))?;
    let mut sid = vec![0; length];
    memory.read(pointer, &mut sid)?;
    Ok(sid)
}

fn write_ansi_directory(
    path: &[u8],
    esp: u32,
    memory: &mut impl GuestMemory,
) -> Result<u32, &'static str> {
    let [_, output, capacity] = arguments::<3>(memory, esp)?;
    let required = u32::try_from(path.len()).map_err(|_| "directory length")?;
    if capacity < required {
        return Ok(required);
    }
    memory.write(output, path)?;
    Ok(required - 1)
}

fn write_fixed_ansi(record: &mut [u8], offset: usize, capacity: usize, value: &str) {
    let destination = &mut record[offset..offset + capacity];
    let bytes = value.as_bytes();
    let length = bytes.len().min(capacity.saturating_sub(1));
    destination[..length].copy_from_slice(&bytes[..length]);
}

pub fn read_c_string(
    memory: &impl GuestMemory,
    address: u32,
    limit: usize,
) -> Result<String, &'static str> {
    let mut bytes = Vec::new();
    for offset in 0..limit {
        let mut byte = [0];
        memory.read(
            address
                .checked_add(offset as u32)
                .ok_or("string overflow")?,
            &mut byte,
        )?;
        if byte[0] == 0 {
            return String::from_utf8(bytes).map_err(|_| "non-ASCII string");
        }
        bytes.push(byte[0]);
    }
    Err("unterminated string")
}

fn read_c_bytes(
    memory: &impl GuestMemory,
    address: u32,
) -> Result<Vec<u8>, ProviderDispatchError> {
    let mut bytes = Vec::new();
    for offset in 0..staticstr::MAX_C_STRING {
        let mut byte = [0];
        memory.read(
            address
                .checked_add(u32::try_from(offset).map_err(|_| ProviderDispatchError::Fault("string offset"))?)
                .ok_or(ProviderDispatchError::Fault("string overflow"))?,
            &mut byte,
        )?;
        bytes.push(byte[0]);
        if byte[0] == 0 {
            return Ok(bytes);
        }
    }
    Err(ProviderDispatchError::Fault("unterminated string"))
}

fn crt_strncpy(
    memory: &mut impl GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, ProviderDispatchError> {
    let count = usize::try_from(count).map_err(|_| ProviderDispatchError::Fault("strncpy count"))?;
    if count > staticstr::MAX_C_STRING {
        return Err(ProviderDispatchError::Frontier { api: "strncpy", detail: "count exceeds compatibility bound".into() });
    }
    let mut output = vec![0; count];
    let mut terminated = false;
    for (offset, byte) in output.iter_mut().enumerate() {
        if !terminated {
            memory.read(
                source
                    .checked_add(u32::try_from(offset).map_err(|_| ProviderDispatchError::Fault("strncpy offset"))?)
                    .ok_or(ProviderDispatchError::Fault("strncpy source overflow"))?,
                core::slice::from_mut(byte),
            )?;
            terminated = *byte == 0;
        }
    }
    memory.write(destination, &output)?;
    Ok(destination)
}

fn crt_strcase(
    memory: &mut impl GuestMemory,
    string: u32,
    upper: bool,
) -> Result<u32, ProviderDispatchError> {
    let mut bytes = read_c_bytes(memory, string)?;
    if upper { staticstr::upper_in_place(&mut bytes) } else { staticstr::lower_in_place(&mut bytes) };
    memory.write(string, &bytes)?;
    Ok(string)
}

pub fn crt_atol(
    memory: &impl GuestMemory,
    string_ptr: u32,
) -> Result<(u32, bool, String), ProviderDispatchError> {
    if string_ptr == 0 {
        return Err(ProviderDispatchError::Frontier {
            api: "atol",
            detail: "str=NULL".into(),
        });
    }

    let text = read_c_string(memory, string_ptr, 256)?;
    let bytes = text.as_bytes();
    let mut at = 0usize;
    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }

    let negative = match bytes.get(at) {
        Some(b'-') => {
            at += 1;
            true
        }
        Some(b'+') => {
            at += 1;
            false
        }
        _ => false,
    };
    let limit = if negative { 0x8000_0000u64 } else { 0x7fff_ffffu64 };
    let mut any = false;
    let mut magnitude = 0u64;
    let mut overflow = false;

    while let Some(&byte) = bytes.get(at) {
        if !byte.is_ascii_digit() {
            break;
        }
        any = true;
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(u64::from(byte - b'0'));
        if magnitude > limit {
            overflow = true;
            magnitude = limit;
        }
        at += 1;
    }

    let value = if !any {
        0i32
    } else if negative {
        if magnitude == 0x8000_0000 {
            i32::MIN
        } else {
            -(magnitude as i32)
        }
    } else {
        magnitude as i32
    };
    Ok((value as u32, overflow, text))
}

fn crt_strrchr(
    memory: &impl GuestMemory,
    string: u32,
    character: u32,
) -> Result<u32, &'static str> {
    let bytes = read_c_bytes(memory, string).map_err(|_| "strrchr string")?;
    staticstr::rchr(&bytes, character as u8)
        .map(|offset| string + u32::try_from(offset).expect("bounded string offset"))
        .ok_or("strrchr not found")
        .or(Ok(0))
}

fn crt_strstr(memory: &impl GuestMemory, haystack: u32, needle: u32) -> Result<u32, &'static str> {
    let haystack_bytes = read_c_bytes(memory, haystack).map_err(|_| "strstr haystack")?;
    let needle_bytes = read_c_bytes(memory, needle).map_err(|_| "strstr needle")?;
    Ok(staticstr::find(&haystack_bytes, &needle_bytes)
        .map(|offset| haystack + u32::try_from(offset).expect("bounded string offset"))
        .unwrap_or(0))
}

fn crt_strnicmp(
    memory: &impl GuestMemory,
    left: u32,
    right: u32,
    count: u32,
) -> Result<u32, &'static str> {
    // MSVCRT compares unsigned ANSI bytes after locale case-folding. The XP
    // personality is CP1252, including its handful of non-ASCII case pairs.
    let fold = |byte: u8| encode_cp1252(cp1252_lower(decode_cp1252(byte))).unwrap_or(byte);
    let left = read_c_bytes(memory, left).map_err(|_| "strnicmp left")?;
    let right = read_c_bytes(memory, right).map_err(|_| "strnicmp right")?;
    Ok(staticstr::compare_with(&left, &right, Some(count as usize), fold) as u32)
}

fn crt_memmove(
    memory: &mut impl GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, ProviderDispatchError> {
    if count == 0 || destination == source {
        return Ok(destination);
    }
    let length = usize::try_from(count).map_err(|_| ProviderDispatchError::Fault("memmove count"))?;
    source
        .checked_add(count - 1)
        .ok_or(ProviderDispatchError::Fault("memmove source overflow"))?;
    destination
        .checked_add(count - 1)
        .ok_or(ProviderDispatchError::Fault("memmove destination overflow"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| ProviderDispatchError::Fault("memmove allocation"))?;
    bytes.resize(length, 0);
    memory.read(source, &mut bytes)?;
    memory.write(destination, &bytes)?;
    Ok(destination)
}

fn crt_full_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let path = path.replace('/', "\\");
    if path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\") {
        Some(path)
    } else {
        Some(format!("{CHILD_WORKING_DIRECTORY}\\{path}"))
    }
}

fn wsprintf_a(memory: &mut impl GuestMemory, esp: u32) -> Result<u32, ProviderDispatchError> {
    let [_, output, format] = arguments::<3>(memory, esp)?;
    if output == 0 || format == 0 {
        return Err(ProviderDispatchError::Frontier {
            api: "wsprintfA",
            detail: "null output or format".into(),
        });
    }
    let format = read_c_string(memory, format, 1024)?;
    let mut values = 0u32;
    let mut next = || -> Result<u32, ProviderDispatchError> {
        let offset = values
            .checked_mul(4)
            .and_then(|offset| {
                esp.checked_add(12)
                    .and_then(|base| base.checked_add(offset))
            })
            .ok_or(ProviderDispatchError::Fault("wsprintfA argument overflow"))?;
        values = values
            .checked_add(1)
            .ok_or(ProviderDispatchError::Fault("wsprintfA argument count"))?;
        Ok(read_u32(memory, offset)?)
    };
    let mut rendered = String::new();
    let mut characters = format.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            rendered.push(character);
            continue;
        }
        let Some(specifier) = characters.next() else {
            return Err(ProviderDispatchError::Frontier {
                api: "wsprintfA",
                detail: "trailing percent".into(),
            });
        };
        match specifier {
            '%' => rendered.push('%'),
            's' => {
                let pointer = next()?;
                if pointer == 0 {
                    rendered.push_str("(null)");
                } else {
                    rendered.push_str(&read_c_string(memory, pointer, 1024)?);
                }
            }
            'c' => rendered.push((next()? as u8) as char),
            'd' | 'i' => rendered.push_str(&(next()? as i32).to_string()),
            'u' => rendered.push_str(&next()?.to_string()),
            'x' => rendered.push_str(&format!("{:x}", next()?)),
            'X' => rendered.push_str(&format!("{:X}", next()?)),
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "wsprintfA",
                    detail: format!("unsupported format %{specifier}"),
                });
            }
        }
        if rendered.len() > 4096 {
            return Err(ProviderDispatchError::Frontier {
                api: "wsprintfA",
                detail: "rendered output exceeds 4096 bytes".into(),
            });
        }
    }
    memory.write(output, rendered.as_bytes())?;
    memory.write(
        output
            .checked_add(
                u32::try_from(rendered.len())
                    .map_err(|_| ProviderDispatchError::Fault("wsprintfA length"))?,
            )
            .ok_or(ProviderDispatchError::Fault("wsprintfA output overflow"))?,
        &[0],
    )?;
    u32::try_from(rendered.len()).map_err(|_| ProviderDispatchError::Fault("wsprintfA length"))
}

fn crt_vsnprintf(memory: &mut impl GuestMemory, esp: u32) -> Result<u32, ProviderDispatchError> {
    let [_, output, count, format, va_list] = arguments::<5>(memory, esp)?;
    if format == 0 || va_list == 0 {
        return Err(ProviderDispatchError::Frontier {
            api: "_vsnprintf",
            detail: "null format or va_list".into(),
        });
    }
    if count != 0 && output == 0 {
        return Err(ProviderDispatchError::Frontier {
            api: "_vsnprintf",
            detail: "null output with nonzero count".into(),
        });
    }

    let format = read_c_string(memory, format, 4096)?;
    let mut argument_index = 0u32;
    let mut next = || -> Result<u32, ProviderDispatchError> {
        let address = argument_index
            .checked_mul(4)
            .and_then(|offset| va_list.checked_add(offset))
            .ok_or(ProviderDispatchError::Fault("_vsnprintf va_list overflow"))?;
        argument_index = argument_index
            .checked_add(1)
            .ok_or(ProviderDispatchError::Fault("_vsnprintf argument count"))?;
        Ok(read_u32(memory, address)?)
    };

    let mut rendered = String::new();
    let mut characters = format.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '%' {
            rendered.push(character);
            continue;
        }
        if characters.peek() == Some(&'%') {
            characters.next();
            rendered.push('%');
            continue;
        }
        let mut left_justify = false;
        let mut zero_pad = false;
        while let Some(flag) = characters.peek().copied() {
            match flag {
                '-' => left_justify = true,
                '0' => zero_pad = true,
                '+' | ' ' | '#' => {}
                _ => break,
            }
            characters.next();
        }
        let mut width = 0usize;
        if characters.peek() == Some(&'*') {
            characters.next();
            width = (next()? as i32).unsigned_abs() as usize;
        } else {
            while let Some(digit) = characters.peek().and_then(|value| value.to_digit(10)) {
                characters.next();
                width = width
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(digit as usize))
                    .ok_or(ProviderDispatchError::Fault("_vsnprintf width overflow"))?;
            }
        }
        let mut precision = None;
        if characters.peek() == Some(&'.') {
            characters.next();
            if characters.peek() == Some(&'*') {
                characters.next();
                precision = Some(next()? as usize);
            } else {
                let mut value = 0usize;
                while let Some(digit) = characters.peek().and_then(|value| value.to_digit(10)) {
                    characters.next();
                    value = value
                        .checked_mul(10)
                        .and_then(|value| value.checked_add(digit as usize))
                        .ok_or(ProviderDispatchError::Fault(
                            "_vsnprintf precision overflow",
                        ))?;
                }
                precision = Some(value);
            }
        }
        while matches!(characters.peek(), Some('h' | 'l' | 'L' | 'I' | 'w')) {
            characters.next();
            if characters.peek() == Some(&'6') {
                characters.next();
                if characters.peek() == Some(&'4') {
                    characters.next();
                }
            }
        }
        let Some(specifier) = characters.next() else {
            return Err(ProviderDispatchError::Frontier {
                api: "_vsnprintf",
                detail: "trailing percent".into(),
            });
        };
        let mut field = match specifier {
            's' | 'S' => {
                let pointer = next()?;
                let value = if pointer == 0 {
                    "(null)".into()
                } else {
                    read_c_string(memory, pointer, 4096)?
                };
                match precision {
                    Some(limit) => value.chars().take(limit).collect(),
                    None => value,
                }
            }
            'c' | 'C' => char::from(next()? as u8).to_string(),
            'd' | 'i' => (next()? as i32).to_string(),
            'u' => next()?.to_string(),
            'x' => format!("{:x}", next()?),
            'X' => format!("{:X}", next()?),
            'p' => format!("{:08x}", next()?),
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "_vsnprintf",
                    detail: format!("unsupported format %{specifier}"),
                });
            }
        };
        if width > field.len() {
            let padding = width - field.len();
            let fill = if zero_pad && !left_justify { '0' } else { ' ' };
            let pad: String = std::iter::repeat_n(fill, padding).collect();
            if left_justify {
                field.push_str(&pad);
            } else {
                field = format!("{pad}{field}");
            }
        }
        rendered.push_str(&field);
        if rendered.len() > 65_536 {
            return Err(ProviderDispatchError::Frontier {
                api: "_vsnprintf",
                detail: "rendered output exceeds 65536 bytes".into(),
            });
        }
    }

    let capacity =
        usize::try_from(count).map_err(|_| ProviderDispatchError::Fault("_vsnprintf count"))?;
    if rendered.len() < capacity {
        memory.write(output, rendered.as_bytes())?;
        memory.write(
            output
                .checked_add(
                    u32::try_from(rendered.len())
                        .map_err(|_| ProviderDispatchError::Fault("_vsnprintf length"))?,
                )
                .ok_or(ProviderDispatchError::Fault("_vsnprintf output overflow"))?,
            &[0],
        )?;
        return u32::try_from(rendered.len())
            .map_err(|_| ProviderDispatchError::Fault("_vsnprintf length"));
    }
    if capacity != 0 {
        memory.write(output, &rendered.as_bytes()[..capacity.min(rendered.len())])?;
    }
    Ok(u32::MAX)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    pub address: u32,
    pub bytes: Vec<u8>,
    pub executable: bool,
}

/// The prepared userspace-owned process image handed to the generic x86 ABI.
pub struct PreparedProcess {
    pub mappings: Vec<Mapping>,
    pub xp: XpProcess,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProcessAFrame {
    pub return_address: u32,
    pub application_name: u32,
    pub command_line: u32,
    pub process_attributes: u32,
    pub thread_attributes: u32,
    pub inherit_handles: u32,
    pub creation_flags: u32,
    pub environment: u32,
    pub current_directory: u32,
    pub startup_info: u32,
    pub process_information: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitForMultipleObjectsFrame {
    pub return_address: u32,
    pub count: u32,
    pub handles_pointer: u32,
    pub handles: [u32; 2],
    pub wait_all: u32,
    pub timeout: u32,
}

#[derive(Clone, Debug)]
struct BitmapObject {
    resource_id: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    clr_used: u32,
    dib: Vec<u8>,
    decoded_rgba: Vec<u8>,
    palette: Vec<[u8; 4]>,
    bits_va: u32,
    bits_len: usize,
    row_stride: u32,
    pixel_offset: usize,
    stock: bool,
}

#[derive(Clone, Debug)]
struct DeviceContext {
    compatible_with: DcCompatibility,
    target: DcTarget,
    selected_palette: u32,
    palette_force_background: bool,
    realized_palette: Option<u32>,
    text_color: u32,
    bk_color: u32,
    bk_mode: u32,
}

#[derive(Clone, Copy, Debug)]
struct ActivePaint {
    hwnd: u32,
    hdc: u32,
}

#[derive(Clone, Debug)]
enum DcTarget {
    Memory { selected_bitmap: u32 },
    WindowPaint { hwnd: u32 },
}

#[derive(Clone, Debug)]
struct PaletteObject {
    version: u16,
    entries: Vec<PaletteEntry>,
    stock: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PaletteEntry {
    red: u8,
    green: u8,
    blue: u8,
    flags: u8,
}

#[derive(Clone, Copy, Debug)]
enum DcCompatibility {
    Display,
}

#[derive(Clone, Debug)]
enum GdiObject {
    Bitmap(BitmapObject),
    DeviceContext(DeviceContext),
    Palette(PaletteObject),
}

#[derive(Clone, Debug)]
enum UserImage {
    Icon {
        module: u32,
        name: String,
        width: u32,
        height: u32,
        resource_id: u16,
        bytes: Vec<u8>,
    },
    Cursor {
        module: u32,
        name: String,
        width: u32,
        height: u32,
        resource_id: u16,
        hotspot_x: u16,
        hotspot_y: u16,
        bytes: Vec<u8>,
        shared: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserImageLoadResult {
    pub handle: u32,
    pub module: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub resource_id: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserCursorLoadResult {
    pub handle: u32,
    pub module: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub resource_id: u16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub resource_bytes: usize,
    pub shared: bool,
}

impl PreparedProcess {
    pub fn new(mut materialized: pe32::Materialized) -> Result<Self, &'static str> {
        let mut thunks = vec![0; THUNK_PAGE_BYTES];
        crate::imports::patch(&mut materialized.image, &materialized.imports, &mut thunks)?;
        thunk32::install_thread_exit(&mut thunks)?;
        thunk32::install_guest_return(&mut thunks)?;
        let mut teb = vec![0; 0x1000];
        teb[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut process_data = vec![0; 0x1000];
        process_data[..COMMAND_LINE.len()].copy_from_slice(COMMAND_LINE);
        Ok(Self {
            mappings: vec![
                Mapping {
                    address: pe32::IMAGE_BASE,
                    bytes: materialized.image,
                    executable: true,
                },
                Mapping {
                    address: thunk32::THUNK_BASE,
                    bytes: thunks,
                    executable: true,
                },
                Mapping {
                    address: TEB_VA,
                    bytes: teb,
                    executable: false,
                },
                Mapping {
                    address: HEAP_VA,
                    bytes: vec![0; 0x1000],
                    executable: false,
                },
                Mapping {
                    address: PROCESS_DATA_VA,
                    bytes: process_data,
                    executable: false,
                },
                Mapping {
                    address: STACK_BASE,
                    bytes: vec![0; STACK_BYTES],
                    executable: false,
                },
            ],
            xp: XpProcess::new(materialized.imports),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadObject {
    pub handle: u32,
    pub tid: u32,
    pub start_address: u32,
    pub parameter: u32,
    pub requested_stack_size: u32,
    pub suspend_count: u32,
    pub exit_code: Option<u32>,
    pub open: bool,
}

#[derive(Clone, Debug)]
struct RegisteredClass {
    atom: u16,
    name: String,
    style: u32,
    wndproc: u32,
    cls_extra: i32,
    wnd_extra: i32,
    instance: u32,
    icon: u32,
    cursor: u32,
    background: u32,
    menu_name: Option<String>,
    icon_sm: u32,
}

#[derive(Clone, Debug)]
struct Message {
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
    time: u32,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegistryHandle {
    node: crate::session::RegistryNodeId,
    access: u32,
}

#[derive(Clone, Copy, Debug)]
struct TokenHandle {
    pid: u32,
    access: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileBacking {
    SelfImage,
    War3Mpq,
    TrueosFs(u32),
    Scratch(u32),
}

#[derive(Clone, Debug)]
struct ResidentFile {
    win_path: String,
    trueos_path: String,
    bytes: Arc<Vec<u8>>,
    attributes: u32,
}

#[derive(Clone, Debug)]
struct ScratchFile {
    path: String,
    bytes: Vec<u8>,
    attributes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileHandle {
    backing: FileBacking,
    cursor: u64,
    access: u32,
    share: u32,
}

fn canonical_file_path(path: &str) -> String {
    let normalized = path.replace('/', "\\").to_ascii_lowercase();
    let unc = normalized.starts_with("\\\\");

    let mut canonical = String::with_capacity(normalized.len());
    let mut previous_separator = false;

    for (index, character) in normalized.chars().enumerate() {
        if character == '\\' {
            // Retain the leading pair for a UNC path, but collapse every
            // other run of directory separators into one DOS separator.
            if !previous_separator || (unc && index == 1) {
                canonical.push('\\');
            }
            previous_separator = true;
        } else {
            canonical.push(character);
            previous_separator = false;
        }
    }

    canonical
}

fn is_self_image_path(path: &str) -> bool {
    canonical_file_path(path) == r"c:\warcraft iii\war3.exe"
}

fn is_war3_pre_cache_search(path: &str) -> bool {
    canonical_file_path(path) == r"c:\warcraft iii\filecache\*.pre"
}

fn is_war3_mpq_path(path: &str) -> bool {
    canonical_file_path(path) == r"c:\warcraft iii\war3.mpq"
}

fn warcraft_drive_relative_path(path: &str) -> Option<String> {
    canonical_file_path(path)
        .strip_prefix(r"c:\warcraft iii\")
        .map(str::to_owned)
}

fn is_war3_scratch_path(path: &str) -> bool {
    matches!(
        canonical_file_path(path).as_str(),
        r"c:\windows\sintf16.dll" | r"c:\windows\sintf32.dll" | r"c:\windows\sintfnt.dll"
    )
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CrtAllocation {
    pub pointer: u32,
    pub requested: u32,
    pub end: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CrtResize {
    pub old_pointer: u32,
    pub pointer: u32,
    pub used_bytes: u32,
    pub required_bytes: u32,
    pub end: u32,
    pub moved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualReservation {
    pub base: u32,
    pub size: u32,
    pub committed: Vec<VirtualCommit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualCommit {
    pub base: u32,
    pub size: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VirtualCommitRequest {
    pub reservation_base: u32,
    pub reservation_size: u32,
    pub base: u32,
    pub size: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VirtualNullCommitRequest {
    pub base: u32,
    pub size: u32,
    pub next_reserve: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WinHeap {
    options: u32,
    initial_size: u32,
    maximum_size: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct HeapCreateResult {
    pub options: u32,
    pub initial_size: u32,
    pub maximum_size: u32,
    pub handle: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WinHeapAllocation {
    pub heap: u32,
    pub flags: u32,
    pub requested: u32,
    pub pointer: u32,
    pub end: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GlobalAllocation {
    pub flags: u32,
    pub requested: u32,
    pub pointer: u32,
    pub end: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderDispatchError {
    Unsupported,
    Fault(&'static str),
    Frontier { api: &'static str, detail: String },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ProcessImage {
    Launcher,
    War3Child,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadedModule {
    requested_name: String,
    stored_name: String,
    handle: u32,
    kind: LoadedModuleKind,
    filename: Option<String>,
    thread_library_calls_disabled: bool,
    load_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadedModuleKind {
    MainImage,
    NativeImage,
    ExternalProvider,
}

/// The semantic result of one successful `FreeLibrary` reference release.
///
/// Native images remain registered at their final reference until guest
/// detach/unmap behavior is modeled; external personality providers can be
/// removed immediately because TRUEOS owns their implementation lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleRelease {
    Retained { remaining: u32 },
    ExternalProviderUnloaded { module: String },
    NativeUnloadRequired { module: String },
}

fn module_basename(name: &str) -> &str {
    name.rsplit(['\\', '/']).next().unwrap_or(name)
}

fn module_names_match(left: &str, right: &str) -> bool {
    module_basename(left).eq_ignore_ascii_case(module_basename(right))
}

/// DLLs supplied by the XP personality rather than by the Warcraft install.
///
/// This deliberately remains a closed set: a missing application DLL must not
/// become a successful system load merely because it has a `.dll` suffix.
pub fn is_system_provider_module(module: &str) -> bool {
    matches!(
        module_basename(module).to_ascii_lowercase().as_str(),
        "kernel32.dll"
            | "user32.dll"
            | "gdi32.dll"
            | "advapi32.dll"
            | "winmm.dll"
            | "msvcrt.dll"
            | "ole32.dll"
            | "imm32.dll"
            | "comdlg32.dll"
            | "comctl32.dll"
            | "opengl32.dll"
            | "d3d8.dll"
    )
}

impl ProcessImage {
    const fn filename(self) -> &'static [u8] {
        match self {
            Self::Launcher => LAUNCHER_IMAGE_FILENAME,
            Self::War3Child => CHILD_IMAGE_FILENAME,
        }
    }
}

impl From<&'static str> for ProviderDispatchError {
    fn from(error: &'static str) -> Self {
        Self::Fault(error)
    }
}

impl core::fmt::Display for ProviderDispatchError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("unsupported child provider import"),
            Self::Fault(error) => formatter.write_str(error),
            Self::Frontier { api, detail } => write!(formatter, "{api} frontier: {detail}"),
        }
    }
}

pub struct XpProcess {
    image: ProcessImage,
    loaded_modules: Vec<LoadedModule>,
    next_provider_module_handle: u32,
    imports: Vec<LauncherImport>,
    provider_imports: Vec<ProviderImport>,
    provider_thunks: Vec<u8>,
    provider_modules: Vec<ChildProvider>,
    pub call_count: u32,
    pub threads: Vec<ThreadObject>,
    next_tid: u32,
    next_thread_handle: u32,
    heap_next: u32,
    allocations: HashMap<u32, u32>,
    heaps: HashMap<u32, WinHeap>,
    next_heap_handle: u32,
    win_heap_next: u32,
    win_heap_allocations: HashMap<u32, WinHeapAllocation>,
    global_allocations: HashMap<u32, GlobalAllocation>,
    crt_heap_next: u32,
    crt_allocations: HashMap<u32, u32>,
    crt_onexit_callbacks: Vec<u32>,
    pub crt_app_type: u32,
    crt_rng_seed: u32,
    d3d8_ref_count: u32,
    virtual_reservations: Vec<VirtualReservation>,
    virtual_reserve_next: u32,
    tls_allocated: [bool; 64],
    tls_values: HashMap<(u32, u32), u32>,
    critical_sections: HashMap<u32, (u32, u32)>,
    registry_handles: HashMap<u32, RegistryHandle>,
    next_registry_handle: u32,
    token_handles: HashMap<u32, TokenHandle>,
    next_token_handle: u32,
    file_handles: HashMap<u32, FileHandle>,
    war3_mpq_bytes: Option<Arc<Vec<u8>>>,
    resident_files: HashMap<u32, ResidentFile>,
    next_resident_file: u32,
    next_file_handle: u32,
    next_find_handle: u32,
    find_handles: HashSet<u32>,
    scratch_files: HashMap<u32, ScratchFile>,
    scratch_paths: HashMap<String, u32>,
    next_scratch_file: u32,
    sid_allocations: HashMap<u32, u32>,
    next_sid: u32,
    /// Compatibility mirror for callers of the original process-wide API.
    last_error: u32,
    last_errors: HashMap<u32, u32>,
    active_last_error_tid: Option<u32>,
    last_error_compat_tid: Option<u32>,
    last_format_message_encoding: Option<MessageResourceEncoding>,
    unhandled_exception_filter: u32,
    registered_classes: HashMap<String, RegisteredClass>,
    messages: VecDeque<Message>,
    runnable_thread: Option<u32>,
    desktop_size: (u32, u32),
    gdi_objects: HashMap<u32, GdiObject>,
    active_paints: HashMap<u32, ActivePaint>,
    window_dcs: HashMap<u32, u32>,
    window_pixel_formats: HashMap<u32, u32>,
    gamma_ramp: [u16; 3 * 256],
    gl_runtime: Option<GlRuntime>,
    next_gdi_handle: u32,
    next_gdi_dib_va: u32,
    user_images: HashMap<u32, UserImage>,
    next_user_image_handle: u32,
}

struct WglContext {
    hdc: u32,
    hwnd: u32,
    pixel_format: u32,
    current_tid: Option<u32>,
    matrix_mode: u32,
    modelview_matrix: [f32; 16],
    projection_matrix: [f32; 16],
    texture_matrix: [f32; 16],
    light_model_ambient: [f32; 4],
    light0_specular: [f32; 4],
    light0_ambient: [f32; 4],
    light0_diffuse: [f32; 4],
    light0_position_eye: [f32; 4],
    light0_enabled: bool,
    ui4_window_id: Option<u32>,
    viewport: [i32; 4],
    clear_color: [f32; 4],
    vertex_array_enabled: bool,
    color_array_enabled: bool,
    vertex_pointer: Option<GlArrayPointer>,
    color_pointer: Option<GlArrayPointer>,
    observed_writes: VecDeque<GlWriteNote>,
}

struct GlWriteNote {
    symbol: &'static str,
    arguments: Vec<u32>,
    description: String,
}

#[derive(Clone, Copy)]
struct GlArrayPointer {
    size: u32,
    kind: u32,
    stride: u32,
    address: u32,
}

struct GlRuntime {
    device: Device,
    queue: Queue,
    contexts: HashMap<u32, WglContext>,
    next_context: u32,
    triangle_renderer: Option<staticgl_triangle::TriangleRenderer>,
}

impl XpProcess {
    pub fn loaded_module_handle(&self, requested: &str) -> Option<u32> {
        self.loaded_modules
            .iter()
            .find(|module| {
                module_names_match(&module.requested_name, requested)
                    || module_names_match(&module.stored_name, requested)
            })
            .map(|module| module.handle)
    }

    pub fn loaded_module_name(&self, handle: u32) -> Option<&str> {
        self.loaded_modules
            .iter()
            .find(|module| module.handle == handle)
            .map(|module| module_basename(&module.stored_name))
    }

    pub fn external_provider_module_name(&self, handle: u32) -> Option<&str> {
        self.loaded_modules
            .iter()
            .find(|module| {
                module.handle == handle && module.kind == LoadedModuleKind::ExternalProvider
            })
            .map(|module| module_basename(&module.stored_name))
    }

    /// Admit a system DLL that was discovered through LoadLibraryA at runtime.
    /// Existing modules retain their reference count; a newly admitted module
    /// begins with the one reference owned by this call.
    pub fn load_runtime_external_provider(
        &mut self,
        module: &str,
    ) -> Result<(u32, u32, bool), &'static str> {
        if let Some(handle) = self.loaded_module_handle(module) {
            let references = self.retain_loaded_module(handle)?;
            return Ok((handle, references, true));
        }

        self.register_external_provider_module(module)?;
        let handle = self
            .loaded_module_handle(module)
            .ok_or("registered external provider missing")?;
        Ok((handle, 1, false))
    }

    pub fn provider_thunk_address(&self, module: &str, symbol: &ProviderSymbol) -> Option<u32> {
        self.provider_imports
            .iter()
            .position(|import| {
                import.module.eq_ignore_ascii_case(module) && import.symbol == *symbol
            })
            .and_then(|id| thunk32::address(u32::try_from(id).ok()?))
    }

    /// A dynamically callable provider export, whether its behavior is
    /// modeled or it has an explicit external ABI contract.
    pub fn provider_export_address(&self, module: &str, symbol: &ProviderSymbol) -> Option<u32> {
        let import = ProviderImport {
            module: module.into(),
            symbol: symbol.clone(),
            iat_rva: 0,
        };
        crate::child_loader::provider_data_export_address(&import).or_else(|| {
            self.provider_imports
                .iter()
                .enumerate()
                .find(|(_, import)| {
                    import.module.eq_ignore_ascii_case(module)
                        && import.symbol == *symbol
                        && crate::child_loader::external_export_thunk_kind(import).is_some()
                })
                .and_then(|(id, _)| thunk32::address(u32::try_from(id).ok()?))
        })
    }

    pub fn new(imports: Vec<LauncherImport>) -> Self {
        Self::with_image(imports, ProcessImage::Launcher)
    }

    pub fn new_child() -> Self {
        Self::with_image(Vec::new(), ProcessImage::War3Child)
    }

    fn with_image(imports: Vec<LauncherImport>, image: ProcessImage) -> Self {
        let image_filename = std::str::from_utf8(image.filename())
            .expect("process image filename must be ASCII")
            .trim_end_matches('\0');
        let mut heaps = HashMap::new();
        heaps.insert(
            PROCESS_HEAP_HANDLE,
            WinHeap {
                options: 0,
                initial_size: 0,
                maximum_size: 0,
            },
        );
        let mut gdi_objects = HashMap::new();
        gdi_objects.insert(
            STOCK_MONO_BITMAP,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 0,
                width: 1,
                height: 1,
                planes: 1,
                bit_count: 1,
                compression: 0,
                size_image: 0,
                clr_used: 0,
                dib: Vec::new(),
                decoded_rgba: Vec::new(),
                palette: Vec::new(),
                bits_va: 0,
                bits_len: 0,
                row_stride: 0,
                pixel_offset: 0,
                stock: true,
            }),
        );
        gdi_objects.insert(
            STOCK_DEFAULT_PALETTE,
            GdiObject::Palette(PaletteObject {
                version: 0,
                entries: Vec::new(),
                stock: true,
            }),
        );
        Self {
            image,
            loaded_modules: vec![LoadedModule {
                requested_name: module_basename(image_filename).to_owned(),
                stored_name: module_basename(image_filename).to_owned(),
                handle: pe32::IMAGE_BASE,
                kind: LoadedModuleKind::MainImage,
                filename: Some(image_filename.to_owned()),
                thread_library_calls_disabled: false,
                load_count: 1,
            }],
            next_provider_module_handle: PROVIDER_MODULE_HANDLE_BASE,
            imports,
            provider_imports: Vec::new(),
            provider_thunks: Vec::new(),
            provider_modules: Vec::new(),
            call_count: 0,
            threads: Vec::new(),
            next_tid: 2,
            next_thread_handle: THREAD_HANDLE_BASE,
            heap_next: 0,
            allocations: HashMap::new(),
            heaps,
            next_heap_handle: PRIVATE_HEAP_HANDLE_BASE,
            win_heap_next: CHILD_WIN_HEAP_BASE,
            win_heap_allocations: HashMap::new(),
            global_allocations: HashMap::new(),
            crt_heap_next: 0,
            crt_allocations: HashMap::new(),
            crt_onexit_callbacks: Vec::new(),
            crt_app_type: CRT_UNKNOWN_APP,
            // MSVCRT's process-wide rand stream starts from seed one until
            // srand supplies the deterministic seed consumed by rand.
            crt_rng_seed: 1,
            d3d8_ref_count: 0,
            virtual_reservations: Vec::new(),
            virtual_reserve_next: CHILD_VIRTUAL_ALLOC_BASE,
            tls_allocated: [false; 64],
            tls_values: HashMap::new(),
            critical_sections: HashMap::new(),
            registry_handles: HashMap::new(),
            next_registry_handle: 0x5743_8001,
            token_handles: HashMap::new(),
            next_token_handle: TOKEN_HANDLE_BASE,
            file_handles: HashMap::new(),
            war3_mpq_bytes: None,
            resident_files: HashMap::new(),
            next_resident_file: 1,
            next_file_handle: FILE_HANDLE_BASE,
            next_find_handle: FIND_HANDLE_BASE,
            find_handles: HashSet::new(),
            scratch_files: HashMap::new(),
            scratch_paths: HashMap::new(),
            next_scratch_file: 1,
            sid_allocations: HashMap::new(),
            next_sid: PROCESS_SID_ARENA_BASE,
            last_error: 0,
            last_errors: HashMap::new(),
            active_last_error_tid: None,
            last_error_compat_tid: None,
            last_format_message_encoding: None,
            unhandled_exception_filter: 0,
            registered_classes: HashMap::new(),
            messages: VecDeque::new(),
            runnable_thread: None,
            desktop_size: (1920, 1080),
            gdi_objects,
            active_paints: HashMap::new(),
            window_dcs: HashMap::new(),
            window_pixel_formats: HashMap::new(),
            gamma_ramp: identity_gamma_ramp(),
            gl_runtime: None,
            next_gdi_handle: GDI_HANDLE_BASE,
            next_gdi_dib_va: GDI_DIB_BASE,
            user_images: HashMap::new(),
            next_user_image_handle: USER_IMAGE_HANDLE_BASE,
        }
    }

    pub fn import(&self, id: u32) -> Option<&LauncherImport> {
        self.imports.get(id as usize)
    }

    pub const fn crt_rng_seed(&self) -> u32 {
        self.crt_rng_seed
    }

    pub const fn d3d8_ref_count(&self) -> u32 {
        self.d3d8_ref_count
    }

    /// The compact compatibility D3D8 model exposes one stable guest object.
    /// Each Direct3DCreate8 success retains that object for its caller.
    pub fn retain_d3d8_object(&mut self) -> Result<u32, &'static str> {
        self.d3d8_ref_count = self
            .d3d8_ref_count
            .checked_add(1)
            .ok_or("D3D8 reference count overflow")?;
        Ok(self.d3d8_ref_count)
    }

    pub fn install_provider_surface(
        &mut self,
        imports: Vec<ProviderImport>,
        thunks: Vec<u8>,
        modules: Vec<ChildProvider>,
    ) {
        self.try_install_provider_surface(imports, thunks, modules)
            .expect("provider module namespace");
    }

    pub fn try_install_provider_surface(
        &mut self,
        imports: Vec<ProviderImport>,
        thunks: Vec<u8>,
        modules: Vec<ChildProvider>,
    ) -> Result<(), &'static str> {
        self.register_external_provider_modules(&modules)?;
        self.provider_imports = imports;
        self.provider_thunks = thunks;
        self.provider_modules = modules;
        Ok(())
    }

    pub fn register_native_module(
        &mut self,
        requested: &str,
        stored: &str,
        handle: u32,
    ) -> Result<(), &'static str> {
        let filename = self.native_module_filename(stored)?;
        self.register_loaded_module(
            requested,
            stored,
            handle,
            LoadedModuleKind::NativeImage,
            Some(filename),
        )
    }

    pub fn register_runtime_native_module(
        &mut self,
        requested: &str,
        handle: u32,
    ) -> Result<(), &'static str> {
        let stored = module_basename(requested).to_owned();
        self.register_loaded_module(
            requested,
            &stored,
            handle,
            LoadedModuleKind::NativeImage,
            Some(requested.to_owned()),
        )
    }

    fn register_external_provider_modules(
        &mut self,
        modules: &[ChildProvider],
    ) -> Result<(), &'static str> {
        for module in modules {
            let ChildProvider::External { module } = module else {
                continue;
            };
            self.register_external_provider_module(module)?;
        }
        Ok(())
    }

    fn register_external_provider_imports(
        &mut self,
        imports: &[ProviderImport],
    ) -> Result<(), &'static str> {
        for import in imports {
            self.register_external_provider_module(&import.module)?;
        }
        Ok(())
    }

    fn register_external_provider_module(&mut self, module: &str) -> Result<(), &'static str> {
        if let Some(loaded) = self
            .loaded_modules
            .iter()
            .find(|loaded| module_names_match(&loaded.requested_name, module))
        {
            return if loaded.kind == LoadedModuleKind::ExternalProvider {
                Ok(())
            } else {
                Err("loaded module name collision")
            };
        }
        let handle = self.next_provider_module_handle;
        self.next_provider_module_handle = self
            .next_provider_module_handle
            .checked_add(1)
            .ok_or("provider module handle overflow")?;
        self.register_loaded_module(
            module,
            module,
            handle,
            LoadedModuleKind::ExternalProvider,
            None,
        )
    }

    fn native_module_filename(&self, stored: &str) -> Result<String, &'static str> {
        let image = std::str::from_utf8(self.image.filename())
            .map_err(|_| "process image filename encoding")?
            .trim_end_matches('\0');
        let split = image
            .rfind(['\\', '/'])
            .ok_or("process image has no directory")?;
        let mut filename = image[..=split].to_owned();
        filename.push_str(stored);
        Ok(filename)
    }

    fn register_loaded_module(
        &mut self,
        requested: &str,
        stored: &str,
        handle: u32,
        kind: LoadedModuleKind,
        filename: Option<String>,
    ) -> Result<(), &'static str> {
        if handle == 0 {
            return Err("loaded module handle is zero");
        }
        for existing in &self.loaded_modules {
            let same_module = module_names_match(&existing.requested_name, requested)
                || module_names_match(&existing.requested_name, stored)
                || module_names_match(&existing.stored_name, requested)
                || module_names_match(&existing.stored_name, stored);
            if existing.handle == handle {
                if same_module && existing.kind == kind {
                    return Ok(());
                }
                return Err("loaded module handle collision");
            }
            if same_module {
                return Err("loaded module name collision");
            }
        }
        self.loaded_modules.push(LoadedModule {
            requested_name: requested.to_owned(),
            stored_name: stored.to_owned(),
            handle,
            kind,
            filename,
            thread_library_calls_disabled: false,
            load_count: 1,
        });
        Ok(())
    }

    pub fn disable_thread_library_calls(&mut self, handle: u32) -> Result<u32, &'static str> {
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        let Some(module) = self
            .loaded_modules
            .iter_mut()
            .find(|module| module.handle == handle)
        else {
            self.set_last_error(ERROR_INVALID_HANDLE);
            return Ok(0);
        };
        if module.kind != LoadedModuleKind::NativeImage {
            self.set_last_error(ERROR_INVALID_HANDLE);
            return Ok(0);
        }
        module.thread_library_calls_disabled = true;
        Ok(1)
    }

    pub fn thread_library_calls_disabled(&self, handle: u32) -> Option<bool> {
        self.loaded_modules
            .iter()
            .find(|module| module.handle == handle)
            .map(|module| module.thread_library_calls_disabled)
    }

    pub fn retain_loaded_module(&mut self, handle: u32) -> Result<u32, &'static str> {
        let module = self
            .loaded_modules
            .iter_mut()
            .find(|module| module.handle == handle)
            .ok_or("loaded module handle")?;
        module.load_count = module
            .load_count
            .checked_add(1)
            .ok_or("loaded module reference count overflow")?;
        Ok(module.load_count)
    }

    pub fn release_loaded_module(&mut self, handle: u32) -> Result<ModuleRelease, &'static str> {
        let index = self
            .loaded_modules
            .iter()
            .position(|module| module.handle == handle)
            .ok_or("loaded module handle")?;
        let module = &self.loaded_modules[index];
        if module.load_count == 0 {
            return Err("loaded module reference count underflow");
        }
        if module.load_count > 1 {
            self.loaded_modules[index].load_count -= 1;
            return Ok(ModuleRelease::Retained {
                remaining: self.loaded_modules[index].load_count,
            });
        }

        match module.kind {
            LoadedModuleKind::ExternalProvider => {
                let module = self.loaded_modules.remove(index);
                Ok(ModuleRelease::ExternalProviderUnloaded {
                    module: module.stored_name,
                })
            }
            LoadedModuleKind::NativeImage => Ok(ModuleRelease::NativeUnloadRequired {
                module: module.stored_name.clone(),
            }),
            LoadedModuleKind::MainImage => Err("cannot FreeLibrary main image"),
        }
    }

    pub fn provider_import(&self, id: u32) -> Option<&ProviderImport> {
        self.provider_imports.get(id as usize)
    }

    pub fn provider_modules(&self) -> &[ChildProvider] {
        &self.provider_modules
    }

    pub fn provider_import_count(&self) -> usize {
        self.provider_imports.len()
    }

    pub fn set_registry_handle_base(&mut self, base: u32) -> Result<(), &'static str> {
        if !self.registry_handles.is_empty() {
            return Err("registry handles already allocated");
        }
        self.next_registry_handle = base;
        Ok(())
    }

    pub fn open_registry_key(
        &mut self,
        node: crate::session::RegistryNodeId,
        access: u32,
    ) -> Result<u32, &'static str> {
        let handle = self.next_registry_handle;
        self.next_registry_handle = self
            .next_registry_handle
            .checked_add(1)
            .ok_or("registry handle overflow")?;
        if self
            .registry_handles
            .insert(handle, RegistryHandle { node, access })
            .is_some()
        {
            return Err("registry handle collision");
        }
        Ok(handle)
    }

    pub fn registry_handle_node(&self, handle: u32) -> Option<crate::session::RegistryNodeId> {
        self.registry_handles.get(&handle).map(|handle| handle.node)
    }

    pub fn install_war3_mpq(&mut self, bytes: Arc<Vec<u8>>) {
        self.war3_mpq_bytes = Some(bytes);
    }

    pub fn admit_trueos_file(
        &mut self,
        win_path: String,
        trueos_path: String,
        bytes: Arc<Vec<u8>>,
        desired_access: u32,
        share_mode: u32,
    ) -> Result<u32, &'static str> {
        let file_id = self.next_resident_file;
        self.next_resident_file = self
            .next_resident_file
            .checked_add(1)
            .ok_or("resident file overflow")?;
        let handle = self.next_file_handle;
        self.next_file_handle = self
            .next_file_handle
            .checked_add(1)
            .ok_or("file handle overflow")?;
        self.resident_files.insert(
            file_id,
            ResidentFile {
                win_path,
                trueos_path,
                bytes,
                attributes: FILE_ATTRIBUTE_NORMAL,
            },
        );
        self.file_handles.insert(
            handle,
            FileHandle {
                backing: FileBacking::TrueosFs(file_id),
                cursor: 0,
                access: desired_access,
                share: share_mode,
            },
        );
        Ok(handle)
    }

    pub fn close_registry_handle(&mut self, handle: u32) -> bool {
        self.registry_handles.remove(&handle).is_some()
    }

    pub fn has_critical_section(&self, address: u32) -> bool {
        self.critical_sections.contains_key(&address)
    }

    pub fn crt_malloc(&mut self, size: u32) -> Result<Option<CrtAllocation>, &'static str> {
        if size == 0 {
            return Err("CRT malloc zero-size unobserved");
        }
        let aligned = size.checked_add(7).ok_or("CRT malloc size overflow")? & !7;
        let pointer = CHILD_CRT_HEAP_BASE
            .checked_add(self.crt_heap_next)
            .ok_or("CRT malloc pointer overflow")?;
        let next = self
            .crt_heap_next
            .checked_add(aligned)
            .ok_or("CRT malloc heap overflow")?;
        let end = CHILD_CRT_HEAP_BASE
            .checked_add(next)
            .ok_or("CRT malloc heap overflow")?;
        if end > CHILD_CRT_HEAP_LIMIT {
            return Ok(None);
        }
        self.crt_allocations.insert(pointer, size);
        self.crt_heap_next = next;
        Ok(Some(CrtAllocation {
            pointer,
            requested: size,
            end,
        }))
    }

    /// Return the live allocation's eight-byte-aligned capacity, rather than
    /// its requested byte count.
    pub fn crt_allocation_capacity(&self, pointer: u32) -> Option<u32> {
        self.crt_allocations
            .get(&pointer)
            .and_then(|requested| requested.checked_add(7))
            .map(|size| size & !7)
    }

    pub fn virtual_reserve_null(
        &mut self,
        size: u32,
    ) -> Result<Option<VirtualReservation>, &'static str> {
        let rounded = size
            .checked_add(XP_ALLOCATION_GRANULARITY - 1)
            .ok_or("VirtualAlloc size overflow")?
            & !(XP_ALLOCATION_GRANULARITY - 1);
        if rounded == 0 {
            return Err("VirtualAlloc zero-size reservation");
        }
        let base = self.virtual_reserve_next;
        let end = base
            .checked_add(rounded)
            .ok_or("VirtualAlloc address overflow")?;
        if end > CHILD_VIRTUAL_ALLOC_LIMIT {
            return Ok(None);
        }
        let reservation = VirtualReservation {
            base,
            size: rounded,
            committed: Vec::new(),
        };
        self.virtual_reservations.push(reservation.clone());
        self.virtual_reserve_next = end;
        Ok(Some(reservation))
    }

    pub fn virtual_reservation_at(&self, base: u32) -> Option<&VirtualReservation> {
        self.virtual_reservations
            .iter()
            .find(|reservation| reservation.base == base)
    }

    pub fn virtual_reservation_containing(
        &self,
        address: u32,
        size: u32,
    ) -> Option<&VirtualReservation> {
        let end = address.checked_add(size)?;
        self.virtual_reservations.iter().find(|reservation| {
            let reservation_end = reservation.base.checked_add(reservation.size);
            address >= reservation.base && reservation_end.is_some_and(|limit| end <= limit)
        })
    }

    pub fn virtual_reservation_state(&self) -> (usize, u32) {
        (self.virtual_reservations.len(), self.virtual_reserve_next)
    }

    pub fn virtual_prepare_release(&self, address: u32, size: u32) -> Option<VirtualReservation> {
        if size != 0 {
            return None;
        }
        self.virtual_reservations
            .iter()
            .find(|reservation| reservation.base == address)
            .cloned()
    }

    pub fn virtual_prepare_null_commit(
        &self,
        requested: u32,
    ) -> Result<Option<VirtualNullCommitRequest>, &'static str> {
        if requested == 0 {
            return Err("VirtualAlloc zero-size null commit");
        }
        let size = requested
            .checked_add(XP_PAGE_SIZE - 1)
            .ok_or("VirtualAlloc null commit size overflow")?
            & !(XP_PAGE_SIZE - 1);
        let base = self.virtual_reserve_next;
        if base % XP_ALLOCATION_GRANULARITY != 0 {
            return Err("VirtualAlloc allocation cursor alignment");
        }
        let end = base
            .checked_add(size)
            .ok_or("VirtualAlloc null commit address overflow")?;
        if end > CHILD_VIRTUAL_ALLOC_LIMIT {
            return Ok(None);
        }
        let next_reserve = end
            .checked_add(XP_ALLOCATION_GRANULARITY - 1)
            .ok_or("VirtualAlloc next allocation overflow")?
            & !(XP_ALLOCATION_GRANULARITY - 1);
        if next_reserve > CHILD_VIRTUAL_ALLOC_LIMIT {
            return Ok(None);
        }
        Ok(Some(VirtualNullCommitRequest {
            base,
            size,
            next_reserve,
        }))
    }

    pub fn virtual_finish_null_commit(
        &mut self,
        request: VirtualNullCommitRequest,
    ) -> Result<(), &'static str> {
        if self.virtual_reserve_next != request.base {
            return Err("VirtualAlloc allocation cursor changed");
        }
        self.virtual_reservations.push(VirtualReservation {
            base: request.base,
            size: request.size,
            committed: vec![VirtualCommit {
                base: request.base,
                size: request.size,
            }],
        });
        self.virtual_reserve_next = request.next_reserve;
        Ok(())
    }

    pub fn virtual_finish_release(&mut self, base: u32, size: u32) -> Result<(), &'static str> {
        let index = self
            .virtual_reservations
            .iter()
            .position(|reservation| reservation.base == base && reservation.size == size)
            .ok_or("VirtualFree reservation disappeared")?;
        self.virtual_reservations.remove(index);
        Ok(())
    }

    pub fn virtual_prepare_commit(
        &self,
        address: u32,
        size: u32,
    ) -> Result<Option<VirtualCommitRequest>, &'static str> {
        if address == 0 || size == 0 || address % XP_PAGE_SIZE != 0 {
            return Ok(None);
        }
        let rounded = size
            .checked_add(XP_PAGE_SIZE - 1)
            .ok_or("VirtualAlloc commit size overflow")?
            & !(XP_PAGE_SIZE - 1);
        let end = address
            .checked_add(rounded)
            .ok_or("VirtualAlloc commit address overflow")?;
        let Some(reservation) = self.virtual_reservations.iter().find(|reservation| {
            reservation
                .base
                .checked_add(reservation.size)
                .is_some_and(|reservation_end| {
                    address >= reservation.base && end <= reservation_end
                })
        }) else {
            return Ok(None);
        };
        if reservation.committed.iter().any(|commit| {
            commit
                .base
                .checked_add(commit.size)
                .is_some_and(|commit_end| address < commit_end && commit.base < end)
        }) {
            return Err("VirtualAlloc overlapping commit");
        }
        Ok(Some(VirtualCommitRequest {
            reservation_base: reservation.base,
            reservation_size: reservation.size,
            base: address,
            size: rounded,
        }))
    }

    pub fn virtual_finish_commit(
        &mut self,
        request: VirtualCommitRequest,
    ) -> Result<(), &'static str> {
        let reservation = self
            .virtual_reservations
            .iter_mut()
            .find(|reservation| {
                reservation.base == request.reservation_base
                    && reservation.size == request.reservation_size
            })
            .ok_or("VirtualAlloc reservation disappeared")?;
        reservation.committed.push(VirtualCommit {
            base: request.base,
            size: request.size,
        });
        Ok(())
    }

    pub fn virtual_commit_state(&self) -> (usize, u32) {
        let mut ranges = 0usize;
        let mut bytes = 0u32;
        for reservation in &self.virtual_reservations {
            ranges += reservation.committed.len();
            for commit in &reservation.committed {
                bytes = bytes.saturating_add(commit.size);
            }
        }
        (ranges, bytes)
    }

    /// Reserve a replacement CRT block for an internal CRT table resize.  The
    /// caller copies guest bytes and commits its pointers before retiring the
    /// old logical allocation with `retire_crt_allocation`.
    pub fn crt_resize(
        &mut self,
        old_pointer: u32,
        used_bytes: u32,
        required_bytes: u32,
    ) -> Result<Option<CrtResize>, &'static str> {
        if required_bytes == 0 || required_bytes < used_bytes {
            return Err("invalid CRT resize size");
        }
        let allocation = match self.crt_malloc(required_bytes)? {
            Some(allocation) => allocation,
            None => return Ok(None),
        };
        Ok(Some(CrtResize {
            old_pointer,
            pointer: allocation.pointer,
            used_bytes,
            required_bytes,
            end: allocation.end,
            moved: allocation.pointer != old_pointer,
        }))
    }

    /// Grow an internal callback vector with spare capacity. The logical end
    /// remains the caller's used length; it must copy only live entries and
    /// retire the old block only after committing the guest pointers.
    pub fn crt_grow_callback_table(
        &mut self,
        pointer: u32,
        used_bytes: u32,
        required_bytes: u32,
    ) -> Result<Option<CrtResize>, &'static str> {
        let capacity = self.crt_allocation_capacity(pointer).ok_or("untracked CRT callback table")?;
        if used_bytes > capacity || required_bytes <= capacity || required_bytes < used_bytes {
            return Err("invalid CRT callback table growth");
        }
        let reserve = capacity.checked_mul(2).unwrap_or(required_bytes).max(64).max(required_bytes);
        if let Some(allocation) = self.crt_resize(pointer, used_bytes, reserve)? {
            return Ok(Some(allocation));
        }
        // Spare capacity must never turn an otherwise valid append into OOM.
        if reserve != required_bytes {
            return self.crt_resize(pointer, used_bytes, required_bytes);
        }
        Ok(None)
    }

    pub fn retire_crt_allocation(&mut self, pointer: u32) -> bool {
        self.crt_allocations.remove(&pointer).is_some()
    }

    fn allocate_and_initialize_sid(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let frame = arguments::<12>(memory, esp)?;
        let authority = frame[1];
        let count = usize::try_from(frame[2])
            .map_err(|_| ProviderDispatchError::Fault("AllocateAndInitializeSid count"))?;
        if count > ALLOCATE_AND_INITIALIZE_SID_MAX_SUB_AUTHORITIES {
            return Err(ProviderDispatchError::Frontier {
                api: "AllocateAndInitializeSid",
                detail: format!("unobserved subauthority count={count}"),
            });
        }

        let mut sid = Vec::with_capacity(SID_HEADER_BYTES + count * 4);
        sid.extend_from_slice(&[1, count as u8]);
        let mut authority_bytes = [0; 6];
        memory.read(authority, &mut authority_bytes)?;
        sid.extend_from_slice(&authority_bytes);
        for subauthority in &frame[3..3 + count] {
            sid.extend_from_slice(&subauthority.to_le_bytes());
        }

        let length = u32::try_from(sid.len())
            .map_err(|_| ProviderDispatchError::Fault("AllocateAndInitializeSid length"))?;
        let aligned_length = length.checked_add(3).ok_or(ProviderDispatchError::Fault(
            "AllocateAndInitializeSid length",
        ))? & !3;
        let pointer = self.next_sid;
        let next = pointer
            .checked_add(aligned_length)
            .ok_or(ProviderDispatchError::Fault(
                "AllocateAndInitializeSid arena overflow",
            ))?;
        if next > PROCESS_SID_ARENA_LIMIT {
            self.set_last_error(ERROR_NOT_ENOUGH_MEMORY);
            return Ok(0);
        }

        memory.write(pointer, &sid)?;
        memory.write(frame[11], &pointer.to_le_bytes())?;
        self.sid_allocations.insert(pointer, length);
        self.next_sid = next;
        Ok(1)
    }

    fn equal_sid(&self, esp: u32, memory: &impl GuestMemory) -> Result<u32, ProviderDispatchError> {
        let [_, first, second] = arguments::<3>(memory, esp)?;
        Ok(u32::from(
            canonical_sid(memory, first)? == canonical_sid(memory, second)?,
        ))
    }

    fn file_handle(&self, handle: u32) -> Result<FileHandle, ProviderDispatchError> {
        self.file_handles
            .get(&handle)
            .copied()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "file handle",
                detail: format!("unmodeled handle=0x{handle:08x}"),
            })
    }

    fn create_scratch_file(
        &mut self,
        path: String,
        attributes: u32,
    ) -> Result<u32, ProviderDispatchError> {
        let id = self.next_scratch_file;
        self.next_scratch_file = self
            .next_scratch_file
            .checked_add(1)
            .ok_or("scratch file overflow")?;
        self.scratch_paths.insert(path.clone(), id);
        self.scratch_files.insert(
            id,
            ScratchFile {
                path,
                bytes: Vec::new(),
                attributes,
            },
        );
        Ok(id)
    }

    fn file_length(
        &self,
        file: FileHandle,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<u64, ProviderDispatchError> {
        match file.backing {
            FileBacking::SelfImage => {
                u64::try_from(Self::self_image_bytes(self_image_bytes)?.len())
                    .map_err(|_| ProviderDispatchError::Fault("self image length"))
            }
            FileBacking::War3Mpq => self
                .war3_mpq_bytes
                .as_ref()
                .map(|bytes| bytes.len() as u64)
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "War3.mpq",
                    detail: "resident backing unavailable".into(),
                }),
            FileBacking::TrueosFs(id) => self
                .resident_files
                .get(&id)
                .map(|file| file.bytes.len() as u64)
                .ok_or(ProviderDispatchError::Fault("resident file disappeared")),
            FileBacking::Scratch(id) => self
                .scratch_files
                .get(&id)
                .map(|scratch| scratch.bytes.len() as u64)
                .ok_or(ProviderDispatchError::Fault("scratch file disappeared")),
        }
    }

    fn self_image_bytes<'a>(
        self_image_bytes: Option<&'a [u8]>,
    ) -> Result<&'a [u8], ProviderDispatchError> {
        self_image_bytes.ok_or_else(|| ProviderDispatchError::Frontier {
            api: "self image file",
            detail: "backing unavailable outside the child runtime".into(),
        })
    }

    fn interlocked_exchange(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, target, value] = arguments::<3>(memory, esp)?;
        if target == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "InterlockedExchange",
                detail: "target=NULL".into(),
            });
        }
        if target & 3 != 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "InterlockedExchange",
                detail: format!("unaligned target=0x{target:08x} value=0x{value:08x}"),
            });
        }
        let previous = read_u32(memory, target)?;
        write_u32(memory, target, value)?;
        Ok(previous)
    }

    fn interlocked_add(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
        delta: u32,
        api: &'static str,
    ) -> Result<(u32, u32, u32), ProviderDispatchError> {
        let [_, target] = arguments::<2>(memory, esp)?;
        if target == 0 {
            return Err(ProviderDispatchError::Frontier {
                api,
                detail: "target=NULL".into(),
            });
        }
        if target & 3 != 0 {
            return Err(ProviderDispatchError::Frontier {
                api,
                detail: format!("unaligned target=0x{target:08x}"),
            });
        }
        let before = read_u32(memory, target)?;
        let after = before.wrapping_add(delta);
        write_u32(memory, target, after)?;
        Ok((target, before, after))
    }

    fn interlocked_increment(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let (_, _, after) = self.interlocked_add(esp, memory, 1, "InterlockedIncrement")?;
        Ok(after)
    }

    fn interlocked_decrement(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let (_, _, after) = self.interlocked_add(esp, memory, u32::MAX, "InterlockedDecrement")?;
        Ok(after)
    }

    fn get_system_info(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        let mut info = [0u8; 36];
        info[0..2].copy_from_slice(&PROCESSOR_ARCHITECTURE_INTEL.to_le_bytes());
        info[2..4].copy_from_slice(&0u16.to_le_bytes());
        info[4..8].copy_from_slice(&XP_PAGE_SIZE.to_le_bytes());
        info[8..12].copy_from_slice(&XP_MIN_APPLICATION_ADDRESS.to_le_bytes());
        info[12..16].copy_from_slice(&XP_MAX_APPLICATION_ADDRESS.to_le_bytes());
        info[16..20].copy_from_slice(&1u32.to_le_bytes());
        info[20..24].copy_from_slice(&1u32.to_le_bytes());
        info[24..28].copy_from_slice(&PROCESSOR_INTEL_PENTIUM.to_le_bytes());
        info[28..32].copy_from_slice(&XP_ALLOCATION_GRANULARITY.to_le_bytes());
        info[32..34].copy_from_slice(&XP_PROCESSOR_LEVEL.to_le_bytes());
        info[34..36].copy_from_slice(&0u16.to_le_bytes());
        memory.write(output, &info)?;
        Ok(0)
    }

    fn global_memory_status(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, status] = arguments::<2>(memory, esp)?;
        if status == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "GlobalMemoryStatus",
                detail: "lpBuffer=NULL".into(),
            });
        }

        let values = [
            32u32,
            XP_MEMORY_LOAD,
            XP_TOTAL_PHYS,
            XP_AVAIL_PHYS,
            XP_TOTAL_PAGEFILE,
            XP_AVAIL_PAGEFILE,
            XP_TOTAL_VIRTUAL,
            XP_AVAIL_VIRTUAL,
        ];
        let mut bytes = [0u8; 32];
        for (index, value) in values.into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        memory.write(status, &bytes)?;
        Ok(0)
    }

    fn d3d8_get_adapter_identifier(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, this, adapter, flags, output] = arguments::<5>(memory, esp)?;
        if this != thunk32::CHILD_D3D8_OBJECT_ADDRESS {
            return Err(ProviderDispatchError::Frontier {
                api: "IDirect3D8::GetAdapterIdentifier",
                detail: format!("this=0x{this:08x}"),
            });
        }
        if output == 0 {
            return Ok(D3DERR_INVALIDCALL);
        }
        if adapter != 0 {
            memory.write(output, &[0; D3DADAPTER_IDENTIFIER8_BYTES])?;
            return Ok(D3DERR_INVALIDCALL);
        }
        if flags & !D3DENUM_NO_WHQL_LEVEL != 0 {
            return Ok(D3DERR_INVALIDCALL);
        }

        let mut identifier = [0u8; D3DADAPTER_IDENTIFIER8_BYTES];
        identifier[0..12].copy_from_slice(b"trueos-d3d8\0");
        let description = TRUEOS_DISPLAY_ADAPTER_DESCRIPTION.as_bytes();
        identifier[0x200..0x200 + description.len()].copy_from_slice(description);
        identifier[0x408..0x40c].copy_from_slice(&TRUEOS_D3D8_VENDOR_ID.to_le_bytes());
        identifier[0x40c..0x410].copy_from_slice(&TRUEOS_D3D8_DEVICE_ID.to_le_bytes());
        identifier[0x410..0x414].copy_from_slice(&TRUEOS_D3D8_SUBSYSTEM_ID.to_le_bytes());
        identifier[0x414..0x418].copy_from_slice(&TRUEOS_D3D8_REVISION.to_le_bytes());
        identifier[0x418..0x428].copy_from_slice(&TRUEOS_D3D8_ADAPTER_GUID);
        memory.write(output, &identifier)?;
        Ok(D3D_OK)
    }

    fn enum_display_devices_a(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const DISPLAY_DEVICEA_BYTES: u32 = 0x1a8;
        const DISPLAY_DEVICE_ATTACHED_TO_DESKTOP: u32 = 0x0000_0001;
        const DISPLAY_DEVICE_PRIMARY_DEVICE: u32 = 0x0000_0004;
        const DISPLAY_DEVICE_ACTIVE: u32 = 0x0000_0001;

        let [_, device_ptr, index, output, flags] = arguments::<5>(memory, esp)?;
        if output == 0 {
            return Ok(0);
        }
        let cb = read_u32(memory, output)?;
        if cb != DISPLAY_DEVICEA_BYTES {
            return Err(ProviderDispatchError::Frontier {
                api: "EnumDisplayDevicesA",
                detail: format!("unexpected cb={cb}"),
            });
        }
        if flags != 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "EnumDisplayDevicesA",
                detail: format!("unobserved flags=0x{flags:08x}"),
            });
        }

        let mut record = [0u8; DISPLAY_DEVICEA_BYTES as usize];
        record[..4].copy_from_slice(&DISPLAY_DEVICEA_BYTES.to_le_bytes());
        if device_ptr == 0 {
            if index != 0 {
                return Ok(0);
            }
            write_fixed_ansi(&mut record, 0x04, 32, r"\\.\DISPLAY1");
            write_fixed_ansi(
                &mut record,
                0x24,
                128,
                TRUEOS_DISPLAY_ADAPTER_DESCRIPTION,
            );
            record[0xa4..0xa8].copy_from_slice(
                &(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP | DISPLAY_DEVICE_PRIMARY_DEVICE).to_le_bytes(),
            );
        } else {
            let device = read_c_string(memory, device_ptr, 32)?;
            if !device.eq_ignore_ascii_case(r"\\.\DISPLAY1") || index != 0 {
                return Ok(0);
            }
            write_fixed_ansi(&mut record, 0x04, 32, r"\\.\DISPLAY1\Monitor0");
            write_fixed_ansi(&mut record, 0x24, 128, "TRUEOS UI4 Display");
            record[0xa4..0xa8].copy_from_slice(&DISPLAY_DEVICE_ACTIVE.to_le_bytes());
        }
        memory.write(output, &record)?;
        Ok(1)
    }

    fn enum_display_settings_a(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, device_ptr, mode_num, output] = arguments::<4>(memory, esp)?;
        if output == 0 {
            return Ok(0);
        }
        if device_ptr != 0
            && !read_c_string(memory, device_ptr, 32)?.eq_ignore_ascii_case(r"\\.\DISPLAY1")
        {
            return Ok(0);
        }

        let dm_size = read_u16(memory, output + 0x24)?;
        if dm_size < 0x7c {
            return Err(ProviderDispatchError::Frontier {
                api: "EnumDisplaySettingsA",
                detail: format!("unexpected dmSize={dm_size}"),
            });
        }
        if !matches!(mode_num, ENUM_CURRENT_SETTINGS | ENUM_REGISTRY_SETTINGS | 0) {
            return Ok(0);
        }

        let (width, height) = self.desktop_size();
        let mut mode = vec![0u8; dm_size as usize];
        mode[0x24..0x26].copy_from_slice(&dm_size.to_le_bytes());
        mode[0x28..0x2c].copy_from_slice(&TRUEOS_DISPLAY_FIELDS.to_le_bytes());
        mode[0x68..0x6c].copy_from_slice(&32u32.to_le_bytes());
        mode[0x6c..0x70].copy_from_slice(&width.to_le_bytes());
        mode[0x70..0x74].copy_from_slice(&height.to_le_bytes());
        mode[0x78..0x7c].copy_from_slice(&60u32.to_le_bytes());
        memory.write(output, &mode)?;
        Ok(1)
    }

    fn change_display_settings_ex_a(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const CDS_FULLSCREEN: u32 = 0x0000_0004;
        const DISP_CHANGE_SUCCESSFUL: u32 = 0;

        let [_, device_ptr, devmode, hwnd, flags, lparam] = arguments::<6>(memory, esp)?;
        if device_ptr == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "ChangeDisplaySettingsExA",
                detail: "null device unobserved".into(),
            });
        }
        let device = read_c_string(memory, device_ptr, 32)?;
        if !device.eq_ignore_ascii_case(r"\\.\DISPLAY1")
            || hwnd != 0
            || flags != CDS_FULLSCREEN
            || lparam != 0
            || devmode == 0
        {
            return Err(ProviderDispatchError::Frontier {
                api: "ChangeDisplaySettingsExA",
                detail: format!(
                    "device={device:?} devmode=0x{devmode:08x} hwnd=0x{hwnd:08x} flags=0x{flags:08x} lparam=0x{lparam:08x}"
                ),
            });
        }

        let dm_size = read_u16(memory, devmode + 0x24)?;
        let dm_extra = read_u16(memory, devmode + 0x26)?;
        let dm_fields = read_u32(memory, devmode + 0x28)?;
        let bpp = read_u32(memory, devmode + 0x68)?;
        let width = read_u32(memory, devmode + 0x6c)?;
        let height = read_u32(memory, devmode + 0x70)?;
        let display_flags = read_u32(memory, devmode + 0x74)?;
        let frequency = read_u32(memory, devmode + 0x78)?;
        let (current_width, current_height) = self.desktop_size();

        if dm_size != 156
            || dm_extra != 0
            || dm_fields != TRUEOS_DISPLAY_FIELDS
            || bpp != 32
            || width != current_width
            || height != current_height
            || display_flags != 0
            || frequency != 60
        {
            return Err(ProviderDispatchError::Frontier {
                api: "ChangeDisplaySettingsExA",
                detail: format!(
                    "unobserved mode size={dm_size} extra={dm_extra} fields=0x{dm_fields:08x} {width}x{height}x{bpp}@{frequency} display_flags=0x{display_flags:08x}"
                ),
            });
        }

        Ok(DISP_CHANGE_SUCCESSFUL)
    }

    fn d3d8_release(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, this] = arguments::<2>(memory, esp)?;
        if this != thunk32::CHILD_D3D8_OBJECT_ADDRESS {
            return Err(ProviderDispatchError::Frontier {
                api: "IDirect3D8::Release",
                detail: format!("this=0x{this:08x}"),
            });
        }
        if self.d3d8_ref_count == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "IDirect3D8::Release",
                detail: "release with zero references".into(),
            });
        }

        self.d3d8_ref_count -= 1;
        Ok(self.d3d8_ref_count)
    }

}

include!("process_provider_dispatch.rs");
include!("process_runtime.rs");
include!("staticgdi.rs");
include!("staticgl.rs");


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapDiagnostic {
    pub resource_id: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub size_image: u32,
    pub clr_used: u32,
    pub dib_bytes: usize,
    pub rgba: Vec<u8>,
    pub bits_va: u32,
    pub bits_len: usize,
    pub row_stride: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DibLayout {
    pub pixel_offset: usize,
    pub row_stride: usize,
    pub bits_len: usize,
    pub palette: Vec<[u8; 4]>,
}

pub fn dib_layout(dib: &[u8]) -> Result<DibLayout, &'static str> {
    if dib.len() < 40 {
        return Err("bitmap resource header truncated");
    }
    let header_size = u32::from_le_bytes(dib[0..4].try_into().unwrap()) as usize;
    if header_size < 40 || header_size > dib.len() {
        return Err("bitmap header unsupported");
    }
    let width = i32::from_le_bytes(dib[4..8].try_into().unwrap());
    let height = i32::from_le_bytes(dib[8..12].try_into().unwrap());
    let planes = u16::from_le_bytes(dib[12..14].try_into().unwrap());
    let bits = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
    let clr_used = u32::from_le_bytes(dib[32..36].try_into().unwrap());
    if width <= 0 || height == 0 || planes != 1 || compression != 0 {
        return Err("bitmap layout unsupported");
    }
    if !matches!(bits, 8 | 24 | 32) {
        return Err("bitmap depth unsupported");
    }
    let palette_count = if bits == 8 {
        if clr_used == 0 {
            256
        } else {
            clr_used as usize
        }
    } else {
        0
    };
    if palette_count > 256 {
        return Err("bitmap palette too large");
    }
    let palette_start = header_size;
    let palette_end = palette_start
        .checked_add(
            palette_count
                .checked_mul(4)
                .ok_or("bitmap palette overflow")?,
        )
        .ok_or("bitmap palette overflow")?;
    if palette_end > dib.len() {
        return Err("bitmap palette outside resource");
    }
    let mut palette = Vec::with_capacity(palette_count);
    for index in 0..palette_count {
        let offset = palette_start + index * 4;
        palette.push([
            dib[offset],
            dib[offset + 1],
            dib[offset + 2],
            dib[offset + 3],
        ]);
    }
    let bytes_per_pixel = if bits == 8 { 1 } else { usize::from(bits / 8) };
    let row_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or("bitmap row overflow")?;
    let row_stride = row_bytes.checked_add(3).ok_or("bitmap stride overflow")? & !3;
    let bits_len = row_stride
        .checked_mul(height.unsigned_abs() as usize)
        .ok_or("bitmap bits overflow")?;
    if palette_end
        .checked_add(bits_len)
        .ok_or("bitmap bits overflow")?
        > dib.len()
    {
        return Err("bitmap bits outside resource");
    }
    Ok(DibLayout {
        pixel_offset: palette_end,
        row_stride,
        bits_len,
        palette,
    })
}

/// Adapt a Windows RT_BITMAP DIB resource to the production BMP decoder.
/// The DIB bytes are appended byte-for-byte; only the missing file header is
/// synthesized. `bfOffBits` is derived from the DIB's header, masks, and
/// palette rather than assuming a 40-byte header has no trailing metadata.
pub fn bmp_file_from_dib(dib: &[u8]) -> Result<Vec<u8>, &'static str> {
    if dib.len() < 40 {
        return Err("bitmap resource header truncated");
    }
    let header_size = u32::from_le_bytes(dib[0..4].try_into().unwrap());
    if header_size < 40 || header_size as usize > dib.len() {
        return Err("bitmap header unsupported");
    }
    let planes = u16::from_le_bytes(dib[12..14].try_into().unwrap());
    let bit_count = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
    if planes != 1 {
        return Err("bitmap planes invalid");
    }
    let clr_used = u32::from_le_bytes(dib[32..36].try_into().unwrap());
    let masks = if header_size == 40 {
        match compression {
            3 => 12usize,
            6 => 16usize,
            _ => 0,
        }
    } else {
        0
    };
    let palette_entries = if bit_count <= 8 {
        if clr_used != 0 {
            clr_used as usize
        } else {
            1usize
                .checked_shl(bit_count as u32)
                .ok_or("bitmap palette overflow")?
        }
    } else {
        0
    };
    let pixel_offset = (header_size as usize)
        .checked_add(masks)
        .and_then(|value| value.checked_add(palette_entries.checked_mul(4)?))
        .ok_or("bitmap pixel offset overflow")?;
    if pixel_offset > dib.len() {
        return Err("bitmap pixel data outside resource");
    }
    let file_size = 14usize
        .checked_add(dib.len())
        .ok_or("bitmap file size overflow")?;
    let file_size_u32 = u32::try_from(file_size).map_err(|_| "bitmap file too large")?;
    let offset_u32 =
        u32::try_from(14usize + pixel_offset).map_err(|_| "bitmap offset too large")?;
    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size_u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&offset_u32.to_le_bytes());
    bmp.extend_from_slice(dib);
    Ok(bmp)
}

fn read_u16(memory: &impl GuestMemory, address: u32) -> Result<u16, &'static str> {
    let mut b = [0; 2];
    memory.read(address, &mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_byte(memory: &impl GuestMemory, address: u32) -> Result<u8, &'static str> {
    let mut byte = [0];
    memory.read(address, &mut byte)?;
    Ok(byte[0])
}

fn read_i32(memory: &impl GuestMemory, address: u32) -> Result<i32, &'static str> {
    Ok(read_u32(memory, address)? as i32)
}

struct ResourceData {
    address: u32,
    size: u32,
}

enum ResourceKey<'a> {
    Id(u32),
    Name(&'a str),
}

fn resource_name_matches(stored: &str, requested: &str) -> bool {
    stored.eq_ignore_ascii_case(requested)
}

fn numeric_resource(
    memory: &impl GuestMemory,
    module_base: u32,
    type_id: u32,
    resource_id: u32,
) -> Result<ResourceData, &'static str> {
    resource_data(
        memory,
        module_base,
        ResourceKey::Id(type_id),
        ResourceKey::Id(resource_id),
    )
}

fn resource_data(
    memory: &impl GuestMemory,
    module_base: u32,
    type_key: ResourceKey<'_>,
    resource_key: ResourceKey<'_>,
) -> Result<ResourceData, &'static str> {
    let pe = read_u32(memory, module_base + 0x3c)?;
    let optional = module_base
        .checked_add(pe)
        .and_then(|value| value.checked_add(24))
        .ok_or("resource optional offset")?;
    let root_rva = read_u32(memory, optional + 96 + 16)?;
    let resource_size = read_u32(memory, optional + 96 + 20)?;
    if root_rva == 0 || resource_size < 16 {
        return Err("resource directory range");
    }
    let root = module_base.checked_add(root_rva).ok_or("resource root")?;
    let type_directory = resource_directory_entry(memory, root, root, type_key)?;
    let resource_directory = resource_directory_entry(memory, root, type_directory, resource_key)?;
    let data_entry = resource_first_language_data(memory, root, resource_directory)?;
    let data_rva = read_u32(memory, data_entry)?;
    let data_size = read_u32(memory, data_entry + 4)?;
    Ok(ResourceData {
        address: module_base.checked_add(data_rva).ok_or("resource data")?,
        size: data_size,
    })
}

fn resource_directory_entry(
    memory: &impl GuestMemory,
    root: u32,
    directory: u32,
    wanted: ResourceKey<'_>,
) -> Result<u32, &'static str> {
    let named = u32::from(read_u16(memory, directory + 12)?);
    let ids = u32::from(read_u16(memory, directory + 14)?);
    let entries = directory.checked_add(16).ok_or("resource entries overflow")?;
    let count = match wanted {
        ResourceKey::Id(_) => ids,
        ResourceKey::Name(_) => named,
    };
    let start = match wanted {
        ResourceKey::Id(_) => named,
        ResourceKey::Name(_) => 0,
    };
    for index in 0..count {
        let entry = entries
            .checked_add((start + index).checked_mul(8).ok_or("resource entry overflow")?)
            .ok_or("resource entry overflow")?;
        let key = read_u32(memory, entry)?;
        let matches = match wanted {
            ResourceKey::Id(id) => key == id,
            ResourceKey::Name(name) => {
                if key & 0x8000_0000 == 0 {
                    false
                } else {
                    let address = root.checked_add(key & 0x7fff_ffff).ok_or("resource name overflow")?;
                    let length = usize::from(read_u16(memory, address)?);
                    let mut units = Vec::with_capacity(length);
                    for offset in 0..length {
                        units.push(read_u16(memory, address + 2 + (offset as u32) * 2)?);
                    }
                    resource_name_matches(
                        &String::from_utf16(&units).map_err(|_| "resource name UTF-16")?,
                        name,
                    )
                }
            }
        };
        if !matches {
            continue;
        }
        let child = read_u32(memory, entry + 4)?;
        if child & 0x8000_0000 == 0 {
            return Err("resource entry is not a directory");
        }
        return root.checked_add(child & 0x7fff_ffff).ok_or("resource directory overflow");
    }
    Err("resource key not found")
}
fn resource_first_language_data(
    memory: &impl GuestMemory,
    root: u32,
    directory: u32,
) -> Result<u32, &'static str> {
    let named = u32::from(read_u16(memory, directory + 12)?);
    let ids = u32::from(read_u16(memory, directory + 14)?);
    if ids == 0 {
        return Err("resource language missing");
    }
    let entry = directory
        .checked_add(16 + named * 8)
        .ok_or("resource language overflow")?;
    let child = read_u32(memory, entry + 4)?;
    if child & 0x8000_0000 != 0 {
        return Err("resource language is a directory");
    }
    root.checked_add(child).ok_or("resource data overflow")
}

enum MessageTableLookup {
    TypeMissing,
    LanguageMissing,
    MessageMissing,
    Fault(&'static str),
}

fn resource_language_data(
    memory: &impl GuestMemory,
    root: u32,
    directory: u32,
    language_id: u32,
) -> Result<u32, &'static str> {
    let named = u32::from(read_u16(memory, directory + 12)?);
    let ids = u32::from(read_u16(memory, directory + 14)?);
    let entries = directory
        .checked_add(16)
        .and_then(|value| value.checked_add(named.checked_mul(8)?))
        .ok_or("resource language entries overflow")?;
    for index in 0..ids {
        let entry = entries
            .checked_add(
                index
                    .checked_mul(8)
                    .ok_or("resource language entry overflow")?,
            )
            .ok_or("resource language entry overflow")?;
        if read_u32(memory, entry)? != language_id {
            continue;
        }
        let child = read_u32(memory, entry + 4)?;
        if child & 0x8000_0000 != 0 {
            return Err("resource language is a directory");
        }
        return root
            .checked_add(child)
            .ok_or("resource language data overflow");
    }
    Err("resource language not found")
}

fn message_table_text(
    memory: &impl GuestMemory,
    module_base: u32,
    language_id: u32,
    message_id: u32,
) -> Result<(Vec<u8>, MessageResourceEncoding), MessageTableLookup> {
    let pe = read_u32(memory, module_base + 0x3c).map_err(MessageTableLookup::Fault)?;
    let optional = module_base
        .checked_add(pe)
        .and_then(|value| value.checked_add(24))
        .ok_or(MessageTableLookup::Fault("message table optional offset"))?;
    let root_rva = read_u32(memory, optional + 96 + 16).map_err(MessageTableLookup::Fault)?;
    let resource_size = read_u32(memory, optional + 96 + 20).map_err(MessageTableLookup::Fault)?;
    if root_rva == 0 || resource_size < 16 {
        return Err(MessageTableLookup::TypeMissing);
    }
    let root = module_base
        .checked_add(root_rva)
        .ok_or(MessageTableLookup::Fault("message table root"))?;
    let type_directory = match resource_directory_entry(memory, root, root, ResourceKey::Id(RT_MESSAGETABLE)) {
        Ok(directory) => directory,
        Err("resource id not found") => return Err(MessageTableLookup::TypeMissing),
        Err(error) => return Err(MessageTableLookup::Fault(error)),
    };
    let named =
        u32::from(read_u16(memory, type_directory + 12).map_err(MessageTableLookup::Fault)?);
    let ids = u32::from(read_u16(memory, type_directory + 14).map_err(MessageTableLookup::Fault)?);
    let entries = type_directory
        .checked_add(16)
        .and_then(|value| value.checked_add(named.checked_mul(8)?))
        .ok_or(MessageTableLookup::Fault("message table entries overflow"))?;
    let mut found_language = false;
    for index in 0..ids {
        let entry = entries
            .checked_add(
                index
                    .checked_mul(8)
                    .ok_or(MessageTableLookup::Fault("message table entry overflow"))?,
            )
            .ok_or(MessageTableLookup::Fault("message table entry overflow"))?;
        let resource_directory = read_u32(memory, entry + 4).map_err(MessageTableLookup::Fault)?;
        if resource_directory & 0x8000_0000 == 0 {
            return Err(MessageTableLookup::Fault("message table resource is data"));
        }
        let resource_directory =
            root.checked_add(resource_directory & 0x7fff_ffff)
                .ok_or(MessageTableLookup::Fault(
                    "message table resource directory",
                ))?;
        let data_entry = match resource_language_data(memory, root, resource_directory, language_id)
        {
            Ok(data) => data,
            Err("resource language not found") => continue,
            Err(error) => return Err(MessageTableLookup::Fault(error)),
        };
        found_language = true;
        let data_rva = read_u32(memory, data_entry).map_err(MessageTableLookup::Fault)?;
        let data_size = read_u32(memory, data_entry + 4).map_err(MessageTableLookup::Fault)?;
        let data = module_base
            .checked_add(data_rva)
            .ok_or(MessageTableLookup::Fault("message table data"))?;
        if let Some(value) = message_table_entry(memory, data, data_size, message_id)
            .map_err(MessageTableLookup::Fault)?
        {
            return Ok(value);
        }
    }
    if found_language {
        Err(MessageTableLookup::MessageMissing)
    } else {
        Err(MessageTableLookup::LanguageMissing)
    }
}

fn message_table_entry(
    memory: &impl GuestMemory,
    data: u32,
    data_size: u32,
    message_id: u32,
) -> Result<Option<(Vec<u8>, MessageResourceEncoding)>, &'static str> {
    if data_size < 4 {
        return Err("message table header truncated");
    }
    let blocks = read_u32(memory, data)?;
    let block_bytes = blocks
        .checked_mul(12)
        .ok_or("message table blocks overflow")?;
    if 4u32
        .checked_add(block_bytes)
        .ok_or("message table blocks overflow")?
        > data_size
    {
        return Err("message table blocks truncated");
    }
    for block in 0..blocks {
        let address = data + 4 + block * 12;
        let low = read_u32(memory, address)?;
        let high = read_u32(memory, address + 4)?;
        let offset = read_u32(memory, address + 8)?;
        if low > high || message_id < low || message_id > high || offset >= data_size {
            continue;
        }
        let mut entry = data.checked_add(offset).ok_or("message entry offset")?;
        for _ in low..message_id {
            let relative = entry.checked_sub(data).ok_or("message entry range")?;
            if relative.checked_add(4).ok_or("message entry range")? > data_size {
                return Err("message entry truncated");
            }
            let length = u32::from(read_u16(memory, entry)?);
            if length < 4 || relative.checked_add(length).ok_or("message entry range")? > data_size
            {
                return Err("message entry length");
            }
            entry = entry.checked_add(length).ok_or("message entry overflow")?;
        }
        let relative = entry.checked_sub(data).ok_or("message entry range")?;
        if relative.checked_add(4).ok_or("message entry range")? > data_size {
            return Err("message entry truncated");
        }
        let length = u32::from(read_u16(memory, entry)?);
        let flags = read_u16(memory, entry + 2)?;
        if length < 4 || relative.checked_add(length).ok_or("message entry range")? > data_size {
            return Err("message entry length");
        }
        let text_len = usize::try_from(length - 4).map_err(|_| "message entry length")?;
        let mut text = vec![0; text_len];
        memory.read(entry + 4, &mut text)?;
        if flags & 1 != 0 {
            if text.len() % 2 != 0 {
                return Err("unicode message entry length");
            }
            while text.ends_with(&[0, 0]) {
                text.truncate(text.len() - 2);
            }
            let mut ansi = Vec::with_capacity(text.len() / 2);
            for pair in text.chunks_exact(2) {
                ansi.push(encode_cp1252(u16::from_le_bytes([pair[0], pair[1]])).unwrap_or(b'?'));
            }
            return Ok(Some((ansi, MessageResourceEncoding::Unicode)));
        }
        while text.last() == Some(&0) {
            text.pop();
        }
        return Ok(Some((text, MessageResourceEncoding::Ansi)));
    }
    Ok(None)
}

fn format_message_text(text: &[u8], message_id: u32) -> Result<Vec<u8>, ProviderDispatchError> {
    let mut output = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index] != b'%' {
            output.push(text[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(&escape) = text.get(index) else {
            output.push(b'%');
            break;
        };
        index += 1;
        match escape {
            b'0' => break,
            b'%' => output.push(b'%'),
            b'b' => output.push(b' '),
            b'.' => output.push(b'.'),
            b'!' => output.push(b'!'),
            b'n' => output.extend_from_slice(b"\r\n"),
            b'r' => output.push(b'\r'),
            b't' => output.push(b'\t'),
            b'1'..=b'9' => {
                return Err(ProviderDispatchError::Frontier {
                    api: "FormatMessageA",
                    detail: format!("insert-with-null-arguments message_id=0x{message_id:08x}"),
                });
            }
            other => output.push(other),
        }
    }
    Ok(output)
}

fn cp1252_lower(value: u16) -> u16 {
    match value {
        0x0041..=0x005a => value + 0x20,
        0x00c0..=0x00d6 | 0x00d8..=0x00de => value + 0x20,
        0x0160 => 0x0161,
        0x0152 => 0x0153,
        0x017d => 0x017e,
        0x0178 => 0x00ff,
        _ => value,
    }
}

fn cp1252_upper(value: u16) -> u16 {
    match value {
        0x0061..=0x007a => value - 0x20,
        0x00e0..=0x00f6 | 0x00f8..=0x00fe => value - 0x20,
        0x0161 => 0x0160,
        0x0153 => 0x0152,
        0x017e => 0x017d,
        0x00ff => 0x0178,
        _ => value,
    }
}

fn lc_map_scalar(value: u16, mode: LcMapMode) -> u16 {
    match mode {
        LcMapMode::Lower => cp1252_lower(value),
        LcMapMode::Upper => cp1252_upper(value),
    }
}

fn system_font_advance_cp1252(ch: u8) -> Option<u32> {
    match ch {
        b' ' | b'.' | b'i' | b'l' | b't' => Some(4),
        b'r' => Some(5),
        b'C' | b'E' => Some(9),
        b'B' | b'R' | 0xa9 => Some(10),
        b'm' => Some(12),
        b'0' | b'2' | b'A' | b'a' | b'd' | b'e' | b'g' | b'h' | b'n' | b'o' | b'p' | b's'
        | b'v' | b'y' | b'z' => Some(8),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CreateThreadFrame {
    pub return_address: u32,
    pub thread_attributes: u32,
    pub stack_size: u32,
    pub start_address: u32,
    pub parameter: u32,
    pub creation_flags: u32,
    pub thread_id_pointer: u32,
}
impl CreateThreadFrame {
    pub fn validate_launcher_checkpoint(self) -> Result<(), &'static str> {
        if self
            != (Self {
                return_address: 0x0040_2039,
                thread_attributes: 0,
                stack_size: 0x2000,
                start_address: 0x0040_2072,
                parameter: 0x0021_0560,
                creation_flags: CREATE_SUSPENDED,
                thread_id_pointer: 0x0021_0560,
            })
        {
            return Err("unexpected CreateThread frame");
        }
        Ok(())
    }
}

/// Host diagnostic reads honor effective WC3 mappings: the PE overlay wins
/// over a numerically overlapping generic stack allocation.
pub fn diagnostic_read<'a>(
    address: u32,
    image: &'a Mapping,
    stack: &'a Mapping,
) -> Option<&'a [u8]> {
    range(image, address).or_else(|| range(stack, address))
}
fn range(mapping: &Mapping, address: u32) -> Option<&[u8]> {
    let offset = usize::try_from(address.checked_sub(mapping.address)?).ok()?;
    mapping.bytes.get(offset..)
}

#[cfg(test)]
crate::wc3_process_tests_1!();
