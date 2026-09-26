use super::*;
#[path = "loop_checkpoint.rs"]
pub(super) mod loop_checkpoint;

const ERROR_PROC_NOT_FOUND: u32 = 127;
const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_INVALID_ADDRESS: u32 = 487;
const D3D_SDK_VERSION: u32 = 220;
const D3D8_METHODS: [&str; 16] = [
    "IDirect3D8::QueryInterface",
    "IDirect3D8::AddRef",
    "IDirect3D8::Release",
    "IDirect3D8::RegisterSoftwareDevice",
    "IDirect3D8::GetAdapterCount",
    "IDirect3D8::GetAdapterIdentifier",
    "IDirect3D8::GetAdapterModeCount",
    "IDirect3D8::EnumAdapterModes",
    "IDirect3D8::GetAdapterDisplayMode",
    "IDirect3D8::CheckDeviceType",
    "IDirect3D8::CheckDeviceFormat",
    "IDirect3D8::CheckDeviceMultiSampleType",
    "IDirect3D8::CheckDepthStencilMatch",
    "IDirect3D8::GetDeviceCaps",
    "IDirect3D8::GetAdapterMonitor",
    "IDirect3D8::CreateDevice",
];
const MEM_RELEASE: u32 = 0x0000_8000;
const EXEC_SAMPLE_PREEMPTIONS: u64 = 8;
const MAX_SEH_CHAIN_DEPTH: u32 = 64;
const STORM_EXPAND_BEGIN: u32 = 0x1501_d5a9;
const STORM_EXPAND_TRAP: u32 = 0x1501_d5e0;
const STORM_EXPAND_AFTER_TRAP: u32 = STORM_EXPAND_TRAP + 3;
const STORM_EXPAND_TAIL: u32 = 0x1501_d61e;
const STORM_EXPAND_SIGNATURE: [u8; 32] = [
    0xbf, 0x85, 0x0f, 0xb4, 0x4f, 0x64, 0x5f, 0xcc,
    0x7f, 0x8b, 0xe7, 0x7c, 0xaf, 0xf7, 0x71, 0xf9,
    0xa9, 0x0b, 0x38, 0x46, 0x29, 0x04, 0xd5, 0xe5,
    0xdf, 0x62, 0xc1, 0xb3, 0xb9, 0x58, 0xc4, 0xdc,
];
const STORM_EXPAND_ORIGINAL: [u8; 3] = [0x8b, 0x45, 0xf8];

const WAR3_EVENT_POOL_BEGIN: u32 = 0x0040_29a0;
const WAR3_EVENT_POOL_TRAP: u32 = 0x0040_29e0;
const WAR3_EVENT_POOL_AFTER_TRAP: u32 = WAR3_EVENT_POOL_TRAP + 3;
const WAR3_EVENT_POOL_TAIL: u32 = 0x0040_2a47;
const WAR3_EVENT_POOL_ORIGINAL: [u8; 3] = [0x8b, 0x4d, 0xf0];
const WAR3_EVENT_POOL_SIGNATURE: [u8; 32] = [
    0x61, 0xa6, 0x2f, 0x92, 0x31, 0x93, 0x26, 0x48,
    0x6b, 0xe6, 0x58, 0x5a, 0x7e, 0xb4, 0x21, 0x03,
    0x46, 0xbe, 0xcd, 0x33, 0xc0, 0x04, 0x39, 0x79,
    0xb9, 0xc3, 0xb8, 0x58, 0x8b, 0xd5, 0xe5, 0xa1,
];
const WAR3_EVENT_POOL_HANDLES: u32 = 0x0045_70b8;
const WAR3_EVENT_POOL_GENERATIONS: u32 = 0x0045_90b8;

const WAR3_REPEATED_NULL_CALL_SLOT: u32 = 0x0049_a960;
const WAR3_DIVIDE_EXCEPTION_HANDLER: u32 = 0x0045_a0c0;
const WAR3_DIVIDE_EXCEPTION_EIP: u32 = 0x0045_ae47;
const WAR3_DIVIDE_TABLE_BASE_SLOT: u32 = 0x0049_ef00;
const WAR3_DIVIDE_INDEX_SLOT: u32 = 0x0049_c490;
const WAR3_DIVIDE_STATE_POINTER_SLOT: u32 = 0x0049_dc6c;
const WAR3_SCAN_INDEX: u32 = 0x0049_dc90;
const WAR3_SCAN_SOURCE: u32 = 0x0049_a594;
const WAR3_SCAN_STAGE: u32 = 0x0049_a590;
const WAR3_SCAN_CHECKSUM: u32 = 0x0049_a598;
const WAR3_SCAN_RESET: u32 = 0x0049_2614;
const WAR3_SCAN_GATE: u32 = 0x0049_a430;
const WAR3_SCAN_BOUND: u32 = 0x0049_c650;
const WAR3_SCAN_COUNT: u32 = 0x0049_a980;
const WAR3_HOTLOOP_START: u32 = 0x0045_af60;
const WAR3_SCAN_STEP_START: u32 = 0x0045_af54;
const WAR3_SCAN_STEP_END: u32 = 0x0045_b010;
const WAR3_DWORD_SCAN_INDEX: u32 = 0x0049_aa5c;
const WAR3_TABLE_FILL_BASE_SLOT: u32 = 0x0049_d024;
const WAR3_TABLE_FILL_BOUND: u32 = 0x0000_2000;
const WAR3_TABLE_FILL_VALUE: u32 = 0x0045_e2f0;
const WAR3_TABLE_FILL_STEP_START: u32 = 0x0046_1496;
const WAR3_TABLE_FILL_STEP_END: u32 = 0x0046_14c3;
const WAR3_TABLE_FILL_HEARTBEAT_STRIDE: u32 = 0x100;

// Patch one verified three-byte instruction to a VM exit. The original is
// restored at the exit before any semantic decision, so a failed guard resumes
// the unmodified guest loop. The match is against the prepared guest image,
// after import resolution, rather than the older checked-in game installation.
fn install_war3_event_pool_trap(child: &PendingChild) -> Result<bool, String> {
    if cfg!(feature = "guest-event-pool") { return Ok(false); }
    let mut code = [0u8; (WAR3_EVENT_POOL_TAIL - WAR3_EVENT_POOL_BEGIN) as usize];
    if child.address_space.read(WAR3_EVENT_POOL_BEGIN, &mut code).ok() != Some(code.len())
        || Sha256::digest(code).as_slice() != WAR3_EVENT_POOL_SIGNATURE
    { return Ok(false); }
    if child.address_space.write(WAR3_EVENT_POOL_TRAP, &[0x0f, 0x01, 0xc1]).ok() != Some(3) {
        return Err("install War3 event pool trap failed".into());
    }
    logl::log(level::IMPORTANT, format_args!("WC3 EVENT POOL RUST ARMED eip=0x{WAR3_EVENT_POOL_TRAP:08x}"));
    Ok(true)
}

fn read_event_pool_word(child: &PendingChild, address: u32) -> Option<u32> {
    let mut bytes = [0u8; 4];
    (child.address_space.read(address, &mut bytes).ok() == Some(4))
        .then(|| u32::from_le_bytes(bytes))
}

fn write_event_pool_word(child: &PendingChild, address: u32, value: u32) -> Result<(), String> {
    if child.address_space.write(address, &value.to_le_bytes()).ok() != Some(4) {
        return Err(format!("War3 event pool write failed at 0x{address:08x}"));
    }
    Ok(())
}

fn complete_war3_event_pool(
    child: &PendingChild,
    session: &mut Wc3Session,
    context: &mut GuestContext,
    mut registers: Registers,
) -> Result<bool, String> {
    let original = WAR3_EVENT_POOL_ORIGINAL;
    if child.address_space.write(WAR3_EVENT_POOL_TRAP, &original).ok() != Some(3) {
        return Err("restore War3 event pool instruction failed".into());
    }
    registers.eip = WAR3_EVENT_POOL_TRAP;
    let mut live = [0u8; (WAR3_EVENT_POOL_TAIL - WAR3_EVENT_POOL_BEGIN) as usize];
    let expected = child.image.image.get(
        (WAR3_EVENT_POOL_BEGIN - child.image.image_base) as usize
            ..(WAR3_EVENT_POOL_TAIL - child.image.image_base) as usize
    );
    let debug = context.context.debug_registers().map_err(|error| error.to_string())?;
    let frame = registers.ebp;
    let eligible = child.initterm.is_some()
        && child.parked_threads.is_empty()
        && registers.eflags & 0x0007_4500 == 0 // TF, DF, NT, RF, VM, AC
        && debug.dr7 & 0x23ff == 0
        && registers.esi == WAR3_EVENT_POOL_HANDLES
        && registers.ebx == WAR3_EVENT_POOL_GENERATIONS
        && registers.edi == 0x0020_0000
        && registers.eax == 0
        && registers.esp == frame.wrapping_sub(0x40)
        && frame >= STACK_BASE + 0x40
        && frame < STACK_TOP - 8
        && read_event_pool_word(child, frame.wrapping_add(4)) == Some(0x0040_2988)
        && read_event_pool_word(child, frame.wrapping_sub(4)) == Some(0)
        && read_event_pool_word(child, frame.wrapping_sub(8)) == Some(WAR3_EVENT_POOL_GENERATIONS)
        && read_event_pool_word(child, frame.wrapping_sub(0x10)) == Some(0)
        && read_event_pool_word(child, frame.wrapping_sub(0x0c)) == Some(0x400)
        && child.address_space.read(WAR3_EVENT_POOL_BEGIN, &mut live).ok() == Some(live.len())
        && expected == Some(live.as_slice())
        && session.next_event_handle.checked_add(2048).is_some();
    if !eligible {
        context.context.set_registers(registers).map_err(|error| error.to_string())?;
        logl::log(level::IMPORTANT, format_args!("WC3 EVENT POOL RUST BYPASS reason=state-or-code-mismatch"));
        return Ok(false);
    }
    // Image maps are RWX throughout this region. Confirm every page before
    // creating real session objects; no guest thread can observe an intermediate
    // state because the child is stopped at the trap.
    for page in ((WAR3_EVENT_POOL_HANDLES & !0xfff)..0x0045_d000).step_by(0x1000) {
        let mut probe = [0u8; 4];
        if child.address_space.read(page, &mut probe).ok() != Some(4) {
            context.context.set_registers(registers).map_err(|error| error.to_string())?;
            return Ok(false);
        }
    }
    let tables = wc3::event_pool::build(|manual_reset| {
        let (handle, already_exists) = session.create_event(child.pid, wc3::session::CreateEventRequest {
            name: None,
            manual_reset,
            initial_state: false,
            inheritable: false,
        });
        if already_exists { 0 } else { handle }
    }).map_err(str::to_owned)?;
    for bank in 0..2u32 {
        write_event_pool_word(child, 0x0045_70ac + bank * 4, 0x7ff)?;
        write_event_pool_word(child, 0x0045_709c + bank * 4, 0x3ff)?;
        write_event_pool_word(child, 0x0045_70a4 + bank * 4, 0x400)?;
    }
    let handles = &tables.handles;
    let generations = &tables.generations;
    if child.address_space.write(WAR3_EVENT_POOL_HANDLES, &handles).ok() != Some(handles.len()) {
        return Err("War3 event pool handles write failed".into());
    }
    for (bank, generation) in generations.iter().enumerate() {
        let address = WAR3_EVENT_POOL_GENERATIONS + bank as u32 * 0x2000;
        if child.address_space.write(address, generation).ok() != Some(generation.len()) {
            return Err(format!("War3 event pool generation write failed bank={bank}"));
        }
    }
    session.process_mut(child.pid).ok_or("War3 event owner missing")?.xp
        .set_last_error_for_thread(child.tid, 0);
    write_event_pool_word(child, frame.wrapping_sub(4), 2)?;
    write_event_pool_word(child, frame.wrapping_sub(8), 0x0045_d0b8)?;
    write_event_pool_word(child, frame.wrapping_sub(0x10), 1)?;
    write_event_pool_word(child, frame.wrapping_sub(0x0c), 0)?;
    registers.eax = 0x0045_d0b8;
    registers.ebx = 0x0045_c0b8;
    registers.ecx = 1;
    registers.esi = WAR3_EVENT_POOL_GENERATIONS;
    registers.edi = 0x8020_0000;
    registers.eflags = (registers.eflags & !0x8d5) | 0x44; // final CMP: equality
    registers.eip = WAR3_EVENT_POOL_TAIL;
    context.context.set_registers(registers).map_err(|error| error.to_string())?;
    logl::log(level::IMPORTANT, format_args!("WC3 EVENT POOL RUST COMPLETE pid={} tid={} events=2048 next_handle=0x{:08x}", child.pid, child.tid, session.next_event_handle));
    Ok(true)
}

fn install_storm_record_expand_trap(address_space: &AddressSpace) -> Result<bool, String> {
    if cfg!(feature = "guest-record-expand") { return Ok(false); }
    let mut code = [0u8; (STORM_EXPAND_TAIL + 3 - STORM_EXPAND_BEGIN) as usize];
    if address_space.read(STORM_EXPAND_BEGIN, &mut code).ok() != Some(code.len())
        || Sha256::digest(code).as_slice() != STORM_EXPAND_SIGNATURE
    { return Ok(false); }
    if address_space.write(STORM_EXPAND_TRAP, &[0x0f, 0x01, 0xc1]).ok() != Some(3) {
        return Err("install Storm record expansion trap failed".into());
    }
    logl::log(level::IMPORTANT, format_args!("WC3 RECORD EXPAND RUST ARMED eip=0x{STORM_EXPAND_TRAP:08x}"));
    Ok(true)
}

fn complete_storm_record_expand(
    child: &PendingChild,
    session: &Wc3Session,
    context: &mut GuestContext,
    mut registers: Registers,
) -> Result<bool, String> {
    if child.address_space.write(STORM_EXPAND_TRAP, &STORM_EXPAND_ORIGINAL).ok() != Some(3) {
        return Err("restore Storm record expansion instruction failed".into());
    }
    registers.eip = STORM_EXPAND_TRAP;
    let module = child.native_modules.iter().find(|module| module.stored.eq_ignore_ascii_case("Storm.dll"));
    let expected = module.and_then(|module| module.image.image.get(
        (STORM_EXPAND_BEGIN - module.image.image_base) as usize
            ..(STORM_EXPAND_TAIL + 3 - module.image.image_base) as usize
    ));
    let mut live = [0u8; (STORM_EXPAND_TAIL + 3 - STORM_EXPAND_BEGIN) as usize];
    let debug = context.context.debug_registers().map_err(|error| error.to_string())?;
    let frame = registers.ebp;
    let metadata = read_event_pool_word(child, registers.ebx.wrapping_add(0x138));
    let count = metadata.and_then(|metadata| read_event_pool_word(child, metadata.wrapping_add(0x1c)));
    let base = read_event_pool_word(child, registers.ebx.wrapping_add(0x13c));
    let plan = count.zip(base).and_then(|(count, base)| {
        if !(2..=65_536).contains(&count) { return None; }
        let last = count.checked_sub(1)?;
        let source_end = base.checked_add(count.checked_mul(16)?)?;
        let output_end = base.checked_add(count.checked_mul(44)?)?;
        let expected_source = base.checked_add(last.checked_mul(16)?)?;
        let expected_destination = base.checked_add(last.checked_mul(44)?)?;
        let reservation = session.process(child.pid)?.xp.virtual_reservation_containing(base, output_end - base)?;
        let committed = reservation.committed.iter().any(|commit| {
            commit.base <= base && commit.base.checked_add(commit.size).is_some_and(|end| end >= output_end)
        });
        (committed && source_end <= output_end
            && registers.edi == expected_destination
            && registers.esi == expected_destination.checked_add(36)?
            && read_event_pool_word(child, frame.wrapping_sub(4)) == Some(count)
            && read_event_pool_word(child, frame.wrapping_sub(8)) == Some(expected_source)
            && read_event_pool_word(child, frame.wrapping_sub(12)) == Some(last))
            .then_some((count as usize, base, source_end, output_end))
    });
    let eligible = child.parked_threads.is_empty()
        && registers.eflags & 0x0007_4500 == 0 // TF, DF, NT, RF, VM, AC
        && debug.dr7 & 0x23ff == 0
        && frame >= STACK_BASE + 0x40 && frame < STACK_TOP - 8
        && child.address_space.read(STORM_EXPAND_BEGIN, &mut live).ok() == Some(live.len())
        && expected == Some(live.as_slice());
    let Some((count, base, source_end, output_end)) = plan.filter(|_| eligible) else {
        context.context.set_registers(registers).map_err(|error| error.to_string())?;
        logl::log(level::IMPORTANT, format_args!("WC3 RECORD EXPAND RUST BYPASS reason=state-or-code-mismatch"));
        return Ok(false);
    };
    let mut source = vec![0u8; (source_end - base) as usize];
    if child.address_space.read(base, &mut source).ok() != Some(source.len()) {
        context.context.set_registers(registers).map_err(|error| error.to_string())?;
        return Ok(false);
    }
    let output = wc3::record_expand::build(&source, count)
        .ok_or("Storm record expansion builder rejected guarded count")?;
    if child.address_space.write(base + 44, &output).ok() != Some(output.len()) {
        return Err("Storm record expansion output write failed".into());
    }
    write_event_pool_word(child, frame - 8, base)?;
    write_event_pool_word(child, frame - 12, 0)?;
    registers.eax = 0;
    registers.ecx = base;
    registers.edi = base;
    registers.esi = base + 36;
    registers.eflags = (registers.eflags & !0x8d5) | 0x44;
    registers.eip = STORM_EXPAND_TAIL;
    context.context.set_registers(registers).map_err(|error| error.to_string())?;
    logl::log(level::IMPORTANT, format_args!("WC3 RECORD EXPAND RUST COMPLETE pid={} tid={} records={} input_bytes={} output_bytes={} output_end=0x{:08x}", child.pid, child.tid, count - 1, source.len(), output.len(), output_end));
    Ok(true)
}

fn wc3_cpuid(leaf: u32, subleaf: u32) -> Result<[u32; 4], String> {
    match (leaf, subleaf) {
        (0, _) => Ok([1, 0x756e_6547, 0x6c65_746e, 0x4965_6e69]),
        (1, _) => Ok([
            0x0000_06b1,
            0,
            0,
            (1 << 0) | (1 << 4) | (1 << 8) | (1 << 15) | (1 << 23) | (1 << 24) | (1 << 25),
        ]),
        // Extended CPUID exists, but this PIII-style profile advertises no
        // extended feature, brand, or address-size leaves.
        (0x8000_0000, _) => Ok([0x8000_0000, 0, 0, 0]),
        _ => Err(format!(
            "WC3 CPUID frontier leaf=0x{leaf:08x} subleaf=0x{subleaf:08x}"
        )),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ranges_overlap(destination: u32, source: u32, count: u32) -> bool {
    if count == 0 {
        return false;
    }
    let Some(destination_end) = destination.checked_add(count) else {
        return true;
    };
    let Some(source_end) = source.checked_add(count) else {
        return true;
    };
    destination < source_end && source < destination_end
}

const MSVCRT_EM_INVALID: u32 = 0x0000_0010;
const MSVCRT_EM_DENORMAL: u32 = 0x0008_0000;
const MSVCRT_EM_ZERODIVIDE: u32 = 0x0000_0008;
const MSVCRT_EM_OVERFLOW: u32 = 0x0000_0004;
const MSVCRT_EM_UNDERFLOW: u32 = 0x0000_0002;
const MSVCRT_EM_INEXACT: u32 = 0x0000_0001;
const MSVCRT_MCW_RC: u32 = 0x0000_0300;
const MSVCRT_MCW_PC: u32 = 0x0003_0000;
const MSVCRT_MCW_IC: u32 = 0x0004_0000;
const MSVCRT_MCW_EM: u32 = 0x0008_001f;
const MSVCRT_X87_CONTROL_MASK: u32 =
    MSVCRT_MCW_EM | MSVCRT_MCW_RC | MSVCRT_MCW_PC | MSVCRT_MCW_IC;

fn msvcrt_control_from_x87(fcw: u16) -> u32 {
    let mut out = 0;
    if fcw & 0x0001 != 0 {
        out |= 0x0000_0010;
    }
    if fcw & 0x0002 != 0 {
        out |= 0x0008_0000;
    }
    if fcw & 0x0004 != 0 {
        out |= 0x0000_0008;
    }
    if fcw & 0x0008 != 0 {
        out |= 0x0000_0004;
    }
    if fcw & 0x0010 != 0 {
        out |= 0x0000_0002;
    }
    if fcw & 0x0020 != 0 {
        out |= 0x0000_0001;
    }
    out |= match fcw & 0x0c00 {
        0x0000 => 0x0000_0000,
        0x0400 => 0x0000_0100,
        0x0800 => 0x0000_0200,
        0x0c00 => 0x0000_0300,
        _ => unreachable!(),
    };
    out |= match fcw & 0x0300 {
        0x0000 => 0x0002_0000,
        0x0200 => 0x0001_0000,
        0x0300 => 0x0000_0000,
        _ => 0,
    };
    if fcw & 0x1000 != 0 {
        out |= 0x0004_0000;
    }
    out
}

/// Apply the MSVCRT abstract control word to x87 while retaining unrelated
/// native control-word bits.
fn x87_control_from_msvcrt(original_fcw: u16, control: u32) -> u16 {
    let mut fcw = original_fcw & !0x1f3f;

    if control & MSVCRT_EM_INVALID != 0 { fcw |= 1 << 0; }
    if control & MSVCRT_EM_DENORMAL != 0 { fcw |= 1 << 1; }
    if control & MSVCRT_EM_ZERODIVIDE != 0 { fcw |= 1 << 2; }
    if control & MSVCRT_EM_OVERFLOW != 0 { fcw |= 1 << 3; }
    if control & MSVCRT_EM_UNDERFLOW != 0 { fcw |= 1 << 4; }
    if control & MSVCRT_EM_INEXACT != 0 { fcw |= 1 << 5; }

    fcw |= match control & MSVCRT_MCW_PC {
        0x0002_0000 => 0x0000, // _PC_24
        0x0001_0000 => 0x0200, // _PC_53
        0x0000_0000 => 0x0300, // _PC_64
        _ => 0x0300,
    };
    fcw |= match control & MSVCRT_MCW_RC {
        0x0000_0000 => 0x0000, // nearest
        0x0000_0100 => 0x0400, // down
        0x0000_0200 => 0x0800, // up
        0x0000_0300 => 0x0c00, // chop
        _ => unreachable!(),
    };
    if control & MSVCRT_MCW_IC != 0 {
        fcw |= 0x1000; // affine
    }
    fcw
}

/// Translate the x87 sticky exception flags into MSVCRT's `_SW_*` layout.
fn msvcrt_status_from_x87(fsw: u16) -> u32 {
    let mut out = 0;
    if fsw & 0x0001 != 0 {
        out |= 0x0000_0010; // _SW_INVALID
    }
    if fsw & 0x0002 != 0 {
        out |= 0x0008_0000; // _SW_DENORMAL
    }
    if fsw & 0x0004 != 0 {
        out |= 0x0000_0008; // _SW_ZERODIVIDE
    }
    if fsw & 0x0008 != 0 {
        out |= 0x0000_0004; // _SW_OVERFLOW
    }
    if fsw & 0x0010 != 0 {
        out |= 0x0000_0002; // _SW_UNDERFLOW
    }
    if fsw & 0x0020 != 0 {
        out |= 0x0000_0001; // _SW_INEXACT
    }
    out
}

const TABLE_CHECKPOINT_PAGE_BYTES: usize = 4096;
const TABLE_BOUNDARY: wc3::checkpoint::Boundary = wc3::checkpoint::Boundary {
    from: TABLE_CHECKPOINT_FROM_EIP,
    to: TABLE_CHECKPOINT_TO_EIP,
    table_bytes: TABLE_CHECKPOINT_TABLE_BYTES,
};
use wc3::checkpoint::{TableCheckpoint, TableCheckpointPage};
fn checkpoint_encode(checkpoint: &TableCheckpoint) -> Result<Vec<u8>, String> {
    wc3::checkpoint::encode(checkpoint, TABLE_BOUNDARY)
}
fn checkpoint_decode(bytes: &[u8]) -> Result<TableCheckpoint, String> {
    wc3::checkpoint::decode(bytes, TABLE_BOUNDARY)
}

fn checkpoint_sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn checkpoint_read(child: &PendingChild, va: u32, bytes: &mut [u8]) -> Result<(), String> {
    let read = child
        .address_space
        .read(va, bytes)
        .map_err(|error| format!("table checkpoint read 0x{va:08x}: {error}"))?;
    if read == bytes.len() {
        Ok(())
    } else {
        Err(format!(
            "table checkpoint short read 0x{va:08x}: {read}/{}",
            bytes.len()
        ))
    }
}

fn checkpoint_write(child: &mut PendingChild, va: u32, bytes: &[u8]) -> Result<(), String> {
    let written = child
        .address_space
        .write(va, bytes)
        .map_err(|error| format!("table checkpoint write 0x{va:08x}: {error}"))?;
    if written == bytes.len() {
        Ok(())
    } else {
        Err(format!(
            "table checkpoint short write 0x{va:08x}: {written}/{}",
            bytes.len()
        ))
    }
}

fn checkpoint_capture_pages(
    child: &PendingChild,
    ranges: &[(u32, u32)],
) -> Result<Vec<CachedPage>, String> {
    let mut pages = std::collections::BTreeMap::new();
    for &(start, len) in ranges {
        let end = start
            .checked_add(len)
            .ok_or("table checkpoint range overflow")?;
        let mut va = start & !0xfff;
        let page_end = end
            .checked_add(0xfff)
            .ok_or("table checkpoint page range overflow")?
            & !0xfff;
        while va < page_end {
            let mut before = vec![0; TABLE_CHECKPOINT_PAGE_BYTES];
            checkpoint_read(child, va, &mut before)?;
            pages.entry(va).or_insert_with(|| CachedPage {
                va,
                before_sha256: checkpoint_sha256(&before),
                before,
            });
            va = va
                .checked_add(TABLE_CHECKPOINT_PAGE_BYTES as u32)
                .ok_or("table checkpoint VA overflow")?;
        }
    }
    Ok(pages.into_values().collect())
}

fn table_checkpoint_quiescent(child: &PendingChild) -> Result<(), &'static str> {
    if child.execution != ChildExecutionState::ImageEntryRunning {
        return Err("execution");
    }
    if child.seh.is_some() {
        return Err("pending-seh");
    }
    if child.unhandled_filter_call.is_some() {
        return Err("pending-uef");
    }
    if child.initterm.is_some() {
        return Err("initterm");
    }
    if child.cipow.is_some() {
        return Err("cipow");
    }
    Ok(())
}

fn table_checkpoint_table_base(child: &PendingChild) -> Result<u32, String> {
    child_read_u32(child, WAR3_TABLE_FILL_BASE_SLOT)
        .filter(|base| *base != 0)
        .ok_or_else(|| "table checkpoint table base unavailable".into())
}

fn table_checkpoint_host_state(
    session: &Wc3Session,
    pid: u32,
) -> Result<(u32, usize, usize, usize), String> {
    let process = session
        .process(pid)
        .ok_or_else(|| "table checkpoint child process missing".to_owned())?;
    Ok((
        process.xp.call_count,
        process.xp.provider_import_count(),
        session.objects.len(),
        process.handles.len(),
    ))
}

fn begin_table_checkpoint_capture(
    child: &mut PendingChild,
    context: &GuestThreadContext,
    restored: Registers,
    restored_debug: DebugRegisters,
    session: &Wc3Session,
) -> Result<(), String> {
    table_checkpoint_quiescent(child).map_err(str::to_owned)?;
    let (call_count, provider_import_count, session_object_count, process_handle_count) =
        table_checkpoint_host_state(session, child.pid)?;
    let table_base = table_checkpoint_table_base(child)?;
    let image_bytes =
        u32::try_from(child.image.image.len()).map_err(|_| "table checkpoint image size")?;
    let teb = thread_teb_va(child.tid)?;
    let pages = checkpoint_capture_pages(
        child,
        &[
            (child.image.image_base, image_bytes),
            (PROCESS_DATA_VA, 0x1000),
            (teb, 0x1000),
            (STACK_BASE, STACK_BYTES as u32),
            (table_base, TABLE_CHECKPOINT_TABLE_BYTES),
        ],
    )?;
    child.table_checkpoint_capture = Some(TableCheckpointCapture {
        table_base,
        before_registers: restored,
        before_debug_registers: restored_debug,
        before_extended_state: context
            .extended_state()
            .map_err(|error| error.to_string())?,
        pages,
        call_count,
        provider_import_count,
        session_object_count,
        process_handle_count,
        provider_thunk_bytes: child.provider_thunk_bytes,
        crt_heap_mapped_end: child.crt_heap_mapped_end,
        win_heap_mapped_end: child.win_heap_mapped_end,
    });
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT CAPTURE entry=0x{:08x} index=0 table=0x{:08x} pages={} call_count={}",
            restored.eip,
            table_base,
            child.table_checkpoint_capture.as_ref().unwrap().pages.len(),
            call_count,
        ),
    );
    Ok(())
}

async fn finish_table_checkpoint_capture(
    child: &mut PendingChild,
    context: &GuestThreadContext,
    restored: Registers,
    restored_debug: DebugRegisters,
    session: &Wc3Session,
) -> Result<(), String> {
    table_checkpoint_quiescent(child).map_err(str::to_owned)?;
    let capture = child
        .table_checkpoint_capture
        .take()
        .ok_or_else(|| "table checkpoint capture missing".to_owned())?;
    if child_read_u32(child, WAR3_DWORD_SCAN_INDEX) != Some(WAR3_TABLE_FILL_BOUND) {
        return Err("table checkpoint exit index".into());
    }
    if table_checkpoint_table_base(child)? != capture.table_base {
        return Err("table checkpoint table base changed".into());
    }
    let (call_count, provider_import_count, session_object_count, process_handle_count) =
        table_checkpoint_host_state(session, child.pid)?;
    if child.execution != ChildExecutionState::ImageEntryRunning
        || call_count != capture.call_count
        || provider_import_count != capture.provider_import_count
        || session_object_count != capture.session_object_count
        || process_handle_count != capture.process_handle_count
        || child.provider_thunk_bytes != capture.provider_thunk_bytes
        || child.crt_heap_mapped_end != capture.crt_heap_mapped_end
        || child.win_heap_mapped_end != capture.win_heap_mapped_end
    {
        return Err("table checkpoint purity invariant".into());
    }
    let mut pages = Vec::with_capacity(capture.pages.len());
    for page in capture.pages {
        let mut after = vec![0; page.before.len()];
        checkpoint_read(child, page.va, &mut after)?;
        pages.push(TableCheckpointPage {
            va: page.va,
            before_sha256: page.before_sha256,
            after: (after != page.before).then_some(after),
        });
    }
    let checkpoint = TableCheckpoint {
        image_sha256: checkpoint_sha256(child.self_image_bytes.as_slice()),
        table_base: capture.table_base,
        before_registers: capture.before_registers,
        before_debug_registers: capture.before_debug_registers,
        before_extended_state: capture.before_extended_state,
        after_registers: restored,
        after_debug_registers: restored_debug,
        after_extended_state: context
            .extended_state()
            .map_err(|error| error.to_string())?,
        pages,
        after_single_step_count: child.single_step_count,
        after_dword_scan_watch: child.dword_scan_watch,
    };
    let encoded = checkpoint_encode(&checkpoint)?;
    async_fs::create_dir_all(TABLE_CHECKPOINT_DIR)
        .await
        .map_err(|error| format!("create table checkpoint directory: {error}"))?;
    async_fs::write_file(TABLE_CHECKPOINT_PATH, &encoded)
        .await
        .map_err(|error| format!("write table checkpoint: {error}"))?;
    let readback = async_fs::read_file(TABLE_CHECKPOINT_PATH)
        .await
        .map_err(|error| format!("read back table checkpoint: {error}"))?;
    if readback != encoded || checkpoint_decode(&readback).is_err() {
        return Err("table checkpoint read-back verification".into());
    }
    let changed = checkpoint
        .pages
        .iter()
        .filter(|page| page.after.is_some())
        .count();
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT CREATED table=0x{:08x} pages={} changed={} bytes={}",
            checkpoint.table_base,
            checkpoint.pages.len(),
            changed,
            encoded.len(),
        ),
    );
    Ok(())
}

async fn try_restore_table_checkpoint(
    child: &mut PendingChild,
    context: &mut GuestThreadContext,
    restored: &mut Registers,
    restored_debug: &mut DebugRegisters,
) -> Result<bool, String> {
    let bytes = match async_fs::read_file(TABLE_CHECKPOINT_PATH).await {
        Ok(bytes) => bytes,
        Err(error) => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD TABLE CHECKPOINT BYPASS reason=cache-unavailable error={error}"
                ),
            );
            return Ok(false);
        }
    };
    let checkpoint = match checkpoint_decode(&bytes) {
        Ok(checkpoint) => checkpoint,
        Err(reason) => {
            logl::log(
                level::IMPORTANT,
                format_args!("WC3 CHILD TABLE CHECKPOINT BYPASS reason={reason}"),
            );
            return Ok(false);
        }
    };
    let bypass = |reason: &str| {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD TABLE CHECKPOINT BYPASS reason={reason}"),
        );
        false
    };
    if table_checkpoint_quiescent(child).is_err() {
        return Ok(bypass("not-quiescent"));
    }
    if checkpoint.image_sha256 != checkpoint_sha256(child.self_image_bytes.as_slice()) {
        return Ok(bypass("image-sha256"));
    }
    if *restored != checkpoint.before_registers {
        return Ok(bypass("registers"));
    }
    if *restored_debug != checkpoint.before_debug_registers {
        return Ok(bypass("debug-registers"));
    }
    if context
        .extended_state()
        .map_err(|error| error.to_string())?
        != checkpoint.before_extended_state
    {
        return Ok(bypass("extended-state"));
    }
    if table_checkpoint_table_base(child)? != checkpoint.table_base {
        return Ok(bypass("table-base"));
    }
    for page in &checkpoint.pages {
        let mut current = vec![0; TABLE_CHECKPOINT_PAGE_BYTES];
        checkpoint_read(child, page.va, &mut current)?;
        if checkpoint_sha256(&current) != page.before_sha256 {
            return Ok(bypass("precondition-page"));
        }
    }
    for page in &checkpoint.pages {
        if let Some(after) = &page.after {
            checkpoint_write(child, page.va, after)?;
        }
    }
    *restored = checkpoint.after_registers;
    *restored_debug = checkpoint.after_debug_registers;
    context
        .set_extended_state(&checkpoint.after_extended_state)
        .map_err(|error| error.to_string())?;
    child.single_step_count = checkpoint.after_single_step_count;
    child.dword_scan_watch = checkpoint.after_dword_scan_watch;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE CHECKPOINT HIT from=0x{:08x} to=0x{:08x} table=0x{:08x} pages={}",
            TABLE_CHECKPOINT_FROM_EIP,
            TABLE_CHECKPOINT_TO_EIP,
            checkpoint.table_base,
            checkpoint.pages.len(),
        ),
    );
    Ok(true)
}

fn war3_scan_single_step(exception: ChildException, registers: Registers) -> bool {
    exception.vector == Some(1)
        && exception.debug_status == Some(0x0000_4000)
        && (WAR3_SCAN_STEP_START..=WAR3_SCAN_STEP_END).contains(&registers.eip)
}

pub(super) fn war3_dword_scan_single_step(exception: ChildException, registers: Registers) -> bool {
    exception.vector == Some(1)
        && exception.debug_status.is_some_and(|dr6| dr6 & 0x4000 != 0)
        && (WAR3_TABLE_FILL_STEP_START..=WAR3_TABLE_FILL_STEP_END).contains(&registers.eip)
}

fn quiet_war3_exception(exception: ChildException, registers: Registers) -> bool {
    (exception.vector == Some(0) && registers.eip == WAR3_DIVIDE_EXCEPTION_EIP)
        || (exception.vector == Some(14)
            && registers.eip == 0x0045_af51
            && exception.fault_linear == Some(0)
            && exception.error.is_some_and(|error| error & 2 != 0))
}

pub(super) fn boring_war3_single_step(
    exception: ChildException,
    registers: Registers,
    handler: u32,
) -> bool {
    exception.vector == Some(1)
        && exception
            .debug_status
            .is_some_and(|dr6| dr6 & 0x0000_e00f == 0x0000_4000)
        && registers.eip != 0
        && handler == WAR3_DIVIDE_EXCEPTION_HANDLER
}

fn current_child_seh_handler(child: &PendingChild, fs_base: u32) -> Option<u32> {
    let mut head = [0; 4];
    if child.address_space.read(fs_base, &mut head).ok()? != head.len() {
        return None;
    }
    let head = u32::from_le_bytes(head);
    (head != u32::MAX)
        .then(|| {
            read_seh_registration(&child.address_space, head)
                .ok()
                .map(|registration| registration.handler)
        })
        .flatten()
}

fn child_image_import_at_rva(child: &PendingChild, rva: u32) -> Option<&pe32::ImportDescriptor> {
    child
        .image
        .imports
        .iter()
        .find(|import| import.iat_rva == rva)
}

fn import_symbol_diagnostic(symbol: &pe32::ImportSymbol) -> String {
    match symbol {
        pe32::ImportSymbol::Name(name) => format!("\"{name}\""),
        pe32::ImportSymbol::Ordinal(ordinal) => format!("ordinal:{ordinal}"),
    }
}

fn log_null_slot_provenance(child: &PendingChild, slot: u32, runtime_value: Option<u32>) {
    let Some(("War3.exe", rva)) = child_pc_owner(child, slot) else {
        return;
    };
    let runtime_value = runtime_value
        .map(|value| format!("0x{value:08x}"))
        .unwrap_or_else(|| "<unreadable>".into());
    let Some(import) = child_image_import_at_rva(child, rva) else {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD NULL SLOT PROVENANCE slot=0x{slot:08x} rva=0x{rva:08x} kind=war3-runtime-pointer runtime_value={runtime_value}",
            ),
        );
        return;
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL SLOT PROVENANCE slot=0x{slot:08x} rva=0x{rva:08x} kind=pe-import module=\"{}\" symbol={} runtime_value={runtime_value}",
            import.module,
            import_symbol_diagnostic(&import.symbol),
        ),
    );
    let (category, normal_path) = if import.module.eq_ignore_ascii_case("Storm.dll") {
        ("storm-export", "War3-native-export-bind")
    } else if import.module.eq_ignore_ascii_case("Mss32.dll") {
        ("mss-export", "War3-native-export-bind")
    } else {
        ("external-provider", "child_loader-provider-thunk-bind")
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL SLOT BINDING slot=0x{slot:08x} category={category} normal_path={normal_path} reason=slot-zero-after-loader",
        ),
    );
}

fn log_child_slot_xrefs(child: &PendingChild, slot: u32) {
    let needle = slot.to_le_bytes();
    let mut matches = 0u32;
    for (offset, bytes) in child.image.image.windows(needle.len()).enumerate() {
        if bytes != needle {
            continue;
        }
        let Ok(rva) = u32::try_from(offset) else {
            break;
        };
        let Some(site) = child.image.image_base.checked_add(rva) else {
            break;
        };
        let context_start = offset.saturating_sub(16);
        let context_end = offset
            .saturating_add(needle.len())
            .saturating_add(16)
            .min(child.image.image.len());
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SLOT XREF slot=0x{slot:08x} site=0x{site:08x} rva=0x{rva:08x} bytes=\"{}\"",
                diagnostic_hex_bytes(&child.image.image[context_start..context_end]),
            ),
        );
        matches = matches.saturating_add(1);
    }
    if matches == 0 {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD SLOT XREF slot=0x{slot:08x} matches=0",),
        );
    }
}

fn should_log_execution_sample(count: u64) -> bool {
    count == 1 || count % EXEC_SAMPLE_PREEMPTIONS == 0
}

fn service_sync_request(
    session: &mut Wc3Session,
    caller_pid: u32,
    caller_tid: u32,
    request: SessionRequest,
    contexts: &mut [GuestContext],
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<u32, String> {
    match request {
        SessionRequest::CreateEvent(request) => {
            let (handle, already_exists) = session.create_event(caller_pid, request);
            session
                .process_mut(caller_pid)
                .ok_or_else(|| "event process missing".to_owned())?
                .xp
                .set_last_error_for_thread(caller_tid, if handle == 0 { 6 } else if already_exists { 183 } else { 0 });
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD EVENT CREATE pid={} handle=0x{:08x} already_exists={}",
                    caller_pid, handle, already_exists as u8,
                ),
            );
            Ok(handle)
        }
        SessionRequest::OpenEvent {
            key,
            desired_access,
            inheritable,
            name,
        } => match session.open_event(key, &name, inheritable) {
            Ok((handle, object)) => {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD OPENEVENTA RESULT pid={} tid={} desired_access=0x{:08x} access=EVENT_MODIFY_STATE inheritable={} name={:?} object={} handle=0x{:08x} result=success last_error=unchanged cleanup=12-by-thunk",
                        key.pid,
                        key.tid,
                        desired_access,
                        inheritable as u8,
                        name,
                        object,
                        handle,
                    ),
                );
                Ok(handle)
            }
            Err(error) => {
                session
                    .process_mut(key.pid)
                    .ok_or_else(|| "OpenEventA process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(key.tid, error);
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD OPENEVENTA RESULT pid={} tid={} desired_access=0x{:08x} inheritable={} name={:?} handle=NULL result=failure error={}",
                        key.pid,
                        key.tid,
                        desired_access,
                        inheritable as u8,
                        name,
                        error,
                    ),
                );
                Ok(0)
            }
        },
        SessionRequest::SetEvent { pid, tid, handle } => match session.set_event(pid, handle) {
            Ok(outcome) => {
                let waiters_woken = outcome.woken.len();
                resume_completed_waiters(
                    &outcome.woken,
                    "set-event",
                    contexts,
                    wait_deadlines,
                )?;
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EVENT SET pid={} tid={} handle=0x{:08x} manual_reset={} was_signaled={} waiters_woken={} result=1",
                        pid,
                        tid,
                        handle,
                        outcome.manual_reset as u8,
                        outcome.was_signaled as u8,
                        waiters_woken,
                    ),
                );
                Ok(1)
            }
            Err(_) => {
                session
                    .process_mut(pid)
                    .ok_or_else(|| "SetEvent process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(tid, 6);
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EVENT SET pid={} tid={} handle=0x{:08x} result=0 error=6",
                        pid, tid, handle,
                    ),
                );
                Ok(0)
            }
        },
        SessionRequest::ResetEvent { pid, tid, handle } => match session.reset_event(pid, handle) {
            Ok(outcome) => {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EVENT RESET pid={} tid={} handle=0x{:08x} manual_reset={} was_signaled={} result=1",
                        pid,
                        tid,
                        handle,
                        outcome.manual_reset as u8,
                        outcome.was_signaled as u8,
                    ),
                );
                Ok(1)
            }
            Err(_) => {
                session
                    .process_mut(pid)
                    .ok_or_else(|| "ResetEvent process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(tid, 6);
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD EVENT RESET pid={} tid={} handle=0x{:08x} result=0 error=6",
                        pid, tid, handle,
                    ),
                );
                Ok(0)
            }
        },
        SessionRequest::CreateMutex { key, request } => {
            let (handle, error, existed) = match session.create_mutex(key, request) {
                Ok((handle, existed)) => (handle, if existed { 183 } else { 0 }, existed),
                Err(error) => (0, error, false),
            };
            session
                .process_mut(key.pid)
                .ok_or_else(|| "mutex process missing".to_owned())?
                .xp
                .set_last_error_for_thread(key.tid, error);
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD MUTEX CREATE pid={} tid={} handle=0x{:08x} already_exists={} error={}",
                    key.pid, key.tid, handle, existed as u8, error
                ),
            );
            Ok(handle)
        }
        SessionRequest::ReleaseMutex { key, handle } => match session.release_mutex(key, handle) {
            Ok(woken) => {
                resume_completed_waiters(&woken, "release-mutex", contexts, wait_deadlines)?;
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD MUTEX RELEASE pid={} tid={} handle=0x{:08x} waiters_woken={} result=1",
                        key.pid,
                        key.tid,
                        handle,
                        woken.len()
                    ),
                );
                Ok(1)
            }
            Err(error) => {
                session
                    .process_mut(key.pid)
                    .ok_or_else(|| "mutex process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(key.tid, error);
                Ok(0)
            }
        },
        SessionRequest::CreateIoCompletionPort {
            caller,
            concurrency,
        } => match session.create_io_completion_port(caller, concurrency) {
            Ok((handle, object)) => {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD CREATEIOCOMPLETIONPORT RESULT \\
                         pid={} tid={} handle=0x{:08x} object={} concurrency={} \\
                         queue_depth=0 associations=0 transport=offline \\
                         trueos_network=0 result=success cleanup=16-by-thunk",
                        caller.pid,
                        caller.tid,
                        handle,
                        object,
                        concurrency,
                    ),
                );
                Ok(handle)
            }
            Err(error) => {
                session
                    .process_mut(caller.pid)
                    .ok_or_else(|| "IOCP process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(caller.tid, error);
                Ok(0)
            }
        },
        SessionRequest::CloseHandle { pid, handle } => {
            if session.close_handle(pid, handle) {
                Ok(1)
            } else {
                session
                    .process_mut(pid)
                    .ok_or_else(|| "CloseHandle process missing".to_owned())?
                    .xp
                    .set_last_error_for_thread(caller_tid, 6);
                Ok(0)
            }
        }
        _ => Err("unexpected synchronization request".into()),
    }
}

fn resume_completed_waiters(
    woken: &[CompletedWait],
    reason: &str,
    contexts: &mut [GuestContext],
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<(), String> {
    for completed in woken {
        let request = &completed.request;
        let index = context_index(contexts, request.key)
            .ok_or_else(|| format!("{reason} waiter context missing"))?;
        let mut registers = wait_deadlines
            .remove(&request.key)
            .map(|wait| wait.resume_registers)
            .unwrap_or(
                contexts[index]
                    .context
                    .registers()
                    .map_err(|error| error.to_string())?,
            );
        registers.eax = completed.result;
        contexts[index]
            .context
            .set_registers(registers)
            .map_err(|error| error.to_string())?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 WAIT SIGNALED pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} index={} reason={} result=0x{:08x}",
                request.key.pid,
                request.key.tid,
                request.handles[0],
                request.handles[1],
                request.count,
                request.wait_all,
                completed.result.saturating_sub(WAIT_OBJECT_0),
                reason,
                completed.result,
            ),
        );
    }
    Ok(())
}

fn terminate_launcher_thread(
    session: &mut Wc3Session,
    contexts: &mut Vec<GuestContext>,
    active: usize,
    exit_code: u32,
    thread_calls: &mut HashMap<(u32, u32), u32>,
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
) -> Result<Option<usize>, String> {
    let key = contexts
        .get(active)
        .ok_or_else(|| "ExitThread active context missing".to_owned())?
        .key();
    if key.pid != LAUNCHER_PID || key.tid == LAUNCHER_TID {
        return Err("ExitThread launcher-primary-thread frontier".into());
    }
    contexts.remove(active);
    session
        .launcher_mut()
        .xp
        .exit_thread(key.tid, exit_code)
        .map_err(str::to_owned)?;
    let woken = session
        .signal_thread(key, exit_code)
        .map_err(str::to_owned)?;
    wait_deadlines.remove(&key);
    thread_calls.remove(&(key.pid, key.tid));
    resume_completed_waiters(&woken, "thread-exit", contexts, wait_deadlines)?;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 EXITTHREAD pid={} tid={} exit_code=0x{:08x} contexts_removed=1 waiters_woken={} result=terminated",
            key.pid,
            key.tid,
            exit_code,
            woken.len(),
        ),
    );
    if contexts.is_empty() {
        return Ok(None);
    }
    Ok(pop_runnable_context(session, contexts).or(Some(active % contexts.len())))
}

fn child_pc_owner(child: &PendingChild, eip: u32) -> Option<(&str, u32)> {
    let in_image = |base: u32, size: u32| {
        base.checked_add(size)
            .filter(|end| base <= eip && eip < *end)
            .map(|_| eip - base)
    };
    if let Some(rva) = in_image(child.image.image_base, child.image.size_of_image) {
        return Some(("War3.exe", rva));
    }
    for module in &child.native_modules {
        if let Some(rva) = in_image(module.image.image_base, module.image.size_of_image) {
            return Some((module.stored.as_str(), rva));
        }
    }
    let provider_end = thunk32::THUNK_BASE.checked_add(child.provider_thunk_bytes as u32)?;
    if thunk32::THUNK_BASE <= eip && eip < provider_end {
        return Some(("provider-thunks", eip - thunk32::THUNK_BASE));
    }
    let control_end = thunk32::CHILD_CONTROL_BASE.checked_add(0x1000)?;
    if thunk32::CHILD_CONTROL_BASE <= eip && eip < control_end {
        return Some(("child-control", eip - thunk32::CHILD_CONTROL_BASE));
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NullCallSource {
    Register {
        name: &'static str,
        target: u32,
    },
    AbsoluteMemory {
        slot: u32,
        target: Option<u32>,
    },
    RegisterMemory {
        name: &'static str,
        displacement: i32,
        slot: u32,
        target: Option<u32>,
    },
    Unknown,
}

fn register_value(registers: Registers, index: u8) -> (&'static str, u32) {
    match index {
        0 => ("eax", registers.eax),
        1 => ("ecx", registers.ecx),
        2 => ("edx", registers.edx),
        3 => ("ebx", registers.ebx),
        4 => ("esp", registers.esp),
        5 => ("ebp", registers.ebp),
        6 => ("esi", registers.esi),
        7 => ("edi", registers.edi),
        _ => unreachable!("x86 register index is three bits"),
    }
}

pub(super) fn classify_null_call_source(
    bytes: &[u8],
    registers: Registers,
    read_word: impl Fn(u32) -> Option<u32>,
) -> NullCallSource {
    let (modrm, displacement) = if let Some(instruction) =
        bytes.get(bytes.len().saturating_sub(2)..)
        && instruction.len() == 2
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 3
    {
        (instruction[1], 0)
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(6)..)
        && instruction.len() == 6
        && instruction[0] == 0xff
        && (instruction[1] == 0x15 || instruction[1] >> 6 == 2)
    {
        let modrm = instruction[1];
        if modrm == 0x15 {
            let slot = u32::from_le_bytes(instruction[2..6].try_into().unwrap());
            return NullCallSource::AbsoluteMemory {
                slot,
                target: read_word(slot),
            };
        }
        (
            modrm,
            i32::from_le_bytes(instruction[2..6].try_into().unwrap()),
        )
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(3)..)
        && instruction.len() == 3
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 1
    {
        (instruction[1], i32::from(instruction[2] as i8))
    } else if let Some(instruction) = bytes.get(bytes.len().saturating_sub(2)..)
        && instruction.len() == 2
        && instruction[0] == 0xff
        && instruction[1] >> 6 == 0
    {
        (instruction[1], 0)
    } else {
        return NullCallSource::Unknown;
    };
    if (modrm >> 3) & 7 != 2 {
        return NullCallSource::Unknown;
    }
    let mode = modrm >> 6;
    let base = modrm & 7;
    if mode == 3 {
        let (name, target) = register_value(registers, base);
        return NullCallSource::Register { name, target };
    }

    // ModRM base 4 has a SIB byte, while mod=00/base=5 is absolute addressing.
    if base == 4 || (mode == 0 && base == 5) {
        return NullCallSource::Unknown;
    }
    let (name, value) = register_value(registers, base);
    let slot = value.wrapping_add_signed(displacement);
    NullCallSource::RegisterMemory {
        name,
        displacement,
        slot,
        target: read_word(slot),
    }
}

fn child_fault_stack_return(child: &PendingChild, registers: Registers) -> Option<u32> {
    let mut bytes = [0; 4];
    (child.address_space.read(registers.esp, &mut bytes).ok()? == bytes.len())
        .then(|| u32::from_le_bytes(bytes))
}

fn code_before_return(address_space: &AddressSpace, return_address: u32) -> Option<[u8; 16]> {
    let start = return_address.checked_sub(16)?;
    let mut bytes = [0; 16];
    (address_space.read(start, &mut bytes).ok()? == bytes.len()).then_some(bytes)
}

fn diagnostic_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn child_read_u32(child: &PendingChild, address: u32) -> Option<u32> {
    let mut bytes = [0; 4];
    (child.address_space.read(address, &mut bytes).ok()? == bytes.len())
        .then(|| u32::from_le_bytes(bytes))
}

fn child_read_u16(child: &PendingChild, address: u32) -> Option<u16> {
    let mut bytes = [0; 2];
    (child.address_space.read(address, &mut bytes).ok()? == bytes.len())
        .then(|| u16::from_le_bytes(bytes))
}

fn child_read_u8(child: &PendingChild, address: u32) -> Option<u8> {
    let mut byte = [0; 1];
    (child.address_space.read(address, &mut byte).ok()? == byte.len()).then_some(byte[0])
}

fn log_war3_scan_progress(
    child: &mut PendingChild,
    eip: u32,
    registers: Registers,
    debug: DebugRegisters,
) {
    // Diagnostic reads only: never make guest execution depend on this observer.
    if !cfg!(feature = "trace-scan") {
        return;
    }
    let progress = ScanProgress {
        stage: child_read_u8(child, WAR3_SCAN_STAGE),
        source: child_read_u32(child, WAR3_SCAN_SOURCE),
        checksum: child_read_u8(child, WAR3_SCAN_CHECKSUM),
        index: child_read_u32(child, WAR3_SCAN_INDEX),
        bound: child_read_u32(child, WAR3_SCAN_BOUND),
        reset: child_read_u32(child, WAR3_SCAN_RESET),
        gate: child_read_u32(child, WAR3_SCAN_GATE),
        tf: registers.eflags & wc3::seh::X86_EFLAGS_TF != 0,
        dr7: debug.dr7,
    };
    let previous = child.scan_progress;
    child.scan_progress = Some(progress);
    let reset = previous.is_some_and(|previous| {
        previous
            .source
            .zip(progress.source)
            .is_some_and(|(old, new)| new < old)
            || previous
                .index
                .zip(progress.index)
                .is_some_and(|(old, new)| new < old)
    });
    let debug_transition =
        previous.is_some_and(|previous| previous.tf != progress.tf || previous.dr7 != progress.dr7);
    let heartbeat = progress
        .source
        .is_some_and(|source| source & 0xff == 0 && child.scan_heartbeat_source != Some(source));
    if !heartbeat && !reset && !debug_transition {
        return;
    }
    if heartbeat {
        child.scan_heartbeat_source = progress.source;
    }
    let reason = if reset {
        "reset"
    } else if debug_transition {
        "debug-transition"
    } else {
        "source-boundary"
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD SCAN HEARTBEAT reason={} eip=0x{:08x} index={} source={} bound={} stage={} dr7=0x{:08x} tf={} checksum={} gate={} reset={} dr6=0x{:08x}",
            reason,
            eip,
            progress
                .index
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
            progress
                .source
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
            progress
                .bound
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
            progress
                .stage
                .map(|value| format!("0x{value:02x}"))
                .unwrap_or_else(|| "-".into()),
            debug.dr7,
            u32::from(progress.tf),
            progress
                .checksum
                .map(|value| format!("0x{value:02x}"))
                .unwrap_or_else(|| "-".into()),
            progress
                .gate
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
            progress
                .reset
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
            debug.dr6,
        ),
    );
}

fn observe_war3_dword_scan(child: &mut PendingChild, eip: u32) {
    if !cfg!(feature = "trace-scan") {
        return;
    }
    let Some(index) = child_read_u32(child, WAR3_DWORD_SCAN_INDEX) else {
        return;
    };
    let watch = child.dword_scan_watch.get_or_insert(DwordScanWatch {
        last_heartbeat_index: None,
    });
    if index % WAR3_TABLE_FILL_HEARTBEAT_STRIDE != 0 || watch.last_heartbeat_index == Some(index) {
        return;
    }
    watch.last_heartbeat_index = Some(index);
    let base = child_read_u32(child, WAR3_TABLE_FILL_BASE_SLOT);
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD TABLE FILL eip=0x{:08x} index=0x{:04x} bound=0x{:04x} base={} value=0x{WAR3_TABLE_FILL_VALUE:08x}",
            eip,
            index,
            WAR3_TABLE_FILL_BOUND,
            base.map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "-".into()),
        ),
    );
}

fn divide_loop_progress(child: &PendingChild) -> Option<DivideLoopProgress> {
    let table_base = child_read_u32(child, WAR3_DIVIDE_TABLE_BASE_SLOT)?;
    let index = child_read_u32(child, WAR3_DIVIDE_INDEX_SLOT)?;
    let state_ptr = child_read_u32(child, WAR3_DIVIDE_STATE_POINTER_SLOT)?;
    let input = child_read_u8(child, table_base.checked_add(index)?)?;
    let accumulator = child_read_u16(child, state_ptr)?;
    Some(DivideLoopProgress {
        table_base,
        index,
        state_ptr,
        input,
        accumulator,
    })
}

fn log_null_call_diagnostic(
    child: &PendingChild,
    pid: u32,
    tid: u32,
    registers: Registers,
) -> Option<NullLoopSignature> {
    let Some(return_address) = child_fault_stack_return(child, registers) else {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=<unreadable>"),
        );
        return None;
    };
    if return_address == 0 {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=0x00000000"),
        );
        return None;
    }
    let (return_owner, return_rva) =
        child_pc_owner(child, return_address).unwrap_or(("unknown", 0));
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL CALL RETURN pid={pid} tid={tid} stack_ret=0x{return_address:08x} owner=\"{return_owner}\" rva=0x{return_rva:08x}",
        ),
    );
    let Some(bytes) = code_before_return(&child.address_space, return_address) else {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD NULL CALL BYTES end=0x{return_address:08x} bytes=\"<unreadable>\""
            ),
        );
        return None;
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NULL CALL BYTES end=0x{return_address:08x} bytes=\"{}\"",
            diagnostic_hex_bytes(&bytes),
        ),
    );
    match classify_null_call_source(&bytes, registers, |slot| {
        let mut word = [0; 4];
        (child.address_space.read(slot, &mut word).ok()? == word.len())
            .then(|| u32::from_le_bytes(word))
    }) {
        NullCallSource::Register { name, target } => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-register register={name} target=0x{target:08x}",
                ),
            );
            None
        }
        NullCallSource::AbsoluteMemory { slot, target } => {
            let (slot_owner, slot_rva) = child_pc_owner(child, slot).unwrap_or(("unknown", 0));
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-absolute-memory slot=0x{slot:08x} slot_owner=\"{slot_owner}\" slot_rva=0x{slot_rva:08x} target={}",
                    target
                        .map(|value| format!("0x{value:08x}"))
                        .unwrap_or_else(|| "<unreadable>".into()),
                ),
            );
            if target == Some(0) {
                log_null_slot_provenance(child, slot, target);
                Some(NullLoopSignature {
                    pid,
                    tid,
                    return_address,
                    slot,
                })
            } else {
                None
            }
        }
        NullCallSource::RegisterMemory {
            name,
            displacement,
            slot,
            target,
        } => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=call-register-memory base={name} displacement=0x{:08x} slot=0x{slot:08x} target={}",
                    displacement as u32,
                    target
                        .map(|value| format!("0x{value:08x}"))
                        .unwrap_or_else(|| "<unreadable>".into()),
                ),
            );
            None
        }
        NullCallSource::Unknown => {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD NULL CALL pid={pid} tid={tid} return=0x{return_address:08x} return_owner=\"{return_owner}\" return_rva=0x{return_rva:08x} kind=unclassified",
                ),
            );
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SehRegistration {
    frame: u32,
    next: u32,
    handler: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnwindTargetRelation {
    ExitUnwind,
    CurrentHead,
    ActiveSehRegistration,
    LaterRegistration(u32),
    NotInChain,
}

impl UnwindTargetRelation {
    const fn name(self) -> &'static str {
        match self {
            Self::ExitUnwind => "exit-unwind",
            Self::CurrentHead => "current-head",
            Self::ActiveSehRegistration => "active-seh-registration",
            Self::LaterRegistration(_) => "later-registration",
            Self::NotInChain => "not-in-chain",
        }
    }
}

pub(super) fn rtl_unwind_current_target_registers(
    registers: Registers,
    provider_esp: u32,
    target_ip: u32,
    return_value: u32,
) -> Result<Registers, &'static str> {
    let mut resumed = registers;
    resumed.eip = target_ip;
    resumed.esp = provider_esp
        .checked_add(20)
        .ok_or("RtlUnwind resume ESP overflow")?;
    resumed.eax = return_value;
    Ok(resumed)
}

fn read_seh_registration(
    address_space: &AddressSpace,
    frame: u32,
) -> Result<SehRegistration, String> {
    if frame == 0 || frame & 3 != 0 {
        return Err("invalid SEH registration frame".into());
    }
    let mut bytes = [0; 8];
    if address_space
        .read(frame, &mut bytes)
        .map_err(|error| error.to_string())?
        != bytes.len()
    {
        return Err("short SEH registration read".into());
    }
    let next = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    let handler = u32::from_le_bytes(bytes[4..].try_into().unwrap());
    if handler == 0 {
        return Err("SEH registration has zero handler".into());
    }
    if next == frame {
        return Err("SEH registration self-loop".into());
    }
    Ok(SehRegistration {
        frame,
        next,
        handler,
    })
}

fn read_seh_chain_head(address_space: &AddressSpace, fs_base: u32) -> Result<u32, String> {
    let mut bytes = [0; 4];
    if address_space
        .read(fs_base, &mut bytes)
        .map_err(|error| error.to_string())?
        != bytes.len()
    {
        return Err("short SEH chain head read".into());
    }
    Ok(u32::from_le_bytes(bytes))
}

fn classify_unwind_target(
    child: &PendingChild,
    current_head: u32,
    target_frame: u32,
) -> Result<UnwindTargetRelation, String> {
    if target_frame == 0 {
        return Ok(UnwindTargetRelation::ExitUnwind);
    }
    if target_frame == current_head {
        return Ok(UnwindTargetRelation::CurrentHead);
    }
    if child
        .seh
        .as_ref()
        .is_some_and(|seh| seh.registration == target_frame)
    {
        return Ok(UnwindTargetRelation::ActiveSehRegistration);
    }

    let mut frame = current_head;
    let mut visited = HashSet::new();
    for links in 0..MAX_SEH_CHAIN_DEPTH {
        if frame == u32::MAX {
            return Ok(UnwindTargetRelation::NotInChain);
        }
        if !visited.insert(frame) {
            return Err("SEH registration loop while classifying RtlUnwind target".into());
        }
        if frame == target_frame {
            return Ok(UnwindTargetRelation::LaterRegistration(links));
        }
        frame = read_seh_registration(&child.address_space, frame)?.next;
    }
    if frame == u32::MAX {
        Ok(UnwindTargetRelation::NotInChain)
    } else {
        Err("SEH chain exceeds RtlUnwind classification depth".into())
    }
}

fn begin_child_seh_dispatch(
    child: &mut PendingChild,
    guest: &mut GuestContext,
    exception: ChildException,
    registers: Registers,
) -> Result<(), String> {
    if child.seh.is_some() {
        return Err("nested-SEH frontier".into());
    }
    let mut head = [0; 4];
    if child
        .address_space
        .read(registers.fs_base, &mut head)
        .map_err(|error| error.to_string())?
        != 4
    {
        return Err("short SEH chain head read".into());
    }
    let head = u32::from_le_bytes(head);
    if head == u32::MAX {
        return Err("unhandled-SEH-chain frontier".into());
    }
    let registration = read_seh_registration(&child.address_space, head)?;
    let scan_single_step = war3_scan_single_step(exception, registers);
    let dword_scan_single_step = war3_dword_scan_single_step(exception, registers);
    let boring_single_step = boring_war3_single_step(exception, registers, registration.handler);
    let quiet = !cfg!(feature = "trace-seh")
        || quiet_war3_exception(exception, registers)
        || boring_single_step;
    if !quiet && registration.handler == WAR3_DIVIDE_EXCEPTION_HANDLER && !child.seh_handler_dumped
    {
        child.seh_handler_dumped = true;
        let mut bytes = [0; 128];
        let readable = child
            .address_space
            .read(registration.handler, &mut bytes)
            .ok()
            == Some(bytes.len());
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SEH HANDLER DUMP pid={} tid={} handler=0x{:08x} bytes=\"{}\"",
                child.pid,
                guest.tid,
                registration.handler,
                if readable {
                    diagnostic_hex_bytes(&bytes)
                } else {
                    "<unreadable>".into()
                },
            ),
        );
    }
    let debug_registers = guest
        .context
        .debug_registers()
        .map_err(|error| error.to_string())?;
    if exception.vector == Some(1) && !quiet {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD PRE-SEH DEBUG pid={} tid={} eip=0x{:08x} dr0=0x{:08x} dr1=0x{:08x} dr2=0x{:08x} dr3=0x{:08x} dr6=0x{:08x} dr7=0x{:08x} qualification_dr6={:?}",
                child.pid,
                guest.tid,
                registers.eip,
                debug_registers.dr0,
                debug_registers.dr1,
                debug_registers.dr2,
                debug_registers.dr3,
                debug_registers.dr6,
                debug_registers.dr7,
                exception.debug_status,
            ),
        );
    }
    let context = wc3::seh::encode_x86_context(registers, Some(debug_registers));
    let (record, exception_code, exception_kind) = match exception.vector {
        Some(14) => {
            let linear = exception.fault_linear.ok_or("page fault linear address")?;
            let error = exception.error.ok_or("page fault error")?;
            (
                wc3::seh::encode_page_fault_exception_record(registers.eip, linear, error),
                wc3::seh::STATUS_ACCESS_VIOLATION,
                if error & 0x10 != 0 {
                    "execute"
                } else if error & 2 != 0 {
                    "write"
                } else {
                    "read"
                },
            )
        }
        Some(1) => (
            wc3::seh::encode_single_step_exception_record(registers.eip),
            wc3::seh::STATUS_SINGLE_STEP,
            "single-step",
        ),
        Some(0) => (
            wc3::seh::encode_integer_divide_by_zero_exception_record(registers.eip),
            wc3::seh::STATUS_INTEGER_DIVIDE_BY_ZERO,
            "integer-divide-by-zero",
        ),
        Some(6) => (
            wc3::seh::encode_illegal_instruction_exception_record(registers.eip),
            wc3::seh::STATUS_ILLEGAL_INSTRUCTION,
            "illegal-instruction",
        ),
        _ => return Err("unsupported-exception-mapping frontier".into()),
    };
    let context_va = registers
        .esp
        .checked_sub(wc3::seh::X86_CONTEXT_BYTES as u32)
        .ok_or("SEH context stack underflow")?
        & !15;
    let exception_pointers_va = context_va
        .checked_sub(8)
        .ok_or("SEH exception-pointers stack underflow")?;
    let record_va = exception_pointers_va
        .checked_sub(wc3::seh::EXCEPTION_RECORD_BYTES as u32)
        .ok_or("SEH record stack underflow")?;
    let frame_esp = record_va
        .checked_sub(20)
        .ok_or("SEH call stack underflow")?;
    for (address, bytes) in [
        (context_va, context.as_slice()),
        (record_va, record.as_slice()),
    ] {
        if child
            .address_space
            .write(address, bytes)
            .map_err(|error| error.to_string())?
            != bytes.len()
        {
            return Err("short SEH scratch write".into());
        }
    }
    let frame = [
        thunk32::CHILD_SEH_RETURN_ADDRESS,
        record_va,
        registration.frame,
        context_va,
        0,
    ];
    let mut bytes = [0; 20];
    for (index, value) in frame.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    if child
        .address_space
        .write(frame_esp, &bytes)
        .map_err(|error| error.to_string())?
        != bytes.len()
    {
        return Err("short SEH handler frame write".into());
    }
    child.seh = Some(ChildSehDispatch {
        original_registers: registers,
        registration: registration.frame,
        next_registration: registration.next,
        handler: registration.handler,
        exception_record_va: record_va,
        context_va,
        exception_pointers_va,
        preserved_fs_base: registers.fs_base,
        depth: 1,
        quiet,
        boring_single_step,
        scan_single_step,
        dword_scan_single_step,
    });
    let handler_registers =
        wc3::seh::exception_handler_registers(registers, registration.handler, frame_esp);
    if !quiet {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SEH ENTER FLAGS interrupted=0x{:08x} saved_context=0x{:08x} handler_live=0x{:08x} tf_cleared={}",
                registers.eflags,
                registers.eflags,
                handler_registers.eflags,
                u32::from(
                    registers.eflags & wc3::seh::X86_EFLAGS_TF != 0
                        && handler_registers.eflags & wc3::seh::X86_EFLAGS_TF == 0
                ),
            ),
        );
    }
    guest
        .context
        .set_registers(handler_registers)
        .map_err(|error| error.to_string())?;
    let (owner, rva) = child_pc_owner(child, registration.handler).unwrap_or(("unknown", 0));
    if !quiet {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD SEH DISPATCH pid={} tid={} registration=0x{:08x} next=0x{:08x} handler=0x{:08x} handler_owner={:?} handler_rva=0x{:08x} exception=0x{:08x} address=0x{:08x} kind={}",
                child.pid,
                guest.tid,
                registration.frame,
                registration.next,
                registration.handler,
                owner,
                rva,
                exception_code,
                registers.eip,
                exception_kind
            ),
        );
    }
    Ok(())
}

fn child_seh3_scope_entry(
    child: &PendingChild,
    scope: u32,
    level: i32,
) -> Result<(i32, u32, u32), String> {
    if level < 0 {
        return Err("SEH3 negative scope entry level".into());
    }
    let offset = u32::try_from(level)
        .ok()
        .and_then(|level| level.checked_mul(12))
        .ok_or("SEH3 scope entry overflow")?;
    let address = scope
        .checked_add(offset)
        .ok_or("SEH3 scope address overflow")?;
    let words = read_guest_words(&X86Memory(&child.address_space), address, 3)?;
    Ok((
        i32::from_le_bytes(words[0].to_le_bytes()),
        words[1],
        words[2],
    ))
}

fn schedule_child_seh3_filter(
    child: &mut PendingChild,
    context: &mut GuestContext,
    registers: Registers,
    call: ChildSeh3Call,
    filter: u32,
) -> Result<(), String> {
    let callback_esp = call
        .provider_esp
        .checked_sub(8)
        .ok_or("SEH3 filter callback stack underflow")?;
    let (exception_record_va, context_va, pointers_va) = {
        let seh = child.seh.as_ref().ok_or("SEH3 filter without active SEH")?;
        (
            seh.exception_record_va,
            seh.context_va,
            seh.exception_pointers_va,
        )
    };
    let pointers = [exception_record_va, context_va];
    let mut pointer_bytes = [0; 8];
    for (index, value) in pointers.into_iter().enumerate() {
        pointer_bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    if child
        .address_space
        .write(pointers_va, &pointer_bytes)
        .map_err(|error| error.to_string())?
        != 8
    {
        return Err("short SEH3 exception-pointers write".into());
    }
    // MSVC _except_handler3 publishes EXCEPTION_POINTERS at frame[-1].
    // Compiler-generated filter funclets access this as [EBP-0x14].
    let xpointers_slot = call
        .frame
        .checked_sub(4)
        .ok_or("SEH3 exception-pointers slot underflow")?;
    write_child_u32(child, xpointers_slot, pointers_va)?;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD CRT EH3 FILTER FRAME frame=0x{:08x} anchor=0x{:08x} xpointers_slot=0x{:08x} xpointers=0x{:08x} record=0x{:08x} context=0x{:08x}",
            call.frame,
            call.frame + 0x10,
            xpointers_slot,
            pointers_va,
            exception_record_va,
            context_va,
        ),
    );
    let frame = [thunk32::CHILD_CALLBACK_RETURN_ADDRESS, pointers_va];
    let mut frame_bytes = [0; 8];
    for (index, value) in frame.into_iter().enumerate() {
        frame_bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    if child
        .address_space
        .write(callback_esp, &frame_bytes)
        .map_err(|error| error.to_string())?
        != 8
    {
        return Err("short SEH3 filter callback frame write".into());
    }
    child.seh3_call = Some(call);
    let mut callback = registers;
    callback.eip = filter;
    callback.esp = callback_esp;
    callback.ebp = child
        .seh
        .as_ref()
        .unwrap()
        .registration
        .checked_add(0x10)
        .ok_or("SEH3 EBP overflow")?;
    context
        .context
        .set_registers(callback)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn schedule_child_seh3_finally(
    child: &mut PendingChild,
    context: &mut GuestContext,
    registers: Registers,
    call: ChildSeh3Call,
    handler: u32,
) -> Result<(), String> {
    let callback_esp = call
        .provider_esp
        .checked_sub(4)
        .ok_or("SEH3 finally callback stack underflow")?;
    if child
        .address_space
        .write(
            callback_esp,
            &thunk32::CHILD_CALLBACK_RETURN_ADDRESS.to_le_bytes(),
        )
        .map_err(|error| error.to_string())?
        != 4
    {
        return Err("short SEH3 finally callback frame write".into());
    }
    child.seh3_call = Some(call);
    let mut callback = registers;
    callback.eip = handler;
    callback.esp = callback_esp;
    callback.ebp = child
        .seh
        .as_ref()
        .unwrap()
        .registration
        .checked_add(0x10)
        .ok_or("SEH3 EBP overflow")?;
    context
        .context
        .set_registers(callback)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProcSelector {
    Name(String),
    Ordinal(u16),
}

impl ProcSelector {
    fn provider_symbol(&self) -> child_loader::ProviderSymbol {
        match self {
            Self::Name(name) => child_loader::ProviderSymbol::Name(name.clone()),
            Self::Ordinal(ordinal) => child_loader::ProviderSymbol::Ordinal(*ordinal),
        }
    }
}

pub(super) fn eax_absolute_store(bytes: &[u8]) -> Option<u32> {
    if bytes.len() >= 5 && bytes[0] == 0xa3 {
        return Some(u32::from_le_bytes(bytes[1..5].try_into().unwrap()));
    }
    if bytes.len() >= 6 && bytes[0] == 0x89 && bytes[1] == 0x05 {
        return Some(u32::from_le_bytes(bytes[2..6].try_into().unwrap()));
    }
    None
}

fn log_get_proc_address_continuation(
    child: &PendingChild,
    pid: u32,
    tid: u32,
    module: &str,
    caller_return: u32,
    selector: &ProcSelector,
    result: u32,
) {
    let mut bytes = [0; 16];
    let readable = child.address_space.read(caller_return, &mut bytes).ok() == Some(bytes.len());
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD GETPROCADDRESS CALLER pid={} tid={} module={:?} selector={:?} result=0x{:08x} return=0x{:08x} after=\"{}\"",
            pid,
            tid,
            module,
            selector,
            result,
            caller_return,
            if readable {
                diagnostic_hex_bytes(&bytes)
            } else {
                "<unreadable>".into()
            },
        ),
    );
    if !readable {
        return;
    }
    if let Some(destination) = eax_absolute_store(&bytes) {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD GETPROCADDRESS STORE pid={} tid={} module={:?} selector={:?} result=0x{:08x} return=0x{:08x} destination=0x{:08x}",
                pid, tid, module, selector, result, caller_return, destination,
            ),
        );
    }
}

fn install_child_provider_imports(
    child: &mut PendingChild,
    process: &mut XpProcess,
    imports: Vec<child_loader::ProviderImport>,
) -> Result<Vec<u32>, String> {
    let (addresses, old_bytes, new_bytes, updated_from, updated) = process
        .append_provider_imports(imports)
        .map_err(str::to_owned)?;
    child.provider_thunk_bytes = new_bytes;
    if new_bytes > old_bytes {
        child
            .address_space
            .map(
                thunk32::THUNK_BASE + old_bytes as u32,
                new_bytes - old_bytes,
                Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
            )
            .map_err(|error| error.to_string())?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD PROVIDER THUNK GROW pid={} old_bytes={} new_bytes={}",
                child.pid, old_bytes, new_bytes
            ),
        );
    }
    let existing_update = old_bytes.saturating_sub(updated_from).min(updated.len());
    if existing_update != 0 {
        child
            .address_space
            .write(
                thunk32::THUNK_BASE + updated_from as u32,
                &updated[..existing_update],
            )
            .map_err(|error| error.to_string())?;
    }
    if existing_update < updated.len() {
        child
            .address_space
            .write(
                thunk32::THUNK_BASE + old_bytes as u32,
                &updated[existing_update..],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(addresses)
}

/// Install the one stable guest-visible `IDirect3D8` object.  Its methods are
/// observation thunks: the first COM call is deliberately the next frontier.
fn install_child_d3d8_object(
    child: &mut PendingChild,
    process: &mut XpProcess,
) -> Result<(), String> {
    let imports = D3D8_METHODS.map(|symbol| child_loader::ProviderImport {
        module: "d3d8.dll".into(),
        symbol: child_loader::ProviderSymbol::Name(symbol.into()),
        iat_rva: 0,
    });
    let existing = imports
        .iter()
        .map(|import| process.provider_thunk_address(&import.module, &import.symbol))
        .collect::<Option<Vec<_>>>();
    let addresses = match existing {
        Some(addresses) => addresses,
        None => install_child_provider_imports(child, process, imports.to_vec())?,
    };

    for (slot, address) in addresses.into_iter().enumerate() {
        let slot_address = thunk32::CHILD_D3D8_VTABLE_ADDRESS
            .checked_add(u32::try_from(slot).map_err(|_| "D3D8 vtable slot")? * 4)
            .ok_or("D3D8 vtable address overflow")?;
        if child
            .address_space
            .write(slot_address, &address.to_le_bytes())
            .map_err(|error| format!("write D3D8 vtable: {error}"))?
            != 4
        {
            return Err("short D3D8 vtable write".into());
        }
    }
    if child
        .address_space
        .write(
            thunk32::CHILD_D3D8_OBJECT_ADDRESS,
            &thunk32::CHILD_D3D8_VTABLE_ADDRESS.to_le_bytes(),
        )
        .map_err(|error| format!("write D3D8 object: {error}"))?
        != 4
    {
        return Err("short D3D8 object write".into());
    }
    Ok(())
}

fn runtime_native_module_image<'a>(
    child: &'a PendingChild,
    module: &str,
) -> Option<(&'a pe32::PeImage, &'a str)> {
    if module.eq_ignore_ascii_case("War3.exe") {
        return Some((&child.image, "War3.exe"));
    }
    child.native_modules.iter().find_map(|native| {
        (native.stored.eq_ignore_ascii_case(module)
            || native.requested.eq_ignore_ascii_case(module))
        .then_some((&native.image, native.stored.as_str()))
    })
}

fn runtime_native_export_address(
    child: &PendingChild,
    module: &str,
    symbol: &pe32::ImportSymbol,
) -> Result<Option<(u32, String)>, String> {
    let Some((image, stored)) = runtime_native_module_image(child, module) else {
        return Ok(None);
    };
    let export = match symbol {
        pe32::ImportSymbol::Name(name) => image
            .exports
            .iter()
            .find(|export| export.name.as_deref() == Some(name.as_str())),
        pe32::ImportSymbol::Ordinal(ordinal) => image
            .exports
            .iter()
            .find(|export| export.ordinal == u32::from(*ordinal)),
    }
    .ok_or_else(|| match symbol {
        pe32::ImportSymbol::Name(name) => {
            format!("WC3 CHILD RUNTIME NATIVE EXPORT MISSING module={stored:?} symbol={name:?}")
        }
        pe32::ImportSymbol::Ordinal(ordinal) => {
            format!("WC3 CHILD RUNTIME NATIVE EXPORT MISSING module={stored:?} ordinal={ordinal}")
        }
    })?;
    let pe32::ExportTarget::Rva(rva) = &export.target else {
        let pe32::ExportTarget::Forwarder(forwarder) = &export.target else {
            unreachable!()
        };
        return Err(format!(
            "WC3 CHILD RUNTIME NATIVE EXPORT FORWARDER FRONTIER module={stored:?} forwarder={forwarder:?}"
        ));
    };
    if *rva >= image.size_of_image {
        return Err(format!(
            "WC3 CHILD RUNTIME NATIVE EXPORT RVA OUTSIDE IMAGE module={stored:?} rva=0x{rva:08x} size=0x{:08x}",
            image.size_of_image
        ));
    }
    let address = image
        .image_base
        .checked_add(*rva)
        .ok_or("runtime native export VA overflow")?;
    Ok(Some((address, stored.to_owned())))
}

enum RuntimeImportBind {
    Complete,
    NeedNativeDependency { requested: String, stored: String },
}

fn bind_runtime_local_image_imports(
    child: &mut PendingChild,
    process: &mut XpProcess,
    parent: &str,
    parent_base: u32,
    image: &pe32::PeImage,
    listing: Option<&async_fs::DirListing>,
) -> Result<RuntimeImportBind, String> {
    let mut native_imports = 0usize;
    let mut provider_imports = Vec::new();
    let mut classified = HashSet::new();
    for import in &image.imports {
        if let Some((address, provider)) =
            runtime_native_export_address(child, &import.module, &import.symbol)?
        {
            if classified.insert(import.module.clone()) {
                logl::log(
                    level::IMPORTANT,
                    format_args!(
                        "WC3 CHILD RUNTIME IMPORT CLASSIFY parent={parent:?} module={:?} kind=already-loaded-native target={provider:?}",
                        import.module,
                    ),
                );
            }
            let iat = parent_base
                .checked_add(import.iat_rva)
                .ok_or("runtime native IAT overflow")?;
            child
                .address_space
                .write(iat, &address.to_le_bytes())
                .map_err(|error| error.to_string())?;
            native_imports += 1;
            continue;
        }

        let stored = match listing {
            Some(listing) => {
                child_loader::resolve_file(listing, &import.module).map_err(str::to_owned)?
            }
            None => None,
        };
        if let Some(stored) = stored {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD RUNTIME IMPORT CLASSIFY parent={parent:?} module={:?} kind=local-native-unloaded stored={stored:?}",
                    import.module,
                ),
            );
            return Ok(RuntimeImportBind::NeedNativeDependency {
                requested: import.module.clone(),
                stored,
            });
        }
        if classified.insert(import.module.clone()) {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD RUNTIME IMPORT CLASSIFY parent={parent:?} module={:?} kind=external-provider",
                    import.module,
                ),
            );
        }
        provider_imports.push(child_loader::ProviderImport {
            module: import.module.clone(),
            symbol: match &import.symbol {
                pe32::ImportSymbol::Name(name) => child_loader::ProviderSymbol::Name(name.clone()),
                pe32::ImportSymbol::Ordinal(ordinal) => {
                    child_loader::ProviderSymbol::Ordinal(*ordinal)
                }
            },
            iat_rva: import.iat_rva,
        });
    }
    let provider_addresses = install_child_provider_imports(child, process, provider_imports)?;
    for (import, address) in image
        .imports
        .iter()
        .filter(|import| runtime_native_module_image(child, &import.module).is_none())
        .zip(provider_addresses)
    {
        let iat = parent_base
            .checked_add(import.iat_rva)
            .ok_or("runtime provider IAT overflow")?;
        child
            .address_space
            .write(iat, &address.to_le_bytes())
            .map_err(|error| error.to_string())?;
    }
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD RUNTIME IMPORTS READY parent={parent:?} native_imports={native_imports} provider_imports={} patched_iat={}",
            image.imports.len() - native_imports,
            image.imports.len(),
        ),
    );
    Ok(RuntimeImportBind::Complete)
}

pub(super) async fn run_loop(
    address_space: &AddressSpace,
    mut memory: X86Memory<'_>,
    mut session: &mut Wc3Session,
    mut contexts: &mut Vec<GuestContext>,
    pending_child: &mut Option<PendingChild>,
    thread_calls: &mut HashMap<(u32, u32), u32>,
    default_proc_messages: &mut HashSet<u32>,
    mut frames: &mut HashMap<u32, Frame>,
    window_rgba: &mut HashMap<u32, Vec<u8>>,
    mut wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
    mut previous_wait_timeout: &mut Option<(ThreadKey, u32, u32)>,
    idle_poll_sites: &mut HashMap<IdlePollSite, IdlePollStats>,
    child_get_command_line_logged: &mut bool,
    active_message_box: &mut Option<ActiveMessageBox>,
    mut active: usize,
) -> Result<(), String> {
    'child_run: loop {
        if let Some(child) = pending_child.as_mut() {
            if contexts[active].pid == child.pid {
                child.activate_thread(contexts[active].tid);
            }
        }
        if let Some(modal) = active_message_box.as_mut() {
            if modal.request.caller.pid != LAUNCHER_PID || modal.buttons.is_empty() {
                return Err("invalid active MessageBoxA modal state".into());
            }
            while modal
                .frame
                .take_pointer_event()
                .map_err(|error| format!("poll MessageBoxA pointer event: {error:?}"))?
                .is_some()
            {}
            trueos::vsys::poll_once();
            tokio::task::yield_now().await;
            trueos::vsys::sleep_ms(8);
            continue;
        }
        let exit = if contexts[active].started {
            contexts[active].context.resume().await
        } else {
            contexts[active].started = true;
            contexts[active].context.run().await
        }
        .map_err(|error| error.to_string())?;
        let active_key = contexts
            .get(active)
            .ok_or_else(|| "active guest context missing".to_owned())?
            .key();
        if let Some(child) = pending_child.as_mut() {
            if active_key.tid == child.tid {
                loop_checkpoint::observe_exit(child, active_key, &exit);
            }
        }
        match exit.kind {
            ExitKind::Cpuid if active_key.pid != LAUNCHER_PID => {
                let child = pending_child
                    .as_ref()
                    .filter(|child| child.pid == active_key.pid)
                    .ok_or_else(|| "CPUID child missing".to_owned())?;
                let eip = exit.registers.eip;
                let mut opcode = [0; 2];
                if child.address_space.read(eip, &mut opcode).map_err(|error| error.to_string())? != 2 {
                    return Err("short CPUID opcode read".into());
                }
                if opcode != [0x0f, 0xa2] {
                    return Err(format!("VM-exit reason CPUID but bytes={:02x} {:02x}", opcode[0], opcode[1]));
                }
                let leaf = exit.registers.eax;
                let subleaf = exit.registers.ecx;
                logl::log(level::IMPORTANT, format_args!(
                    "WC3 CHILD CPUID CALL pid={} tid={} eip=0x{:08x} leaf=0x{:08x} subleaf=0x{:08x}",
                    active_key.pid, active_key.tid, eip, leaf, subleaf,
                ));
                let [eax, ebx, ecx, edx] = wc3_cpuid(leaf, subleaf)?;
                let mut registers = exit.registers;
                registers.eax = eax;
                registers.ebx = ebx;
                registers.ecx = ecx;
                registers.edx = edx;
                registers.eip = eip.checked_add(2).ok_or("CPUID EIP overflow")?;
                contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                logl::log(level::IMPORTANT, format_args!(
                    "WC3 CHILD CPUID RESULT pid={} tid={} leaf=0x{:08x} subleaf=0x{:08x} eax=0x{:08x} ebx=0x{:08x} ecx=0x{:08x} edx=0x{:08x} profile=xp-p3 eip=0x{:08x}->0x{:08x}",
                    active_key.pid, active_key.tid, leaf, subleaf, eax, ebx, ecx, edx, eip, registers.eip,
                ));
                continue;
            }
            // A transient VMCS always starts with VMLAUNCH.  Its preemption
            // timer is therefore a Blueprint scheduling boundary, not an x86
            // program stop: Context::resume() restores the logical context
            // into a fresh VMCS on whichever Tokio carrier runs next.
            ExitKind::Other if exit.detail == 52 => {
                if cfg!(feature = "trace-scan") {
                    let (preemptions, same_page) = {
                        let context = contexts
                            .get_mut(active)
                            .ok_or_else(|| "active guest context missing".to_owned())?;
                        context.preemption_count = context
                            .preemption_count
                            .checked_add(1)
                            .ok_or_else(|| "preemption count overflow".to_owned())?;
                        let page = exit.registers.eip & !0xfff;
                        if context.last_preemption_page == page {
                            context.same_page_preemptions =
                                context.same_page_preemptions.saturating_add(1);
                        } else {
                            context.last_preemption_page = page;
                            context.same_page_preemptions = 1;
                        }
                        (context.preemption_count, context.same_page_preemptions)
                    };
                    if active_key.pid != LAUNCHER_PID
                        && should_log_execution_sample(preemptions)
                        && pending_child.as_ref().is_some_and(|child| {
                            child.execution == ChildExecutionState::ImageEntryRunning
                                && !(child.scan_progress.is_some()
                                    && (WAR3_SCAN_STEP_START..=WAR3_SCAN_STEP_END)
                                        .contains(&exit.registers.eip))
                        })
                    {
                        let child = pending_child.as_ref().unwrap();
                    let (owner, rva) =
                        child_pc_owner(child, exit.registers.eip).unwrap_or(("unknown", 0));
                    let mut code = [0u8; 16];
                    let code_len = child
                        .address_space
                        .read(exit.registers.eip, &mut code)
                        .unwrap_or(0);
                    let code = code[..code_len]
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXEC SAMPLE pid={} tid={} during=\"War3.exe:ENTRY\" preemptions={} owner={:?} rva=0x{:08x} eip=0x{:08x} esp=0x{:08x} ebp=0x{:08x} eax=0x{:08x} same_page={} code={}",
                            active_key.pid,
                            active_key.tid,
                            preemptions,
                            owner,
                            rva,
                            exit.registers.eip,
                            exit.registers.esp,
                            exit.registers.ebp,
                            exit.registers.eax,
                            same_page,
                            code,
                        ),
                    );
                    let scan_index = child_read_u32(child, WAR3_SCAN_INDEX);
                    let scan_source = child_read_u32(child, WAR3_SCAN_SOURCE);
                    let scan_bound = child_read_u32(child, WAR3_SCAN_BOUND);
                    let scan_count = child_read_u32(child, WAR3_SCAN_COUNT);
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD HOTLOOP pid={} tid={} preemptions={} eip=0x{:08x} index={} source={} bound={} count={} eflags=0x{:08x}",
                            active_key.pid,
                            active_key.tid,
                            preemptions,
                            exit.registers.eip,
                            scan_index
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_source
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_bound
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            scan_count
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                            exit.registers.eflags,
                        ),
                    );
                        if same_page == 1024 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXEC HOTPAGE pid={} tid={} owner={:?} page=0x{:08x} eip=0x{:08x} samples={}",
                                    active_key.pid,
                                    active_key.tid,
                                    owner,
                                    exit.registers.eip & !0xfff,
                                    exit.registers.eip,
                                    same_page,
                                ),
                            );
                        }
                    }
                }
                expire_runtime_waits(
                    &mut session,
                    &mut contexts,
                    &mut wait_deadlines,
                    &mut previous_wait_timeout,
                )?;
                if !session.blocked.contains_key(&active_key)
                    && context_index(&contexts, active_key).is_some()
                {
                    session.enqueue(active_key);
                }
                if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                    active = next;
                }
                tokio::task::yield_now().await;
                continue;
            }
            ExitKind::VmCall => {
                let active_pid = active_key.pid;
                let active_tid = active_key.tid;
                include!("asupersync_child_vmcall.rs");
                include!("asupersync_launcher_vmcall.rs");
            }
            ExitKind::Exception if active_key.pid != LAUNCHER_PID => {
                let exception = decode_child_exception(exit.detail, exit.qualification);
                let registers = exit.registers;
                let child = pending_child
                    .as_ref()
                    .filter(|child| child.pid == active_key.pid)
                    .ok_or_else(|| "exception child missing pending state".to_owned())?;
                let scope = child_execution_scope(child).map_err(str::to_owned)?;
                if exit.registers.eip == 0x0040_1d0b {
                    let eip = exit.registers.eip;

                    let code_base = eip - 16;
                    let mut code = [0u8; 48];
                    child
                        .address_space
                        .read(code_base, &mut code)
                        .map_err(|e| e.to_string())?;

                    let stack =
                        read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 12)?;

                    let ebp_base = exit.registers.ebp.saturating_sub(0x40);
                    let mut ebp_bytes = [0u8; 0x80];
                    child
                        .address_space
                        .read(ebp_base, &mut ebp_bytes)
                        .map_err(|e| e.to_string())?;

                    let slot = 0x0044_b288u32;
                    let slot_value = child_read_u32(child, slot);

                    let thunk_id = if exit.registers.eax >= thunk32::THUNK_BASE {
                        let delta = exit.registers.eax - thunk32::THUNK_BASE;
                        let width = thunk32::THUNK_BYTES as u32;

                        if delta % width == 0 {
                            Some(delta / width)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let provider_import = thunk_id.and_then(|id| {
                        session
                            .process(active_key.pid)
                            .and_then(|process| process.xp.provider_import(id))
                            .cloned()
                    });

                    let slot_rva = slot
                        .checked_sub(child.image.image_base)
                        .ok_or("401D0B slot below image base")?;

                    let pe_import = child
                        .image
                        .imports
                        .iter()
                        .find(|import| import.iat_rva == slot_rva);

                    let data_export = provider_import
                        .as_ref()
                        .and_then(child_loader::provider_data_export_address);

                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD 401D0B FRONTIER \
                             eax=0x{:08x} ebx=0x{:08x} ecx=0x{:08x} edx=0x{:08x} \
                             esi=0x{:08x} edi=0x{:08x} ebp=0x{:08x} esp=0x{:08x} \
                             code_base=0x{:08x} code=\"{}\" \
                             stack={:08x?} ebp_base=0x{:08x} ebp_bytes=\"{}\"",
                            exit.registers.eax,
                            exit.registers.ebx,
                            exit.registers.ecx,
                            exit.registers.edx,
                            exit.registers.esi,
                            exit.registers.edi,
                            exit.registers.ebp,
                            exit.registers.esp,
                            code_base,
                            hex_bytes(&code),
                            stack,
                            ebp_base,
                            hex_bytes(&ebp_bytes),
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD 401D0B PROVENANCE \
                             slot=0x{:08x} slot_rva=0x{:08x} slot_value={:?} \
                             thunk_id={:?} provider_import={:?} \
                             provider_data_export={:?} pe_import={:?}",
                            slot,
                            slot_rva,
                            slot_value,
                            thunk_id,
                            provider_import,
                            data_export,
                            pe_import,
                        ),
                    );
                }
                let quiet_exception = quiet_war3_exception(exception, registers)
                    || current_child_seh_handler(child, registers.fs_base).is_some_and(|handler| {
                        boring_war3_single_step(exception, registers, handler)
                    });
                if !quiet_exception
                    && exception.vector == Some(1)
                    && matches!(registers.eip, 0x0045_af54 | 0x0045_af5a)
                {
                    let gate = child_read_u32(child, 0x0049_a430);
                    let reset = child_read_u32(child, 0x0049_2614);
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD DB STATUS eip=0x{:08x} dr6={:?} gate_49a430={:?} reset_492614={:?}",
                            registers.eip, exception.debug_status, gate, reset,
                        ),
                    );
                }
                if !quiet_exception {
                    if exception.vector == Some(6) {
                        if let Some(start) = registers.eip.checked_sub(16) {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXCEPTION CODE BEFORE address=0x{:08x} bytes=\"{}\"",
                                    start,
                                    exception_code_window(&child.address_space, start),
                                ),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD EXCEPTION CODE eip=0x{:08x} bytes=\"{}\"",
                                registers.eip,
                                exception_code_window(&child.address_space, registers.eip),
                            ),
                        );
                    }
                    logl::trace!(
                        "trace-seh",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION RAW detail=0x{:08x} qualification=0x{:016x}",
                            exit.detail, exit.qualification,
                        ),
                    );
                    // Preceding bytes expose the call/return or pointer-producing
                    // instruction; these are raw bytes, not instruction boundaries.
                    if cfg!(feature = "trace-seh") {
                        for distance in [32u32, 16] {
                            if let Some(start) = registers.eip.checked_sub(distance) {
                                logl::trace!(
                                    "trace-seh",
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD EXCEPTION CODE BEFORE address=0x{:08x} bytes=\"{}\"",
                                        start,
                                        exception_code_window(&child.address_space, start),
                                    ),
                                );
                            }
                        }
                    }
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} vector={} name=\"{}\" type={} valid={} error_valid={} error={}",
                            active_key.pid,
                            active_key.tid,
                            scope,
                            registers.eip,
                            registers.esp,
                            exception
                                .vector
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".into()),
                            exception.name,
                            exception
                                .interruption_type
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".into()),
                            exception.valid as u8,
                            exception
                                .error_valid
                                .map(|value| (value as u8).to_string())
                                .unwrap_or_else(|| "-".into()),
                            exception
                                .error
                                .map(|value| format!("0x{value:08x}"))
                                .unwrap_or_else(|| "-".into()),
                        ),
                    );
                    logl::trace!(
                        "trace-seh",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION FAULT {}",
                            child_exception_fault_detail(exception),
                        ),
                    );
                    logl::trace!(
                        "trace-seh",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION REGS eax=0x{:08x} ebx=0x{:08x} ecx=0x{:08x} edx=0x{:08x} esi=0x{:08x} edi=0x{:08x} ebp=0x{:08x} esp=0x{:08x} eip=0x{:08x} eflags=0x{:08x} fs_base=0x{:08x}",
                            registers.eax,
                            registers.ebx,
                            registers.ecx,
                            registers.edx,
                            registers.esi,
                            registers.edi,
                            registers.ebp,
                            registers.esp,
                            registers.eip,
                            registers.eflags,
                            registers.fs_base,
                        ),
                    );
                    logl::trace!(
                        "trace-seh",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION CODE eip=0x{:08x} bytes=\"{}\"",
                            registers.eip,
                            exception_code_window(&child.address_space, registers.eip),
                        ),
                    );
                    logl::trace!(
                        "trace-seh",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXCEPTION STACK esp=0x{:08x} words=[redacted]",
                            registers.esp,
                        ),
                    );
                    if let ChildExecutionState::DllInitRunning { native_index } = child.execution {
                        let module = child
                            .native_modules
                            .get(native_index)
                            .ok_or_else(|| "child DLL exception native index".to_owned())?;
                        if module.stored.eq_ignore_ascii_case("Storm.dll")
                            && registers.eip == 0x1503_62ee
                        {
                            log_child_fault_precursor(
                                child,
                                module,
                                session
                                    .process(active_key.pid)
                                    .ok_or_else(|| "faulted child process missing".to_owned())?,
                                active_key.pid,
                                active_key.tid,
                                registers.eax,
                            )?;
                        }
                    }
                }
                let null_execute = exception.vector == Some(14)
                    && registers.eip == 0
                    && exception.fault_linear == Some(0)
                    && exception.error.is_some_and(|error| error & 0x10 != 0);
                let null_loop_signature = null_execute
                    .then(|| {
                        log_null_call_diagnostic(child, active_key.pid, active_key.tid, registers)
                    })
                    .flatten();
                let child = pending_child
                    .as_mut()
                    .filter(|child| child.pid == active_key.pid)
                    .ok_or_else(|| "exception child missing mutable pending state".to_owned())?;
                if null_execute {
                    if let Some(signature) = null_loop_signature {
                        let count = match child.repeated_null_call {
                            Some(mut watch) if watch.signature == signature => {
                                watch.count = watch.count.saturating_add(1);
                                child.repeated_null_call = Some(watch);
                                watch.count
                            }
                            _ => {
                                child.repeated_null_call = Some(NullLoopWatch {
                                    signature,
                                    count: 1,
                                });
                                1
                            }
                        };
                        if count == 3 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WATCH reason=repeated-null-call pid={} tid={} return=0x{:08x} slot=0x{:08x} target=0x00000000 repeats={} action=continue-seh",
                                    signature.pid,
                                    signature.tid,
                                    signature.return_address,
                                    signature.slot,
                                    count,
                                ),
                            );
                        }
                    } else {
                        child.repeated_null_call = None;
                    }
                }
                if exception.vector == Some(0) {
                    let Some(progress) = divide_loop_progress(child) else {
                        child.repeated_divide_fault = None;
                        begin_child_seh_dispatch(
                            child,
                            &mut contexts[active],
                            exception,
                            registers,
                        )?;
                        continue;
                    };
                    let signature = DivideLoopSignature {
                        pid: active_key.pid,
                        tid: active_key.tid,
                        eip: registers.eip,
                        esp: registers.esp,
                        eax: registers.eax,
                        ecx: registers.ecx,
                        edx: registers.edx,
                        progress,
                    };
                    let count = match child.repeated_divide_fault {
                        Some(mut watch) if watch.signature == signature => {
                            watch.count = watch.count.saturating_add(1);
                            child.repeated_divide_fault = Some(watch);
                            watch.count
                        }
                        _ => {
                            child.repeated_divide_fault = Some(DivideLoopWatch {
                                signature,
                                count: 1,
                            });
                            1
                        }
                    };
                    if count >= 3 {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD OPERATOR STOP reason=repeated-divide-error pid={} tid={} eip=0x{:08x} esp=0x{:08x} eax=0x{:08x} ecx=0x{:08x} edx=0x{:08x} table_base=0x{:08x} index=0x{:08x} state_ptr=0x{:08x} input=0x{:02x} accum=0x{:04x} repeats={}",
                                signature.pid,
                                signature.tid,
                                signature.eip,
                                signature.esp,
                                signature.eax,
                                signature.ecx,
                                signature.edx,
                                signature.progress.table_base,
                                signature.progress.index,
                                signature.progress.state_ptr,
                                signature.progress.input,
                                signature.progress.accumulator,
                                count,
                            ),
                        );
                        return Ok(());
                    }
                } else {
                    child.repeated_divide_fault = None;
                }
                begin_child_seh_dispatch(child, &mut contexts[active], exception, registers)?;
                continue;
            }
            ExitKind::Halted => {
                if active_key.pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_ref()
                        .filter(|child| child.pid == active_key.pid)
                        .ok_or_else(|| "halted child missing pending state".to_owned())?;
                    let scope = child_execution_scope(child).map_err(str::to_owned)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXECUTION FAULT pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} kind=Halted detail={}",
                            active_key.pid,
                            active_key.tid,
                            scope,
                            exit.registers.eip,
                            exit.registers.esp,
                            exit.detail
                        ),
                    );
                    return Ok(());
                }
                let halted_tid = contexts.remove(active).tid;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3: x86 context halted tid={halted_tid} after {} calls",
                        session.launcher().xp.call_count
                    ),
                );
                if contexts.is_empty() {
                    return Ok(());
                }
                active %= contexts.len();
            }
            kind => {
                if active_key.pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_ref()
                        .filter(|child| child.pid == active_key.pid)
                        .ok_or_else(|| "faulted child missing pending state".to_owned())?;
                    let scope = child_execution_scope(child).map_err(str::to_owned)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD EXECUTION FAULT pid={} tid={} during={:?} eip=0x{:08x} esp=0x{:08x} kind={:?} detail={}",
                            active_key.pid,
                            active_key.tid,
                            scope,
                            exit.registers.eip,
                            exit.registers.esp,
                            kind,
                            exit.detail
                        ),
                    );
                    return Ok(());
                }
                return Err(format!(
                    "x86 context stopped: kind={kind:?} detail={} qualification=0x{:x} eip=0x{:08x}",
                    exit.detail, exit.qualification, exit.registers.eip,
                ));
            }
        }
    }
}

#[cfg(test)]
mod control_word_tests {
    use super::*;

    #[test]
    fn control87_round_trips_the_observed_x87_environment() {
        let before_fcw = 0x027f;
        let old_control = msvcrt_control_from_x87(before_fcw);
        assert_eq!(old_control, 0x0009_001f);

        let mask = 0x000f_ffff;
        let effective_mask = mask & MSVCRT_X87_CONTROL_MASK;
        assert_eq!(effective_mask, 0x000f_031f);

        let updated_control =
            (old_control & !effective_mask) | (0x0009_001f & effective_mask);
        assert_eq!(updated_control, old_control);
        assert_eq!(x87_control_from_msvcrt(before_fcw, updated_control), before_fcw);
    }

    #[test]
    fn controlfp_does_not_change_the_denormal_mask() {
        let before_fcw = 0x027d;
        let old_control = msvcrt_control_from_x87(before_fcw);
        let requested = old_control | MSVCRT_EM_DENORMAL;
        let effective_mask = MSVCRT_X87_CONTROL_MASK & !MSVCRT_EM_DENORMAL;
        let updated_control = (old_control & !effective_mask) | (requested & effective_mask);

        assert_eq!(updated_control & MSVCRT_EM_DENORMAL, 0);
        assert_eq!(x87_control_from_msvcrt(before_fcw, updated_control) & 0x0002, 0);
    }
}
