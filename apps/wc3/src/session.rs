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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ThreadKey {
    pub pid: Pid,
    pub tid: Tid,
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
    ShowWindow { pid: Pid, hwnd: u32, show: u32 },
    UpdateWindow { pid: Pid, hwnd: u32 },
    SetFocus { pid: Pid, hwnd: u32 },
    CloseHandle { pid: Pid, handle: u32 },
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
        self.runnable.push_back(key);
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
        self.processes.insert(
            pid,
            Wc3Process {
                pid,
                xp: XpProcess::new(Vec::new()),
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
