//! Session-owned state shared by the WC3 launcher and its future child.
//!
//! This module is deliberately independent of VMX and guest address spaces.
//! `XpProcess` remains the personality for one address space; this session is
//! the authority for cross-process objects, names, HWNDs, and scheduling.

use std::collections::{HashMap, VecDeque};

use crate::process::{CreateProcessAFrame, XpProcess};

pub type Pid = u32;
pub type Tid = u32;
pub type ObjectId = u64;

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
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
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
pub enum SessionRequest {
    CreateProcess(CreateProcessRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestCall {
    pub address: u32,
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
    pub sequence: u64,
}

impl Wc3Session {
    pub fn new(xp: XpProcess) -> Self {
        let pid = 1;
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
            runnable: VecDeque::new(),
            blocked: HashMap::new(),
            next_pid: 2,
            next_tid: 2,
            next_object: 1,
            sequence: 0,
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
}
