//! Session-owned state shared by the WC3 launcher and its future child.
//!
//! This module is deliberately independent of VMX and guest address spaces.
//! `XpProcess` remains the personality for one address space; this session is
//! the authority for cross-process objects, names, HWNDs, and scheduling.

use std::collections::{HashMap, VecDeque};

use crate::ThisToThat;
use crate::process::{CURRENT_THREAD_PSEUDO_HANDLE, CreateProcessAFrame, ThreadObject, XpProcess};

pub type Pid = u32;
pub type Tid = u32;
pub type ObjectId = u64;
pub const LAUNCHER_PID: Pid = 1;
pub const LAUNCHER_TID: Tid = 1;
pub const PROCESS_HANDLE_BASE: u32 = 0x5743_6001;
pub const THREAD_HANDLE_BASE: u32 = 0x5743_5001;
pub const WINDOW_HANDLE_BASE: u32 = 0x5743_4001;
pub const DESKTOP_HWND: u32 = 0x5743_3000;

/// XP base priorities for NORMAL_PRIORITY_CLASS.
pub const fn normal_class_base_priority(priority: i32) -> Option<u8> {
    match priority {
        -15 => Some(1), // THREAD_PRIORITY_IDLE
        -2 => Some(6),  // THREAD_PRIORITY_LOWEST
        -1 => Some(7),  // THREAD_PRIORITY_BELOW_NORMAL
        0 => Some(8),   // THREAD_PRIORITY_NORMAL
        1 => Some(9),   // THREAD_PRIORITY_ABOVE_NORMAL
        2 => Some(10),  // THREAD_PRIORITY_HIGHEST
        15 => Some(15), // THREAD_PRIORITY_TIME_CRITICAL
        _ => None,
    }
}

pub type RegistryNodeId = u32;

const HKEY_CLASSES_ROOT: u32 = 0x8000_0000;
const HKEY_CURRENT_USER: u32 = 0x8000_0001;
const HKEY_LOCAL_MACHINE: u32 = 0x8000_0002;
const HKEY_USERS: u32 = 0x8000_0003;
const HKEY_CURRENT_CONFIG: u32 = 0x8000_0005;
const ROOT_NODE_BASE: RegistryNodeId = 0xffff_ff00;
const REGISTRY_ROOTS: [(u32, &str); 5] = [
    (HKEY_CLASSES_ROOT, "HKEY_CLASSES_ROOT"),
    (HKEY_CURRENT_USER, "HKEY_CURRENT_USER"),
    (HKEY_LOCAL_MACHINE, "HKEY_LOCAL_MACHINE"),
    (HKEY_USERS, "HKEY_USERS"),
    (HKEY_CURRENT_CONFIG, "HKEY_CURRENT_CONFIG"),
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RegistryEncoding {
    Utf16Le,
    Utf8,
}

/// A POD-sized reference into the immutable `.reg` backing.  The index owns no
/// key path strings or value maps; values are decoded only by a future value API.
#[derive(Clone, Copy, Debug)]
pub struct RegistryKeyIndexEntry {
    root: u32,
    header_start: u32,
    header_end: u32,
    tail_start: u32,
    tail_end: u32,
    body_start: u32,
    body_end: u32,
    path_hash: u64,
}

pub struct RegistryValue {
    pub ty: u32,
    pub bytes: Vec<u8>,
}

/// Reserved for future writes.  It remains empty for this read-only loader path.
pub struct RegistryOverlay {
    created_keys: HashMap<String, RegistryNodeId>,
    deleted_keys: std::collections::HashSet<String>,
    generation: u64,
}

pub struct RegistryImage {
    backing: std::sync::Arc<Vec<u8>>,
    pub encoding: RegistryEncoding,
    entries: Vec<RegistryKeyIndexEntry>,
    scanned_bytes: usize,
    /// Lazy value materialization is deliberately empty during key indexing.
    loaded_values: HashMap<RegistryNodeId, HashMap<String, RegistryValue>>,
    mutations: RegistryOverlay,
}
pub enum RegistryState {
    Unloaded,
    Ready(RegistryImage),
}

impl RegistryImage {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        Self::index(bytes.to_vec(), |_, _| {})
    }

    /// Build the initial index by walking raw physical lines once.  In particular,
    /// this never decodes registry values, builds a full text copy, or allocates a
    /// path string for each key.
    pub fn index(
        bytes: Vec<u8>,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<Self, &'static str> {
        let encoding = if bytes.starts_with(&[0xff, 0xfe]) {
            if (bytes.len() - 2) % 2 != 0 {
                return Err("registry UTF-16 odd trailing byte");
            }
            RegistryEncoding::Utf16Le
        } else {
            RegistryEncoding::Utf8
        };
        let offset = match encoding {
            RegistryEncoding::Utf16Le => 2,
            RegistryEncoding::Utf8 if bytes.starts_with(&[0xef, 0xbb, 0xbf]) => 3,
            RegistryEncoding::Utf8 => 0,
        };
        let backing = std::sync::Arc::new(bytes);
        let mut entries: Vec<RegistryKeyIndexEntry> = Vec::with_capacity(65_536);
        let mut cursor = offset;
        let mut previous: Option<usize> = None;
        let mut next_progress = 1024 * 1024;

        while cursor < backing.len() {
            let old_cursor = cursor;
            let line_start = cursor;
            let stride = width(encoding);
            let mut line_end = backing.len();
            while cursor + stride <= backing.len() {
                if unit(&backing, cursor, encoding) == 10 {
                    line_end = cursor;
                    cursor += stride;
                    break;
                }
                cursor += stride;
                if cursor >= next_progress {
                    progress(cursor, entries.len());
                    while next_progress <= cursor {
                        next_progress += 1024 * 1024;
                    }
                }
            }
            if cursor <= old_cursor {
                return Err("registry index cursor did not advance");
            }

            let (trim_start, trim_end) = trim_line(&backing, line_start, line_end, encoding);
            if let Some((root, tail_start, tail_end, path_hash)) =
                parse_key_header(&backing, trim_start, trim_end, encoding)
            {
                if let Some(previous) = previous {
                    entries[previous].body_end = u32_offset(line_start)?;
                }
                entries.push(RegistryKeyIndexEntry {
                    root,
                    header_start: u32_offset(line_start)?,
                    header_end: u32_offset(line_end)?,
                    tail_start: u32_offset(tail_start)?,
                    tail_end: u32_offset(tail_end)?,
                    body_start: u32_offset(cursor)?,
                    body_end: 0,
                    path_hash,
                });
                previous = Some(entries.len() - 1);
            }
            if cursor >= next_progress {
                progress(cursor, entries.len());
                while next_progress <= cursor {
                    next_progress += 1024 * 1024;
                }
            }
        }
        if let Some(previous) = previous {
            entries[previous].body_end = u32_offset(backing.len())?;
        }
        entries.sort_unstable_by_key(|entry| (entry.root, entry.path_hash));
        Ok(Self {
            backing,
            encoding,
            entries,
            scanned_bytes: cursor,
            loaded_values: HashMap::new(),
            mutations: RegistryOverlay {
                created_keys: HashMap::new(),
                deleted_keys: std::collections::HashSet::new(),
                generation: 0,
            },
        })
    }

    pub fn root(&self, hkey: u32) -> Option<RegistryNodeId> {
        REGISTRY_ROOTS
            .iter()
            .position(|(root, _)| *root == hkey)
            .map(|index| ROOT_NODE_BASE + index as u32)
    }

    pub fn child_path(&self, node: RegistryNodeId, path: &str) -> Option<RegistryNodeId> {
        let path = path.trim_matches('\\');
        if let Some(root) = root_from_node(node) {
            return if path.is_empty() {
                Some(node)
            } else {
                self.find(root, path)
            };
        }
        let entry = *self.entries.get(node as usize)?;
        if path.is_empty() {
            return Some(node);
        }
        let base = self.decode_tail(entry)?;
        let full = if base.is_empty() {
            path.to_owned()
        } else {
            format!("{base}\\{path}")
        };
        self.find(entry.root, &full)
    }

    pub fn stats(&self) -> (usize, usize) {
        (REGISTRY_ROOTS.len(), self.entries.len())
    }

    pub fn index_entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn scanned_bytes(&self) -> usize {
        self.scanned_bytes
    }
    pub fn values_loaded(&self, node: RegistryNodeId) -> bool {
        self.loaded_values.contains_key(&node)
    }

    pub fn value(&self, node: RegistryNodeId, name: &str) -> Option<&RegistryValue> {
        self.loaded_values.get(&node)?.get(&canonical(name))
    }

    pub fn key_identity(&self, node: RegistryNodeId) -> Option<(u32, String)> {
        if let Some(root) = root_from_node(node) {
            return Some((root, String::new()));
        }
        let entry = *self.entries.get(node as usize)?;
        Some((entry.root, self.decode_tail(entry)?))
    }

    fn find(&self, root: u32, path: &str) -> Option<RegistryNodeId> {
        let hash = hash_query(path, self.encoding);
        let first = self
            .entries
            .partition_point(|entry| (entry.root, entry.path_hash) < (root, hash));
        let end = self
            .entries
            .partition_point(|entry| (entry.root, entry.path_hash) <= (root, hash));
        self.entries[first..end]
            .iter()
            .enumerate()
            .find_map(|(offset, entry)| {
                tail_matches(&self.backing, *entry, path, self.encoding)
                    .then_some((first + offset) as RegistryNodeId)
            })
    }

    fn decode_tail(&self, entry: RegistryKeyIndexEntry) -> Option<String> {
        ThisToThat::decode_span(
            &self.backing[entry.tail_start as usize..entry.tail_end as usize],
            self.encoding == RegistryEncoding::Utf16Le,
        )
        .ok()
    }

    /// Kept intentionally lazy for the next observed value API.  It only ever
    /// decodes one key's body range, never the 27 MiB registry image.
    pub fn ensure_values_loaded(&mut self, node: RegistryNodeId) -> Result<(), &'static str> {
        if self.loaded_values.contains_key(&node) {
            return Ok(());
        }
        let entry = *self.entries.get(node as usize).ok_or("registry node")?;
        let text = ThisToThat::decode_span(
            &self.backing[entry.body_start as usize..entry.body_end as usize],
            self.encoding == RegistryEncoding::Utf16Le,
        )?;
        let mut values = HashMap::new();
        for line in text.lines() {
            if let Some((name, data)) = line.trim().split_once('=') {
                let name = if name == "@" {
                    String::new()
                } else {
                    unquote(name)?
                };
                if let Some(hex) = data.strip_prefix("dword:") {
                    let value = u32::from_str_radix(hex.trim(), 16)
                        .map_err(|_| "registry dword")?;
                    values.insert(
                        canonical(&name),
                        RegistryValue {
                            ty: 4,
                            bytes: value.to_le_bytes().to_vec(),
                        },
                    );
                    continue;
                }
                if let Some(hex) = data.strip_prefix("hex:") {
                    let bytes = hex
                        .split(',')
                        .filter_map(|value| {
                            u8::from_str_radix(value.trim().trim_end_matches('\\'), 16).ok()
                        })
                        .collect();
                    values.insert(canonical(&name), RegistryValue { ty: 3, bytes });
                }
            }
        }
        self.loaded_values.insert(node, values);
        Ok(())
    }
}

fn u32_offset(offset: usize) -> Result<u32, &'static str> {
    u32::try_from(offset).map_err(|_| "registry exceeds u32 offsets")
}
fn width(encoding: RegistryEncoding) -> usize {
    if encoding == RegistryEncoding::Utf16Le {
        2
    } else {
        1
    }
}
fn unit(bytes: &[u8], at: usize, encoding: RegistryEncoding) -> u16 {
    match encoding {
        RegistryEncoding::Utf16Le => u16::from_le_bytes([bytes[at], bytes[at + 1]]),
        RegistryEncoding::Utf8 => bytes[at] as u16,
    }
}
fn is_space(value: u16) -> bool {
    value == b' ' as u16 || value == b'\t' as u16 || value == b'\r' as u16
}
fn fold(value: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&value) {
        value + 32
    } else {
        value
    }
}
fn trim_line(
    bytes: &[u8],
    mut start: usize,
    mut end: usize,
    encoding: RegistryEncoding,
) -> (usize, usize) {
    let stride = width(encoding);
    while start < end && is_space(unit(bytes, start, encoding)) {
        start += stride;
    }
    while start < end && is_space(unit(bytes, end - stride, encoding)) {
        end -= stride;
    }
    (start, end)
}
fn ascii_at(bytes: &[u8], at: usize, encoding: RegistryEncoding, expected: u8) -> bool {
    fold(unit(bytes, at, encoding)) == expected.to_ascii_lowercase() as u16
}
fn parse_key_header(
    bytes: &[u8],
    start: usize,
    end: usize,
    encoding: RegistryEncoding,
) -> Option<(u32, usize, usize, u64)> {
    let stride = width(encoding);
    if end <= start + 2 * stride
        || unit(bytes, start, encoding) != b'[' as u16
        || unit(bytes, end - stride, encoding) != b']' as u16
    {
        return None;
    }
    let content_start = start + stride;
    let content_end = end - stride;
    for (root, name) in REGISTRY_ROOTS {
        let name_bytes = name.as_bytes();
        let root_end = content_start.checked_add(name_bytes.len() * stride)?;
        if root_end > content_end
            || !name_bytes.iter().enumerate().all(|(index, byte)| {
                ascii_at(bytes, content_start + index * stride, encoding, *byte)
            })
        {
            continue;
        }
        if root_end == content_end {
            return Some((root, root_end, root_end, FNV_OFFSET));
        }
        if unit(bytes, root_end, encoding) != b'\\' as u16 {
            return None;
        }
        let tail_start = root_end + stride;
        if tail_start >= content_end {
            return None;
        }
        return Some((
            root,
            tail_start,
            content_end,
            hash_span(bytes, tail_start, content_end, encoding),
        ));
    }
    None
}
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
fn hash_unit(mut hash: u64, value: u16) -> u64 {
    hash ^= fold(value) as u64;
    hash.wrapping_mul(FNV_PRIME)
}
fn hash_span(bytes: &[u8], start: usize, end: usize, encoding: RegistryEncoding) -> u64 {
    let mut hash = FNV_OFFSET;
    let stride = width(encoding);
    let mut at = start;
    while at < end {
        hash = hash_unit(hash, unit(bytes, at, encoding));
        at += stride;
    }
    hash
}
fn hash_query(path: &str, encoding: RegistryEncoding) -> u64 {
    match encoding {
        RegistryEncoding::Utf16Le => path.encode_utf16().fold(FNV_OFFSET, hash_unit),
        RegistryEncoding::Utf8 => path
            .bytes()
            .fold(FNV_OFFSET, |hash, value| hash_unit(hash, value as u16)),
    }
}
fn tail_matches(
    bytes: &[u8],
    entry: RegistryKeyIndexEntry,
    path: &str,
    encoding: RegistryEncoding,
) -> bool {
    let mut at = entry.tail_start as usize;
    let end = entry.tail_end as usize;
    let stride = width(encoding);
    match encoding {
        RegistryEncoding::Utf16Le => {
            path.encode_utf16().all(|expected| {
                let matched = at < end && fold(unit(bytes, at, encoding)) == fold(expected);
                at += stride;
                matched
            }) && at == end
        }
        RegistryEncoding::Utf8 => {
            path.bytes().all(|expected| {
                let matched = at < end && fold(unit(bytes, at, encoding)) == fold(expected as u16);
                at += stride;
                matched
            }) && at == end
        }
    }
}
fn root_from_node(node: RegistryNodeId) -> Option<u32> {
    let index = node.checked_sub(ROOT_NODE_BASE)? as usize;
    REGISTRY_ROOTS.get(index).map(|(root, _)| *root)
}
fn canonical(name: &str) -> String {
    name.bytes()
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}
fn unquote(value: &str) -> Result<String, &'static str> {
    let body = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or("registry quote")?;
    Ok(body.replace("\\\\", "\\").replace("\\\"", "\""))
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ThreadKey {
    pub pid: Pid,
    pub tid: Tid,
}

#[cfg(test)]
crate::wc3_session_tests_1!();

#[derive(Clone, Debug)]
pub struct EventObject {
    pub manual_reset: bool,
    pub signaled: bool,
    pub name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MutexObject {
    pub name: Option<String>,
    pub owner: Option<ThreadKey>,
    pub recursion: u32,
    pub abandoned: bool,
}

#[derive(Clone, Debug)]
pub struct ProcessObject {
    pub pid: Pid,
    pub exit_code: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ThreadSessionObject {
    pub key: ThreadKey,
    pub exit_code: Option<u32>,
    /// Win32 THREAD_PRIORITY_* level. The XP NORMAL_PRIORITY_CLASS base is 8.
    pub priority_level: i32,
}

#[derive(Clone, Debug)]
pub enum SessionObject {
    Event(EventObject),
    Mutex(MutexObject),
    Process(ProcessObject),
    Thread(ThreadSessionObject),
}

#[derive(Clone, Debug)]
pub struct WindowObject {
    pub owner: ThreadKey,
    pub class: String,
    pub wndproc: u32,
    pub title: String,
    pub ex_style: u32,
    pub style: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub parent: u32,
    pub menu: u32,
    pub instance: u32,
    pub param: u32,
    pub visible: bool,
    pub paint_pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateWindowRequest {
    pub owner: ThreadKey,
    pub class: String,
    pub wndproc: u32,
    pub title: String,
    pub ex_style: u32,
    pub style: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub parent: u32,
    pub menu: u32,
    pub instance: u32,
    pub param: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowPresentation {
    Show {
        hwnd: u32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Hide {
        hwnd: u32,
    },
    Destroy {
        hwnd: u32,
    },
}

#[derive(Clone, Debug)]
pub struct HandleEntry {
    pub object: ObjectId,
    pub inheritable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaitRequest {
    pub key: ThreadKey,
    pub return_address: u32,
    pub count: u32,
    pub handles_pointer: u32,
    pub handles: [u32; 2],
    pub wait_all: u32,
    pub timeout: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CriticalSectionWait {
    pub key: ThreadKey,
    pub address: u32,
    pub provider_id: u32,
    pub esp: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedWait {
    pub request: WaitRequest,
    pub result: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetEventResult {
    pub manual_reset: bool,
    pub was_signaled: bool,
    pub woken: Vec<CompletedWait>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResetEventResult {
    pub manual_reset: bool,
    pub was_signaled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DestroyWindowResult {
    pub was_focused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProcessRequest {
    pub frame: CreateProcessAFrame,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateEventRequest {
    pub name: Option<String>,
    pub manual_reset: bool,
    pub initial_state: bool,
    pub inheritable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMutexRequest {
    pub name: Option<String>,
    pub initial_owner: bool,
    pub inheritable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GetExitCodeProcessRequest {
    pub pid: Pid,
    pub tid: Tid,
    pub handle: u32,
    pub exit_code_pointer: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadImageRequest {
    pub resource_id: u32,
    pub dib: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub size_image: u32,
    pub clr_used: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowBlitRequest {
    pub dst_hdc: u32,
    pub hwnd: u32,
    pub dst_x: u32,
    pub dst_y: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub source_bitmap: u32,
    pub src_hdc: u32,
    pub bits_va: u32,
    pub bottom_up: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowTextRequest {
    pub hwnd: u32,
    pub hdc: u32,
    pub text: String,
    pub rect: [i32; 4],
    pub colorref: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedChild {
    pub pid: Pid,
    pub tid: Tid,
    pub process_handle: u32,
    pub thread_handle: u32,
    pub process_object: ObjectId,
    pub thread_object: ObjectId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionRequest {
    CreateProcess(CreateProcessRequest),
    GetExitCodeProcess(GetExitCodeProcessRequest),
    SetEvent {
        pid: Pid,
        tid: Tid,
        handle: u32,
    },
    ResetEvent {
        pid: Pid,
        tid: Tid,
        handle: u32,
    },
    CreateEvent(CreateEventRequest),
    CreateMutex {
        key: ThreadKey,
        request: CreateMutexRequest,
    },
    ReleaseMutex {
        key: ThreadKey,
        handle: u32,
    },
    LoadImage(LoadImageRequest),
    CreateWindow(CreateWindowRequest),
    ShowWindow {
        pid: Pid,
        hwnd: u32,
        show: u32,
    },
    DestroyWindow {
        pid: Pid,
        hwnd: u32,
    },
    UpdateWindow {
        pid: Pid,
        hwnd: u32,
    },
    SetFocus {
        pid: Pid,
        hwnd: u32,
    },
    BeginPaint {
        pid: Pid,
        hwnd: u32,
        paint_struct: u32,
    },
    CloseHandle {
        pid: Pid,
        handle: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestCall {
    pub address: u32,
    pub arguments: [u32; 4],
    pub completion_eax: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersonalityAction {
    Return(u32),
    Session(SessionRequest),
    WindowBlit(WindowBlitRequest),
    WindowText(WindowTextRequest),
    Block(WaitRequest),
    CallGuest(GuestCall),
    ExitThread(u32),
    ExitProcess(u32),
}

pub struct Wc3Process {
    pub pid: Pid,
    pub xp: XpProcess,
    pub handles: HashMap<u32, HandleEntry>,
}

pub struct Wc3Session {
    pub assets: crate::assets::Wc3AssetCache,
    pub registry: RegistryState,
    pub processes: HashMap<Pid, Wc3Process>,
    pub objects: HashMap<ObjectId, SessionObject>,
    pub names: HashMap<String, ObjectId>,
    pub windows: HashMap<u32, WindowObject>,
    pub focused_window: Option<u32>,
    pub runnable: VecDeque<ThreadKey>,
    pub blocked: HashMap<ThreadKey, WaitRequest>,
    pub critical_waiters: VecDeque<CriticalSectionWait>,
    pub next_pid: Pid,
    pub next_tid: Tid,
    pub next_object: ObjectId,
    pub next_process_handle: u32,
    pub next_thread_handle: u32,
    pub next_event_handle: u32,
    pub next_window_handle: u32,
    pub sequence: u64,
    pub window_presentation: Option<WindowPresentation>,
}

impl Wc3Session {
    pub fn new(xp: XpProcess) -> Self {
        let pid = LAUNCHER_PID;
        let mut processes = HashMap::new();
        processes.insert(
            pid,
            Wc3Process {
                pid,
                xp,
                handles: HashMap::new(),
            },
        );
        Self {
            assets: crate::assets::Wc3AssetCache::default(),
            registry: RegistryState::Unloaded,
            processes,
            objects: HashMap::new(),
            names: HashMap::new(),
            windows: HashMap::new(),
            focused_window: None,
            runnable: VecDeque::from([ThreadKey {
                pid: LAUNCHER_PID,
                tid: LAUNCHER_TID,
            }]),
            blocked: HashMap::new(),
            critical_waiters: VecDeque::new(),
            next_pid: 2,
            next_tid: 2,
            next_object: 1,
            next_process_handle: PROCESS_HANDLE_BASE,
            next_thread_handle: THREAD_HANDLE_BASE,
            next_event_handle: 0x5743_2001,
            next_window_handle: WINDOW_HANDLE_BASE,
            sequence: 0,
            window_presentation: None,
        }
    }

    pub fn enqueue(&mut self, key: ThreadKey) {
        if !self.runnable.contains(&key) {
            self.runnable.push_back(key);
        }
    }

    /// Removes the highest-base-priority runnable thread accepted by `available`.
    /// Equal priorities retain queue order, giving round-robin behavior.
    pub fn take_highest_runnable(
        &mut self,
        mut available: impl FnMut(ThreadKey) -> bool,
    ) -> Option<ThreadKey> {
        let mut best: Option<(usize, u8)> = None;
        for (index, key) in self.runnable.iter().copied().enumerate() {
            if !available(key) {
                continue;
            }
            let priority = self.thread_base_priority(key).unwrap_or(8);
            if best.is_none_or(|(_, current)| priority > current) {
                best = Some((index, priority));
            }
        }
        self.runnable.remove(best?.0)
    }

    pub fn thread_base_priority(&self, key: ThreadKey) -> Option<u8> {
        self.objects.values().find_map(|object| match object {
            SessionObject::Thread(thread) if thread.key == key => {
                normal_class_base_priority(thread.priority_level)
            }
            _ => None,
        })
    }

    fn resolve_thread_key(&self, caller: ThreadKey, handle: u32) -> Result<ThreadKey, u32> {
        if handle == CURRENT_THREAD_PSEUDO_HANDLE {
            return Ok(caller);
        }
        let object = self
            .process(caller.pid)
            .and_then(|process| process.handles.get(&handle))
            .ok_or(6u32)?
            .object;
        let Some(SessionObject::Thread(thread)) = self.objects.get(&object) else {
            return Err(6);
        };
        Ok(thread.key)
    }

    pub fn set_thread_priority(
        &mut self,
        caller: ThreadKey,
        handle: u32,
        priority: i32,
    ) -> Result<(ThreadKey, i32, u8), u32> {
        let base = normal_class_base_priority(priority).ok_or(87u32)?;
        let target = self.resolve_thread_key(caller, handle)?;
        let thread = self.objects.values_mut().find_map(|object| match object {
            SessionObject::Thread(thread) if thread.key == target => Some(thread),
            _ => None,
        }).ok_or(6u32)?;
        let old = thread.priority_level;
        thread.priority_level = priority;
        Ok((target, old, base))
    }

    pub fn get_thread_priority(
        &self,
        caller: ThreadKey,
        handle: u32,
    ) -> Result<(ThreadKey, i32, u8), u32> {
        let target = self.resolve_thread_key(caller, handle)?;
        let thread = self.objects.values().find_map(|object| match object {
            SessionObject::Thread(thread) if thread.key == target => Some(thread),
            _ => None,
        }).ok_or(6u32)?;
        let level = thread.priority_level;
        let base = normal_class_base_priority(level).ok_or(87u32)?;
        Ok((target, level, base))
    }

    pub fn defer_wait(&mut self, request: WaitRequest) {
        self.blocked.insert(request.key, request);
    }

    pub fn block_critical_section(&mut self, wait: CriticalSectionWait) -> Result<(), &'static str> {
        if self.critical_waiters.iter().any(|entry| entry.key == wait.key) {
            return Err("thread already blocked on a critical section");
        }
        self.critical_waiters.push_back(wait);
        Ok(())
    }

    pub fn take_critical_waiter(&mut self, pid: Pid, address: u32) -> Option<CriticalSectionWait> {
        let index = self
            .critical_waiters
            .iter()
            .position(|wait| wait.key.pid == pid && wait.address == address)?;
        self.critical_waiters.remove(index)
    }

    pub fn note(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    pub fn process(&self, pid: Pid) -> Option<&Wc3Process> {
        self.processes.get(&pid)
    }

    pub fn process_mut(&mut self, pid: Pid) -> Option<&mut Wc3Process> {
        self.processes.get_mut(&pid)
    }

    pub fn launcher(&self) -> &Wc3Process {
        self.process(LAUNCHER_PID).expect("launcher process")
    }

    pub fn launcher_mut(&mut self) -> &mut Wc3Process {
        self.process_mut(LAUNCHER_PID).expect("launcher process")
    }

    /// Transfer the process personality's one-shot notification into the
    /// session scheduler. The personality may observe the Windows operation;
    /// only the session decides which logical thread is runnable.
    pub fn absorb_runnable_thread(&mut self) -> Option<ThreadObject> {
        let thread = self.launcher_mut().xp.take_runnable_thread();
        if let Some(thread) = thread.clone() {
            self.register_thread(LAUNCHER_PID, thread.handle, thread.tid);
            self.enqueue(ThreadKey {
                pid: LAUNCHER_PID,
                tid: thread.tid,
            });
        }
        thread
    }

    pub fn deferred_runnable_tid(&self) -> Option<Tid> {
        self.runnable
            .iter()
            .find_map(|key| (key.pid == LAUNCHER_PID && key.tid != LAUNCHER_TID).then_some(key.tid))
    }

    pub fn focused_root(&self) -> Option<u32> {
        self.focused_window
    }

    pub fn create_window(&mut self, request: CreateWindowRequest) -> Result<u32, &'static str> {
        if request.parent != DESKTOP_HWND && !self.windows.contains_key(&request.parent) {
            return Err("unknown window parent");
        }
        let hwnd = self.next_window_handle;
        self.next_window_handle = self
            .next_window_handle
            .checked_add(1)
            .ok_or("HWND overflow")?;
        self.windows.insert(
            hwnd,
            WindowObject {
                owner: request.owner,
                class: request.class,
                wndproc: request.wndproc,
                title: request.title,
                ex_style: request.ex_style,
                style: request.style,
                x: request.x,
                y: request.y,
                width: request.width,
                height: request.height,
                parent: request.parent,
                menu: request.menu,
                instance: request.instance,
                param: request.param,
                visible: request.style & 0x1000_0000 != 0,
                paint_pending: false,
            },
        );
        Ok(hwnd)
    }

    pub fn show_window(&mut self, hwnd: u32, show: u32) -> Result<u32, &'static str> {
        let window = self.windows.get_mut(&hwnd).ok_or("unknown window")?;
        let old = window.visible;
        let visible = show != 0;
        window.visible = visible;
        window.paint_pending = visible;
        if old != visible {
            self.window_presentation = Some(if visible {
                WindowPresentation::Show {
                    hwnd,
                    x: window.x,
                    y: window.y,
                    width: window.width,
                    height: window.height,
                }
            } else {
                WindowPresentation::Hide { hwnd }
            });
        }
        Ok(old as u32)
    }

    pub fn destroy_window(
        &mut self,
        pid: Pid,
        hwnd: u32,
    ) -> Result<DestroyWindowResult, &'static str> {
        let window = self
            .windows
            .get(&hwnd)
            .ok_or("DestroyWindow unknown window")?;
        if window.owner.pid != pid {
            return Err("DestroyWindow window owner mismatch");
        }
        let was_focused = self.focused_window == Some(hwnd);
        self.windows.remove(&hwnd);
        if was_focused {
            self.focused_window = None;
        }
        self.window_presentation = Some(WindowPresentation::Destroy { hwnd });
        Ok(DestroyWindowResult { was_focused })
    }

    pub fn update_window(&mut self, hwnd: u32) -> Result<u32, &'static str> {
        let window = self.windows.get_mut(&hwnd).ok_or("unknown window")?;
        let pending = window.paint_pending;
        window.paint_pending = false;
        Ok(pending as u32)
    }

    pub fn begin_paint_window(&self, pid: Pid, hwnd: u32) -> Result<(u32, u32), &'static str> {
        let window = self.windows.get(&hwnd).ok_or("unknown paint window")?;
        if window.owner.pid != pid || !window.visible {
            return Err("BeginPaint requires visible owned window");
        }
        Ok((window.width, window.height))
    }

    pub fn set_focus(&mut self, hwnd: u32) -> Result<u32, &'static str> {
        if hwnd != 0 && !self.windows.contains_key(&hwnd) {
            return Err("unknown window");
        }
        Ok(self.focused_window.replace(hwnd).unwrap_or(0))
    }

    pub fn take_window_presentation(&mut self) -> Option<WindowPresentation> {
        self.window_presentation.take()
    }

    pub fn create_child(&mut self) -> CreatedChild {
        let pid = self.next_pid;
        self.next_pid += 1;
        let tid = self.next_tid;
        self.next_tid += 1;
        let process_object = self.next_object;
        self.next_object += 1;
        let thread_object = self.next_object;
        self.next_object += 1;
        self.objects.insert(
            process_object,
            SessionObject::Process(ProcessObject {
                pid,
                exit_code: None,
            }),
        );
        self.objects.insert(
            thread_object,
            SessionObject::Thread(ThreadSessionObject {
                key: ThreadKey { pid, tid },
                exit_code: None,
                priority_level: 0,
            }),
        );
        let process_handle = self.next_process_handle;
        self.next_process_handle += 1;
        let thread_handle = self.next_thread_handle;
        self.next_thread_handle += 1;
        self.launcher_mut().handles.insert(
            process_handle,
            HandleEntry {
                object: process_object,
                inheritable: false,
            },
        );
        self.launcher_mut().handles.insert(
            thread_handle,
            HandleEntry {
                object: thread_object,
                inheritable: false,
            },
        );
        let mut xp = XpProcess::new_child();
        let registry_base = 0x5743_8001u32.saturating_add(pid.saturating_mul(0x100));
        xp.set_registry_handle_base(registry_base)
            .expect("new process has no registry handles");
        self.processes.insert(
            pid,
            Wc3Process {
                pid,
                xp,
                handles: HashMap::new(),
            },
        );
        CreatedChild {
            pid,
            tid,
            process_handle,
            thread_handle,
            process_object,
            thread_object,
        }
    }

    pub fn register_thread(&mut self, pid: Pid, handle: u32, tid: Tid) -> ObjectId {
        let object = self.next_object;
        self.next_object += 1;
        self.objects.insert(
            object,
            SessionObject::Thread(ThreadSessionObject {
                key: ThreadKey { pid, tid },
                exit_code: None,
                priority_level: 0,
            }),
        );
        if let Some(process) = self.process_mut(pid) {
            process.handles.insert(
                handle,
                HandleEntry {
                    object,
                    inheritable: false,
                },
            );
        }
        self.next_tid = self.next_tid.max(tid + 1);
        self.next_thread_handle = self.next_thread_handle.max(handle + 1);
        object
    }

    /// Create a child-process thread object without starting guest execution.
    /// The coordinator prepares its stack/TEB before calling this method and
    /// enqueues it only after both the context and session object exist.
    pub fn create_child_thread(&mut self, pid: Pid) -> Result<(ThreadKey, u32), &'static str> {
        if pid == LAUNCHER_PID || !self.processes.contains_key(&pid) {
            return Err("child thread requires a live child process");
        }
        let tid = self.next_tid;
        let handle = self.next_thread_handle;
        let object = self.next_object;
        let next_tid = tid.checked_add(1).ok_or("thread ID overflow")?;
        let next_handle = handle.checked_add(1).ok_or("thread handle overflow")?;
        let next_object = object.checked_add(1).ok_or("thread object overflow")?;
        let key = ThreadKey { pid, tid };
        self.next_tid = next_tid;
        self.next_thread_handle = next_handle;
        self.next_object = next_object;
        self.objects.insert(
            object,
            SessionObject::Thread(ThreadSessionObject {
                key,
                exit_code: None,
                priority_level: 0,
            }),
        );
        self.process_mut(pid).expect("validated child process").handles.insert(
            handle,
            HandleEntry {
                object,
                inheritable: false,
            },
        );
        Ok((key, handle))
    }

    pub fn create_event(&mut self, pid: Pid, request: CreateEventRequest) -> (u32, bool) {
        let (object, already_exists) = if let Some(name) = request.name.as_ref() {
            if let Some(&object) = self.names.get(name) {
                if !matches!(self.objects.get(&object), Some(SessionObject::Event(_))) {
                    // The caller has the guest TID and records ERROR_INVALID_HANDLE.
                    return (0, false);
                }
                (object, true)
            } else {
                let object = self.next_object;
                self.next_object += 1;
                self.objects.insert(
                    object,
                    SessionObject::Event(EventObject {
                        manual_reset: request.manual_reset,
                        signaled: request.initial_state,
                        name: request.name.clone(),
                    }),
                );
                self.names.insert(name.clone(), object);
                (object, false)
            }
        } else {
            let object = self.next_object;
            self.next_object += 1;
            self.objects.insert(
                object,
                SessionObject::Event(EventObject {
                    manual_reset: request.manual_reset,
                    signaled: request.initial_state,
                    name: None,
                }),
            );
            (object, false)
        };
        let handle = self.next_event_handle;
        self.next_event_handle += 1;
        self.process_mut(pid).expect("event owner").handles.insert(
            handle,
            HandleEntry {
                object,
                inheritable: request.inheritable,
            },
        );
        (handle, already_exists)
    }

    pub fn create_mutex(
        &mut self,
        key: ThreadKey,
        request: CreateMutexRequest,
    ) -> Result<(u32, bool), u32> {
        if !self.processes.contains_key(&key.pid) {
            return Err(6);
        }
        let existing = request
            .name
            .as_ref()
            .and_then(|name| self.names.get(name))
            .copied();
        let (object, already_exists) = if let Some(object) = existing {
            if !matches!(self.objects.get(&object), Some(SessionObject::Mutex(_))) {
                return Err(6);
            }
            (object, true)
        } else {
            let object = self.next_object;
            self.next_object += 1;
            self.objects.insert(
                object,
                SessionObject::Mutex(MutexObject {
                    name: request.name.clone(),
                    owner: request.initial_owner.then_some(key),
                    recursion: u32::from(request.initial_owner),
                    abandoned: false,
                }),
            );
            if let Some(name) = request.name {
                self.names.insert(name, object);
            }
            (object, false)
        };
        let handle = self.next_event_handle;
        self.next_event_handle += 1;
        self.process_mut(key.pid)
            .expect("mutex owner")
            .handles
            .insert(
                handle,
                HandleEntry {
                    object,
                    inheritable: request.inheritable,
                },
            );
        Ok((handle, already_exists))
    }

    pub fn release_mutex(
        &mut self,
        key: ThreadKey,
        handle: u32,
    ) -> Result<Vec<CompletedWait>, u32> {
        let object = self
            .process(key.pid)
            .and_then(|process| process.handles.get(&handle))
            .ok_or(6u32)?
            .object;
        let Some(SessionObject::Mutex(mutex)) = self.objects.get_mut(&object) else {
            return Err(6);
        };
        if mutex.owner != Some(key) {
            return Err(288);
        }
        mutex.recursion -= 1;
        if mutex.recursion != 0 {
            return Ok(Vec::new());
        }
        mutex.owner = None;
        self.reevaluate_blocked_waits().map_err(|_| 87)
    }

    pub fn close_handle(&mut self, pid: Pid, handle: u32) -> bool {
        let Some(entry) = self
            .process_mut(pid)
            .and_then(|process| process.handles.remove(&handle))
        else {
            return false;
        };
        let name = match self.objects.get(&entry.object) {
            Some(SessionObject::Event(event)) => event.name.clone(),
            Some(SessionObject::Mutex(mutex)) => mutex.name.clone(),
            _ => return true,
        };
        let still_open = self.processes.values().any(|process| {
            process
                .handles
                .values()
                .any(|held| held.object == entry.object)
        });
        if !still_open {
            if let Some(name) = name {
                self.names.remove(&name);
            }
            self.objects.remove(&entry.object);
        }
        true
    }

    pub fn signal_thread(
        &mut self,
        key: ThreadKey,
        exit_code: u32,
    ) -> Result<Vec<CompletedWait>, &'static str> {
        let mut found = false;
        for object in self.objects.values_mut() {
            if let SessionObject::Thread(thread) = object {
                if thread.key == key {
                    thread.exit_code = Some(exit_code);
                    found = true;
                }
            }
        }
        if !found {
            return Err("ExitThread thread object missing");
        }
        self.runnable.retain(|queued| *queued != key);
        self.blocked.remove(&key);
        self.critical_waiters.retain(|wait| wait.key != key);
        self.abandon_mutexes(|owner| owner == key);
        self.reevaluate_blocked_waits()
    }

    fn abandon_mutexes(&mut self, matches: impl Fn(ThreadKey) -> bool) {
        for object in self.objects.values_mut() {
            if let SessionObject::Mutex(mutex) = object {
                if mutex.owner.is_some_and(&matches) {
                    mutex.owner = None;
                    mutex.recursion = 0;
                    mutex.abandoned = true;
                }
            }
        }
    }

    pub fn process_exit_code(&self, pid: Pid) -> Option<u32> {
        self.objects.values().find_map(|object| match object {
            SessionObject::Process(process) if process.pid == pid => process.exit_code,
            _ => None,
        })
    }

    /// Resolve a process handle in the caller's handle table.  The result is
    /// intentionally session-owned: an exit code belongs to the process
    /// object, rather than to either process personality's private state.
    pub fn get_exit_code_process(
        &self,
        caller_pid: Pid,
        handle: u32,
    ) -> Result<(Pid, u32), &'static str> {
        let entry = self
            .process(caller_pid)
            .and_then(|process| process.handles.get(&handle))
            .ok_or("GetExitCodeProcess invalid handle")?;
        let SessionObject::Process(process) = self
            .objects
            .get(&entry.object)
            .ok_or("GetExitCodeProcess missing object")?
        else {
            return Err("GetExitCodeProcess handle is not a process");
        };
        Ok((process.pid, process.exit_code.unwrap_or(259)))
    }

    pub fn set_event(&mut self, pid: Pid, handle: u32) -> Result<SetEventResult, &'static str> {
        let object = self
            .process(pid)
            .and_then(|process| process.handles.get(&handle))
            .ok_or("SetEvent invalid handle")?
            .object;
        let (manual_reset, was_signaled) = {
            let Some(SessionObject::Event(event)) = self.objects.get_mut(&object) else {
                return Err("SetEvent handle is not an event");
            };
            let state = (event.manual_reset, event.signaled);
            event.signaled = true;
            state
        };
        Ok(SetEventResult {
            manual_reset,
            was_signaled,
            woken: self.reevaluate_blocked_waits()?,
        })
    }

    pub fn reset_event(&mut self, pid: Pid, handle: u32) -> Result<ResetEventResult, &'static str> {
        let object = self
            .process(pid)
            .and_then(|process| process.handles.get(&handle))
            .ok_or("ResetEvent invalid handle")?
            .object;
        let Some(SessionObject::Event(event)) = self.objects.get_mut(&object) else {
            return Err("ResetEvent handle is not an event");
        };
        let result = ResetEventResult {
            manual_reset: event.manual_reset,
            was_signaled: event.signaled,
        };
        event.signaled = false;
        Ok(result)
    }

    pub fn thread_exit_code(&self, key: ThreadKey) -> Option<u32> {
        self.objects.values().find_map(|object| match object {
            SessionObject::Thread(thread) if thread.key == key => thread.exit_code,
            _ => None,
        })
    }

    pub fn terminate_process(
        &mut self,
        pid: Pid,
        exit_code: u32,
    ) -> Result<Vec<CompletedWait>, &'static str> {
        if !self.processes.contains_key(&pid) {
            return Err("ExitProcess unknown process");
        }
        let mut found_process_object = false;
        for object in self.objects.values_mut() {
            match object {
                SessionObject::Process(process) if process.pid == pid => {
                    if process.exit_code.is_some() {
                        return Err("ExitProcess process already terminated");
                    }
                    process.exit_code = Some(exit_code);
                    found_process_object = true;
                }
                SessionObject::Thread(thread) if thread.key.pid == pid => {
                    thread.exit_code = Some(exit_code);
                }
                _ => {}
            }
        }
        if !found_process_object {
            return Err("ExitProcess process object missing");
        }

        self.runnable.retain(|key| key.pid != pid);
        self.blocked.retain(|key, _| key.pid != pid);
        self.critical_waiters.retain(|wait| wait.key.pid != pid);
        self.abandon_mutexes(|owner| owner.pid == pid);
        let handles: Vec<_> = self
            .process(pid)
            .ok_or("ExitProcess process missing")?
            .handles
            .keys()
            .copied()
            .collect();
        for handle in handles {
            self.close_handle(pid, handle);
        }

        self.reevaluate_blocked_waits()
    }

    fn reevaluate_blocked_waits(&mut self) -> Result<Vec<CompletedWait>, &'static str> {
        let blocked = std::mem::take(&mut self.blocked);
        let mut woken = Vec::new();
        for (key, request) in blocked {
            match self.poll_wait(&request)? {
                Some(result) if result != 0x0000_0102 && result != u32::MAX => {
                    woken.push(CompletedWait { request, result });
                }
                Some(_) | None => {
                    self.blocked.insert(key, request);
                }
            }
        }
        for completed in &woken {
            self.enqueue(completed.request.key);
        }
        Ok(woken)
    }

    pub fn block_wait(&mut self, request: WaitRequest) -> Result<(), &'static str> {
        if !(1..=2).contains(&request.count) || request.wait_all > 1 {
            return Err("unsupported wait shape");
        }
        self.runnable.retain(|key| *key != request.key);
        self.blocked.insert(request.key, request);
        Ok(())
    }

    pub fn poll_single_wait(&mut self, request: &WaitRequest) -> Result<Option<u32>, &'static str> {
        if request.count != 1 || request.wait_all != 0 {
            return Err("unsupported single-object wait shape");
        }
        self.poll_wait(request)
    }

    pub fn poll_wait(&mut self, request: &WaitRequest) -> Result<Option<u32>, &'static str> {
        if !(1..=2).contains(&request.count) {
            return Err("unsupported wait count");
        }
        if request.wait_all > 1 {
            return Err("unsupported wait_all value");
        }

        let mut objects = [0; 2];
        let mut signaled = [false; 2];
        for index in 0..request.count as usize {
            let Some(entry) = self
                .process(request.key.pid)
                .and_then(|process| process.handles.get(&request.handles[index]))
            else {
                self.process_mut(request.key.pid)
                    .ok_or("wait process missing")?
                    .xp
                    .set_last_error_for_thread(request.key.tid, 6);
                return Ok(Some(u32::MAX));
            };
            objects[index] = entry.object;
            let Some(object) = self.objects.get(&entry.object) else {
                self.process_mut(request.key.pid)
                    .ok_or("wait process missing")?
                    .xp
                    .set_last_error_for_thread(request.key.tid, 6);
                return Ok(Some(u32::MAX));
            };
            signaled[index] = match object {
                SessionObject::Event(event) => event.signaled,
                SessionObject::Mutex(mutex) => {
                    mutex.owner.is_none() || mutex.owner == Some(request.key)
                }
                SessionObject::Process(process) => process.exit_code.is_some(),
                SessionObject::Thread(thread) => thread.exit_code.is_some(),
            };
        }

        let selected: Vec<usize> = if request.wait_all == 0 {
            match signaled[..request.count as usize]
                .iter()
                .position(|signaled| *signaled)
            {
                Some(index) => vec![index],
                None => return Ok((request.timeout == 0).then_some(0x0000_0102)),
            }
        } else if signaled[..request.count as usize]
            .iter()
            .all(|signaled| *signaled)
        {
            (0..request.count as usize).collect()
        } else {
            return Ok((request.timeout == 0).then_some(0x0000_0102));
        };

        let mut abandoned_index = None;
        for index in &selected {
            match self.objects.get_mut(&objects[*index]) {
                Some(SessionObject::Event(event)) if !event.manual_reset => event.signaled = false,
                Some(SessionObject::Mutex(mutex)) => {
                    if mutex.abandoned && abandoned_index.is_none() {
                        abandoned_index = Some(*index as u32);
                    }
                    mutex.owner = Some(request.key);
                    mutex.recursion = mutex
                        .recursion
                        .checked_add(1)
                        .ok_or("mutex recursion overflow")?;
                    mutex.abandoned = false;
                }
                _ => {}
            }
        }
        if let Some(index) = abandoned_index {
            return Ok(Some(0x80 + if request.wait_all == 0 { index } else { 0 }));
        }
        Ok(Some(if request.wait_all == 0 {
            selected[0] as u32
        } else {
            0
        }))
    }

    pub fn event_state(&self, pid: Pid, handle: u32) -> Option<(bool, bool)> {
        let entry = self.process(pid)?.handles.get(&handle)?;
        match self.objects.get(&entry.object)? {
            SessionObject::Event(event) => Some((event.manual_reset, event.signaled)),
            _ => None,
        }
    }

    pub fn describe_handle(&self, pid: Pid, handle: u32) -> String {
        let Some(entry) = self
            .process(pid)
            .and_then(|process| process.handles.get(&handle))
        else {
            return "unknown".into();
        };
        let Some(object) = self.objects.get(&entry.object) else {
            return format!("object_id={} unknown", entry.object);
        };
        let kind = match object {
            SessionObject::Event(event) => format!("Event name={:?}", event.name),
            SessionObject::Mutex(mutex) => format!(
                "Mutex name={:?} owner={:?} recursion={}",
                mutex.name, mutex.owner, mutex.recursion
            ),
            SessionObject::Process(process) => format!("Process pid={}", process.pid),
            SessionObject::Thread(thread) => {
                format!("Thread pid={} tid={}", thread.key.pid, thread.key.tid)
            }
        };
        format!("object_id={} {}", entry.object, kind)
    }
}

#[cfg(test)]
mod child_thread_tests {
    use super::*;

    #[test]
    fn child_thread_handle_is_registered_but_not_runnable_until_enqueued() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (key, handle) = session.create_child_thread(child.pid).unwrap();
        assert_eq!(key.pid, child.pid);
        assert_ne!(key.tid, child.tid);
        assert!(!session.runnable.contains(&key));
        let entry = session.process(child.pid).unwrap().handles.get(&handle).unwrap();
        assert!(matches!(session.objects.get(&entry.object), Some(SessionObject::Thread(thread)) if thread.key == key && thread.exit_code.is_none()));
        session.enqueue(key);
        assert!(session.runnable.contains(&key));
        session.signal_thread(key, 7).unwrap();
        assert!(!session.runnable.contains(&key));
        assert_eq!(session.thread_exit_code(key), Some(7));
    }

    #[test]
    fn critical_section_waiters_are_fifo_and_removed_on_thread_exit() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (first, _) = session.create_child_thread(child.pid).unwrap();
        let (second, _) = session.create_child_thread(child.pid).unwrap();
        for key in [first, second] {
            session
                .block_critical_section(CriticalSectionWait {
                    key,
                    address: 0x1234,
                    provider_id: 7,
                    esp: 0x4321,
                })
                .unwrap();
        }
        assert_eq!(session.take_critical_waiter(child.pid, 0x1234).unwrap().key, first);
        session.signal_thread(second, 0).unwrap();
        assert!(session.take_critical_waiter(child.pid, 0x1234).is_none());
    }

    #[test]
    fn invalid_wait_sets_only_calling_threads_last_error() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (other, _) = session.create_child_thread(child.pid).unwrap();
        session
            .process_mut(child.pid)
            .unwrap()
            .xp
            .set_last_error_for_thread(other.tid, 42);
        let wait = WaitRequest {
            key: ThreadKey { pid: child.pid, tid: child.tid },
            return_address: 0,
            count: 1,
            handles_pointer: 0,
            handles: [0xdead_beef, 0],
            wait_all: 0,
            timeout: 0,
        };
        assert_eq!(session.poll_wait(&wait), Ok(Some(u32::MAX)));
        let xp = &session.process(child.pid).unwrap().xp;
        assert_eq!(xp.last_error_for_thread(child.tid), 6);
        assert_eq!(xp.last_error_for_thread(other.tid), 42);
    }
}
