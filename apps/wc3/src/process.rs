//! Portable Windows XP semantics for the launcher.
//!
//! This module deliberately knows nothing about VMX, VM ids, physical memory,
//! or carrier selection. It consumes an x86 trap frame plus generic guest
//! memory and returns the register value with which execution should resume.

use std::collections::{HashMap, VecDeque};

use crate::{
    imports::{LauncherImport, WinCall},
    pe32, thunk32,
};

pub const ENTRY_VA: u32 = pe32::IMAGE_BASE + pe32::ENTRY_RVA;
pub const TEB_VA: u32 = 0x0020_1000;
pub const HEAP_VA: u32 = 0x0021_0000;
pub const PROCESS_DATA_VA: u32 = 0x0021_1000;
/// Historical launcher stack: 0x0430_0000..0x0440_0000.
pub const STACK_BASE: u32 = 0x0430_0000;
pub const STACK_BYTES: usize = 0x10_0000;
pub const STACK_TOP: u32 = STACK_BASE + STACK_BYTES as u32;
pub const THUNK_PAGE_BYTES: usize = 0x1000;
/// Process state returned by GetCommandLineA.  The launcher constructs its
/// separate `"war3.exe" ` child command line on its native stack.
pub const COMMAND_LINE: &[u8] = b"\"Warcraft III.exe\"\0";
const MODULE_FILENAME: &[u8] = b"C:\\Warcraft III\\Warcraft III.exe\0";
const WINDOWS_XP_GET_VERSION: u32 = 0x0a28_0105;
const CREATE_SUSPENDED: u32 = 4;
const EVENT_HANDLE_BASE: u32 = 0x5743_2001;
const THREAD_HANDLE_BASE: u32 = 0x5743_5001;
const DESKTOP_HWND: u32 = 0x5743_3000;
const WINDOW_HWND: u32 = 0x5743_4001;
const ENVIRONMENT_BLOCK_VA: u32 = PROCESS_DATA_VA + 0x100;

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

fn read_c_string(
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Frontier {
    CreateProcessA(CreateProcessAFrame),
    WaitForMultipleObjects(WaitForMultipleObjectsFrame),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    Value(u32),
    Frontier(Frontier),
}

impl PreparedProcess {
    pub fn new(mut materialized: pe32::Materialized) -> Result<Self, &'static str> {
        let mut thunks = vec![0; THUNK_PAGE_BYTES];
        crate::imports::patch(&mut materialized.image, &materialized.imports, &mut thunks)?;
        thunk32::install_thread_exit(&mut thunks)?;
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
struct EventObject {
    manual_reset: bool,
    signaled: bool,
    name: Option<String>,
    references: u32,
}

#[derive(Clone, Debug)]
struct Window {
    class: String,
    title: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    visible: bool,
    paint_pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowRequest {
    Show {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Hide,
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

pub struct XpProcess {
    imports: Vec<LauncherImport>,
    pub call_count: u32,
    pub threads: Vec<ThreadObject>,
    current_tid: u32,
    next_tid: u32,
    next_thread_handle: u32,
    next_event_handle: u32,
    events: HashMap<u32, EventObject>,
    heap_next: u32,
    allocations: HashMap<u32, u32>,
    tls_allocated: [bool; 64],
    tls_values: HashMap<(u32, u32), u32>,
    critical_sections: HashMap<u32, (u32, u32)>,
    last_error: u32,
    tick_ms: u32,
    classes: Vec<String>,
    window: Option<Window>,
    focused_window: Option<u32>,
    messages: VecDeque<Message>,
    runnable_thread: Option<u32>,
    desktop_size: (u32, u32),
    window_request: Option<WindowRequest>,
}

impl XpProcess {
    pub fn new(imports: Vec<LauncherImport>) -> Self {
        Self {
            imports,
            call_count: 0,
            threads: Vec::new(),
            current_tid: 1,
            next_tid: 2,
            next_thread_handle: THREAD_HANDLE_BASE,
            next_event_handle: EVENT_HANDLE_BASE,
            events: HashMap::new(),
            heap_next: 0,
            allocations: HashMap::new(),
            tls_allocated: [false; 64],
            tls_values: HashMap::new(),
            critical_sections: HashMap::new(),
            last_error: 0,
            tick_ms: 0,
            classes: Vec::new(),
            window: None,
            focused_window: None,
            messages: VecDeque::new(),
            runnable_thread: None,
            desktop_size: (1920, 1080),
            window_request: None,
        }
    }

    pub fn import(&self, id: u32) -> Option<&LauncherImport> {
        self.imports.get(id as usize)
    }

    /// Handle one import VMCALL and return the value for EAX.
    pub fn dispatch(
        &mut self,
        import_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<DispatchResult, &'static str> {
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
            WinCall::EnterCriticalSection => self.enter_critical_section(esp, memory),
            WinCall::LeaveCriticalSection => self.leave_critical_section(esp, memory),
            WinCall::TlsAlloc => self.tls_alloc(),
            WinCall::TlsSetValue => self.tls_set_value(esp, memory),
            WinCall::HeapAlloc => self.heap_alloc(esp, memory),
            WinCall::HeapFree => self.heap_free(esp, memory),
            WinCall::CreateEventA => self.create_event(esp, memory),
            WinCall::GetLastError => Ok(self.last_error),
            WinCall::CloseHandle => self.close_handle(esp, memory),
            WinCall::GetTickCount => {
                Ok(self.tick_ms)
            }
            WinCall::GetCurrentThreadId => Ok(self.current_tid),
            WinCall::GetStartupInfoA => self.get_startup_info(esp, memory),
            WinCall::GetModuleFileNameA => self.get_module_filename(esp, memory),
            WinCall::GetModuleHandleA => Ok(pe32::IMAGE_BASE),
            WinCall::GetStdHandle => self.get_std_handle(esp, memory),
            WinCall::GetFileType => Ok(2),
            WinCall::SetHandleCount => Ok(read_u32(memory, esp + 4)?),
            WinCall::GetCommandLineA => Ok(PROCESS_DATA_VA),
            WinCall::GetEnvironmentStringsW => Ok(0),
            WinCall::GetEnvironmentStringsA => Ok(ENVIRONMENT_BLOCK_VA),
            WinCall::FreeEnvironmentStringsA => Ok(1),
            WinCall::GetACP => Ok(1252),
            WinCall::GetCPInfo => self.get_cp_info(esp, memory),
            WinCall::GetStringTypeW => self.get_string_type(esp, memory),
            WinCall::MultiByteToWideChar => self.multi_byte_to_wide(esp, memory),
            WinCall::WideCharToMultiByte => self.wide_to_multi_byte(esp, memory),
            WinCall::LCMapStringW => self.lc_map_string(esp, memory),
            WinCall::RegisterClassA => self.register_class(esp, memory),
            WinCall::GetDesktopWindow => Ok(DESKTOP_HWND),
            WinCall::GetClientRect => self.get_client_rect(esp, memory),
            WinCall::CreateWindowExA => self.create_window(esp, memory),
            WinCall::ShowWindow => self.show_window(esp, memory),
            WinCall::UpdateWindow => self.update_window(esp, memory),
            WinCall::PeekMessageA => self.peek_message(esp, memory),
            WinCall::SetFocus => self.set_focus(esp, memory),
            WinCall::LoadStringA => self.load_string(esp, memory),
            WinCall::CreateThread => self.create_thread(esp, memory),
            WinCall::ResumeThread => self.resume_thread(esp, memory),
            WinCall::CreateProcessA => {
                return Ok(DispatchResult::Frontier(Frontier::CreateProcessA(
                    self.create_process_a(esp, memory)?,
                )))
            }
            WinCall::WaitForMultipleObjects => {
                return Ok(DispatchResult::Frontier(Frontier::WaitForMultipleObjects(
                    self.wait_for_multiple_objects(esp, memory)?,
                )))
            }
            WinCall::Unsupported => Err("unsupported launcher import"),
        }?;
        Ok(DispatchResult::Value(value))
    }

    fn create_process_a(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<CreateProcessAFrame, &'static str> {
        let [ret, application_name, command_line, process_attributes, thread_attributes,
            inherit_handles, creation_flags, environment, current_directory, startup_info,
            process_information] = arguments::<11>(memory, esp)?;
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
        if ret != 0x0040_1362 || count != 2 || handles_pointer == 0 {
            return Err("unexpected WaitForMultipleObjects frame");
        }
        let handles = [
            read_u32(memory, handles_pointer)?,
            read_u32(
                memory,
                handles_pointer.checked_add(4).ok_or("handles pointer overflow")?,
            )?,
        ];
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
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, address] = arguments::<2>(memory, esp)?;
        let entry = self
            .critical_sections
            .get_mut(&address)
            .ok_or("unknown critical section")?;
        if entry.0 != 0 && entry.0 != self.current_tid {
            return Err("critical section contention");
        }
        entry.0 = self.current_tid;
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
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, address] = arguments::<2>(memory, esp)?;
        let entry = self
            .critical_sections
            .get_mut(&address)
            .ok_or("unknown critical section")?;
        if entry.0 != self.current_tid || entry.1 == 0 {
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

    fn tls_set_value(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, slot, value] = arguments::<3>(memory, esp)?;
        if !self
            .tls_allocated
            .get(slot as usize)
            .copied()
            .unwrap_or(false)
        {
            return Err("TLS slot not allocated");
        }
        self.tls_values.insert((self.current_tid, slot), value);
        Ok(1)
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

    fn create_event(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, attributes, manual_reset, initial, name] = arguments::<5>(memory, esp)?;
        if attributes != 0 {
            return Err("event security attributes unsupported");
        }
        let name = if name == 0 {
            None
        } else {
            Some(read_c_string(memory, name, 260)?)
        };
        if let Some((handle, event)) = self
            .events
            .iter_mut()
            .find(|(_, event)| event.name.is_some() && event.name == name)
        {
            event.references += 1;
            self.last_error = 183;
            return Ok(*handle);
        }
        let handle = self.next_event_handle;
        self.next_event_handle += 1;
        self.events.insert(
            handle,
            EventObject {
                manual_reset: manual_reset != 0,
                signaled: initial != 0,
                name,
                references: 1,
            },
        );
        self.last_error = 0;
        Ok(handle)
    }

    fn close_handle(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let handle = read_u32(memory, esp + 4)?;
        if let Some(event) = self.events.get_mut(&handle) {
            event.references -= 1;
            if event.references == 0 {
                self.events.remove(&handle);
            }
            return Ok(1);
        }
        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.handle == handle && thread.open)
        {
            thread.open = false;
            return Ok(1);
        }
        self.last_error = 6;
        Ok(0)
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
    ) -> Result<u32, &'static str> {
        let [_, module, output, capacity] = arguments::<4>(memory, esp)?;
        if module != 0 && module != pe32::IMAGE_BASE {
            return Err("unknown module");
        }
        if (capacity as usize) < MODULE_FILENAME.len() {
            return Err("module filename buffer");
        }
        memory.write(output, MODULE_FILENAME)?;
        Ok((MODULE_FILENAME.len() - 1) as u32)
    }

    fn get_std_handle(&self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        Ok(match read_u32(memory, esp + 4)? as i32 {
            -10 => 0x5743_1001,
            -11 => 0x5743_1002,
            -12 => 0x5743_1003,
            _ => u32::MAX,
        })
    }

    fn get_cp_info(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, code_page, output] = arguments::<3>(memory, esp)?;
        if code_page != 1252 {
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
        if info_type != 1 {
            return Err("unsupported character info type");
        }
        for index in 0..count {
            let mut bytes = [0; 2];
            memory.read(source + index * 2, &mut bytes)?;
            let value = u16::from_le_bytes(bytes);
            let class = if u8::try_from(value).is_ok_and(|value| value.is_ascii_alphabetic()) {
                0x101
            } else if u8::try_from(value).is_ok_and(|value| value.is_ascii_digit()) {
                0x204
            } else if u8::try_from(value).is_ok_and(|value| value.is_ascii_whitespace()) {
                0x8
            } else {
                0
            };
            memory.write(output + index * 2, &u16::to_le_bytes(class))?;
        }
        Ok(1)
    }

    fn load_string(&self, esp: u32, memory: &mut impl GuestMemory) -> Result<u32, &'static str> {
        let [_, instance, resource_id, buffer, max_chars] = arguments::<5>(memory, esp)?;
        if instance != pe32::IMAGE_BASE || max_chars == 0 {
            return Err("unexpected LoadStringA frame");
        }
        let pe = read_u32(memory, pe32::IMAGE_BASE + 0x3c)?;
        let optional = pe32::IMAGE_BASE
            .checked_add(pe)
            .and_then(|value| value.checked_add(24))
            .ok_or("resource optional offset")?;
        let root_rva = read_u32(memory, optional + 96 + 16)?;
        let resource_size = read_u32(memory, optional + 96 + 20)?;
        if root_rva == 0 || resource_size < 16 {
            return Err("resource directory range");
        }
        let root = pe32::IMAGE_BASE
            .checked_add(root_rva)
            .ok_or("resource root")?;
        let string_type = resource_directory_entry(memory, root, root, 6)?;
        let block_id = (resource_id >> 4)
            .checked_add(1)
            .ok_or("resource block id")?;
        let block = resource_directory_entry(memory, root, string_type, block_id)?;
        let data_entry = resource_first_language_data(memory, root, block)?;
        let data_rva = read_u32(memory, data_entry)?;
        let data_size = read_u32(memory, data_entry + 4)?;
        let data = pe32::IMAGE_BASE
            .checked_add(data_rva)
            .ok_or("resource data")?;
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

    fn multi_byte_to_wide(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, &'static str> {
        let [_, cp, _, source, count, output, capacity] = arguments::<7>(memory, esp)?;
        if cp != 0 && cp != 1252 {
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
        if cp != 0 && cp != 1252 {
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
        let [_, _, flags, source, count, output, capacity] = arguments::<7>(memory, esp)?;
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
            let mapped = if flags & 0x100 != 0 {
                (value as u8).to_ascii_lowercase() as u16
            } else if flags & 0x200 != 0 {
                (value as u8).to_ascii_uppercase() as u16
            } else {
                value
            };
            memory.write(output + index * 2, &mapped.to_le_bytes())?;
        }
        Ok(length)
    }

    fn register_class(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let structure = read_u32(memory, esp + 4)?;
        let name = read_u32(memory, structure + 36)?;
        let name = read_c_string(memory, name, 256)?;
        self.classes.push(name);
        Ok(self.classes.len() as u32)
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
    fn create_window(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let a = arguments::<13>(memory, esp)?;
        if a[1] != 0
            || a[4] != 0x8000_0000
            || a[9] != DESKTOP_HWND
            || a[10] != 0
            || a[11] != pe32::IMAGE_BASE
            || a[12] != 0
            || a[7] == 0
            || a[8] == 0
        {
            return Err("unexpected CreateWindowExA frame");
        }
        let class = read_c_string(memory, a[2], 256)?;
        if !self.classes.contains(&class) {
            return Err("unregistered class");
        }
        let title = read_c_string(memory, a[3], 256)?;
        self.window = Some(Window {
            class,
            title,
            x: a[5] as i32,
            y: a[6] as i32,
            width: a[7],
            height: a[8],
            visible: false,
            paint_pending: false,
        });
        Ok(WINDOW_HWND)
    }
    fn show_window(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let [_, hwnd, show] = arguments::<3>(memory, esp)?;
        if hwnd != WINDOW_HWND {
            return Err("unknown window");
        }
        let w = self.window.as_mut().ok_or("window absent")?;
        let old = w.visible;
        w.visible = show != 0;
        w.paint_pending = w.visible;
        if old != w.visible {
            self.window_request = Some(if w.visible {
                WindowRequest::Show {
                    x: w.x,
                    y: w.y,
                    width: w.width,
                    height: w.height,
                }
            } else {
                WindowRequest::Hide
            });
        }
        Ok(old as u32)
    }
    fn update_window(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        if read_u32(memory, esp + 4)? != WINDOW_HWND {
            return Err("unknown window");
        }
        let w = self.window.as_mut().ok_or("window absent")?;
        let pending = w.paint_pending;
        w.paint_pending = false;
        Ok(pending as u32)
    }
    fn set_focus(&mut self, esp: u32, memory: &impl GuestMemory) -> Result<u32, &'static str> {
        let hwnd = read_u32(memory, esp + 4)?;
        if hwnd != WINDOW_HWND || self.window.is_none() {
            return Err("SetFocus unknown window");
        }
        let previous = self.focused_window.replace(hwnd).unwrap_or(0);
        Ok(previous)
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

    pub fn focused_window(&self) -> Option<u32> {
        self.focused_window
    }

    pub fn set_current_thread(&mut self, tid: u32) {
        self.current_tid = tid;
    }

    pub fn set_desktop_size(&mut self, width: u32, height: u32) {
        self.desktop_size = (width, height);
    }

    pub fn take_window_request(&mut self) -> Option<WindowRequest> {
        self.window_request.take()
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

fn read_u16(memory: &impl GuestMemory, address: u32) -> Result<u16, &'static str> {
    let mut b = [0; 2];
    memory.read(address, &mut b)?;
    Ok(u16::from_le_bytes(b))
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
fn decode_cp1252(byte: u8) -> u16 {
    match byte {
        0x80 => 0x20ac,
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x96 => 0x2013,
        0x97 => 0x2014,
        _ => byte as u16,
    }
}
fn encode_cp1252(value: u16) -> Option<u8> {
    match value {
        0x20ac => Some(0x80),
        0x201a => Some(0x82),
        0x0192 => Some(0x83),
        0x201e => Some(0x84),
        0x2026 => Some(0x85),
        0x2020 => Some(0x86),
        0x2021 => Some(0x87),
        0x2018 => Some(0x91),
        0x2019 => Some(0x92),
        0x201c => Some(0x93),
        0x201d => Some(0x94),
        0x2013 => Some(0x96),
        0x2014 => Some(0x97),
        0..=255 => Some(value as u8),
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
mod tests {
    use super::*;
    struct Memory {
        base: u32,
        bytes: Vec<u8>,
    }
    impl GuestMemory for Memory {
        fn read(&self, a: u32, o: &mut [u8]) -> Result<(), &'static str> {
            let n = (a - self.base) as usize;
            o.copy_from_slice(self.bytes.get(n..n + o.len()).ok_or("read")?);
            Ok(())
        }
        fn write(&mut self, a: u32, i: &[u8]) -> Result<(), &'static str> {
            let n = (a - self.base) as usize;
            self.bytes
                .get_mut(n..n + i.len())
                .ok_or("write")?
                .copy_from_slice(i);
            Ok(())
        }
    }
    #[test]
    fn proven_create_thread_is_logical_and_suspended() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "CreateThread".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [
            0x0040_2039,
            0,
            0x2000,
            0x0040_2072,
            0x0021_0560,
            4,
            0x0021_0560,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(0, esp, &mut memory).unwrap(),
            DispatchResult::Value(THREAD_HANDLE_BASE)
        );
        assert_eq!(read_u32(&memory, 0x0021_0560).unwrap(), 2);
        assert_eq!(xp.threads[0].suspend_count, 1);
    }
    #[test]
    fn image_mapping_precedes_overlapping_stack() {
        let image = Mapping {
            address: 0x0040_0000,
            bytes: {
                let mut b = vec![0; 0x3000];
                b[0x2072..0x2076].copy_from_slice(&[0x55, 0x8b, 0xec, 0x6a]);
                b
            },
            executable: true,
        };
        let stack = Mapping {
            address: 0x0040_0000,
            bytes: vec![0; 0x10000],
            executable: false,
        };
        assert_eq!(
            &diagnostic_read(0x0040_2072, &image, &stack).unwrap()[..4],
            &[0x55, 0x8b, 0xec, 0x6a]
        );
    }

    #[test]
    fn historical_launcher_layout_and_create_process_frontier_match() {
        assert_eq!(STACK_BASE, 0x0430_0000);
        assert_eq!(STACK_TOP, 0x0440_0000);
        assert_eq!(COMMAND_LINE, b"\"Warcraft III.exe\"\0");
        assert_eq!(WINDOW_HWND, 0x5743_4001);

        // This is the hardware-observed #89 frame.  Its positions are a
        // consequence of the restored historical stack, not special cases in
        // the dispatch implementation.
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f8b0;
        let command_line = 0x043f_fb20;
        let startup_info = 0x043f_fc24;
        let process_information = 0x043f_f900;
        memory.write(command_line, b"\"war3.exe\" \0").unwrap();
        for (index, value) in [
            0x0040_12e0,
            0,
            command_line,
            0,
            0,
            1,
            0,
            0,
            0,
            startup_info,
            process_information,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        let frame = XpProcess::new(Vec::new()).create_process_a(esp, &memory).unwrap();
        assert_eq!(
            frame,
            CreateProcessAFrame {
                return_address: 0x0040_12e0,
                application_name: 0,
                command_line,
                process_attributes: 0,
                thread_attributes: 0,
                inherit_handles: 1,
                creation_flags: 0,
                environment: 0,
                current_directory: 0,
                startup_info,
                process_information,
            }
        );
    }

    #[test]
    fn proven_create_window_frame_reaches_ui4_presentation() {
        let mut xp = XpProcess::new(Vec::new());
        xp.classes.push("Warcraft III".into());
        let mut memory = Memory {
            base: pe32::IMAGE_BASE,
            bytes: vec![0; 0x10_000],
        };
        let class = pe32::IMAGE_BASE + 0x80a8;
        let title = pe32::IMAGE_BASE + 0x8080;
        memory.write(class, b"Warcraft III\0").unwrap();
        memory.write(title, b"Warcraft III\0").unwrap();
        let esp = pe32::IMAGE_BASE + 0x9000;
        for (index, value) in [
            0x0040_1aa7,
            0,
            class,
            title,
            0x8000_0000,
            1264,
            704,
            16,
            16,
            DESKTOP_HWND,
            0,
            pe32::IMAGE_BASE,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(xp.create_window(esp, &memory).unwrap(), 0x5743_4001);
        write_u32(&mut memory, esp + 4, WINDOW_HWND).unwrap();
        write_u32(&mut memory, esp + 8, 5).unwrap();
        assert_eq!(xp.show_window(esp, &memory).unwrap(), 0);
        assert_eq!(
            xp.take_window_request(),
            Some(WindowRequest::Show {
                x: 1264,
                y: 704,
                width: 16,
                height: 16,
            })
        );
    }

    #[test]
    fn wait_for_multiple_objects_frontier_captures_both_handles() {
        let xp = XpProcess::new(Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        let handles_pointer = 0x043f_f900;
        let handles = [0x5743_2001, 0x5743_5001];
        for (index, value) in [
            0x0040_1362,
            2,
            handles_pointer,
            0,
            u32::MAX,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        write_u32(&mut memory, handles_pointer, handles[0]).unwrap();
        write_u32(&mut memory, handles_pointer + 4, handles[1]).unwrap();

        assert_eq!(
            xp.wait_for_multiple_objects(esp, &memory).unwrap(),
            WaitForMultipleObjectsFrame {
                return_address: 0x0040_1362,
                count: 2,
                handles_pointer,
                handles,
                wait_all: 0,
                timeout: u32::MAX,
            }
        );
    }

    #[test]
    fn resume_thread_transfers_created_thread_to_blueprint_lifecycle() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "KERNEL32.dll".into(),
                symbol: "CreateThread".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "KERNEL32.dll".into(),
                symbol: "ResumeThread".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [
            0x0040_2039,
            0,
            0x2000,
            0x0040_2072,
            0x0021_0560,
            4,
            0x0021_0560,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let DispatchResult::Value(handle) = xp.dispatch(0, esp, &mut memory).unwrap() else {
            panic!("CreateThread unexpectedly reached a frontier");
        };
        write_u32(&mut memory, esp, 0x0040_0000).unwrap();
        write_u32(&mut memory, esp + 4, handle).unwrap();
        assert_eq!(
            xp.dispatch(1, esp, &mut memory).unwrap(),
            DispatchResult::Value(1)
        );
        let runnable = xp.take_runnable_thread().unwrap();
        assert_eq!(runnable.tid, 2);
        assert_eq!(runnable.suspend_count, 0);
        xp.exit_thread(runnable.tid, 0x1234).unwrap();
        assert_eq!(xp.threads[0].exit_code, Some(0x1234));
    }
}
