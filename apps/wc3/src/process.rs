//! Portable Windows XP semantics for the launcher.
//!
//! This module deliberately knows nothing about VMX, VM ids, physical memory,
//! or carrier selection. It consumes an x86 trap frame plus generic guest
//! memory and returns the register value with which execution should resume.

use std::collections::{HashMap, HashSet, VecDeque};

#[cfg(target_os = "trueos")]
use trueos::clock;
use trueos::clock::UtcDateTime;

use crate::{
    ThisToThat::{decode_cp1252, encode_cp1252},
    child_loader::{ChildProvider, ProviderImport, ProviderOp, ProviderSymbol, provider_op},
    imports::{LauncherImport, WinCall},
    pe32,
    session::{
        CreateEventRequest, CreateMutexRequest, CreateProcessRequest, CreateWindowRequest,
        GetExitCodeProcessRequest, LoadImageRequest, PersonalityAction, SessionRequest, ThreadKey,
        WaitRequest, WindowBlitRequest, WindowTextRequest,
    },
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
// SetUnhandledExceptionFilter callbacks return EXCEPTION_* filter results,
// distinct from the DISPOSITION_* values used by frame-based SEH handlers.
pub const EXCEPTION_FILTER_CONTINUE_EXECUTION: u32 = u32::MAX;
pub const EXCEPTION_FILTER_CONTINUE_SEARCH: u32 = 0;
pub const EXCEPTION_FILTER_EXECUTE_HANDLER: u32 = 1;
pub const CRT_UNKNOWN_APP: u32 = 0;
pub const CRT_CONSOLE_APP: u32 = 1;
pub const CRT_GUI_APP: u32 = 2;
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
const TRANSPARENT: u32 = 1;
const OPAQUE: u32 = 2;
const GDI_DIB_BASE: u32 = 0x0500_0000;
pub const ENVIRONMENT_BLOCK_VA: u32 = PROCESS_DATA_VA + 0x100;

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
    let mut values = [0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_u32(
            memory,
            esp.checked_add((index as u32) * 4)
                .ok_or("stack overflow")?,
        )?;
    }
    Ok(values)
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

fn crt_strrchr(
    memory: &impl GuestMemory,
    string: u32,
    character: u32,
) -> Result<u32, &'static str> {
    let needle = character as u8;
    let mut last = None;
    for offset in 0..1_048_576u32 {
        let address = string
            .checked_add(offset)
            .ok_or("strrchr address overflow")?;
        let mut byte = [0];
        memory.read(address, &mut byte)?;
        if byte[0] == needle {
            last = Some(address);
        }
        if byte[0] == 0 {
            return Ok(last.unwrap_or(0));
        }
    }
    Err("unterminated strrchr string")
}

fn crt_strstr(memory: &impl GuestMemory, haystack: u32, needle: u32) -> Result<u32, &'static str> {
    let mut first_needle = [0];
    memory.read(needle, &mut first_needle)?;
    if first_needle[0] == 0 {
        return Ok(haystack);
    }

    for haystack_offset in 0..1_048_576u32 {
        let candidate = haystack
            .checked_add(haystack_offset)
            .ok_or("strstr haystack address overflow")?;
        let mut first = [0];
        memory.read(candidate, &mut first)?;
        if first[0] == 0 {
            return Ok(0);
        }

        for needle_offset in 0..1_048_576u32 {
            let needle_address = needle
                .checked_add(needle_offset)
                .ok_or("strstr needle address overflow")?;
            let mut wanted = [0];
            memory.read(needle_address, &mut wanted)?;
            if wanted[0] == 0 {
                return Ok(candidate);
            }
            let haystack_address = candidate
                .checked_add(needle_offset)
                .ok_or("strstr candidate address overflow")?;
            let mut actual = [0];
            memory.read(haystack_address, &mut actual)?;
            if actual[0] != wanted[0] {
                break;
            }
        }
    }
    Err("unterminated strstr haystack")
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
    Scratch(u32),
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

fn module_basename(name: &str) -> &str {
    name.rsplit(['\\', '/']).next().unwrap_or(name)
}

fn module_names_match(left: &str, right: &str) -> bool {
    module_basename(left).eq_ignore_ascii_case(module_basename(right))
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
    next_file_handle: u32,
    next_find_handle: u32,
    find_handles: HashSet<u32>,
    scratch_files: HashMap<u32, ScratchFile>,
    scratch_paths: HashMap<String, u32>,
    next_scratch_file: u32,
    sid_allocations: HashMap<u32, u32>,
    next_sid: u32,
    last_error: u32,
    last_format_message_encoding: Option<MessageResourceEncoding>,
    unhandled_exception_filter: u32,
    registered_classes: HashMap<String, RegisteredClass>,
    messages: VecDeque<Message>,
    runnable_thread: Option<u32>,
    desktop_size: (u32, u32),
    gdi_objects: HashMap<u32, GdiObject>,
    active_paints: HashMap<u32, ActivePaint>,
    next_gdi_handle: u32,
    next_gdi_dib_va: u32,
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

    pub fn external_provider_module_name(&self, handle: u32) -> Option<&str> {
        self.loaded_modules
            .iter()
            .find(|module| {
                module.handle == handle && module.kind == LoadedModuleKind::ExternalProvider
            })
            .map(|module| module_basename(&module.stored_name))
    }

    pub fn provider_thunk_address(&self, module: &str, symbol: &ProviderSymbol) -> Option<u32> {
        self.provider_imports
            .iter()
            .position(|import| {
                import.module.eq_ignore_ascii_case(module) && import.symbol == *symbol
            })
            .and_then(|id| thunk32::address(u32::try_from(id).ok()?))
    }

    /// A dynamically callable provider export, as opposed to any static IAT
    /// trap thunk that happens to exist for an unmodeled provider import.
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
                        && provider_op(import).is_modeled()
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
            next_file_handle: FILE_HANDLE_BASE,
            next_find_handle: FIND_HANDLE_BASE,
            find_handles: HashSet::new(),
            scratch_files: HashMap::new(),
            scratch_paths: HashMap::new(),
            next_scratch_file: 1,
            sid_allocations: HashMap::new(),
            next_sid: PROCESS_SID_ARENA_BASE,
            last_error: 0,
            last_format_message_encoding: None,
            unhandled_exception_filter: 0,
            registered_classes: HashMap::new(),
            messages: VecDeque::new(),
            runnable_thread: None,
            desktop_size: (1920, 1080),
            gdi_objects,
            active_paints: HashMap::new(),
            next_gdi_handle: GDI_HANDLE_BASE,
            next_gdi_dib_va: GDI_DIB_BASE,
        }
    }

    pub fn import(&self, id: u32) -> Option<&LauncherImport> {
        self.imports.get(id as usize)
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

    pub fn release_loaded_module(&mut self, handle: u32) -> Result<u32, &'static str> {
        let module = self
            .loaded_modules
            .iter_mut()
            .find(|module| module.handle == handle)
            .ok_or("loaded module handle")?;
        if module.load_count == 0 {
            return Err("loaded module reference count underflow");
        }
        module.load_count -= 1;
        Ok(module.load_count)
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

    fn dispatch_process_local_provider(
        &mut self,
        pid: u32,
        tid: u32,
        operation: ProviderOp,
        esp: u32,
        memory: &mut impl GuestMemory,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        match operation {
            ProviderOp::GetSystemInfo => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_system_info(esp, memory)?,
                ))
            }
            ProviderOp::InterlockedExchange => {
                let previous = self.interlocked_exchange(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(previous))
            }
            ProviderOp::InterlockedIncrement => {
                let incremented = self.interlocked_increment(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(incremented))
            }
            ProviderOp::InterlockedDecrement => {
                let decremented = self.interlocked_decrement(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(decremented))
            }
            ProviderOp::LoadStringA => {
                let result = self.load_string(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::FormatMessageA => {
                let result = self.format_message_a(esp, memory)?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::TlsAlloc => {
                let slot = self.tls_alloc()?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(slot))
            }
            ProviderOp::TlsSetValue => {
                let result = self.tls_set_value(tid, esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::TlsGetValue => {
                let result = self.tls_get_value(tid, esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtSetAppType => {
                let [_, app_type] = arguments::<2>(memory, esp)?;
                if !matches!(app_type, CRT_UNKNOWN_APP | CRT_CONSOLE_APP | CRT_GUI_APP) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__set_app_type",
                        detail: format!("unobserved app_type={app_type}"),
                    });
                }
                self.crt_app_type = app_type;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::CrtGetFmode => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CRT_FMODE_VA))
            }
            ProviderOp::CrtGetCommode => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CRT_COMMODE_VA))
            }
            ProviderOp::CrtXcptFilter => {
                let [_, exception_code, exception_pointers] = arguments::<3>(memory, esp)?;
                if exception_pointers == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!("null exception pointers code=0x{exception_code:08x}"),
                    });
                }
                let record = read_u32(memory, exception_pointers)?;
                let context = read_u32(
                    memory,
                    exception_pointers
                        .checked_add(4)
                        .ok_or("XcptFilter pointer overflow")?,
                )?;
                if record == 0 || context == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!(
                            "invalid exception pointers ptr=0x{exception_pointers:08x} record=0x{record:08x} context=0x{context:08x}"
                        ),
                    });
                }
                let record_code = read_u32(memory, record)?;
                if record_code != exception_code {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!(
                            "code mismatch argument=0x{exception_code:08x} record=0x{record_code:08x}"
                        ),
                    });
                }
                if !matches!(
                    exception_code,
                    crate::seh::STATUS_ACCESS_VIOLATION | crate::seh::STATUS_ILLEGAL_INSTRUCTION
                ) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!("unobserved exception code=0x{exception_code:08x}"),
                    });
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(EXCEPTION_FILTER_CONTINUE_SEARCH))
            }
            ProviderOp::CrtGetMainArgs => {
                let [_, argc_out, argv_out, env_out, wildcard, startup_info] =
                    arguments::<6>(memory, esp)?;
                if argc_out == 0 || argv_out == 0 || env_out == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__getmainargs",
                        detail: format!(
                            "null output argc=0x{argc_out:08x} argv=0x{argv_out:08x} env=0x{env_out:08x}"
                        ),
                    });
                }
                if wildcard != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__getmainargs",
                        detail: format!("wildcard expansion={wildcard}"),
                    });
                }
                if startup_info != 0 {
                    let new_mode = read_u32(memory, startup_info)?;
                    if new_mode != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "__getmainargs",
                            detail: format!(
                                "startup_info=0x{startup_info:08x} new_mode={new_mode}"
                            ),
                        });
                    }
                }
                write_u32(memory, argc_out, CRT_ARGC)?;
                write_u32(memory, argv_out, CRT_ARGV_VA)?;
                write_u32(memory, env_out, CRT_ENVP_VA)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::CrtOnExit => {
                let [_, func] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if func == 0 || self.crt_onexit_callbacks.len() >= 65_536 {
                    return Ok(PersonalityAction::Return(0));
                }
                self.crt_onexit_callbacks.push(func);
                Ok(PersonalityAction::Return(func))
            }
            ProviderOp::CrtVsnprintf => {
                let result = crt_vsnprintf(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtIsDigit => {
                let [_, character] = arguments::<2>(memory, esp)?;
                // MSVCRT's _DIGIT classification bit is 0x04.  The CRT is
                // currently in its initial C locale, where only ASCII digits
                // have that classification.
                let result = if (b'0' as u32..=b'9' as u32).contains(&character) {
                    0x04
                } else {
                    0
                };
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrrchr => {
                let [_, string, character] = arguments::<3>(memory, esp)?;
                let result = crt_strrchr(memory, string, character)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrstr => {
                let [_, haystack, needle] = arguments::<3>(memory, esp)?;
                let result = crt_strstr(memory, haystack, needle)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::WsprintfA => {
                let result = wsprintf_a(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtFullPath => {
                let [_, output, path, capacity] = arguments::<4>(memory, esp)?;
                let result = if output == 0 || capacity == 0 {
                    0
                } else {
                    let path = read_c_string(memory, path, 1024)?;
                    let Some(full_path) = crt_full_path(&path) else {
                        return Ok(PersonalityAction::Return(0));
                    };
                    let bytes = full_path.as_bytes();
                    if bytes.len().checked_add(1).ok_or("fullpath length")? > capacity as usize {
                        0
                    } else {
                        memory.write(output, bytes)?;
                        memory.write(
                            output
                                .checked_add(
                                    u32::try_from(bytes.len()).map_err(|_| "fullpath length")?,
                                )
                                .ok_or("fullpath output overflow")?,
                            &[0],
                        )?;
                        output
                    }
                };
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::FreeEnvironmentStringsW => {
                let pointer = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                if pointer != ENVIRONMENT_BLOCK_VA {
                    return Err(ProviderDispatchError::Unsupported);
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetStartupInfoA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_startup_info(esp, memory)?,
                ))
            }
            ProviderOp::GetStdHandle => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_std_handle(esp, memory)?))
            }
            ProviderOp::GetFileType => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_file_type(esp, memory)?))
            }
            ProviderOp::SetHandleCount => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.set_handle_count(esp, memory)?,
                ))
            }
            ProviderOp::GetACP => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_acp()))
            }
            ProviderOp::GetCPInfo => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_cp_info(esp, memory)?))
            }
            ProviderOp::GetStringTypeW => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_string_type(esp, memory)?,
                ))
            }
            ProviderOp::MultiByteToWideChar => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.multi_byte_to_wide(esp, memory)?,
                ))
            }
            ProviderOp::LCMapStringW => {
                let flags = read_u32(
                    memory,
                    esp.checked_add(8).ok_or("provider argument overflow")?,
                )?;
                if lc_map_mode(flags).is_none() {
                    return Err(ProviderDispatchError::Unsupported);
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.lc_map_string(esp, memory)?))
            }
            ProviderOp::GetModuleFileNameA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_module_filename(esp, memory)?,
                ))
            }
            ProviderOp::GetModuleHandleA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_module_handle_a(esp, memory)?,
                ))
            }
            ProviderOp::GetCurrentProcess => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CURRENT_PROCESS_PSEUDO_HANDLE))
            }
            ProviderOp::GetCurrentProcessId => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(pid))
            }
            ProviderOp::GetProcessHeap => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(PROCESS_HEAP_HANDLE))
            }
            ProviderOp::GetCurrentThread => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CURRENT_THREAD_PSEUDO_HANDLE))
            }
            ProviderOp::GetCurrentThreadId => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(tid))
            }
            ProviderOp::OpenThreadToken => {
                let [_, thread, desired_access, open_as_self, _token_out] =
                    arguments::<5>(memory, esp)?;
                if thread != CURRENT_THREAD_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenThreadToken",
                        detail: format!("non-current-thread handle=0x{thread:08x}"),
                    });
                }
                if desired_access != TOKEN_QUERY || open_as_self != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenThreadToken",
                        detail: format!(
                            "unobserved shape access=0x{desired_access:08x} open_as_self={open_as_self}"
                        ),
                    });
                }

                // The current WC3 thread has no impersonation token. On this
                // failure path, Windows leaves the caller's output word alone.
                self.set_last_error(ERROR_NO_TOKEN);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::OpenProcessToken => {
                let [_, process, desired_access, token_out] = arguments::<4>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenProcessToken",
                        detail: format!("non-current-process handle=0x{process:08x}"),
                    });
                }
                if desired_access != TOKEN_QUERY {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenProcessToken",
                        detail: format!("unobserved access=0x{desired_access:08x}"),
                    });
                }
                let handle = self.next_token_handle;
                self.next_token_handle = self
                    .next_token_handle
                    .checked_add(1)
                    .ok_or("token handle overflow")?;
                self.token_handles.insert(
                    handle,
                    TokenHandle {
                        pid,
                        access: desired_access,
                    },
                );
                memory.write(token_out, &handle.to_le_bytes())?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetTokenInformation => {
                let [
                    _,
                    token,
                    information_class,
                    information,
                    information_len,
                    return_len,
                ] = arguments::<6>(memory, esp)?;
                let Some(token_handle) = self.token_handles.get(&token).copied() else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    return Ok(PersonalityAction::Return(0));
                };
                if token_handle.access & TOKEN_QUERY == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: format!("token without TOKEN_QUERY handle=0x{token:08x}"),
                    });
                }
                if information_class != TOKEN_GROUPS_CLASS {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: format!("unobserved information class={information_class}"),
                    });
                }
                if return_len == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: "null ReturnLength".into(),
                    });
                }
                memory.write(return_len, &XP_TOKEN_GROUPS_REQUIRED.to_le_bytes())?;
                if information == 0 || information_len < XP_TOKEN_GROUPS_REQUIRED {
                    self.set_last_error(ERROR_INSUFFICIENT_BUFFER);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }

                let sid = information
                    .checked_add(12)
                    .ok_or("GetTokenInformation SID address overflow")?;
                memory.write(information, &1u32.to_le_bytes())?;
                memory.write(information + 4, &sid.to_le_bytes())?;
                memory.write(information + 8, &XP_TOKEN_GROUP_ATTRIBUTES.to_le_bytes())?;
                memory.write(sid, &XP_EVERYONE_SID)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::AllocateAndInitializeSid => {
                let result = self.allocate_and_initialize_sid(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::EqualSid => {
                let result = self.equal_sid(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::ReadProcessMemory => {
                let [_, process, source, destination, size, bytes_read] =
                    arguments::<6>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "ReadProcessMemory",
                        detail: format!("non-self-process handle=0x{process:08x}"),
                    });
                }
                let size = usize::try_from(size)
                    .map_err(|_| ProviderDispatchError::Fault("ReadProcessMemory size"))?;
                let mut bytes = vec![0; size];
                if memory.read(source, &mut bytes).is_err()
                    || memory.write(destination, &bytes).is_err()
                {
                    self.set_last_error(299);
                    return Ok(PersonalityAction::Return(0));
                }
                if bytes_read != 0 {
                    memory.write(bytes_read, &(size as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::WriteProcessMemory => {
                let [_, process, destination, source, size, bytes_written] =
                    arguments::<6>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "WriteProcessMemory",
                        detail: format!("non-self-process handle=0x{process:08x}"),
                    });
                }
                let size = usize::try_from(size)
                    .map_err(|_| ProviderDispatchError::Fault("WriteProcessMemory size"))?;
                let mut bytes = vec![0; size];
                if memory.read(source, &mut bytes).is_err()
                    || memory.write(destination, &bytes).is_err()
                {
                    self.set_last_error(299);
                    return Ok(PersonalityAction::Return(0));
                }
                if bytes_written != 0 {
                    memory.write(bytes_written, &(size as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetLastError => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.last_error))
            }
            ProviderOp::GetTickCount => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(monotonic_counter_millis()))
            }
            ProviderOp::DisableThreadLibraryCalls => {
                let [_, module] = arguments::<2>(memory, esp)?;
                Ok(PersonalityAction::Return(
                    self.disable_thread_library_calls(module)?,
                ))
            }
            ProviderOp::CreateFileA => {
                let [
                    _,
                    filename,
                    desired_access,
                    share_mode,
                    security_attributes,
                    creation_disposition,
                    flags_and_attributes,
                    template_file,
                ] = arguments::<8>(memory, esp)?;
                if filename == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: "null filename".into(),
                    });
                }
                let path = read_c_string(memory, filename, 1024)?;
                if is_war3_scratch_path(&path) {
                    if security_attributes != 0 || template_file != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "scratch security=0x{security_attributes:08x} \\
                                 template=0x{template_file:08x}"
                            ),
                        });
                    }
                    let canonical = canonical_file_path(&path);
                    let existing = self.scratch_paths.get(&canonical).copied();
                    let file_id = match creation_disposition {
                        CREATE_NEW => {
                            if existing.is_some() {
                                self.set_last_error(ERROR_FILE_EXISTS);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            }
                            self.create_scratch_file(canonical, flags_and_attributes)?
                        }
                        CREATE_ALWAYS => {
                            if let Some(id) = existing {
                                self.scratch_files
                                    .get_mut(&id)
                                    .ok_or("scratch file disappeared")?
                                    .bytes
                                    .clear();
                                self.set_last_error(ERROR_ALREADY_EXISTS);
                                id
                            } else {
                                self.set_last_error(0);
                                self.create_scratch_file(canonical, flags_and_attributes)?
                            }
                        }
                        OPEN_EXISTING => {
                            let Some(id) = existing else {
                                self.set_last_error(ERROR_FILE_NOT_FOUND);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            };
                            id
                        }
                        OPEN_ALWAYS => {
                            if let Some(id) = existing {
                                self.set_last_error(ERROR_ALREADY_EXISTS);
                                id
                            } else {
                                self.set_last_error(0);
                                self.create_scratch_file(canonical, flags_and_attributes)?
                            }
                        }
                        TRUNCATE_EXISTING => {
                            let Some(id) = existing else {
                                self.set_last_error(ERROR_FILE_NOT_FOUND);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            };
                            if desired_access & GENERIC_WRITE == 0 {
                                self.set_last_error(ERROR_ACCESS_DENIED);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            }
                            self.scratch_files
                                .get_mut(&id)
                                .ok_or("scratch file disappeared")?
                                .bytes
                                .clear();
                            id
                        }
                        other => {
                            return Err(ProviderDispatchError::Frontier {
                                api: "CreateFileA",
                                detail: format!(
                                    "scratch disposition={other} access=0x{desired_access:08x} \\
                                     share=0x{share_mode:08x} flags=0x{flags_and_attributes:08x}"
                                ),
                            });
                        }
                    };
                    let handle = self.next_file_handle;
                    self.next_file_handle = self
                        .next_file_handle
                        .checked_add(1)
                        .ok_or("file handle overflow")?;
                    self.file_handles.insert(
                        handle,
                        FileHandle {
                            backing: FileBacking::Scratch(file_id),
                            cursor: 0,
                            access: desired_access,
                            share: share_mode,
                        },
                    );
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(handle));
                }
                if !is_self_image_path(&path) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "unmodeled path={path:?} filename=0x{filename:08x} \\
                             access=0x{desired_access:08x} share=0x{share_mode:08x} \\
                             security=0x{security_attributes:08x} \\
                             disposition=0x{creation_disposition:08x} \\
                             flags=0x{flags_and_attributes:08x} \\
                             template=0x{template_file:08x}"
                        ),
                    });
                }
                if creation_disposition != 3 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!("self-image disposition=0x{creation_disposition:08x}"),
                    });
                }
                if desired_access & FILE_WRITE_ACCESS_MASK != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "self-image write access=0x{desired_access:08x} \\
                             share=0x{share_mode:08x} flags=0x{flags_and_attributes:08x}"
                        ),
                    });
                }
                if security_attributes != 0 || template_file != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "self-image security=0x{security_attributes:08x} \\
                             template=0x{template_file:08x}"
                        ),
                    });
                }

                let handle = self.next_file_handle;
                self.next_file_handle = self
                    .next_file_handle
                    .checked_add(1)
                    .ok_or("file handle overflow")?;
                self.file_handles.insert(
                    handle,
                    FileHandle {
                        backing: FileBacking::SelfImage,
                        cursor: 0,
                        access: desired_access,
                        share: share_mode,
                    },
                );
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(handle))
            }
            ProviderOp::GetFileSize => {
                let [_, handle, high] = arguments::<3>(memory, esp)?;
                let file = self.file_handle(handle)?;
                let length = self.file_length(file, self_image_bytes)?;
                if high != 0 {
                    memory.write(high, &((length >> 32) as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(length as u32))
            }
            ProviderOp::SetFilePointer => {
                let [_, handle, distance_low, distance_high, move_method] =
                    arguments::<5>(memory, esp)?;
                let file = self.file_handle(handle)?;
                let high = if distance_high == 0 {
                    0i64
                } else {
                    i64::from(i32::from_le_bytes(
                        read_u32(memory, distance_high)?.to_le_bytes(),
                    ))
                };
                let distance =
                    (high << 32) + i64::from(i32::from_le_bytes(distance_low.to_le_bytes()));
                let base = match move_method {
                    FILE_BEGIN => 0,
                    FILE_CURRENT => file.cursor,
                    FILE_END => self.file_length(file, self_image_bytes)?,
                    _ => {
                        return Err(ProviderDispatchError::Frontier {
                            api: "SetFilePointer",
                            detail: format!("unmodeled move method={move_method}"),
                        });
                    }
                };
                let cursor = if distance >= 0 {
                    base.checked_add(distance as u64)
                } else {
                    base.checked_sub(distance.unsigned_abs())
                }
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "SetFilePointer",
                    detail: format!("cursor outside self image base={base} distance={distance}"),
                })?;
                if distance_high != 0 {
                    memory.write(distance_high, &((cursor >> 32) as u32).to_le_bytes())?;
                }
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("self image handle disappeared")?
                    .cursor = cursor;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(cursor as u32))
            }
            ProviderOp::ReadFile => {
                let [_, handle, output, requested, bytes_read, overlapped] =
                    arguments::<6>(memory, esp)?;
                if overlapped != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "ReadFile",
                        detail: format!("overlapped=0x{overlapped:08x}"),
                    });
                }
                let file = self.file_handle(handle)?;
                if file.access & GENERIC_READ == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                }
                let start = usize::try_from(file.cursor)
                    .map_err(|_| ProviderDispatchError::Fault("file cursor"))?;
                let requested = usize::try_from(requested)
                    .map_err(|_| ProviderDispatchError::Fault("ReadFile size"))?;
                let transferred = match file.backing {
                    FileBacking::SelfImage => {
                        let backing = Self::self_image_bytes(self_image_bytes)?;
                        let source_start = start.min(backing.len());
                        let transferred = if start > backing.len() {
                            0
                        } else {
                            requested.min(backing.len() - start)
                        };
                        memory.write(output, &backing[source_start..source_start + transferred])?;
                        transferred
                    }
                    FileBacking::Scratch(id) => {
                        let scratch = self
                            .scratch_files
                            .get(&id)
                            .ok_or("scratch file disappeared")?;
                        let source_start = start.min(scratch.bytes.len());
                        let transferred = if start > scratch.bytes.len() {
                            0
                        } else {
                            requested.min(scratch.bytes.len() - start)
                        };
                        memory.write(
                            output,
                            &scratch.bytes[source_start..source_start + transferred],
                        )?;
                        transferred
                    }
                };
                if bytes_read != 0 {
                    memory.write(bytes_read, &(transferred as u32).to_le_bytes())?;
                }
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("self image handle disappeared")?
                    .cursor =
                    file.cursor
                        .checked_add(u64::try_from(transferred).map_err(|_| {
                            ProviderDispatchError::Fault("ReadFile transferred length")
                        })?)
                        .ok_or("self image cursor overflow")?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::WriteFile => {
                let [_, handle, source, requested, bytes_written, overlapped] =
                    arguments::<6>(memory, esp)?;
                if overlapped != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "WriteFile",
                        detail: format!("overlapped=0x{overlapped:08x}"),
                    });
                }
                let file = self.file_handle(handle)?;
                if file.access & GENERIC_WRITE == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                }
                let FileBacking::Scratch(file_id) = file.backing else {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                };
                let requested = usize::try_from(requested)
                    .map_err(|_| ProviderDispatchError::Fault("WriteFile size"))?;
                let mut input = vec![0; requested];
                memory.read(source, &mut input)?;
                let start = usize::try_from(file.cursor)
                    .map_err(|_| ProviderDispatchError::Fault("WriteFile cursor"))?;
                let end = start
                    .checked_add(requested)
                    .ok_or(ProviderDispatchError::Fault("WriteFile extent"))?;
                let scratch = self
                    .scratch_files
                    .get_mut(&file_id)
                    .ok_or("scratch file disappeared")?;
                if scratch.bytes.len() < end {
                    scratch.bytes.resize(end, 0);
                }
                scratch.bytes[start..end].copy_from_slice(&input);
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("file handle disappeared")?
                    .cursor = end as u64;
                if bytes_written != 0 {
                    memory.write(bytes_written, &(requested as u32).to_le_bytes())?;
                }
                self.set_last_error(0);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::FlushFileBuffers => {
                let [_, handle] = arguments::<2>(memory, esp)?;
                let Some(file) = self.file_handles.get(&handle).copied() else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                };
                if file.access & GENERIC_WRITE == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }
                match file.backing {
                    // Scratch writes update the process-local byte vector
                    // synchronously, so there is no lower buffered layer.
                    FileBacking::Scratch(_) => {}
                    FileBacking::SelfImage => {
                        self.set_last_error(ERROR_ACCESS_DENIED);
                        self.call_count = self
                            .call_count
                            .checked_add(1)
                            .ok_or("call count overflow")?;
                        return Ok(PersonalityAction::Return(0));
                    }
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetWindowsDirectoryA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_windows_directory_a(esp, memory)?,
                ))
            }
            ProviderOp::GetSystemDirectoryA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_system_directory_a(esp, memory)?,
                ))
            }
            ProviderOp::GetTempPathA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_temp_path_a(esp, memory)?,
                ))
            }
            ProviderOp::SetCurrentDirectoryA => {
                let [_, directory] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if directory == 0 || read_c_string(memory, directory, 1024)?.is_empty() {
                    self.set_last_error(ERROR_PATH_NOT_FOUND);
                    return Ok(PersonalityAction::Return(0));
                }
                // WC3's modeled file operations use their absolute virtual paths,
                // so this establishes the API result without introducing a second
                // relative-path resolver.
                self.set_last_error(0);
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetFileAttributesA => {
                let [_, filename] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if filename == 0 {
                    self.set_last_error(ERROR_FILE_NOT_FOUND);
                    return Ok(PersonalityAction::Return(INVALID_FILE_ATTRIBUTES));
                }
                let path = read_c_string(memory, filename, 1024)?;
                let attributes = if is_self_image_path(&path) {
                    Some(FILE_ATTRIBUTE_NORMAL)
                } else {
                    self.scratch_paths
                        .get(&canonical_file_path(&path))
                        .and_then(|id| self.scratch_files.get(id))
                        .map(|file| file.attributes)
                };
                match attributes {
                    Some(attributes) => {
                        self.set_last_error(0);
                        Ok(PersonalityAction::Return(attributes))
                    }
                    None => {
                        self.set_last_error(ERROR_FILE_NOT_FOUND);
                        Ok(PersonalityAction::Return(INVALID_FILE_ATTRIBUTES))
                    }
                }
            }
            ProviderOp::SetFileAttributesA => {
                let [_, filename, attributes] = arguments::<3>(memory, esp)?;
                if filename == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "SetFileAttributesA",
                        detail: format!("filename=NULL attributes=0x{attributes:08x}"),
                    });
                }
                let path = read_c_string(memory, filename, 1024)?;
                let _temporary = attributes & FILE_ATTRIBUTE_TEMPORARY != 0;
                if let Some(id) = self.scratch_paths.get(&canonical_file_path(&path)).copied() {
                    self.scratch_files
                        .get_mut(&id)
                        .ok_or("scratch file disappeared")?
                        .attributes = attributes;
                    self.set_last_error(0);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(1));
                }
                if !self.path_exists(&path) {
                    self.set_last_error(ERROR_FILE_NOT_FOUND);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }
                Err(ProviderDispatchError::Frontier {
                    api: "SetFileAttributesA",
                    detail: format!("existing path={path:?} attributes=0x{attributes:08x}"),
                })
            }
            ProviderOp::FindFirstFileA => {
                let [_, pattern, find_data] = arguments::<3>(memory, esp)?;
                if pattern == 0 || find_data == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "FindFirstFileA",
                        detail: format!("null pattern=0x{pattern:08x} find_data=0x{find_data:08x}"),
                    });
                }
                let pattern = read_c_string(memory, pattern, 1024)?;
                if !is_self_image_path(&pattern) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "FindFirstFileA",
                        detail: format!(
                            "unmodeled pattern={pattern:?} find_data=0x{find_data:08x}"
                        ),
                    });
                }
                let image_size = self_image_bytes.map(|bytes| bytes.len() as u64).ok_or(
                    ProviderDispatchError::Fault("self image file backing unavailable"),
                )?;
                let mut data = [0u8; 320];
                data[0..4].copy_from_slice(&FILE_ATTRIBUTE_NORMAL.to_le_bytes());
                data[20..24].copy_from_slice(&((image_size >> 32) as u32).to_le_bytes());
                data[24..28].copy_from_slice(&(image_size as u32).to_le_bytes());
                data[44..53].copy_from_slice(b"War3.exe\0");
                memory.write(find_data, &data)?;
                let handle = self.next_find_handle;
                self.next_find_handle = self
                    .next_find_handle
                    .checked_add(1)
                    .ok_or("find handle overflow")?;
                self.find_handles.insert(handle);
                self.set_last_error(0);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(handle))
            }
            ProviderOp::FindClose => {
                let [_, handle] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if self.find_handles.remove(&handle) {
                    self.set_last_error(0);
                    Ok(PersonalityAction::Return(1))
                } else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    Ok(PersonalityAction::Return(0))
                }
            }
            ProviderOp::QueryPerformanceFrequency => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.query_performance_frequency(esp, memory)?,
                ))
            }
            ProviderOp::QueryPerformanceCounter => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.query_performance_counter(esp, memory)?,
                ))
            }
            ProviderOp::GetLocalTime | ProviderOp::GetSystemTime => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.write_current_system_time(esp, memory)?,
                ))
            }
            ProviderOp::GetTimeZoneInformation => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_time_zone_information(esp, memory)?,
                ))
            }
            ProviderOp::TimeGetTime => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(monotonic_counter_millis()))
            }
            _ => Err(ProviderDispatchError::Unsupported),
        }
    }

    pub fn dispatch_provider_for_process_typed(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        self.dispatch_provider_for_process_typed_with_self_image(
            pid,
            tid,
            provider_id,
            esp,
            memory,
            None,
        )
    }

    pub fn dispatch_provider_for_process_typed_with_self_image(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        let provider = self
            .provider_import(provider_id)
            .cloned()
            .ok_or("unknown child provider import")?;
        let operation = provider_op(&provider);
        if operation.is_generic_process_local() {
            return self.dispatch_process_local_provider(
                pid,
                tid,
                operation,
                esp,
                memory,
                self_image_bytes,
            );
        }
        let action = match operation {
            ProviderOp::CreateEventA => Some(PersonalityAction::Session(
                SessionRequest::CreateEvent(self.create_event_request(esp, memory)?),
            )),
            ProviderOp::ResetEvent => Some(PersonalityAction::Session(SessionRequest::ResetEvent {
                pid,
                tid,
                handle: arguments::<2>(memory, esp)?[1],
            })),
            ProviderOp::CreateMutexA => {
                let [_, attributes, initial_owner, name] = arguments::<4>(memory, esp)?;
                let inheritable = if attributes == 0 {
                    false
                } else {
                    let [length, descriptor, inherit] = arguments::<3>(memory, attributes)?;
                    if length != 12 {
                        self.set_last_error(87);
                        return Ok(PersonalityAction::Return(0));
                    }
                    if descriptor != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateMutexA",
                            detail: "non-default security descriptor".into(),
                        });
                    }
                    inherit != 0
                };
                Some(PersonalityAction::Session(SessionRequest::CreateMutex {
                    key: ThreadKey { pid, tid },
                    request: CreateMutexRequest {
                        name: if name == 0 {
                            None
                        } else {
                            Some(read_c_string(memory, name, 260)?)
                        },
                        initial_owner: initial_owner != 0,
                        inheritable,
                    },
                }))
            }
            ProviderOp::ReleaseMutex => {
                Some(PersonalityAction::Session(SessionRequest::ReleaseMutex {
                    key: ThreadKey { pid, tid },
                    handle: arguments::<2>(memory, esp)?[1],
                }))
            }
            ProviderOp::CloseHandle => {
                let handle = arguments::<2>(memory, esp)?[1];
                if self.token_handles.remove(&handle).is_some()
                    || self.file_handles.remove(&handle).is_some()
                {
                    Some(PersonalityAction::Return(1))
                } else {
                    Some(PersonalityAction::Session(SessionRequest::CloseHandle {
                        pid,
                        handle,
                    }))
                }
            }
            ProviderOp::WaitForSingleObject => {
                let [return_address, handle, timeout] = arguments::<3>(memory, esp)?;
                Some(PersonalityAction::Block(WaitRequest {
                    key: ThreadKey { pid, tid },
                    return_address,
                    count: 1,
                    handles_pointer: 0,
                    handles: [handle, 0],
                    wait_all: 0,
                    timeout,
                }))
            }
            _ => None,
        };
        if let Some(action) = action {
            self.call_count = self
                .call_count
                .checked_add(1)
                .ok_or("call count overflow")?;
            return Ok(action);
        }
        match (&provider.module[..], &provider.symbol) {
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "GetEnvironmentStringsW" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(ENVIRONMENT_BLOCK_VA))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "GetCommandLineA" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(PROCESS_DATA_VA))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "GetVersionExA" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_version_ex(esp, memory)?))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "WideCharToMultiByte" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.wide_to_multi_byte(esp, memory)?,
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "HeapCreate" =>
            {
                Ok(PersonalityAction::Return(
                    self.create_win_heap(esp, memory)?.handle,
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "GetVersion" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(WINDOWS_XP_GET_VERSION))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "InitializeCriticalSection" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.initialize_critical_section(esp, memory)?,
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "EnterCriticalSection" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.enter_critical_section(tid, esp, memory)?,
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "LeaveCriticalSection" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.leave_critical_section(tid, esp, memory)?,
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "SetLastError" =>
            {
                let value = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                self.set_last_error(value);
                Ok(PersonalityAction::Return(0))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "ExitProcess" =>
            {
                let exit_code = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::ExitProcess(exit_code))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "SetUnhandledExceptionFilter" =>
            {
                let filter = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.set_unhandled_exception_filter(filter),
                ))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("MSVCRT.dll") && symbol == "malloc" =>
            {
                let size = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.crt_malloc(size)?
                        .map(|allocation| allocation.pointer)
                        .unwrap_or(0),
                ))
            }
            _ => Err(ProviderDispatchError::Unsupported),
        }
    }

    pub fn dispatch_provider_for_process(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        self.dispatch_provider_for_process_typed(pid, tid, provider_id, esp, memory)
            .map_err(|error| match error {
                ProviderDispatchError::Unsupported => "unsupported child provider import",
                ProviderDispatchError::Fault(error) => error,
                ProviderDispatchError::Frontier { .. } => "child provider frontier",
            })
    }

    pub fn crt_onexit_count(&self) -> usize {
        self.crt_onexit_callbacks.len()
    }

    pub fn crt_onexit_callbacks(&self) -> &[u32] {
        &self.crt_onexit_callbacks
    }

    pub fn create_win_heap(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<HeapCreateResult, &'static str> {
        let [_, options, initial_size, maximum_size] = arguments::<4>(memory, esp)?;
        let handle = self.next_heap_handle;
        self.next_heap_handle = self
            .next_heap_handle
            .checked_add(1)
            .ok_or("HeapCreate handle overflow")?;
        self.heaps.insert(
            handle,
            WinHeap {
                options,
                initial_size,
                maximum_size,
            },
        );
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        Ok(HeapCreateResult {
            options,
            initial_size,
            maximum_size,
            handle,
        })
    }

    pub fn alloc_win_heap(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<Option<WinHeapAllocation>, &'static str> {
        let [_, heap, flags, bytes] = arguments::<4>(memory, esp)?;
        if !self.heaps.contains_key(&heap) {
            self.last_error = 6;
            return Ok(None);
        }
        if flags & !HEAP_ALLOC_ALLOWED_FLAGS != 0 {
            return Err("HeapAlloc flags frontier");
        }
        let logical = bytes.max(1);
        let aligned = logical.checked_add(7).ok_or("HeapAlloc size overflow")? & !7;
        let pointer = self.win_heap_next;
        let end = pointer
            .checked_add(aligned)
            .ok_or("HeapAlloc address overflow")?;
        if end > CHILD_WIN_HEAP_LIMIT {
            return Ok(None);
        }
        let allocation = WinHeapAllocation {
            heap,
            flags,
            requested: bytes,
            pointer,
            end,
        };
        self.win_heap_next = end;
        self.win_heap_allocations.insert(pointer, allocation);
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        Ok(Some(allocation))
    }

    pub fn alloc_global_fixed(
        &mut self,
        flags: u32,
        bytes: u32,
    ) -> Result<Option<GlobalAllocation>, ProviderDispatchError> {
        if flags & GMEM_MOVEABLE != 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "GlobalAlloc",
                detail: format!("movable-memory flags=0x{flags:08x} bytes={bytes}"),
            });
        }
        if !matches!(flags, GMEM_FIXED | GMEM_ZEROINIT) {
            return Err(ProviderDispatchError::Frontier {
                api: "GlobalAlloc",
                detail: format!("unobserved flags=0x{flags:08x} bytes={bytes}"),
            });
        }
        if bytes == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "GlobalAlloc",
                detail: format!("zero-size flags=0x{flags:08x}"),
            });
        }
        let aligned = bytes.checked_add(7).ok_or("GlobalAlloc size overflow")? & !7;
        let pointer = self.win_heap_next;
        let end = pointer
            .checked_add(aligned)
            .ok_or("GlobalAlloc address overflow")?;
        if end > CHILD_WIN_HEAP_LIMIT {
            self.set_last_error(ERROR_NOT_ENOUGH_MEMORY);
            return Ok(None);
        }
        let allocation = GlobalAllocation {
            flags,
            requested: bytes,
            pointer,
            end,
        };
        self.win_heap_next = end;
        self.global_allocations.insert(pointer, allocation);
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        Ok(Some(allocation))
    }

    pub fn free_win_heap(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<Option<WinHeapAllocation>, &'static str> {
        let [_, heap, flags, pointer] = arguments::<4>(memory, esp)?;
        if flags != 0 {
            return Err("HeapFree flags frontier");
        }
        let Some(allocation) = self.win_heap_allocations.get(&pointer).copied() else {
            self.last_error = 6;
            return Ok(None);
        };
        if allocation.heap != heap {
            self.last_error = 6;
            return Ok(None);
        }
        self.win_heap_allocations.remove(&pointer);
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        Ok(Some(allocation))
    }

    pub fn append_provider_imports(
        &mut self,
        imports: Vec<ProviderImport>,
    ) -> Result<(Vec<u32>, usize, usize, usize, Vec<u8>), &'static str> {
        self.register_external_provider_imports(&imports)?;
        let old_bytes = self.provider_thunks.len();
        let first = u32::try_from(self.provider_imports.len()).map_err(|_| "provider id")?;
        let updated_from = usize::try_from(first)
            .map_err(|_| "provider thunk offset")?
            .checked_mul(thunk32::THUNK_BYTES)
            .ok_or("provider thunk offset")?;
        let mut addresses = Vec::with_capacity(imports.len());
        for (offset, import) in imports.into_iter().enumerate() {
            let id = first
                .checked_add(u32::try_from(offset).map_err(|_| "provider id")?)
                .ok_or("provider id")?;
            let address = crate::child_loader::provider_data_export_address(&import)
                .or_else(|| thunk32::address(id))
                .ok_or("provider address")?;
            addresses.push(address);
            self.provider_imports.push(import);
        }
        let required = self
            .provider_imports
            .len()
            .checked_mul(thunk32::THUNK_BYTES)
            .ok_or("provider bytes")?;
        let new_bytes = required.checked_add(0xfff).ok_or("provider page")? & !0xfff;
        self.provider_thunks.resize(new_bytes, 0x90);
        for (offset, _) in addresses.iter().enumerate() {
            let id = first + offset as u32;
            let start = id as usize * thunk32::THUNK_BYTES;
            let import = self.provider_import(id).ok_or("provider import")?;
            thunk32::write(
                id,
                crate::child_loader::provider_thunk_kind(import),
                &mut self.provider_thunks[start..start + thunk32::THUNK_BYTES],
            )?;
        }
        Ok((
            addresses,
            old_bytes,
            new_bytes,
            updated_from,
            self.provider_thunks[updated_from..].to_vec(),
        ))
    }

    /// Handle one import VMCALL and return the value for EAX.
    pub fn dispatch(
        &mut self,
        tid: u32,
        import_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        self.dispatch_for_process(1, tid, import_id, esp, memory)
    }

    pub fn dispatch_for_process(
        &mut self,
        pid: u32,
        tid: u32,
        import_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        let import = self
            .import(import_id)
            .cloned()
            .ok_or("invalid WC3 import id")?;
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        let call = WinCall::from_import(&import);
        let value = match call {
            WinCall::GetVersion => Ok(WINDOWS_XP_GET_VERSION),
            WinCall::HeapCreate => Ok(0x5743_0001),
            WinCall::GetVersionExA => self.get_version_ex(esp, memory),
            WinCall::InitializeCriticalSection => self.initialize_critical_section(esp, memory),
            WinCall::EnterCriticalSection => self.enter_critical_section(tid, esp, memory),
            WinCall::LeaveCriticalSection => self.leave_critical_section(tid, esp, memory),
            WinCall::TlsAlloc => self.tls_alloc(),
            WinCall::TlsSetValue => self.tls_set_value(tid, esp, memory),
            WinCall::TlsGetValue => self.tls_get_value(tid, esp, memory),
            WinCall::HeapAlloc => self.heap_alloc(esp, memory),
            WinCall::HeapFree => self.heap_free(esp, memory),
            WinCall::CreateEventA => {
                return Ok(PersonalityAction::Session(SessionRequest::CreateEvent(
                    self.create_event_request(esp, memory)?,
                )));
            }
            WinCall::SetEvent => {
                return Ok(PersonalityAction::Session(SessionRequest::SetEvent {
                    pid,
                    tid,
                    handle: read_u32(memory, esp + 4)?,
                }));
            }
            WinCall::GetLastError => Ok(self.last_error),
            WinCall::SetLastError => {
                let value = read_u32(memory, esp + 4)?;
                self.set_last_error(value);
                Ok(0)
            }
            WinCall::CloseHandle => {
                return Ok(PersonalityAction::Session(SessionRequest::CloseHandle {
                    pid,
                    handle: read_u32(memory, esp + 4)?,
                }));
            }
            WinCall::GetTickCount => Ok(monotonic_counter_millis()),
            WinCall::GetCurrentThreadId => Ok(tid),
            WinCall::GetStartupInfoA => self.get_startup_info(esp, memory),
            WinCall::GetModuleFileNameA => {
                self.get_module_filename(esp, memory)
                    .map_err(|error| match error {
                        ProviderDispatchError::Unsupported => "unsupported child provider import",
                        ProviderDispatchError::Fault(error) => error,
                        ProviderDispatchError::Frontier { .. } => "module filename frontier",
                    })
            }
            WinCall::GetModuleHandleA => Ok(pe32::IMAGE_BASE),
            WinCall::GetStdHandle => self.get_std_handle(esp, memory),
            WinCall::GetFileType => self.get_file_type(esp, memory),
            WinCall::SetHandleCount => self.set_handle_count(esp, memory),
            WinCall::GetCommandLineA => Ok(PROCESS_DATA_VA),
            WinCall::GetEnvironmentStringsW => Ok(0),
            WinCall::GetEnvironmentStringsA => Ok(ENVIRONMENT_BLOCK_VA),
            WinCall::FreeEnvironmentStringsA => Ok(1),
            WinCall::GetACP => Ok(self.get_acp()),
            WinCall::GetCPInfo => self.get_cp_info(esp, memory),
            WinCall::GetStringTypeW => self.get_string_type(esp, memory),
            WinCall::MultiByteToWideChar => self.multi_byte_to_wide(esp, memory),
            WinCall::WideCharToMultiByte => self.wide_to_multi_byte(esp, memory),
            WinCall::LCMapStringW => self.lc_map_string(esp, memory),
            WinCall::RegisterClassA => self.register_class(esp, memory),
            WinCall::GetDesktopWindow => Ok(DESKTOP_HWND),
            WinCall::GetClientRect => self.get_client_rect(esp, memory),
            WinCall::CreateWindowExA => {
                return Ok(PersonalityAction::Session(SessionRequest::CreateWindow(
                    self.create_window_request(esp, memory, ThreadKey { pid, tid })?,
                )));
            }
            WinCall::ShowWindow => {
                let [_, hwnd, show] = arguments::<3>(memory, esp)?;
                return Ok(PersonalityAction::Session(SessionRequest::ShowWindow {
                    pid,
                    hwnd,
                    show,
                }));
            }
            WinCall::DestroyWindow => {
                return Ok(PersonalityAction::Session(SessionRequest::DestroyWindow {
                    pid,
                    hwnd: read_u32(memory, esp + 4)?,
                }));
            }
            WinCall::UpdateWindow => {
                let hwnd = read_u32(memory, esp + 4)?;
                return Ok(PersonalityAction::Session(SessionRequest::UpdateWindow {
                    pid,
                    hwnd,
                }));
            }
            WinCall::PeekMessageA => self.peek_message(esp, memory),
            WinCall::SetFocus => {
                let hwnd = read_u32(memory, esp + 4)?;
                return Ok(PersonalityAction::Session(SessionRequest::SetFocus {
                    pid,
                    hwnd,
                }));
            }
            WinCall::DefWindowProcA => self.def_window_proc(esp, memory),
            WinCall::DrawTextA => return self.draw_text_a(esp, memory),
            WinCall::MessageBoxA => return Err("MessageBoxA requires runtime modal dispatch"),
            WinCall::BeginPaint => {
                let [_, hwnd, paint_struct] = arguments::<3>(memory, esp)?;
                return Ok(PersonalityAction::Session(SessionRequest::BeginPaint {
                    pid,
                    hwnd,
                    paint_struct,
                }));
            }
            WinCall::EndPaint => self.end_paint(esp, memory),
            WinCall::LoadStringA => self.load_string(esp, memory),
            WinCall::LoadImageA => {
                return Ok(PersonalityAction::Session(SessionRequest::LoadImage(
                    self.load_image_request(esp, memory)?,
                )));
            }
            WinCall::GetObjectA => self.get_object_a(esp, memory),
            WinCall::CreateCompatibleDC => self.create_compatible_dc(esp, memory),
            WinCall::SelectObject => self.select_object(esp, memory),
            WinCall::GetDIBColorTable => self.get_dib_color_table(esp, memory),
            WinCall::CreatePalette => self.create_palette(esp, memory),
            WinCall::SelectPalette => self.select_palette(esp, memory),
            WinCall::RealizePalette => self.realize_palette(esp, memory),
            WinCall::SetTextColor => self.set_text_color(esp, memory),
            WinCall::SetBkColor => self.set_bk_color(esp, memory),
            WinCall::SetBkMode => self.set_bk_mode(esp, memory),
            WinCall::BitBlt => return self.bit_blt(esp, memory),
            WinCall::DeleteDC => self.delete_dc(esp, memory),
            WinCall::DeleteObject => self.delete_object(esp, memory),
            WinCall::CreateThread => self.create_thread(esp, memory),
            WinCall::ExitThread => {
                let [_, exit_code] = arguments::<2>(memory, esp)?;
                return Ok(PersonalityAction::ExitThread(exit_code));
            }
            WinCall::ResumeThread => self.resume_thread(esp, memory),
            WinCall::CreateProcessA => {
                return Ok(PersonalityAction::Session(SessionRequest::CreateProcess(
                    CreateProcessRequest {
                        frame: self.create_process_a(esp, memory)?,
                    },
                )));
            }
            WinCall::GetExitCodeProcess => {
                let [_, handle, exit_code_pointer] = arguments::<3>(memory, esp)?;
                return Ok(PersonalityAction::Session(
                    SessionRequest::GetExitCodeProcess(GetExitCodeProcessRequest {
                        pid,
                        tid,
                        handle,
                        exit_code_pointer,
                    }),
                ));
            }
            WinCall::WaitForMultipleObjects => {
                let frame = self.wait_for_multiple_objects(esp, memory)?;
                return Ok(PersonalityAction::Block(WaitRequest {
                    key: ThreadKey { pid, tid },
                    return_address: frame.return_address,
                    count: frame.count,
                    handles_pointer: frame.handles_pointer,
                    handles: frame.handles,
                    wait_all: frame.wait_all,
                    timeout: frame.timeout,
                }));
            }
            WinCall::WaitForSingleObject => {
                let [ret, handle, timeout] = arguments::<3>(memory, esp)?;
                return Ok(PersonalityAction::Block(WaitRequest {
                    key: ThreadKey { pid, tid },
                    return_address: ret,
                    count: 1,
                    handles_pointer: 0,
                    handles: [handle, 0],
                    wait_all: 0,
                    timeout,
                }));
            }
            WinCall::Unsupported => Err("unsupported launcher import"),
        }?;
        Ok(PersonalityAction::Return(value))
    }

    fn create_process_a(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<CreateProcessAFrame, &'static str> {
        let [
            ret,
            application_name,
            command_line,
            process_attributes,
            thread_attributes,
            inherit_handles,
            creation_flags,
            environment,
            current_directory,
            startup_info,
            process_information,
        ] = arguments::<11>(memory, esp)?;
        if ret != 0x0040_12E0
            || application_name != 0
            || read_c_string(memory, command_line, 64)? != "\"war3.exe\" "
            || process_attributes != 0
            || thread_attributes != 0
            || inherit_handles != 1
            || creation_flags != 0
            || environment != 0
            || current_directory != 0
            || startup_info == 0
            || process_information == 0
        {
            return Err("unexpected CreateProcessA frame");
        }
        // These are launcher-local output structures, not the STARTUPINFOA
        // synthesized by GetStartupInfoA.  At #89 both are pristine zeroed
        // storage, including STARTUPINFOA.cb.
        let mut startup_info_bytes = [0u8; 68];
        memory.read(startup_info, &mut startup_info_bytes)?;
        let mut process_information_bytes = [0u8; 16];
        memory.read(process_information, &mut process_information_bytes)?;
        if startup_info_bytes != [0; 68] || process_information_bytes != [0; 16] {
            return Err("unexpected CreateProcessA output storage");
        }
        Ok(CreateProcessAFrame {
            return_address: ret,
            application_name,
            command_line,
            process_attributes,
            thread_attributes,
            inherit_handles,
            creation_flags,
            environment,
            current_directory,
            startup_info,
            process_information,
        })
    }

    fn wait_for_multiple_objects(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<WaitForMultipleObjectsFrame, &'static str> {
        let [ret, count, handles_pointer, wait_all, timeout] = arguments::<5>(memory, esp)?;
        let mut handles = [0; 2];
        if handles_pointer != 0 && count <= 1024 {
            handles[0] = read_u32(memory, handles_pointer)?;
            if count >= 2 {
                handles[1] = read_u32(
                    memory,
                    handles_pointer
                        .checked_add(4)
                        .ok_or("handles pointer overflow")?,
                )?;
            }
        }
        Ok(WaitForMultipleObjectsFrame {
            return_address: ret,
            count,
            handles_pointer,
            handles,
            wait_all,
            timeout,
        })
    }

    fn get_version_ex(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        if read_u32(memory, output)? != 0x94 {
            return Err("GetVersionExA structure size");
        }
        let mut info = [0; 0x94];
        for (offset, value) in [(0, 0x94), (4, 5), (8, 1), (12, 2600), (16, 2)] {
            info[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
        }
        memory.write(output, &info)?;
        Ok(1)
    }

    fn initialize_critical_section(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, address] = arguments::<2>(memory, esp)?;
        let mut bytes = [0; 0x18];
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        memory.write(address, &bytes)?;
        self.critical_sections.insert(address, (0, 0));
        Ok(0)
    }

    fn enter_critical_section(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, address] = arguments::<2>(memory, esp)?;
        let entry = self
            .critical_sections
            .get_mut(&address)
            .ok_or("unknown critical section")?;
        if entry.0 != 0 && entry.0 != tid {
            return Err("critical section contention");
        }
        entry.0 = tid;
        entry.1 = entry
            .1
            .checked_add(1)
            .ok_or("critical recursion overflow")?;
        write_u32(memory, address + 4, entry.1 - 1)?;
        write_u32(memory, address + 8, entry.1)?;
        write_u32(memory, address + 12, entry.0)?;
        Ok(0)
    }

    fn leave_critical_section(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, address] = arguments::<2>(memory, esp)?;
        let entry = self
            .critical_sections
            .get_mut(&address)
            .ok_or("unknown critical section")?;
        if entry.0 != tid || entry.1 == 0 {
            return Err("critical section owner");
        }
        entry.1 -= 1;
        if entry.1 == 0 {
            entry.0 = 0;
        }
        write_u32(
            memory,
            address + 4,
            if entry.1 == 0 { u32::MAX } else { entry.1 - 1 },
        )?;
        write_u32(memory, address + 8, entry.1)?;
        write_u32(memory, address + 12, entry.0)?;
        Ok(0)
    }

    fn tls_alloc(&mut self) -> Result<u32, &'static str> {
        let slot = self
            .tls_allocated
            .iter()
            .position(|used| !used)
            .ok_or("TLS slots exhausted")?;
        self.tls_allocated[slot] = true;
        Ok(slot as u32)
    }

    fn tls_set_value(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, slot, value] = arguments::<3>(memory, esp)?;
        if !self
            .tls_allocated
            .get(slot as usize)
            .copied()
            .unwrap_or(false)
        {
            return Err("TLS slot not allocated");
        }
        self.tls_values.insert((tid, slot), value);
        Ok(1)
    }

    fn tls_get_value(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, slot] = arguments::<2>(memory, esp)?;
        if slot as usize >= self.tls_allocated.len() {
            self.last_error = 87;
            return Ok(0);
        }
        let value = self.tls_values.get(&(tid, slot)).copied().unwrap_or(0);
        // A successful TlsGetValue explicitly clears LastError so callers can
        // distinguish an empty TLS slot from a failed lookup.
        self.last_error = 0;
        Ok(value)
    }

    fn heap_alloc(&mut self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, heap, flags, bytes] = arguments::<4>(memory, esp)?;
        if heap != 0x5743_0001 || bytes == 0 {
            return Err("HeapAlloc frame");
        }
        let aligned = bytes.checked_add(7).ok_or("heap overflow")? & !7;
        let pointer = HEAP_VA.checked_add(self.heap_next).ok_or("heap overflow")?;
        if self
            .heap_next
            .checked_add(aligned)
            .filter(|end| *end <= 0x1000)
            .is_none()
        {
            return Err("launcher heap exhausted");
        }
        self.heap_next += aligned;
        self.allocations.insert(pointer, bytes);
        if flags & 8 != 0 {
            memory.write(pointer, &vec![0; bytes as usize])?;
        }
        Ok(pointer)
    }

    fn heap_free(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, heap, flags, pointer] = arguments::<4>(memory, esp)?;
        if heap != 0x5743_0001 || flags != 0 || self.allocations.remove(&pointer).is_none() {
            self.last_error = 6;
            return Ok(0);
        }
        Ok(1)
    }

    fn create_event_request(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<CreateEventRequest, &'static str> {
        let [_, attributes, manual_reset, initial, name] = arguments::<5>(memory, esp)?;
        let inheritable = if attributes == 0 {
            false
        } else {
            read_u32(memory, attributes + 8)? != 0
        };
        let name = if name == 0 {
            None
        } else {
            Some(read_c_string(memory, name, 260)?)
        };
        Ok(CreateEventRequest {
            name,
            manual_reset: manual_reset != 0,
            initial_state: initial != 0,
            inheritable,
        })
    }

    fn format_message_a(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [
            _,
            flags,
            source,
            message_id,
            language_id,
            buffer,
            capacity,
            arguments,
        ] = arguments::<8>(memory, esp)?;
        if flags != FORMAT_MESSAGE_FROM_HMODULE {
            return Err(ProviderDispatchError::Frontier {
                api: "FormatMessageA",
                detail: format!("unobserved flags=0x{flags:08x}"),
            });
        }
        if source == 0 || language_id == 0 || buffer == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "FormatMessageA",
                detail: format!(
                    "invalid FROM_HMODULE frame source=0x{source:08x} language=0x{language_id:08x} buffer=0x{buffer:08x}"
                ),
            });
        }
        if arguments != 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "FormatMessageA",
                detail: format!("non-null argument array=0x{arguments:08x}"),
            });
        }

        self.last_format_message_encoding = None;
        let (resolved_language_id, _) = format_message_language_resolution(language_id);
        let (text, encoding) = match message_table_text(
            memory,
            source,
            resolved_language_id,
            message_id,
        ) {
            Ok(value) => value,
            Err(MessageTableLookup::TypeMissing) => {
                self.set_last_error(ERROR_RESOURCE_TYPE_NOT_FOUND);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                return Ok(0);
            }
            Err(MessageTableLookup::LanguageMissing) => {
                self.set_last_error(ERROR_RESOURCE_LANG_NOT_FOUND);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                return Ok(0);
            }
            Err(MessageTableLookup::MessageMissing) => {
                self.set_last_error(ERROR_MR_MID_NOT_FOUND);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                return Ok(0);
            }
            Err(MessageTableLookup::Fault(error)) => return Err(error.into()),
        };
        let output = format_message_text(&text, message_id)?;
        let required = output
            .len()
            .checked_add(1)
            .ok_or("FormatMessageA output length")?;
        if required > capacity as usize {
            self.set_last_error(ERROR_INSUFFICIENT_BUFFER);
            self.call_count = self
                .call_count
                .checked_add(1)
                .ok_or("call count overflow")?;
            return Ok(0);
        }
        let mut terminated = output;
        terminated.push(0);
        memory.write(buffer, &terminated)?;
        self.last_format_message_encoding = Some(encoding);
        self.call_count = self
            .call_count
            .checked_add(1)
            .ok_or("call count overflow")?;
        u32::try_from(terminated.len() - 1)
            .map_err(|_| ProviderDispatchError::Fault("FormatMessageA output too long"))
    }

    pub fn set_last_error(&mut self, value: u32) {
        self.last_error = value;
    }

    pub fn last_error(&self) -> u32 {
        self.last_error
    }

    pub fn last_format_message_encoding(&self) -> Option<MessageResourceEncoding> {
        self.last_format_message_encoding
    }

    fn path_exists(&self, path: &str) -> bool {
        if is_self_image_path(path) {
            return true;
        }
        let canonical = canonical_file_path(path);
        self.scratch_paths
            .get(&canonical)
            .and_then(|id| self.scratch_files.get(id))
            .is_some_and(|scratch| scratch.path == canonical)
    }

    pub fn scratch_file_snapshot(&self, path: &str) -> Option<Vec<u8>> {
        let canonical = canonical_file_path(path);
        let id = *self.scratch_paths.get(&canonical)?;
        self.scratch_files.get(&id).map(|file| file.bytes.clone())
    }

    pub fn set_unhandled_exception_filter(&mut self, filter: u32) -> u32 {
        let previous = self.unhandled_exception_filter;
        self.unhandled_exception_filter = filter;
        previous
    }

    pub fn unhandled_exception_filter(&self) -> u32 {
        self.unhandled_exception_filter
    }

    pub fn complete_unhandled_exception_filter(
        &self,
        result: Option<u32>,
    ) -> Result<u32, &'static str> {
        match result {
            None | Some(EXCEPTION_FILTER_CONTINUE_SEARCH) => Ok(EXCEPTION_FILTER_EXECUTE_HANDLER),
            Some(EXCEPTION_FILTER_CONTINUE_EXECUTION) => Ok(EXCEPTION_FILTER_CONTINUE_EXECUTION),
            Some(EXCEPTION_FILTER_EXECUTE_HANDLER) => Ok(EXCEPTION_FILTER_EXECUTE_HANDLER),
            Some(_) => Err("top-level exception filter result"),
        }
    }

    fn get_startup_info(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let output = read_u32(memory, esp + 4)?;
        let mut bytes = [0; 0x44];
        bytes[..4].copy_from_slice(&0x44u32.to_le_bytes());
        memory.write(output, &bytes)?;
        Ok(0)
    }

    fn get_module_filename(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, module, output, capacity] = arguments::<4>(memory, esp)?;
        let loaded = if module == 0 {
            self.loaded_modules
                .iter()
                .find(|loaded| loaded.kind == LoadedModuleKind::MainImage)
                .ok_or("main image module missing")?
        } else {
            self.loaded_modules
                .iter()
                .find(|loaded| loaded.handle == module)
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "GetModuleFileNameA",
                    detail: format!("unknown-hmodule handle=0x{module:08x}"),
                })?
        };
        let filename = loaded
            .filename
            .as_ref()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "GetModuleFileNameA",
                detail: format!(
                    "module-filename-unmodeled handle=0x{module:08x} name={:?}",
                    loaded.stored_name,
                ),
            })?;
        let bytes = filename.as_bytes();
        let required = bytes.len().checked_add(1).ok_or("module filename length")?;
        if (capacity as usize) < required {
            return Err("module filename buffer".into());
        }
        memory.write(output, bytes)?;
        memory.write(
            output
                .checked_add(bytes.len() as u32)
                .ok_or("module filename output overflow")?,
            &[0],
        )?;
        Ok(bytes.len() as u32)
    }

    fn get_module_handle_a(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, module_name] = arguments::<2>(memory, esp)?;
        if module_name == 0 {
            return Ok(pe32::IMAGE_BASE);
        }
        let requested = read_c_string(memory, module_name, 260)?;
        if let Some(handle) = self.loaded_module_handle(&requested) {
            return Ok(handle);
        }
        self.last_error = ERROR_MOD_NOT_FOUND;
        Ok(0)
    }

    fn get_windows_directory_a(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        write_ansi_directory(XP_WINDOWS_DIRECTORY, esp, memory)
    }

    fn get_system_directory_a(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        write_ansi_directory(XP_SYSTEM_DIRECTORY, esp, memory)
    }

    fn get_temp_path_a(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, capacity, output] = arguments::<3>(memory, esp)?;
        let required = u32::try_from(XP_TEMP_DIRECTORY.len())
            .map_err(|_| ProviderDispatchError::Fault("GetTempPathA length"))?;
        if capacity < required {
            return Ok(required);
        }
        if output == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "GetTempPathA",
                detail: format!("null output capacity={capacity}"),
            });
        }
        memory.write(output, XP_TEMP_DIRECTORY)?;
        Ok(required - 1)
    }

    fn query_performance_frequency(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        memory.write(output, &XP_PERFORMANCE_COUNTER_FREQUENCY.to_le_bytes())?;
        Ok(1)
    }

    fn query_performance_counter(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        memory.write(output, &monotonic_counter_nanos().to_le_bytes())?;
        Ok(1)
    }

    fn write_current_system_time(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        let nanos = wall_clock_unix_nanos().ok_or("system time wall clock unavailable")?;
        let words = xp_system_time_from_unix_nanos(nanos);
        let mut bytes = [0u8; 16];
        for (index, word) in words.iter().enumerate() {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
        memory.write(output, &bytes)?;
        Ok(0)
    }

    fn get_time_zone_information(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, output] = arguments::<2>(memory, esp)?;
        memory.write(output, &xp_utc_time_zone_information())?;
        Ok(TIME_ZONE_ID_UNKNOWN)
    }

    fn get_std_handle(&self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        Ok(match read_u32(memory, esp + 4)? as i32 {
            -10 => 0x5743_1001,
            -11 => 0x5743_1002,
            -12 => 0x5743_1003,
            _ => u32::MAX,
        })
    }

    fn get_file_type(&self, _esp: u32, _memory: &impl GuestMemory) -> Result<u32, &'static str> {
        Ok(2)
    }

    fn set_handle_count(&self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        read_u32(memory, esp + 4)
    }

    fn get_acp(&self) -> u32 {
        XP_ANSI_CODE_PAGE
    }

    fn get_cp_info(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, code_page, output] = arguments::<3>(memory, esp)?;
        if code_page != XP_ANSI_CODE_PAGE {
            return Err("unsupported code page");
        }
        let mut info = [0; 0x14];
        info[..4].copy_from_slice(&1u32.to_le_bytes());
        info[4] = b'?';
        memory.write(output, &info)?;
        Ok(1)
    }

    fn get_string_type(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, info_type, source, count, output] = arguments::<5>(memory, esp)?;
        if info_type != CT_CTYPE1 {
            return Err("unsupported character info type");
        }
        if source == output {
            return Err("GetStringTypeW aliased buffers");
        }
        let signed_count = count as i32;
        let length = if signed_count < 0 {
            let mut length = 0u32;
            loop {
                let offset = length
                    .checked_mul(2)
                    .ok_or("GetStringTypeW length overflow")?;
                let address = source
                    .checked_add(offset)
                    .ok_or("GetStringTypeW source overflow")?;
                let value = read_u16(memory, address)?;
                length = length
                    .checked_add(1)
                    .ok_or("GetStringTypeW length overflow")?;
                if value == 0 {
                    break length;
                }
            }
        } else {
            count
        };
        for index in 0..length {
            let offset = index
                .checked_mul(2)
                .ok_or("GetStringTypeW length overflow")?;
            let source_address = source
                .checked_add(offset)
                .ok_or("GetStringTypeW source overflow")?;
            let output_address = output
                .checked_add(offset)
                .ok_or("GetStringTypeW output overflow")?;
            let class = ascii_ctype1(read_u16(memory, source_address)?);
            memory.write(output_address, &class.to_le_bytes())?;
        }
        Ok(1)
    }

    fn load_string(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, instance, resource_id, buffer, max_chars] = arguments::<5>(memory, esp)?;
        if instance == 0 || max_chars == 0 {
            return Err("unexpected LoadStringA frame");
        }
        let resource = numeric_resource(
            memory,
            instance,
            6,
            (resource_id >> 4)
                .checked_add(1)
                .ok_or("resource block id")?,
        )?;
        let data = resource.address;
        let data_size = resource.size;
        let data_end = data.checked_add(data_size).ok_or("resource data range")?;
        let slot = resource_id & 0x0f;
        let mut cursor = data;
        for index in 0..16 {
            let length = u32::from(read_u16(memory, cursor)?);
            cursor += 2;
            let end = cursor
                .checked_add(length.checked_mul(2).ok_or("resource string overflow")?)
                .ok_or("resource string overflow")?;
            if end > data_end {
                return Err("resource string outside block");
            }
            if index == slot {
                let copied = length.min(max_chars - 1);
                let mut output = Vec::with_capacity(copied as usize + 1);
                for character in 0..copied {
                    output.push(
                        encode_cp1252(read_u16(memory, cursor + character * 2)?).unwrap_or(b'?'),
                    );
                }
                output.push(0);
                memory.write(buffer, &output)?;
                return Ok(copied);
            }
            cursor = end;
        }
        Err("resource string slot missing")
    }

    fn load_image_request(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<LoadImageRequest, &'static str> {
        let [ret, module, name, image_type, cx, cy, flags] = arguments::<7>(memory, esp)?;
        if module != pe32::IMAGE_BASE
            || name & 0xffff_0000 != 0
            || image_type != 0
            || cx != 0
            || cy != 0
            || flags != 0x2000
        {
            return Err("unexpected LoadImageA frame");
        }
        let resource_id = name & 0xffff;
        let resource = numeric_resource(memory, pe32::IMAGE_BASE, 2, resource_id)?;
        if resource.size < 40 {
            return Err("bitmap resource header truncated");
        }
        let header_size = read_u32(memory, resource.address)?;
        if !matches!(header_size, 40 | 108 | 124) || header_size > resource.size {
            return Err("bitmap header unsupported");
        }
        let width = read_i32(memory, resource.address + 4)?;
        let height = read_i32(memory, resource.address + 8)?;
        let planes = read_u16(memory, resource.address + 12)?;
        let bit_count = read_u16(memory, resource.address + 14)?;
        let compression = read_u32(memory, resource.address + 16)?;
        let size_image = read_u32(memory, resource.address + 20)?;
        let clr_used = read_u32(memory, resource.address + 32)?;
        if width == 0 || height == 0 || planes != 1 {
            return Err("bitmap dimensions or planes invalid");
        }
        let mut dib = vec![0; resource.size as usize];
        for (index, byte) in dib.iter_mut().enumerate() {
            let mut value = [0];
            memory.read(resource.address + index as u32, &mut value)?;
            *byte = value[0];
        }
        let _ = ret;
        Ok(LoadImageRequest {
            resource_id,
            dib,
            width,
            height,
            planes,
            bit_count,
            compression,
            size_image,
            clr_used,
        })
    }

    fn get_object_a(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, handle, buffer_bytes, output] = arguments::<4>(memory, esp)?;
        let Some(GdiObject::Bitmap(bitmap)) = self.gdi_objects.get(&handle) else {
            return Ok(0);
        };
        let required = 24u32;
        if output == 0 {
            return Ok(required);
        }
        if buffer_bytes < required {
            return Ok(0);
        }
        write_u32(memory, output, 0)?;
        write_u32(memory, output + 4, bitmap.width as u32)?;
        write_u32(memory, output + 8, bitmap.height.unsigned_abs())?;
        write_u32(memory, output + 12, bitmap.row_stride)?;
        memory.write(output + 16, &bitmap.planes.to_le_bytes())?;
        memory.write(output + 18, &bitmap.bit_count.to_le_bytes())?;
        write_u32(memory, output + 20, bitmap.bits_va)?;
        let _ = ret;
        Ok(required)
    }

    fn create_compatible_dc(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [ret, source_dc] = arguments::<2>(memory, esp)?;
        let compatibility = if source_dc == 0 {
            DcCompatibility::Display
        } else {
            match self.gdi_objects.get(&source_dc) {
                Some(GdiObject::DeviceContext(dc)) => dc.compatible_with,
                Some(_) => return Err("CreateCompatibleDC source is not a device context"),
                None => return Err("unknown CreateCompatibleDC source DC"),
            }
        };
        let handle = self.next_gdi_handle;
        self.next_gdi_handle = self
            .next_gdi_handle
            .checked_add(1)
            .ok_or("GDI handle overflow")?;
        self.gdi_objects.insert(
            handle,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: compatibility,
                target: DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let _ = ret;
        Ok(handle)
    }

    pub fn begin_paint(
        &mut self,
        hwnd: u32,
        paint_struct: u32,
        width: u32,
        height: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        if self.active_paints.contains_key(&paint_struct) {
            return Err("BeginPaint PAINTSTRUCT already active");
        }
        let handle = self.next_gdi_handle;
        self.next_gdi_handle = self
            .next_gdi_handle
            .checked_add(1)
            .ok_or("GDI handle overflow")?;
        self.gdi_objects.insert(
            handle,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        memory.write(paint_struct, &[0; 64])?;
        for (offset, value) in [
            (0, handle),
            (4, 0),
            (8, 0),
            (12, 0),
            (16, width),
            (20, height),
            (24, 0),
            (28, 0),
        ] {
            write_u32(memory, paint_struct + offset, value)?;
        }
        self.active_paints
            .insert(paint_struct, ActivePaint { hwnd, hdc: handle });
        Ok(handle)
    }

    fn end_paint(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, hwnd, paint_struct] = arguments::<3>(memory, esp)?;
        if paint_struct == 0 {
            return Ok(0);
        }
        let Some(active) = self.active_paints.get(&paint_struct).copied() else {
            return Ok(0);
        };
        if active.hwnd != hwnd || read_u32(memory, paint_struct)? != active.hdc {
            return Ok(0);
        }
        let valid_window_dc = matches!(
            self.gdi_objects.get(&active.hdc),
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { hwnd: target_hwnd },
                ..
            })) if *target_hwnd == hwnd
        );
        if !valid_window_dc {
            return Ok(0);
        }
        self.active_paints.remove(&paint_struct);
        self.gdi_objects.remove(&active.hdc);
        Ok(1)
    }

    fn select_object(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, hdc, object] = arguments::<3>(memory, esp)?;
        let stock = match self.gdi_objects.get(&object) {
            Some(GdiObject::Bitmap(bitmap)) => bitmap.stock,
            Some(GdiObject::DeviceContext(_)) => return Err("SelectObject requires bitmap"),
            Some(GdiObject::Palette(_)) => return Err("SelectObject requires bitmap"),
            None => return Err("SelectObject unknown bitmap"),
        };
        let old = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { selected_bitmap },
                ..
            })) => *selected_bitmap,
            Some(GdiObject::DeviceContext(_)) => {
                return Err("SelectObject requires memory device context");
            }
            Some(GdiObject::Bitmap(_)) => return Err("SelectObject requires device context"),
            Some(GdiObject::Palette(_)) => return Err("SelectObject requires device context"),
            None => return Err("SelectObject unknown device context"),
        };
        if !stock
            && self.gdi_objects.iter().any(|(handle, value)| {
                *handle != hdc
                    && matches!(
                        value,
                        GdiObject::DeviceContext(DeviceContext { target: DcTarget::Memory { selected_bitmap }, .. }) if *selected_bitmap == object
                    )
            })
        {
            return Err("bitmap already selected into another device context");
        }
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Err("SelectObject unknown device context");
        };
        let DcTarget::Memory { selected_bitmap } = &mut dc.target else {
            return Err("SelectObject requires memory device context");
        };
        *selected_bitmap = object;
        let _ = ret;
        Ok(old)
    }

    fn get_dib_color_table(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [ret, hdc, start, count, output] = arguments::<5>(memory, esp)?;
        if count == 0 || output == 0 {
            return Ok(0);
        }
        let selected = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { selected_bitmap },
                ..
            })) => *selected_bitmap,
            _ => return Ok(0),
        };
        let palette = match self.gdi_objects.get(&selected) {
            Some(GdiObject::Bitmap(bitmap)) if !bitmap.palette.is_empty() => &bitmap.palette,
            _ => return Ok(0),
        };
        let start = start as usize;
        if start >= palette.len() {
            return Ok(0);
        }
        let copied = (count as usize).min(palette.len() - start);
        for (index, entry) in palette[start..start + copied].iter().enumerate() {
            let address = output
                .checked_add(
                    (index as u32)
                        .checked_mul(4)
                        .ok_or("palette output overflow")?,
                )
                .ok_or("palette output overflow")?;
            memory.write(address, entry)?;
        }
        let _ = ret;
        Ok(copied as u32)
    }

    fn create_palette(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, log_palette] = arguments::<2>(memory, esp)?;
        let allocation = self
            .allocations
            .get(&log_palette)
            .copied()
            .ok_or("CreatePalette requires live heap allocation")?;
        let version = read_u16(memory, log_palette)?;
        let count = read_u16(memory, log_palette + 2)?;
        if count == 0 || count > 256 {
            return Err("CreatePalette entry count unsupported");
        }
        let required = 4usize
            .checked_add(
                usize::from(count)
                    .checked_mul(4)
                    .ok_or("palette size overflow")?,
            )
            .ok_or("palette size overflow")?;
        if required > allocation as usize {
            return Err("CreatePalette palette exceeds heap allocation");
        }
        let mut entries = Vec::with_capacity(count as usize);
        for index in 0..count as u32 {
            let address = log_palette
                .checked_add(4 + index * 4)
                .ok_or("palette address overflow")?;
            entries.push(PaletteEntry {
                red: read_byte(memory, address)?,
                green: read_byte(memory, address + 1)?,
                blue: read_byte(memory, address + 2)?,
                flags: read_byte(memory, address + 3)?,
            });
        }
        let handle = self.next_gdi_handle;
        self.next_gdi_handle = self
            .next_gdi_handle
            .checked_add(1)
            .ok_or("GDI handle overflow")?;
        self.gdi_objects.insert(
            handle,
            GdiObject::Palette(PaletteObject {
                version,
                entries,
                stock: false,
            }),
        );
        let _ = ret;
        Ok(handle)
    }

    fn select_palette(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, hdc, hpalette, force_background] = arguments::<4>(memory, esp)?;
        if !matches!(self.gdi_objects.get(&hpalette), Some(GdiObject::Palette(_))) {
            return Ok(0);
        }
        let old = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(dc)) => dc.selected_palette,
            _ => return Ok(0),
        };
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Ok(0);
        };
        dc.selected_palette = hpalette;
        dc.palette_force_background = force_background != 0;
        dc.realized_palette = None;
        let _ = ret;
        Ok(old)
    }

    fn realize_palette(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [ret, hdc] = arguments::<2>(memory, esp)?;
        let (selected_palette, already_realized) = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(dc)) => (
                dc.selected_palette,
                dc.realized_palette == Some(dc.selected_palette),
            ),
            _ => return Ok(u32::MAX),
        };
        let entry_count = match self.gdi_objects.get(&selected_palette) {
            Some(GdiObject::Palette(palette)) => palette.entries.len(),
            _ => return Ok(u32::MAX),
        };
        if already_realized {
            return Ok(0);
        }
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Ok(u32::MAX);
        };
        dc.realized_palette = Some(selected_palette);
        let _ = ret;
        u32::try_from(entry_count).map_err(|_| "palette entry count overflow")
    }

    fn set_text_color(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, hdc, color] = arguments::<3>(memory, esp)?;
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Ok(u32::MAX);
        };
        let old = dc.text_color;
        dc.text_color = color & 0x00ff_ffff;
        let _ = ret;
        Ok(old)
    }

    fn set_bk_color(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, hdc, color] = arguments::<3>(memory, esp)?;
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Ok(u32::MAX);
        };
        let old = dc.bk_color;
        dc.bk_color = color & 0x00ff_ffff;
        let _ = ret;
        Ok(old)
    }

    fn set_bk_mode(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, hdc, mode] = arguments::<3>(memory, esp)?;
        if !matches!(mode, TRANSPARENT | OPAQUE) {
            return Ok(0);
        }
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Ok(0);
        };
        let old = dc.bk_mode;
        dc.bk_mode = mode;
        Ok(old)
    }

    fn bit_blt(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        let [
            ret,
            hdc_dst,
            x,
            y,
            width,
            height,
            hdc_src,
            x_src,
            y_src,
            rop,
        ] = arguments::<10>(memory, esp)?;
        if rop != 0x00cc_0020 {
            return Err("unsupported BitBlt raster operation");
        }
        if x != 0 || y != 0 || x_src != 0 || y_src != 0 || width == 0 || height == 0 {
            return Err("unsupported BitBlt coordinates or dimensions");
        }
        let hwnd = match self.gdi_objects.get(&hdc_dst) {
            Some(GdiObject::DeviceContext(dc)) => match &dc.target {
                DcTarget::WindowPaint { hwnd } => {
                    if dc.realized_palette != Some(dc.selected_palette) {
                        return Err("BitBlt destination palette is not realized");
                    }
                    *hwnd
                }
                DcTarget::Memory { .. } => return Err("BitBlt destination is not window paint DC"),
            },
            _ => return Err("BitBlt unknown destination DC"),
        };
        let bitmap_handle = match self.gdi_objects.get(&hdc_src) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { selected_bitmap },
                ..
            })) => *selected_bitmap,
            Some(GdiObject::DeviceContext(_)) => return Err("BitBlt source is not memory DC"),
            _ => return Err("BitBlt unknown source DC"),
        };
        let bitmap = match self.gdi_objects.get(&bitmap_handle) {
            Some(GdiObject::Bitmap(bitmap)) => bitmap.clone(),
            _ => return Err("BitBlt source selection is not bitmap"),
        };
        if bitmap.planes != 1
            || bitmap.bit_count != 8
            || bitmap.compression != 0
            || bitmap.width <= 0
            || bitmap.height == 0
            || width > bitmap.width as u32
            || height > bitmap.height.unsigned_abs()
            || bitmap.palette.len() > 256
        {
            return Err("unsupported BitBlt bitmap shape");
        }
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or("BitBlt RGBA size overflow")?;
        let mut rgba = vec![0; pixels];
        let mut row = vec![0; bitmap.row_stride as usize];
        for logical_y in 0..height as usize {
            let physical_y = if bitmap.height > 0 {
                bitmap.height as usize - 1 - logical_y
            } else {
                logical_y
            };
            let source = bitmap
                .bits_va
                .checked_add(
                    u32::try_from(
                        physical_y
                            .checked_mul(bitmap.row_stride as usize)
                            .ok_or("BitBlt source row overflow")?,
                    )
                    .map_err(|_| "BitBlt source row overflow")?,
                )
                .ok_or("BitBlt source address overflow")?;
            memory.read(source, &mut row)?;
            for logical_x in 0..width as usize {
                let index = row[logical_x] as usize;
                let entry = *bitmap
                    .palette
                    .get(index)
                    .ok_or("BitBlt palette index out of range")?;
                let destination = (logical_y * width as usize + logical_x) * 4;
                rgba[destination..destination + 4]
                    .copy_from_slice(&[entry[2], entry[1], entry[0], 255]);
            }
        }
        let _ = ret;
        Ok(PersonalityAction::WindowBlit(WindowBlitRequest {
            dst_hdc: hdc_dst,
            hwnd,
            dst_x: x,
            dst_y: y,
            width,
            height,
            rgba,
            source_bitmap: bitmap_handle,
            src_hdc: hdc_src,
            bits_va: bitmap.bits_va,
            bottom_up: bitmap.height > 0,
        }))
    }

    fn delete_dc(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, hdc] = arguments::<2>(memory, esp)?;
        let is_dc = matches!(
            self.gdi_objects.get(&hdc),
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { .. },
                ..
            }))
        );
        if is_dc {
            self.gdi_objects.remove(&hdc);
            let _ = ret;
            return Ok(1);
        }
        let _ = ret;
        Ok(0)
    }

    fn delete_object(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [ret, object] = arguments::<2>(memory, esp)?;
        let Some(value) = self.gdi_objects.get(&object) else {
            let _ = ret;
            return Ok(0);
        };
        match value {
            GdiObject::DeviceContext(_) => Ok(0),
            GdiObject::Bitmap(bitmap) => {
                if bitmap.stock
                    || self.gdi_objects.iter().any(|(handle, value)| {
                        *handle != object
                            && matches!(
                                value,
                                GdiObject::DeviceContext(DeviceContext { target: DcTarget::Memory { selected_bitmap }, .. }) if *selected_bitmap == object
                            )
                    })
                {
                    return Ok(0);
                }
                self.gdi_objects.remove(&object);
                Ok(1)
            }
            GdiObject::Palette(_) => {
                if self.gdi_objects.get(&object).is_some_and(|value| {
                    matches!(value, GdiObject::Palette(PaletteObject { stock: true, .. }))
                }) || self.gdi_objects.values().any(|value| {
                    matches!(
                        value,
                        GdiObject::DeviceContext(DeviceContext { selected_palette, .. })
                            if *selected_palette == object
                    )
                }) {
                    return Ok(0);
                }
                self.gdi_objects.remove(&object);
                Ok(1)
            }
        }
    }

    fn multi_byte_to_wide(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, cp, _, source, count, output, capacity] = arguments::<7>(memory, esp)?;
        if cp != 0 && cp != XP_ANSI_CODE_PAGE {
            return Err("unsupported code page");
        }
        let bytes = if count == u32::MAX {
            read_c_string(memory, source, 4096)?
                .into_bytes()
                .into_iter()
                .chain([0])
                .collect::<Vec<_>>()
        } else {
            let mut v = vec![0; count as usize];
            memory.read(source, &mut v)?;
            v
        };
        if output == 0 {
            return Ok(bytes.len() as u32);
        }
        if capacity < bytes.len() as u32 {
            return Ok(0);
        }
        for (index, byte) in bytes.iter().enumerate() {
            memory.write(
                output + (index as u32) * 2,
                &decode_cp1252(*byte).to_le_bytes(),
            )?;
        }
        Ok(bytes.len() as u32)
    }

    fn wide_to_multi_byte(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, cp, _, source, count, output, capacity, _, _] = arguments::<9>(memory, esp)?;
        if cp != 0 && cp != XP_ANSI_CODE_PAGE {
            return Err("unsupported code page");
        }
        let length = if count == u32::MAX {
            let mut n = 0;
            loop {
                if read_u16(memory, source + n * 2)? == 0 {
                    break n + 1;
                }
                n += 1;
            }
        } else {
            count
        };
        if output == 0 {
            return Ok(length);
        }
        if capacity < length {
            return Ok(0);
        }
        for index in 0..length {
            let value = read_u16(memory, source + index * 2)?;
            memory.write(output + index, &[encode_cp1252(value).unwrap_or(b'?')])?;
        }
        Ok(length)
    }

    fn lc_map_string(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, _locale, flags, source, count, output, capacity] = arguments::<7>(memory, esp)?;
        let mode = lc_map_mode(flags).ok_or("unsupported LCMapStringW flags")?;
        let signed_count = count as i32;
        if signed_count == 0 {
            return Err("LCMapStringW zero source length");
        }
        let length = if signed_count < 0 {
            let mut length = 0u32;
            loop {
                let offset = length
                    .checked_mul(2)
                    .ok_or("LCMapStringW length overflow")?;
                let address = source
                    .checked_add(offset)
                    .ok_or("LCMapStringW source overflow")?;
                let value = read_u16(memory, address)?;
                length = length
                    .checked_add(1)
                    .ok_or("LCMapStringW length overflow")?;
                if value == 0 {
                    break length;
                }
            }
        } else {
            count
        };
        if capacity == 0 {
            return Ok(length);
        }
        if output == 0 || capacity < length {
            return Ok(0);
        }
        for index in 0..length {
            let offset = index.checked_mul(2).ok_or("LCMapStringW length overflow")?;
            let source_address = source
                .checked_add(offset)
                .ok_or("LCMapStringW source overflow")?;
            let output_address = output
                .checked_add(offset)
                .ok_or("LCMapStringW output overflow")?;
            let mapped = lc_map_scalar(read_u16(memory, source_address)?, mode);
            memory.write(output_address, &mapped.to_le_bytes())?;
        }
        Ok(length)
    }

    fn register_class(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let structure = read_u32(memory, esp + 4)?;
        let name_ptr = read_u32(memory, structure + 36)?;
        let name = read_c_string(memory, name_ptr, 256)?;
        let menu_ptr = read_u32(memory, structure + 32)?;
        let menu_name = if menu_ptr == 0 || menu_ptr >> 16 == 0 {
            None
        } else {
            Some(read_c_string(memory, menu_ptr, 256)?)
        };
        let atom = self.registered_classes.len() as u16 + 1;
        self.registered_classes.insert(
            name.clone(),
            RegisteredClass {
                atom,
                name,
                style: read_u32(memory, structure)?,
                wndproc: read_u32(memory, structure + 4)?,
                cls_extra: read_u32(memory, structure + 8)? as i32,
                wnd_extra: read_u32(memory, structure + 12)? as i32,
                instance: read_u32(memory, structure + 16)?,
                icon: read_u32(memory, structure + 20)?,
                cursor: read_u32(memory, structure + 24)?,
                background: read_u32(memory, structure + 28)?,
                menu_name,
            },
        );
        Ok(atom as u32)
    }
    fn get_client_rect(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, hwnd, out] = arguments::<3>(memory, esp)?;
        if hwnd != DESKTOP_HWND {
            return Err("unknown desktop");
        }
        for (o, v) in [
            (0, 0u32),
            (4, 0),
            (8, self.desktop_size.0),
            (12, self.desktop_size.1),
        ] {
            write_u32(memory, out + o, v)?;
        }
        Ok(1)
    }

    fn def_window_proc(&self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, _, _, _, _] = arguments::<5>(memory, esp)?;
        Ok(0)
    }

    fn draw_text_a(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        let [ret, hdc, text_ptr, count_raw, rect_ptr, format] = arguments::<6>(memory, esp)?;
        let _ = ret;
        if (count_raw as i32) < 0 {
            return Err("DrawTextA count=-1 unsupported");
        }
        if !matches!(
            self.gdi_objects.get(&hdc),
            Some(GdiObject::DeviceContext(_))
        ) {
            return Err("DrawTextA unknown device context");
        }
        if format != 0x0000_0411 && format != 0x0000_0011 {
            return Err("DrawTextA format unsupported");
        }
        if rect_ptr == 0 {
            return Err("DrawTextA requires RECT");
        }
        let mut bytes = vec![0; count_raw as usize];
        memory.read(text_ptr, &mut bytes)?;
        let mut text = String::with_capacity(bytes.len());
        for byte in &bytes {
            let codepoint = decode_cp1252(*byte) as u32;
            let character = char::from_u32(codepoint).ok_or("DrawTextA CP1252 decode")?;
            text.push(character);
        }
        let measured_width = bytes
            .iter()
            .map(|byte| system_font_advance_cp1252(*byte).ok_or("DrawTextA character unsupported"))
            .try_fold(0u32, |total, advance| {
                total
                    .checked_add(advance?)
                    .ok_or("DrawTextA width overflow")
            })?;
        let left = read_i32(memory, rect_ptr)?;
        let top = read_i32(memory, rect_ptr + 4)?;
        let right = read_i32(memory, rect_ptr + 8)?;
        let _bottom = read_i32(memory, rect_ptr + 12)?;
        let available_width = right.checked_sub(left).ok_or("DrawTextA RECT overflow")?;
        if measured_width > u32::try_from(available_width).map_err(|_| "DrawTextA RECT width")? {
            return Err("DrawTextA word wrap frontier");
        }
        if format == 0x0000_0411 {
            write_u32(
                memory,
                rect_ptr + 8,
                left.checked_add(measured_width as i32)
                    .ok_or("DrawTextA right overflow")? as u32,
            )?;
            write_u32(
                memory,
                rect_ptr + 12,
                top.checked_add(16).ok_or("DrawTextA bottom overflow")? as u32,
            )?;
            return Ok(PersonalityAction::Return(16));
        }

        let (hwnd, colorref) = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(dc)) => match &dc.target {
                DcTarget::WindowPaint { hwnd } => (*hwnd, dc.text_color),
                DcTarget::Memory { .. } => return Err("DrawTextA target is memory DC"),
            },
            _ => return Err("DrawTextA unknown device context"),
        };
        Ok(PersonalityAction::WindowText(WindowTextRequest {
            hwnd,
            hdc,
            text,
            rect: [left, top, right, read_i32(memory, rect_ptr + 12)?],
            colorref,
            height: 16,
        }))
    }

    fn create_window_request(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
        owner: ThreadKey,
    ) -> Result<CreateWindowRequest, &'static str> {
        let a = arguments::<13>(memory, esp)?;
        if a[7] == 0 || a[8] == 0 || (a[11] != 0 && a[11] != pe32::IMAGE_BASE) {
            return Err("unexpected CreateWindowExA frame");
        }
        let class = read_c_string(memory, a[2], 256)?;
        let registered = self
            .registered_classes
            .get(&class)
            .ok_or("unregistered class")?;
        let title = read_c_string(memory, a[3], 256)?;
        Ok(CreateWindowRequest {
            owner,
            class,
            wndproc: registered.wndproc,
            title,
            ex_style: a[1],
            style: a[4],
            x: a[5] as i32,
            y: a[6] as i32,
            width: a[7],
            height: a[8],
            parent: a[9],
            menu: a[10],
            instance: a[11],
            param: a[12],
        })
    }
    fn peek_message(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, out, hwnd, min, max, flags] = arguments::<6>(memory, esp)?;
        let pos = self.messages.iter().position(|m| {
            (hwnd == 0 || m.hwnd == hwnd)
                && ((min == 0 && max == 0) || (m.message >= min && m.message <= max))
        });
        let Some(pos) = pos else { return Ok(0) };
        let m = if flags & 1 != 0 {
            self.messages.remove(pos).unwrap()
        } else {
            self.messages[pos].clone()
        };
        for (o, v) in [
            (0, m.hwnd),
            (4, m.message),
            (8, m.wparam),
            (12, m.lparam),
            (16, m.time),
            (20, m.x as u32),
            (24, m.y as u32),
        ] {
            write_u32(memory, out + o, v)?;
        }
        Ok(1)
    }

    fn create_thread(
        &mut self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [ret, attrs, stack, start, parameter, flags, tid_ptr] = arguments::<7>(memory, esp)?;
        let frame = CreateThreadFrame {
            return_address: ret,
            thread_attributes: attrs,
            stack_size: stack,
            start_address: start,
            parameter,
            creation_flags: flags,
            thread_id_pointer: tid_ptr,
        };
        frame.validate_launcher_checkpoint()?;
        let tid = self.next_tid;
        let handle = self.next_thread_handle;
        self.next_tid += 1;
        self.next_thread_handle += 1;
        self.threads.push(ThreadObject {
            handle,
            tid,
            start_address: start,
            parameter,
            requested_stack_size: stack,
            suspend_count: u32::from(flags & CREATE_SUSPENDED != 0),
            exit_code: None,
            open: true,
        });
        write_u32(memory, tid_ptr, tid)?;
        Ok(handle)
    }

    fn resume_thread(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let handle = read_u32(memory, esp + 4)?;
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.open && thread.handle == handle)
            .ok_or("ResumeThread unknown handle")?;
        let old = thread.suspend_count;
        if old != 0 {
            thread.suspend_count -= 1;
        }
        if old == 1 {
            self.runnable_thread = Some(thread.tid);
        }
        Ok(old)
    }

    pub fn take_runnable_thread(&mut self) -> Option<ThreadObject> {
        let tid = self.runnable_thread.take()?;
        self.threads
            .iter()
            .find(|thread| thread.tid == tid)
            .cloned()
    }

    /// The launcher checkpoint deliberately keeps the resumed worker runnable
    /// but unexecuted. This is observable at CreateProcessA #89.
    pub fn deferred_runnable_tid(&self) -> Option<u32> {
        self.runnable_thread
    }

    pub fn set_desktop_size(&mut self, width: u32, height: u32) {
        self.desktop_size = (width, height);
    }

    pub fn admit_bitmap(
        &mut self,
        request: LoadImageRequest,
        decoded_rgba: Vec<u8>,
        bits_va: u32,
        layout: DibLayout,
    ) -> Result<u32, &'static str> {
        if decoded_rgba.is_empty() || decoded_rgba.len() % 4 != 0 {
            return Err("decoded bitmap is not canonical RGBA8");
        }
        let handle = self.next_gdi_handle;
        self.next_gdi_handle = self
            .next_gdi_handle
            .checked_add(1)
            .ok_or("GDI handle overflow")?;
        self.gdi_objects.insert(
            handle,
            GdiObject::Bitmap(BitmapObject {
                resource_id: request.resource_id,
                width: request.width,
                height: request.height,
                planes: request.planes,
                bit_count: request.bit_count,
                compression: request.compression,
                size_image: request.size_image,
                clr_used: request.clr_used,
                dib: request.dib,
                decoded_rgba,
                palette: layout.palette,
                bits_va,
                bits_len: layout.bits_len,
                row_stride: layout.row_stride as u32,
                pixel_offset: layout.pixel_offset,
                stock: false,
            }),
        );
        Ok(handle)
    }

    pub fn allocate_gdi_bits(&mut self, bits_len: usize) -> Result<u32, &'static str> {
        let pages = bits_len
            .checked_add(0xfff)
            .ok_or("DIB allocation overflow")?
            & !0xfff;
        let va = self.next_gdi_dib_va;
        self.next_gdi_dib_va = va
            .checked_add(u32::try_from(pages).map_err(|_| "DIB allocation too large")?)
            .ok_or("DIB VA overflow")?;
        Ok(va)
    }

    pub fn bitmap_info(&self, handle: u32) -> Option<BitmapDiagnostic> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::Bitmap(bitmap) => Some(BitmapDiagnostic {
                resource_id: bitmap.resource_id,
                width: bitmap.width,
                height: bitmap.height,
                planes: bitmap.planes,
                bit_count: bitmap.bit_count,
                compression: bitmap.compression,
                size_image: bitmap.size_image,
                clr_used: bitmap.clr_used,
                dib_bytes: bitmap.dib.len(),
                rgba: bitmap.decoded_rgba.clone(),
                bits_va: bitmap.bits_va,
                bits_len: bitmap.bits_len,
                row_stride: bitmap.row_stride,
            }),
            GdiObject::DeviceContext(_) => None,
            GdiObject::Palette(_) => None,
        }
    }

    pub fn compatible_dc_info(&self, handle: u32) -> Option<(u32, i32, i32, u16)> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(dc) => match (&dc.compatible_with, &dc.target) {
                (DcCompatibility::Display, DcTarget::Memory { selected_bitmap }) => {
                    Some((*selected_bitmap, 1, 1, 1))
                }
                (_, DcTarget::WindowPaint { .. }) => None,
            },
            GdiObject::Bitmap(_) => None,
            GdiObject::Palette(_) => None,
        }
    }

    pub fn dc_target(&self, handle: u32) -> Option<Option<u32>> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { .. },
                ..
            }) => Some(None),
            GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { hwnd },
                ..
            }) => Some(Some(*hwnd)),
            _ => None,
        }
    }

    pub fn selected_bitmap(&self, handle: u32) -> Option<u32> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::Memory { selected_bitmap },
                ..
            }) => Some(*selected_bitmap),
            GdiObject::DeviceContext(_) => None,
            GdiObject::Bitmap(_) => None,
            GdiObject::Palette(_) => None,
        }
    }

    pub fn selected_palette(&self, handle: u32) -> Option<u32> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(dc) => Some(dc.selected_palette),
            _ => None,
        }
    }

    pub fn realized_palette(&self, handle: u32) -> Option<Option<u32>> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(dc) => Some(dc.realized_palette),
            _ => None,
        }
    }

    pub fn text_color(&self, handle: u32) -> Option<u32> {
        match self.gdi_objects.get(&handle) {
            Some(GdiObject::DeviceContext(dc)) => Some(dc.text_color),
            _ => None,
        }
    }

    pub fn text_state(&self, handle: u32) -> Option<(u32, u32, u32)> {
        match self.gdi_objects.get(&handle) {
            Some(GdiObject::DeviceContext(dc)) => Some((dc.text_color, dc.bk_color, dc.bk_mode)),
            _ => None,
        }
    }

    pub fn palette_entries(&self, handle: u32) -> Option<usize> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::Palette(palette) => Some(palette.entries.len()),
            _ => None,
        }
    }

    pub fn bitmap_selected_in_dc(&self, bitmap: u32) -> bool {
        self.gdi_objects.values().any(
            |value| matches!(value, GdiObject::DeviceContext(DeviceContext { target: DcTarget::Memory { selected_bitmap }, .. }) if *selected_bitmap == bitmap),
        )
    }

    pub fn bitmap_stock(&self, handle: u32) -> Option<bool> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::Bitmap(bitmap) => Some(bitmap.stock),
            _ => None,
        }
    }

    pub fn gdi_live(&self, handle: u32) -> bool {
        self.gdi_objects.contains_key(&handle)
    }

    pub fn active_paint_count(&self) -> usize {
        self.active_paints.len()
    }

    pub fn allocation_size(&self, pointer: u32) -> Option<u32> {
        self.allocations.get(&pointer).copied()
    }

    pub fn palette_info(&self, handle: u32) -> Option<(u16, usize)> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::Palette(palette) => Some((palette.version, palette.entries.len())),
            _ => None,
        }
    }

    pub fn exit_thread(&mut self, tid: u32, exit_code: u32) -> Result<(), &'static str> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.tid == tid)
            .ok_or("ExitThread unknown thread")?;
        thread.exit_code = Some(exit_code);
        Ok(())
    }
}

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

fn numeric_resource(
    memory: &impl GuestMemory,
    module_base: u32,
    type_id: u32,
    resource_id: u32,
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
    let type_directory = resource_directory_entry(memory, root, root, type_id)?;
    let resource_directory = resource_directory_entry(memory, root, type_directory, resource_id)?;
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
    wanted: u32,
) -> Result<u32, &'static str> {
    let named = u32::from(read_u16(memory, directory + 12)?);
    let ids = u32::from(read_u16(memory, directory + 14)?);
    let entries = directory
        .checked_add(16)
        .and_then(|value| value.checked_add(named * 8))
        .ok_or("resource entries overflow")?;
    for index in 0..ids {
        let entry = entries + index * 8;
        if read_u32(memory, entry)? != wanted {
            continue;
        }
        let child = read_u32(memory, entry + 4)?;
        if child & 0x8000_0000 == 0 {
            return Err("resource entry is not a directory");
        }
        return root
            .checked_add(child & 0x7fff_ffff)
            .ok_or("resource directory overflow");
    }
    Err("resource id not found")
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
    let type_directory = match resource_directory_entry(memory, root, root, RT_MESSAGETABLE) {
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
