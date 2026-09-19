//! Portable Windows XP semantics for the launcher.
//!
//! This module deliberately knows nothing about VMX, VM ids, physical memory,
//! or carrier selection. It consumes an x86 trap frame plus generic guest
//! memory and returns the register value with which execution should resume.

use std::collections::{HashMap, VecDeque};

use crate::{
    imports::{LauncherImport, WinCall},
    pe32,
    session::{
        CreateEventRequest, CreateProcessRequest, LoadImageRequest, PersonalityAction,
        SessionRequest, ThreadKey, WaitRequest,
    },
    thunk32,
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
const THREAD_HANDLE_BASE: u32 = 0x5743_5001;
const DESKTOP_HWND: u32 = 0x5743_3000;
const WINDOW_HWND: u32 = 0x5743_4001;
const GDI_HANDLE_BASE: u32 = 0x5743_7001;
const STOCK_MONO_BITMAP: u32 = 0x5743_7f01;
const GDI_DIB_BASE: u32 = 0x0500_0000;
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

#[cfg(test)]
fn write_u16(memory: &mut impl GuestMemory, address: u32, value: u16) -> Result<(), &'static str> {
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
    selected_bitmap: u32,
}

#[derive(Clone, Debug)]
struct PaletteObject {
    version: u16,
    entries: Vec<PaletteEntry>,
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
    next_tid: u32,
    next_thread_handle: u32,
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
    gdi_objects: HashMap<u32, GdiObject>,
    next_gdi_handle: u32,
    next_gdi_dib_va: u32,
}

impl XpProcess {
    pub fn new(imports: Vec<LauncherImport>) -> Self {
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
        Self {
            imports,
            call_count: 0,
            threads: Vec::new(),
            next_tid: 2,
            next_thread_handle: THREAD_HANDLE_BASE,
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
            gdi_objects,
            next_gdi_handle: GDI_HANDLE_BASE,
            next_gdi_dib_va: GDI_DIB_BASE,
        }
    }

    pub fn import(&self, id: u32) -> Option<&LauncherImport> {
        self.imports.get(id as usize)
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
            WinCall::HeapAlloc => self.heap_alloc(esp, memory),
            WinCall::HeapFree => self.heap_free(esp, memory),
            WinCall::CreateEventA => {
                return Ok(PersonalityAction::Session(SessionRequest::CreateEvent(
                    self.create_event_request(esp, memory)?,
                )));
            }
            WinCall::GetLastError => Ok(self.last_error),
            WinCall::CloseHandle => {
                return Ok(PersonalityAction::Session(SessionRequest::CloseHandle {
                    pid,
                    handle: read_u32(memory, esp + 4)?,
                }));
            }
            WinCall::GetTickCount => Ok(self.tick_ms),
            WinCall::GetCurrentThreadId => Ok(tid),
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
            WinCall::CreateThread => self.create_thread(esp, memory),
            WinCall::ResumeThread => self.resume_thread(esp, memory),
            WinCall::CreateProcessA => {
                return Ok(PersonalityAction::Session(SessionRequest::CreateProcess(
                    CreateProcessRequest {
                        frame: self.create_process_a(esp, memory)?,
                    },
                )));
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

    pub fn set_last_error(&mut self, value: u32) {
        self.last_error = value;
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
        let resource = numeric_resource(
            memory,
            pe32::IMAGE_BASE,
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
        if source_dc != 0 {
            return Err("unsupported CreateCompatibleDC source DC");
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
                selected_bitmap: STOCK_MONO_BITMAP,
            }),
        );
        let _ = ret;
        Ok(handle)
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
            Some(GdiObject::DeviceContext(dc)) => dc.selected_bitmap,
            Some(GdiObject::Bitmap(_)) => return Err("SelectObject requires device context"),
            Some(GdiObject::Palette(_)) => return Err("SelectObject requires device context"),
            None => return Err("SelectObject unknown device context"),
        };
        if !stock
            && self.gdi_objects.iter().any(|(handle, value)| {
                *handle != hdc
                    && matches!(
                        value,
                        GdiObject::DeviceContext(dc) if dc.selected_bitmap == object
                    )
            })
        {
            return Err("bitmap already selected into another device context");
        }
        let Some(GdiObject::DeviceContext(dc)) = self.gdi_objects.get_mut(&hdc) else {
            return Err("SelectObject unknown device context");
        };
        dc.selected_bitmap = object;
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
            Some(GdiObject::DeviceContext(dc)) => dc.selected_bitmap,
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
            GdiObject::Palette(PaletteObject { version, entries }),
        );
        let _ = ret;
        Ok(handle)
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

    pub fn set_desktop_size(&mut self, width: u32, height: u32) {
        self.desktop_size = (width, height);
    }

    pub fn take_window_request(&mut self) -> Option<WindowRequest> {
        self.window_request.take()
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
            GdiObject::DeviceContext(dc) => match dc.compatible_with {
                DcCompatibility::Display => Some((dc.selected_bitmap, 1, 1, 1)),
            },
            GdiObject::Bitmap(_) => None,
            GdiObject::Palette(_) => None,
        }
    }

    pub fn selected_bitmap(&self, handle: u32) -> Option<u32> {
        match self.gdi_objects.get(&handle)? {
            GdiObject::DeviceContext(dc) => Some(dc.selected_bitmap),
            GdiObject::Bitmap(_) => None,
            GdiObject::Palette(_) => None,
        }
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
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(THREAD_HANDLE_BASE)
        );
        assert_eq!(read_u32(&memory, 0x0021_0560).unwrap(), 2);
        assert_eq!(xp.threads[0].suspend_count, 1);
    }

    #[test]
    fn load_image_resolves_numeric_bitmap_resource_without_fixed_rva() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "USER32.dll".into(),
            symbol: "LoadImageA".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let base = pe32::IMAGE_BASE;
        let mut memory = Memory {
            base,
            bytes: vec![0; 0x6000],
        };
        write_u32(&mut memory, base + 0x3c, 0x80).unwrap();
        write_u32(&mut memory, base + 0x80 + 24 + 96 + 16, 0x1000).unwrap();
        write_u32(&mut memory, base + 0x80 + 24 + 96 + 20, 0x4000).unwrap();
        // RT_BITMAP (2) -> resource ID 106 -> language 1033 -> data entry.
        write_u16(&mut memory, base + 0x1000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x1000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x1000 + 16, 2).unwrap();
        write_u32(&mut memory, base + 0x1000 + 20, 0x8000_1000).unwrap();
        write_u16(&mut memory, base + 0x2000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x2000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x2000 + 16, 106).unwrap();
        write_u32(&mut memory, base + 0x2000 + 20, 0x8000_2000).unwrap();
        write_u16(&mut memory, base + 0x3000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x3000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x3000 + 16, 1033).unwrap();
        write_u32(&mut memory, base + 0x3000 + 20, 0x3000).unwrap();
        write_u32(&mut memory, base + 0x4000, 0x4500).unwrap();
        write_u32(&mut memory, base + 0x4004, 44).unwrap();
        write_u32(&mut memory, base + 0x4500, 40).unwrap();
        write_u32(&mut memory, base + 0x4504, 1).unwrap();
        write_u32(&mut memory, base + 0x4508, 1).unwrap();
        write_u16(&mut memory, base + 0x450c, 1).unwrap();
        write_u16(&mut memory, base + 0x450e, 24).unwrap();
        write_u32(&mut memory, base + 0x4514, 4).unwrap();
        memory.bytes[0x4500 + 40..0x4500 + 44].copy_from_slice(&[0, 0, 255, 0]);
        let esp = base + 0x5000;
        for (index, value) in [0x0040_1501, base, 106, 0, 0, 0, 0x2000]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let PersonalityAction::Session(SessionRequest::LoadImage(request)) =
            xp.dispatch(2, 0, esp, &mut memory).unwrap()
        else {
            panic!("LoadImageA did not produce a decode request");
        };
        assert_eq!(request.resource_id, 106);
        assert_eq!(request.width, 1);
        assert_eq!(request.height, 1);
        assert_eq!(request.bit_count, 24);
        let bmp = bmp_file_from_dib(&request.dib).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(&bmp[14..], request.dib.as_slice());
        let layout = dib_layout(&request.dib).unwrap();
        let handle = xp
            .admit_bitmap(request, vec![255, 0, 0, 255], 0x0500_0000, layout)
            .unwrap();
        let info = xp.bitmap_info(handle).unwrap();
        assert_eq!(handle, GDI_HANDLE_BASE);
        assert_eq!(info.resource_id, 106);
        assert_eq!(info.dib_bytes, 44);
        assert_eq!(info.bits_len, 4);
        assert_eq!(info.rgba, vec![255, 0, 0, 255]);
    }

    #[test]
    fn select_object_returns_previous_bitmap_and_rejects_duplicate_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SelectObject".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        xp.gdi_objects.insert(
            GDI_HANDLE_BASE,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 500,
                height: 400,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 200_000,
                clr_used: 256,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: Vec::new(),
                bits_va: 0x0500_0000,
                bits_len: 200_000,
                row_stride: 500,
                pixel_offset: 1064,
                stock: false,
            }),
        );
        let hdc0 = GDI_HANDLE_BASE + 1;
        let hdc1 = GDI_HANDLE_BASE + 2;
        xp.gdi_objects.insert(
            hdc0,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                selected_bitmap: STOCK_MONO_BITMAP,
            }),
        );
        xp.gdi_objects.insert(
            hdc1,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                selected_bitmap: STOCK_MONO_BITMAP,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_156f, hdc0, GDI_HANDLE_BASE].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_MONO_BITMAP)
        );
        assert_eq!(
            xp.compatible_dc_info(hdc0),
            Some((GDI_HANDLE_BASE, 1, 1, 1))
        );
        for (index, value) in [0x0040_156f, hdc1, GDI_HANDLE_BASE].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        assert_eq!(
            xp.compatible_dc_info(hdc1),
            Some((STOCK_MONO_BITMAP, 1, 1, 1))
        );
    }

    #[test]
    fn get_dib_color_table_reads_live_rgbquad_ranges() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "GetDIBColorTable".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let hdc = GDI_HANDLE_BASE + 1;
        let palette = vec![[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]];
        xp.gdi_objects.insert(
            bitmap,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 1,
                height: 1,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 4,
                clr_used: 3,
                dib: Vec::new(),
                decoded_rgba: vec![3, 2, 1, 255],
                palette,
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 52,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                selected_bitmap: bitmap,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_1587, hdc, 1, 4, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(2)
        );
        let mut output = [0; 8];
        memory.read(0x0021_0600, &mut output).unwrap();
        assert_eq!(&output, &[5, 6, 7, 8, 9, 10, 11, 12]);
        for (index, value) in [0x0040_1587, hdc, 9, 1, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        for (index, value) in [0x0040_1587, hdc, 0, 0, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        for (index, value) in [0x0040_1587, STOCK_MONO_BITMAP, 0, 1, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }
    #[test]
    fn create_palette_preserves_guest_palette_entries_and_bounds_reads() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "CreatePalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let pointer = 0x0021_05e0;
        xp.allocations.insert(pointer, 12);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        write_u16(&mut memory, pointer, 0x0300).unwrap();
        write_u16(&mut memory, pointer + 2, 2).unwrap();
        memory
            .write(pointer + 4, &[10, 20, 30, 0, 40, 50, 60, 1])
            .unwrap();
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_15cb).unwrap();
        write_u32(&mut memory, esp + 4, pointer).unwrap();
        let PersonalityAction::Return(handle) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreatePalette did not return a handle");
        };
        assert_ne!(handle, 0);
        assert_eq!(xp.palette_info(handle), Some((0x0300, 2)));

        write_u16(&mut memory, pointer + 2, 0).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        write_u16(&mut memory, pointer + 2, 3).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        write_u16(&mut memory, pointer + 2, 257).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
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

        let frame = XpProcess::new(Vec::new())
            .create_process_a(esp, &memory)
            .unwrap();
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
        for (index, value) in [0x0040_1362, 2, handles_pointer, 0, u32::MAX]
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
        let PersonalityAction::Return(handle) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreateThread unexpectedly reached a frontier");
        };
        write_u32(&mut memory, esp, 0x0040_0000).unwrap();
        write_u32(&mut memory, esp + 4, handle).unwrap();
        assert_eq!(
            xp.dispatch(1, 1, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        let runnable = xp.take_runnable_thread().unwrap();
        assert_eq!(runnable.tid, 2);
        assert_eq!(runnable.suspend_count, 0);
        xp.exit_thread(runnable.tid, 0x1234).unwrap();
        assert_eq!(xp.threads[0].exit_code, Some(0x1234));
    }
}
