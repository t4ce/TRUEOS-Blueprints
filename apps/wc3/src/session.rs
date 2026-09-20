//! Session-owned state shared by the WC3 launcher and its future child.
//!
//! This module is deliberately independent of VMX and guest address spaces.
//! `XpProcess` remains the personality for one address space; this session is
//! the authority for cross-process objects, names, HWNDs, and scheduling.

use std::collections::{HashMap, VecDeque};

use crate::process::{CreateProcessAFrame, ThreadObject, XpProcess};

pub type Pid = u32;
pub type Tid = u32;
pub type ObjectId = u64;
pub const LAUNCHER_PID: Pid = 1;
pub const LAUNCHER_TID: Tid = 1;
pub const PROCESS_HANDLE_BASE: u32 = 0x5743_6001;
pub const THREAD_HANDLE_BASE: u32 = 0x5743_5001;
pub const WINDOW_HANDLE_BASE: u32 = 0x5743_4001;
pub const DESKTOP_HWND: u32 = 0x5743_3000;

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
        decode_span(
            &self.backing[entry.tail_start as usize..entry.tail_end as usize],
            self.encoding,
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
        let text = decode_span(
            &self.backing[entry.body_start as usize..entry.body_end as usize],
            self.encoding,
        )?;
        let mut values = HashMap::new();
        for line in text.lines() {
            if let Some((name, data)) = line.trim().split_once('=') {
                let name = if name == "@" {
                    String::new()
                } else {
                    unquote(name)?
                };
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
fn decode_span(bytes: &[u8], encoding: RegistryEncoding) -> Result<String, &'static str> {
    match encoding {
        RegistryEncoding::Utf8 => String::from_utf8(bytes.to_vec()).map_err(|_| "registry UTF-8"),
        RegistryEncoding::Utf16Le => std::char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
        )
        .map(|unit| unit.map_err(|_| "registry UTF-16"))
        .collect(),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ThreadKey {
    pub pid: Pid,
    pub tid: Tid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_image_parses_multiroot_case_insensitive_tree() {
        let fixture = b"Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n\"x\"=dword:00000001\n[HKEY_LOCAL_MACHINE\\A]\n[HKEY_USERS\\B]\n[HKEY_CLASSES_ROOT\\C]\n[HKEY_CURRENT_CONFIG\\D]\n";
        let image = RegistryImage::parse(fixture).unwrap();
        let hkcu = image.root(0x8000_0001).unwrap();
        assert!(
            image
                .child_path(hkcu, "software\\BLIZZARD entertainment\\internal")
                .is_some()
        );
        assert!(
            image
                .root(0x8000_0002)
                .and_then(|root| image.child_path(root, "a"))
                .is_some()
        );
    }

    #[test]
    fn registry_state_starts_unloaded_and_utf16_rejects_odd_payloads() {
        let session = Wc3Session::new(XpProcess::new(Vec::new()));
        assert!(matches!(session.registry, RegistryState::Unloaded));
        assert!(matches!(
            RegistryImage::parse(&[0xff, 0xfe, b'R']),
            Err("registry UTF-16 odd trailing byte")
        ));
    }

    #[test]
    fn registry_index_leaves_unopened_values_lazy() {
        let fixture = b"[HKEY_CURRENT_USER\\A]\n\"huge\"=hex:aa,bb,cc,\\\n dd,ee\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n\"x\"=hex:01\n";
        let mut image = RegistryImage::parse(fixture).unwrap();
        let hkcu = image.root(0x8000_0001).unwrap();
        let a = image.child_path(hkcu, "a").unwrap();
        let internal = image
            .child_path(hkcu, "software\\blizzard entertainment\\internal")
            .unwrap();
        assert!(!image.values_loaded(a));
        assert!(!image.values_loaded(internal));
        image.ensure_values_loaded(internal).unwrap();
        assert!(image.values_loaded(internal));
        assert!(!image.values_loaded(a));
    }

    #[test]
    fn registry_index_scans_utf16_crlf_giant_ignored_value_and_final_key() {
        let mut fixture = vec![0xff, 0xfe];
        fixture.extend(
            "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\A]\r\n\"blob\"=hex:"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        for _ in 0..1_100_000 {
            fixture.extend([b'a', 0, b'a', 0, b',', 0]);
        }
        fixture.extend(
            "\r\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        let mut progress = Vec::new();
        let image =
            RegistryImage::index(fixture, |scanned, keys| progress.push((scanned, keys))).unwrap();
        let hkcu = image.root(HKEY_CURRENT_USER).unwrap();
        assert_eq!(image.index_entry_count(), 2);
        assert!(
            image
                .child_path(hkcu, "software\\BLIZZARD entertainment\\internal")
                .is_some()
        );
        assert!(!progress.is_empty());
        assert_eq!(image.scanned_bytes(), image.backing.len());
        assert!(image.loaded_values.is_empty());
    }

    #[test]
    fn registry_root_and_relative_paths_are_case_insensitive() {
        let image = RegistryImage::parse(b"[HKEY_CURRENT_USER\\Software]\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment]\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n").unwrap();
        let hkcu = image.root(HKEY_CURRENT_USER).unwrap();
        let software = image.child_path(hkcu, "software").unwrap();
        let blizzard = image
            .child_path(software, "BLIZZARD ENTERTAINMENT")
            .unwrap();
        assert!(image.child_path(blizzard, "internal").is_some());
        assert!(image.child_path(hkcu, "").is_some());
    }

    #[test]
    fn registry_hash_collision_candidate_requires_text_match() {
        let mut image = RegistryImage::parse(b"[HKEY_CURRENT_USER\\Actual]\n").unwrap();
        image.entries[0].path_hash = hash_query("different", image.encoding);
        assert!(image.find(HKEY_CURRENT_USER, "different").is_none());
    }

    fn request(handle: u32, timeout: u32) -> WaitRequest {
        WaitRequest {
            key: ThreadKey {
                pid: LAUNCHER_PID,
                tid: LAUNCHER_TID,
            },
            return_address: 0x0040_196f,
            count: 1,
            handles_pointer: 0,
            handles: [handle, 0],
            wait_all: 0,
            timeout,
        }
    }

    #[test]
    fn single_event_wait_consumes_auto_reset_and_preserves_manual_reset() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (auto, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(
            session.poll_single_wait(&request(auto, u32::MAX)).unwrap(),
            Some(0)
        );
        assert_eq!(
            session.event_state(LAUNCHER_PID, auto),
            Some((false, false))
        );
        assert_eq!(
            session.poll_single_wait(&request(auto, 0)).unwrap(),
            Some(0x102)
        );

        let (manual, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: true,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(
            session
                .poll_single_wait(&request(manual, u32::MAX))
                .unwrap(),
            Some(0)
        );
        assert_eq!(
            session.event_state(LAUNCHER_PID, manual),
            Some((true, true))
        );
    }

    #[test]
    fn single_event_wait_blocks_only_for_positive_timeout_and_rejects_unknown_handles() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (handle, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: false,
                inheritable: false,
            },
        );
        assert_eq!(
            session.poll_single_wait(&request(handle, 100)).unwrap(),
            None
        );
        assert_eq!(
            session.poll_single_wait(&request(handle, 0)).unwrap(),
            Some(0x102)
        );
        assert_eq!(
            session
                .poll_single_wait(&request(0xdead_beef, 100))
                .unwrap(),
            Some(u32::MAX)
        );
    }

    #[test]
    fn process_exit_signals_persistent_process_and_thread_handles() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let child_key = ThreadKey {
            pid: child.pid,
            tid: child.tid,
        };

        assert_eq!(session.process_exit_code(child.pid), None);
        assert_eq!(session.thread_exit_code(child_key), None);
        assert_eq!(
            session
                .poll_single_wait(&request(child.process_handle, 0))
                .unwrap(),
            Some(0x0000_0102)
        );

        session.terminate_process(child.pid, 0xc000_0005).unwrap();

        assert!(session.process(child.pid).is_some());
        assert_eq!(session.process_exit_code(child.pid), Some(0xc000_0005));
        assert_eq!(session.thread_exit_code(child_key), Some(0xc000_0005));
        for timeout in [0, u32::MAX] {
            assert_eq!(
                session
                    .poll_single_wait(&request(child.process_handle, timeout))
                    .unwrap(),
                Some(0)
            );
            assert_eq!(
                session
                    .poll_single_wait(&request(child.thread_handle, timeout))
                    .unwrap(),
                Some(0)
            );
        }
    }

    #[test]
    fn process_exit_wakes_a_blocked_single_waiter() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let wait = request(child.process_handle, u32::MAX);
        session.block_wait(wait.clone()).unwrap();

        let woken = session.terminate_process(child.pid, 7).unwrap();

        assert_eq!(woken, vec![wait.clone()]);
        assert!(!session.blocked.contains_key(&wait.key));
        assert_eq!(
            session
                .runnable
                .iter()
                .filter(|key| **key == wait.key)
                .count(),
            1
        );
    }

    #[test]
    fn enqueue_deduplicates_a_runnable_child_thread() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let key = ThreadKey {
            pid: child.pid,
            tid: child.tid,
        };
        session.enqueue(key);
        session.enqueue(key);
        assert_eq!(
            session
                .runnable
                .iter()
                .filter(|queued| **queued == key)
                .count(),
            1
        );
    }
}

#[derive(Clone, Debug)]
pub struct EventObject {
    pub manual_reset: bool,
    pub signaled: bool,
    pub name: Option<String>,
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
}

#[derive(Clone, Debug)]
pub enum SessionObject {
    Event(EventObject),
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
    CreateEvent(CreateEventRequest),
    LoadImage(LoadImageRequest),
    CreateWindow(CreateWindowRequest),
    ShowWindow {
        pid: Pid,
        hwnd: u32,
        show: u32,
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

    pub fn defer_wait(&mut self, request: WaitRequest) {
        self.blocked.insert(request.key, request);
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

    pub fn create_event(&mut self, pid: Pid, request: CreateEventRequest) -> (u32, bool) {
        let (object, already_exists) = if let Some(name) = request.name.as_ref() {
            if let Some(&object) = self.names.get(name) {
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

    pub fn close_handle(&mut self, pid: Pid, handle: u32) -> bool {
        let Some(entry) = self
            .process_mut(pid)
            .and_then(|process| process.handles.remove(&handle))
        else {
            return false;
        };
        let Some(SessionObject::Event(event)) = self.objects.get(&entry.object) else {
            return true;
        };
        if let Some(name) = event.name.clone() {
            let still_open = self.processes.values().any(|process| {
                process
                    .handles
                    .values()
                    .any(|held| held.object == entry.object)
            });
            if !still_open {
                self.names.remove(&name);
                self.objects.remove(&entry.object);
            }
        }
        true
    }

    pub fn signal_thread(&mut self, key: ThreadKey, exit_code: u32) {
        for object in self.objects.values_mut() {
            if let SessionObject::Thread(thread) = object {
                if thread.key == key {
                    thread.exit_code = Some(exit_code);
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
    ) -> Result<Vec<WaitRequest>, &'static str> {
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
        self.process_mut(pid)
            .ok_or("ExitProcess process missing")?
            .handles
            .clear();

        let blocked = std::mem::take(&mut self.blocked);
        let mut woken = Vec::new();
        for (key, request) in blocked {
            let signals_terminated_process = request.count == 1
                && request.wait_all == 0
                && self
                    .process(key.pid)
                    .and_then(|process| process.handles.get(&request.handles[0]))
                    .and_then(|entry| self.objects.get(&entry.object))
                    .is_some_and(|object| match object {
                        SessionObject::Process(process) => process.pid == pid,
                        SessionObject::Thread(thread) => thread.key.pid == pid,
                        SessionObject::Event(_) => false,
                    });
            if signals_terminated_process {
                woken.push(request);
            } else {
                self.blocked.insert(key, request);
            }
        }
        for request in &woken {
            self.enqueue(request.key);
        }
        Ok(woken)
    }

    pub fn block_wait(&mut self, request: WaitRequest) -> Result<(), &'static str> {
        for handle in request.handles.iter().take(request.count.min(2) as usize) {
            let entry = self
                .process(request.key.pid)
                .and_then(|process| process.handles.get(handle))
                .ok_or("wait handle is not in process table")?;
            let object = self
                .objects
                .get(&entry.object)
                .ok_or("wait object missing")?;
            let signaled = match object {
                SessionObject::Event(event) => event.signaled,
                SessionObject::Process(process) => process.exit_code.is_some(),
                SessionObject::Thread(thread) => thread.exit_code.is_some(),
            };
            if signaled {
                return Err("unexpected signaled object at #90");
            }
        }
        self.runnable.retain(|key| *key != request.key);
        self.blocked.insert(request.key, request);
        Ok(())
    }

    pub fn poll_single_wait(&mut self, request: &WaitRequest) -> Result<Option<u32>, &'static str> {
        if request.count != 1 || request.wait_all != 0 {
            return Err("unsupported single-object wait shape");
        }
        let Some(entry) = self
            .process(request.key.pid)
            .and_then(|process| process.handles.get(&request.handles[0]))
            .cloned()
        else {
            self.process_mut(request.key.pid)
                .ok_or("wait process missing")?
                .xp
                .set_last_error(6);
            return Ok(Some(u32::MAX));
        };
        let Some(object) = self.objects.get_mut(&entry.object) else {
            self.process_mut(request.key.pid)
                .ok_or("wait process missing")?
                .xp
                .set_last_error(6);
            return Ok(Some(u32::MAX));
        };
        match object {
            SessionObject::Event(event) => {
                if event.signaled {
                    if !event.manual_reset {
                        event.signaled = false;
                    }
                    Ok(Some(0))
                } else if request.timeout == 0 {
                    Ok(Some(0x0000_0102))
                } else {
                    Ok(None)
                }
            }
            SessionObject::Process(process) => {
                if process.exit_code.is_some() {
                    Ok(Some(0))
                } else if request.timeout == 0 {
                    Ok(Some(0x0000_0102))
                } else {
                    Ok(None)
                }
            }
            SessionObject::Thread(thread) => {
                if thread.exit_code.is_some() {
                    Ok(Some(0))
                } else if request.timeout == 0 {
                    Ok(Some(0x0000_0102))
                } else {
                    Ok(None)
                }
            }
        }
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
            SessionObject::Process(process) => format!("Process pid={}", process.pid),
            SessionObject::Thread(thread) => {
                format!("Thread pid={} tid={}", thread.key.pid, thread.key.tid)
            }
        };
        format!("object_id={} {}", entry.object, kind)
    }
}
