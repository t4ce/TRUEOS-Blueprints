//! Portable Windows XP semantics for the launcher.
//!
//! This module deliberately knows nothing about VMX, VM ids, physical memory,
//! or carrier selection. It consumes an x86 trap frame plus generic guest
//! memory and returns the register value with which execution should resume.

use std::collections::{HashMap, VecDeque};

use crate::{
    child_loader::{ChildProvider, ProviderImport, ProviderSymbol},
    imports::{LauncherImport, WinCall},
    pe32,
    session::{
        CreateEventRequest, CreateProcessRequest, CreateWindowRequest, LoadImageRequest,
        PersonalityAction, SessionRequest, ThreadKey, WaitRequest, WindowBlitRequest,
        WindowTextRequest,
    },
    thunk32,
};

pub const ENTRY_VA: u32 = pe32::IMAGE_BASE + pe32::ENTRY_RVA;
pub const TEB_VA: u32 = 0x0020_1000;
pub const HEAP_VA: u32 = 0x0021_0000;
pub const CHILD_CRT_HEAP_BASE: u32 = 0x0100_0000;
pub const CHILD_CRT_HEAP_LIMIT: u32 = 0x0400_0000;
pub const XP_ALLOCATION_GRANULARITY: u32 = 0x0001_0000;
pub const XP_PAGE_SIZE: u32 = 0x1000;
pub const CHILD_VIRTUAL_ALLOC_BASE: u32 = 0x0600_0000;
pub const CHILD_VIRTUAL_ALLOC_LIMIT: u32 = 0x1400_0000;
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
const GDI_HANDLE_BASE: u32 = 0x5743_7001;
const STOCK_MONO_BITMAP: u32 = 0x5743_7f01;
const STOCK_DEFAULT_PALETTE: u32 = 0x5743_7f02;
const TRANSPARENT: u32 = 1;
const OPAQUE: u32 = 2;
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

pub struct XpProcess {
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
    crt_heap_next: u32,
    crt_allocations: HashMap<u32, u32>,
    virtual_reservations: Vec<VirtualReservation>,
    virtual_reserve_next: u32,
    tls_allocated: [bool; 64],
    tls_values: HashMap<(u32, u32), u32>,
    critical_sections: HashMap<u32, (u32, u32)>,
    registry_handles: HashMap<u32, RegistryHandle>,
    next_registry_handle: u32,
    last_error: u32,
    unhandled_exception_filter: u32,
    tick_ms: u32,
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
        gdi_objects.insert(
            STOCK_DEFAULT_PALETTE,
            GdiObject::Palette(PaletteObject {
                version: 0,
                entries: Vec::new(),
                stock: true,
            }),
        );
        Self {
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
            heaps: HashMap::new(),
            next_heap_handle: 0x5743_0001,
            crt_heap_next: 0,
            crt_allocations: HashMap::new(),
            virtual_reservations: Vec::new(),
            virtual_reserve_next: CHILD_VIRTUAL_ALLOC_BASE,
            tls_allocated: [false; 64],
            tls_values: HashMap::new(),
            critical_sections: HashMap::new(),
            registry_handles: HashMap::new(),
            next_registry_handle: 0x5743_8001,
            last_error: 0,
            unhandled_exception_filter: 0,
            tick_ms: 0,
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
        self.provider_imports = imports;
        self.provider_thunks = thunks;
        self.provider_modules = modules;
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
                .is_some_and(|reservation_end| address >= reservation.base && end <= reservation_end)
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

    pub fn dispatch_provider_for_process(
        &mut self,
        _pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        let provider = self
            .provider_import(provider_id)
            .cloned()
            .ok_or("unknown child provider import")?;
        match (&provider.module[..], &provider.symbol) {
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
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "SetUnhandledExceptionFilter" =>
            {
                let filter = read_u32(memory, esp.checked_add(4).ok_or("provider argument overflow")?)?;
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.set_unhandled_exception_filter(filter)))
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
            _ => Err("unsupported child provider import"),
        }
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

    pub fn append_provider_imports(
        &mut self,
        imports: Vec<ProviderImport>,
    ) -> Result<(Vec<u32>, usize, usize, usize, Vec<u8>), &'static str> {
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
            let address = thunk32::address(id).ok_or("provider address")?;
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

    pub fn set_unhandled_exception_filter(&mut self, filter: u32) -> u32 {
        let previous = self.unhandled_exception_filter;
        self.unhandled_exception_filter = filter;
        previous
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
    fn system_font_metric_matches_draw_text_measurement_string() {
        let text = b"Copyright \xa9 2002 Blizzard Entertainment. All Rights Reserved.";
        assert_eq!(text.len(), 61);
        let width = text
            .iter()
            .map(|byte| system_font_advance_cp1252(*byte).unwrap())
            .sum::<u32>();
        assert_eq!(width, 406);
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
        xp.gdi_objects.insert(
            hdc1,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
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
    fn create_compatible_dc_resolves_memory_and_window_paint_sources() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "CreateCompatibleDC".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let paint = GDI_HANDLE_BASE + 3;
        xp.gdi_objects.insert(
            paint,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd: 0x5743_4002 },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0).unwrap();
        write_u32(&mut memory, esp + 4, paint).unwrap();
        let PersonalityAction::Return(result) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreateCompatibleDC did not return");
        };
        assert_ne!(result, paint);
        assert_eq!(xp.dc_target(result), Some(None));
        assert_eq!(xp.selected_bitmap(result), Some(STOCK_MONO_BITMAP));
        assert!(xp.gdi_live(paint));

        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        assert!(!xp.gdi_live(0x5743_7003));
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
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
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
    fn select_palette_swaps_per_dc_state_and_protects_selected_palette() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SelectPalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let paint = GDI_HANDLE_BASE;
        let memory_dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
        for (handle, target) in [
            (paint, DcTarget::WindowPaint { hwnd: 0x5743_4002 }),
            (
                memory_dc,
                DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
            ),
        ] {
            xp.gdi_objects.insert(
                handle,
                GdiObject::DeviceContext(DeviceContext {
                    compatible_with: DcCompatibility::Display,
                    target,
                    selected_palette: STOCK_DEFAULT_PALETTE,
                    palette_force_background: false,
                    realized_palette: None,
                    text_color: 0,
                    bk_color: 0x00ff_ffff,
                    bk_mode: OPAQUE,
                }),
            );
        }
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 10,
                    green: 20,
                    blue: 30,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_16e8, paint, palette, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_DEFAULT_PALETTE)
        );
        assert_eq!(xp.selected_palette(paint), Some(palette));
        assert_eq!(xp.selected_palette(memory_dc), Some(STOCK_DEFAULT_PALETTE));
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(palette)
        );
        write_u32(&mut memory, esp + 4, memory_dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_DEFAULT_PALETTE)
        );
        assert_eq!(xp.selected_palette(memory_dc), Some(palette));
        write_u32(&mut memory, esp + 4, paint).unwrap();
        write_u32(&mut memory, esp + 8, STOCK_DEFAULT_PALETTE).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(palette)
        );
        write_u32(&mut memory, esp + 4, STOCK_DEFAULT_PALETTE).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn realize_palette_is_first_use_only_and_is_invalidated_by_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "RealizePalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let palette = GDI_HANDLE_BASE + 1;
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd: 0x5743_4002 },
                selected_palette: palette,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_16f0).unwrap();
        write_u32(&mut memory, esp + 4, hdc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(xp.realized_palette(hdc), Some(Some(palette)));
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.realized_palette(hdc), Some(Some(palette)));
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
    fn delete_dc_only_removes_device_context_and_releases_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "DeleteDC".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
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
                clr_used: 1,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: vec![[0, 0, 0, 0]],
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 44,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            dc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_15ec).unwrap();
        write_u32(&mut memory, esp + 4, dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(dc));
        assert!(xp.gdi_live(bitmap));
        assert!(xp.gdi_live(palette));

        let dc2 = GDI_HANDLE_BASE + 3;
        xp.gdi_objects.insert(
            dc2,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
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
        write_u32(&mut memory, esp, 0x0040_156f).unwrap();
        write_u32(&mut memory, esp + 4, dc2).unwrap();
        write_u32(&mut memory, esp + 8, bitmap).unwrap();
        assert_eq!(xp.select_object(esp, &memory).unwrap(), STOCK_MONO_BITMAP);

        write_u32(&mut memory, esp + 4, bitmap).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        write_u32(&mut memory, esp + 4, palette).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn delete_object_obeys_gdi_lifetime_and_selection_rules() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "DeleteObject".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
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
                clr_used: 1,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: vec![[0, 0, 0, 0]],
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 44,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            dc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_188a).unwrap();
        write_u32(&mut memory, esp + 4, bitmap).unwrap();

        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(bitmap));

        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        xp.gdi_objects.remove(&dc);
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(bitmap));

        write_u32(&mut memory, esp + 4, palette).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(palette));

        write_u32(&mut memory, esp + 4, dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(STOCK_MONO_BITMAP));

        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn historical_launcher_layout_and_create_process_frontier_match() {
        assert_eq!(STACK_BASE, 0x0430_0000);
        assert_eq!(STACK_TOP, 0x0440_0000);
        assert_eq!(COMMAND_LINE, b"\"Warcraft III.exe\"\0");
        assert_eq!(crate::session::WINDOW_HANDLE_BASE, 0x5743_4001);

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
    fn session_allocates_first_and_second_windows_and_presents_only_visible_one() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "USER32.dll".into(),
                symbol: "RegisterClassA".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "USER32.dll".into(),
                symbol: "CreateWindowExA".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: pe32::IMAGE_BASE,
            bytes: vec![0; 0x10_000],
        };
        let class = pe32::IMAGE_BASE + 0x80a8;
        let title = pe32::IMAGE_BASE + 0x8080;
        let wndclass = pe32::IMAGE_BASE + 0x8100;
        memory.write(class, b"Warcraft III\0").unwrap();
        memory.write(title, b"Launching Warcraft III\0").unwrap();
        for (offset, value) in [0, 0x0040_1630, 0, 0, pe32::IMAGE_BASE, 0, 0, 0, 0, class]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, wndclass + offset as u32 * 4, value).unwrap();
        }
        let register_esp = pe32::IMAGE_BASE + 0x9000;
        write_u32(&mut memory, register_esp, 0).unwrap();
        write_u32(&mut memory, register_esp + 4, wndclass).unwrap();
        assert!(matches!(
            xp.dispatch(1, 0, register_esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        ));
        let esp = pe32::IMAGE_BASE + 0x9000;
        for (index, value) in [
            0x0040_1946,
            0,
            class,
            title,
            0,
            1030,
            520,
            500,
            400,
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
        let PersonalityAction::Session(SessionRequest::CreateWindow(request)) =
            xp.dispatch(1, 1, esp, &mut memory).unwrap()
        else {
            panic!("missing create window request")
        };
        let mut session = crate::session::Wc3Session::new(XpProcess::new(Vec::new()));
        let first = session
            .create_window(crate::session::CreateWindowRequest {
                owner: ThreadKey { pid: 1, tid: 1 },
                style: 0x8000_0000,
                ex_style: 0,
                ..request.clone()
            })
            .unwrap();
        assert_eq!(first, 0x5743_4001);
        let second = session
            .create_window(crate::session::CreateWindowRequest {
                owner: ThreadKey { pid: 1, tid: 2 },
                ..request
            })
            .unwrap();
        assert_eq!(second, 0x5743_4002);
        assert!(!session.windows[&second].visible);
        assert_eq!(session.show_window(second, 1).unwrap(), 0);
        assert_eq!(
            session.take_window_presentation(),
            Some(crate::session::WindowPresentation::Show {
                hwnd: second,
                x: 1030,
                y: 520,
                width: 500,
                height: 400
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
    fn set_text_color_swaps_color_without_changing_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetTextColor".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let hwnd = 0x5743_4002;
        xp.gdi_objects.insert(
            hdc,
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
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_17aa, hdc, 0x0000_c8f0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(xp.text_color(hdc), Some(0));
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0000_c8f0));
        assert_eq!(xp.dc_target(hdc), Some(Some(hwnd)));
        assert_eq!(xp.selected_palette(hdc), Some(STOCK_DEFAULT_PALETTE));
        assert_eq!(xp.realized_palette(hdc), Some(None));

        write_u32(&mut memory, esp + 8, 0x0012_3456).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0x0000_c8f0)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0012_3456));

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(u32::MAX)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0012_3456));
    }

    #[test]
    fn set_bk_color_swaps_default_white_without_changing_other_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetBkColor".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let hwnd = 0x5743_4002;
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0x0000_c8f0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_17b3, hdc, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0x00ff_ffff)
        );
        let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&hdc) else {
            panic!("device context disappeared")
        };
        assert_eq!(dc.bk_color, 0);
        assert_eq!(dc.text_color, 0x0000_c8f0);
        assert_eq!(dc.selected_palette, STOCK_DEFAULT_PALETTE);
        assert!(matches!(dc.target, DcTarget::WindowPaint { hwnd: value } if value == hwnd));

        write_u32(&mut memory, esp + 8, 0x0012_3456).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            xp.gdi_objects.get(&hdc).and_then(|value| match value {
                GdiObject::DeviceContext(dc) => Some(dc.bk_color),
                _ => None,
            }),
            Some(0x0012_3456)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(u32::MAX)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0000_c8f0));
    }

    #[test]
    fn set_bk_mode_uses_opaque_defaults_and_preserves_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetBkMode".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_1561, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let memory_hdc = xp.create_compatible_dc(esp, &memory).unwrap();
        let paint_struct = 0x043f_f500;
        let paint_hdc = xp
            .begin_paint(0x5743_4002, paint_struct, 500, 400, &mut memory)
            .unwrap();

        for hdc in [memory_hdc, paint_hdc] {
            let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&hdc) else {
                panic!("missing device context")
            };
            assert_eq!(dc.bk_mode, OPAQUE);
            assert_eq!(dc.text_color, 0);
            assert_eq!(dc.bk_color, 0x00ff_ffff);
        }

        write_u32(&mut memory, esp, 0x0040_17bc).unwrap();
        write_u32(&mut memory, esp + 4, paint_hdc).unwrap();
        write_u32(&mut memory, esp + 8, TRANSPARENT).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(OPAQUE)
        );
        let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&paint_hdc) else {
            panic!("missing paint device context")
        };
        assert_eq!(dc.bk_mode, TRANSPARENT);
        assert_eq!(dc.text_color, 0);
        assert_eq!(dc.bk_color, 0x00ff_ffff);
        assert_eq!(dc.selected_palette, STOCK_DEFAULT_PALETTE);
        assert!(matches!(dc.target, DcTarget::WindowPaint { hwnd } if hwnd == 0x5743_4002));

        write_u32(&mut memory, esp + 8, OPAQUE).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(TRANSPARENT)
        );
        write_u32(&mut memory, esp + 8, 3).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            xp.gdi_objects
                .get(&paint_hdc)
                .and_then(|value| match value {
                    GdiObject::DeviceContext(dc) => Some(dc.bk_mode),
                    _ => None,
                }),
            Some(OPAQUE)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn end_paint_retires_only_matching_active_window_paint() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "USER32.dll".into(),
                symbol: "EndPaint".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "GDI32.dll".into(),
                symbol: "DeleteDC".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        let ps = 0x043f_f500;
        let hwnd = 0x5743_4002;
        let hdc = xp.begin_paint(hwnd, ps, 500, 400, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, ps).unwrap(), hdc);
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        for (index, value) in [0x0040_1833, hwnd + 1, ps].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        write_u32(&mut memory, esp + 4, hwnd).unwrap();
        write_u32(&mut memory, ps, 0).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        write_u32(&mut memory, ps, hdc).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(xp.active_paint_count(), 0);
        assert!(!xp.gdi_live(hdc));
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        let ps2 = ps + 0x100;
        let hdc2 = xp.begin_paint(hwnd, ps2, 500, 400, &mut memory).unwrap();
        write_u32(&mut memory, esp, 0x0040_15ec).unwrap();
        write_u32(&mut memory, esp + 4, hdc2).unwrap();
        assert_eq!(
            xp.dispatch(2, 1, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(hdc2));
        assert_eq!(xp.active_paint_count(), 1);
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

    #[test]
    fn child_initialize_critical_section_is_process_private() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
            iat_rva: 0,
        };
        let pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let critical_section = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x1501_fbd3).unwrap();
        write_u32(&mut memory, esp + 4, critical_section).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(pid2.critical_sections.contains_key(&critical_section));
        assert!(!pid1.critical_sections.contains_key(&critical_section));
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), u32::MAX);
    }

    #[test]
    fn child_enter_critical_section_dispatch_is_recursive_and_process_private() {
        let initialize = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
            iat_rva: 0,
        };
        let enter = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("EnterCriticalSection".into()),
            iat_rva: 0,
        };
        let leave = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("LeaveCriticalSection".into()),
            iat_rva: 0,
        };
        let pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![initialize, enter, leave], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let critical_section = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x1502_2867).unwrap();
        write_u32(&mut memory, esp + 4, critical_section).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 1, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 1, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 2);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        let before_wrong_owner = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 4, 2, esp, &mut memory),
            Err("critical section owner")
        );
        assert_eq!(memory.bytes, before_wrong_owner);
        pid2.dispatch_provider_for_process(2, 3, 2, esp, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        pid2.dispatch_provider_for_process(2, 3, 2, esp, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), u32::MAX);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 0);
        assert!(!pid1.has_critical_section(critical_section));
        assert!(pid2.has_critical_section(critical_section));
    }

    #[test]
    fn child_set_last_error_returns_void_without_guest_memory_mutation() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetLastError".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1502_e393).unwrap();
        write_u32(&mut memory, esp + 4, 0x1234_5678).unwrap();
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(pid2.last_error, 0x1234_5678);
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_set_last_error_is_process_private() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetLastError".into()),
            iat_rva: 0,
        };
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid1.set_last_error(0xfeed_face);
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1502_e393).unwrap();
        write_u32(&mut memory, esp + 4, 0x1234_5678).unwrap();
        pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
            .unwrap();
        assert_eq!(pid1.last_error, 0xfeed_face);
        assert_eq!(pid2.last_error, 0x1234_5678);
    }

    #[test]
    fn child_crt_malloc_is_aligned_and_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let first = pid2.crt_malloc(0x80).unwrap().unwrap();
        assert_eq!(first.pointer, CHILD_CRT_HEAP_BASE);
        assert_eq!(first.end, CHILD_CRT_HEAP_BASE + 0x80);
        let second = pid2.crt_malloc(1).unwrap().unwrap();
        assert_eq!(second.pointer, CHILD_CRT_HEAP_BASE + 0x80);
        assert_eq!(second.end, CHILD_CRT_HEAP_BASE + 0x88);
        assert_eq!(
            pid1.crt_malloc(0x80).unwrap().unwrap().pointer,
            CHILD_CRT_HEAP_BASE
        );
    }

    #[test]
    fn child_crt_resize_preserves_old_ownership_until_commit() {
        let mut process = XpProcess::new(Vec::new());
        let old = process.crt_malloc(5).unwrap().unwrap();
        assert_eq!(process.crt_allocation_capacity(old.pointer), Some(8));
        let resize = process.crt_resize(old.pointer, 8, 12).unwrap().unwrap();
        assert_ne!(resize.pointer, old.pointer);
        assert_eq!(resize.used_bytes, 8);
        assert_eq!(resize.required_bytes, 12);
        assert!(resize.moved);
        assert_eq!(process.crt_allocation_capacity(old.pointer), Some(8));
        assert_eq!(process.crt_allocation_capacity(resize.pointer), Some(16));
        assert!(process.retire_crt_allocation(old.pointer));
        assert_eq!(process.crt_allocation_capacity(old.pointer), None);
        assert_eq!(process.crt_allocation_capacity(resize.pointer), Some(16));
    }

    #[test]
    fn child_virtual_reservations_are_granular_and_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let first = pid2.virtual_reserve_null(0x10000).unwrap().unwrap();
        assert_eq!(first.base, CHILD_VIRTUAL_ALLOC_BASE);
        assert_eq!(first.size, XP_ALLOCATION_GRANULARITY);
        assert_eq!(
            pid2.virtual_reservation_state(),
            (1, CHILD_VIRTUAL_ALLOC_BASE + XP_ALLOCATION_GRANULARITY)
        );
        let one_byte = pid2.virtual_reserve_null(1).unwrap().unwrap();
        assert_eq!(one_byte.size, XP_ALLOCATION_GRANULARITY);
        let rounded = pid2.virtual_reserve_null(0x10001).unwrap().unwrap();
        assert_eq!(rounded.size, 0x20000);
        assert_eq!(
            pid2.virtual_reservation_containing(rounded.base + 0x10000, 0x10000),
            Some(&rounded)
        );
        assert_eq!(
            pid1.virtual_reserve_null(1).unwrap().unwrap().base,
            CHILD_VIRTUAL_ALLOC_BASE
        );
        assert!(pid1.virtual_reservation_at(one_byte.base).is_none());
        assert_eq!(pid2.virtual_reserve_null(u32::MAX), Err("VirtualAlloc size overflow"));
    }

    #[test]
    fn child_virtual_commit_is_page_granular_and_rejects_overlap() {
        let mut process = XpProcess::new(Vec::new());
        let reservation = process.virtual_reserve_null(0x10000).unwrap().unwrap();
        let request = process
            .virtual_prepare_commit(reservation.base, 0x1000)
            .unwrap()
            .unwrap();
        assert_eq!(request.reservation_base, reservation.base);
        assert_eq!(request.reservation_size, 0x10000);
        assert_eq!(request.size, XP_PAGE_SIZE);
        assert_eq!(process.virtual_reservation_at(reservation.base).unwrap().committed, []);
        process.virtual_finish_commit(request).unwrap();
        assert_eq!(process.virtual_commit_state(), (1, XP_PAGE_SIZE));
        assert_eq!(
            process.virtual_prepare_commit(reservation.base, XP_PAGE_SIZE),
            Err("VirtualAlloc overlapping commit")
        );
        assert_eq!(
            process.virtual_prepare_commit(reservation.base + 0xf000, 0x1001),
            Ok(None)
        );
    }

    #[test]
    fn appended_provider_updates_the_existing_thunk_page_tail() {
        let mut xp = XpProcess::new(Vec::new());
        let existing = (0..337)
            .map(|index| ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(format!("existing_{index}")),
                iat_rva: 0,
            })
            .collect::<Vec<_>>();
        xp.install_provider_surface(existing, vec![0x90; 0x1000], Vec::new());
        let malloc = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("malloc".into()),
            iat_rva: 0,
        };
        let (addresses, old_bytes, new_bytes, updated_from, updated) =
            xp.append_provider_imports(vec![malloc]).unwrap();
        assert_eq!(addresses, vec![thunk32::address(337).unwrap()]);
        assert_eq!(old_bytes, 0x1000);
        assert_eq!(new_bytes, 0x1000);
        assert_eq!(updated_from, 337 * thunk32::THUNK_BYTES);
        assert_eq!(
            &updated[..9],
            &[0xb8, 0x51, 0x01, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]
        );
    }

    #[test]
    fn child_malloc_provider_returns_guest_pointer_without_memory_mutation() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("malloc".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1503_62e0).unwrap();
        write_u32(&mut memory, esp + 4, 0x80).unwrap();
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(CHILD_CRT_HEAP_BASE)
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn unsupported_child_provider_does_not_mutate_memory() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, STACK_TOP - 0x40, &mut memory),
            Err("unsupported child provider import")
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_get_version_returns_xp_version_without_touching_the_stack() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersion".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, STACK_TOP - 0x40, &mut memory)
                .unwrap(),
            PersonalityAction::Return(WINDOWS_XP_GET_VERSION)
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_get_version_ex_a_uses_the_xp_personality() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersionExA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let info = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x2113_1a58).unwrap();
        write_u32(&mut memory, esp + 4, info).unwrap();
        write_u32(&mut memory, info, 0x94).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(
            [0, 4, 8, 12, 16]
                .map(|offset| read_u32(&memory, info + offset).unwrap()),
            [0x94, 5, 1, 2600, 2]
        );
    }

    #[test]
    fn child_heap_create_is_process_private_and_lazy() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("HeapCreate".into()),
            iat_rva: 0,
        };
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid1.install_provider_surface(vec![provider.clone()], Vec::new(), Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        for (index, word) in [0x2113_1b92, 0x0004_0000, 0x0000_1000, 0x0000_0000]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0x5743_0001)
        );
        assert_eq!(
            pid2.heaps.get(&0x5743_0001),
            Some(&WinHeap {
                options: 0x0004_0000,
                initial_size: 0x0000_1000,
                maximum_size: 0,
            })
        );
        assert!(pid2.allocations.is_empty());
        for (index, word) in [0x0040_0000, 0, 0, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid1.dispatch_provider_for_process(1, 1, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0x5743_0001)
        );
        assert_eq!(pid1.heaps.len(), 1);
        assert_eq!(pid2.heaps.len(), 1);
        assert_ne!(pid1.heaps.get(&0x5743_0001), pid2.heaps.get(&0x5743_0001));
    }

    #[test]
    fn registry_handles_are_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let pid1_handle = pid1.open_registry_key(7, 0x0002_0019).unwrap();
        let pid2_handle = pid2.open_registry_key(7, 0x0002_0019).unwrap();
        assert_eq!(pid1_handle, 0x5743_8001);
        assert_eq!(pid2_handle, 0x5743_8001);
        assert_eq!(pid1.registry_handles.len(), 1);
        assert_eq!(pid2.registry_handles.len(), 1);
    }
}
