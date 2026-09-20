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

pub struct RegistryValue { pub ty: u32, pub bytes: Vec<u8> }
pub struct RegistryNode {
    pub name: String,
    pub parent: Option<RegistryNodeId>,
    children: HashMap<String, RegistryNodeId>,
    values: HashMap<String, RegistryValue>,
}
pub struct RegistryImage { pub nodes: Vec<RegistryNode>, roots: HashMap<u32, RegistryNodeId> }

impl RegistryImage {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let text = if bytes.starts_with(&[0xff, 0xfe]) {
            String::from_utf16(&bytes[2..].chunks_exact(2).map(|v| u16::from_le_bytes([v[0], v[1]])).collect::<Vec<_>>()).map_err(|_| "registry UTF-16")?
        } else {
            String::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes).to_vec()).map_err(|_| "registry UTF-8")?
        };
        let mut image = Self { nodes: Vec::new(), roots: HashMap::new() };
        for (root, name) in [(0x8000_0000, "HKEY_CLASSES_ROOT"), (0x8000_0001, "HKEY_CURRENT_USER"), (0x8000_0002, "HKEY_LOCAL_MACHINE"), (0x8000_0003, "HKEY_USERS"), (0x8000_0005, "HKEY_CURRENT_CONFIG")] {
            let id = image.push_node(name.into(), None); image.roots.insert(root, id);
        }
        let mut current = None;
        let mut lines = text.lines().peekable();
        while let Some(raw) = lines.next() {
            let mut line = raw.trim().to_owned();
            while line.ends_with('\\') { line.pop(); line.push_str(lines.next().ok_or("registry continuation")?.trim()); }
            if line.is_empty() || line.starts_with(';') || line == "REGEDIT4" || line == "Windows Registry Editor Version 5.00" { continue; }
            if line.starts_with('[') && line.ends_with(']') {
                current = image.add_key(&line[1..line.len()-1]); continue;
            }
            if let Some(node) = current { image.add_value(node, &line)?; }
        }
        Ok(image)
    }
    fn push_node(&mut self, name: String, parent: Option<RegistryNodeId>) -> RegistryNodeId { let id = self.nodes.len() as u32; self.nodes.push(RegistryNode { name, parent, children: HashMap::new(), values: HashMap::new() }); id }
    fn add_key(&mut self, header: &str) -> Option<RegistryNodeId> {
        let (root_name, tail) = header.split_once('\\').unwrap_or((header, ""));
        let root = [("HKEY_CLASSES_ROOT",0x8000_0000),("HKEY_CURRENT_USER",0x8000_0001),("HKEY_LOCAL_MACHINE",0x8000_0002),("HKEY_USERS",0x8000_0003),("HKEY_CURRENT_CONFIG",0x8000_0005)].iter().find(|(name,_)| name.eq_ignore_ascii_case(root_name)).map(|(_,root)| *root)?;
        let mut node = *self.roots.get(&root)?;
        for part in tail.split('\\').filter(|part| !part.is_empty()) {
            let key = canonical(part);
            node = match self.nodes[node as usize].children.get(&key) { Some(id) => *id, None => { let id = self.push_node(part.into(), Some(node)); self.nodes[node as usize].children.insert(key, id); id } };
        }
        Some(node)
    }
    fn add_value(&mut self, node: RegistryNodeId, line: &str) -> Result<(), &'static str> {
        let (name_part, data) = line.split_once('=').ok_or("registry value")?;
        let name = if name_part == "@" { String::new() } else { unquote(name_part)? };
        let (ty, bytes) = if data.starts_with('"') { (1, unquote(data)?.into_bytes()) } else if let Some(hex) = data.strip_prefix("dword:") { (4, u32::from_str_radix(hex, 16).map_err(|_| "registry dword")?.to_le_bytes().to_vec()) } else if let Some(rest) = data.strip_prefix("hex") { let (ty, values) = if let Some(rest) = rest.strip_prefix(':') { (3, rest) } else { let (ty, values) = rest.strip_prefix('(').and_then(|r| r.split_once("):" )).ok_or("registry hex type")?; (u32::from_str_radix(ty,16).map_err(|_| "registry hex type")?, values) }; (ty, values.split(',').filter(|v| !v.is_empty()).map(|v| u8::from_str_radix(v.trim(),16).map_err(|_| "registry hex")).collect::<Result<Vec<_>,_>>().map_err(|_| "registry hex")?) } else { return Err("registry value encoding"); };
        self.nodes[node as usize].values.insert(canonical(&name), RegistryValue { ty, bytes }); Ok(())
    }
    pub fn root(&self, hkey: u32) -> Option<RegistryNodeId> { self.roots.get(&hkey).copied() }
    pub fn child_path(&self, mut node: RegistryNodeId, path: &str) -> Option<RegistryNodeId> { for part in path.split('\\').filter(|part| !part.is_empty()) { node = *self.nodes.get(node as usize)?.children.get(&canonical(part))?; } Some(node) }
    pub fn stats(&self) -> (usize, usize, usize) { (self.roots.len(), self.nodes.len(), self.nodes.iter().map(|n| n.values.len()).sum()) }
}
fn canonical(name: &str) -> String { name.bytes().map(|byte| byte.to_ascii_lowercase() as char).collect() }
fn unquote(value: &str) -> Result<String, &'static str> { let body = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or("registry quote")?; Ok(body.replace("\\\\", "\\").replace("\\\"", "\"")) }

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
        assert!(image.child_path(hkcu, "software\\BLIZZARD entertainment\\internal").is_some());
        assert!(image.root(0x8000_0002).and_then(|root| image.child_path(root, "a")).is_some());
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
        assert_eq!(session.poll_single_wait(&request(auto, u32::MAX)).unwrap(), Some(0));
        assert_eq!(session.event_state(LAUNCHER_PID, auto), Some((false, false)));
        assert_eq!(session.poll_single_wait(&request(auto, 0)).unwrap(), Some(0x102));

        let (manual, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: true,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(session.poll_single_wait(&request(manual, u32::MAX)).unwrap(), Some(0));
        assert_eq!(session.event_state(LAUNCHER_PID, manual), Some((true, true)));
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
        assert_eq!(session.poll_single_wait(&request(handle, 100)).unwrap(), None);
        assert_eq!(session.poll_single_wait(&request(handle, 0)).unwrap(), Some(0x102));
        assert_eq!(
            session.poll_single_wait(&request(0xdead_beef, 100)).unwrap(),
            Some(u32::MAX)
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
            session.runnable.iter().filter(|queued| **queued == key).count(),
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
    pub registry: RegistryImage,
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
            registry: RegistryImage { nodes: Vec::new(), roots: HashMap::new() },
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
            SessionObject::Process(ProcessObject { pid }),
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
        let mut xp = XpProcess::new(Vec::new());
        let registry_base = 0x5743_8001u32.saturating_add(pid.saturating_mul(0x100));
        xp.set_registry_handle_base(registry_base).expect("new process has no registry handles");
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
                SessionObject::Process(_) | SessionObject::Thread(_) => false,
            };
            if signaled {
                return Err("unexpected signaled object at #90");
            }
        }
        self.runnable.retain(|key| *key != request.key);
        self.blocked.insert(request.key, request);
        Ok(())
    }

    pub fn poll_single_wait(
        &mut self,
        request: &WaitRequest,
    ) -> Result<Option<u32>, &'static str> {
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
        let SessionObject::Event(event) = object else {
            self.process_mut(request.key.pid)
                .ok_or("wait process missing")?
                .xp
                .set_last_error(6);
            return Ok(Some(u32::MAX));
        };
        if event.signaled {
            if !event.manual_reset {
                event.signaled = false;
            }
            return Ok(Some(0));
        }
        if request.timeout == 0 {
            return Ok(Some(0x0000_0102));
        }
        Ok(None)
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
