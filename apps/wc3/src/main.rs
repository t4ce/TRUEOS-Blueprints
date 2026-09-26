#[cfg(test)]
#[macro_use]
#[path = "test.rs"]
mod test;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use sha2::{Digest, Sha256};
use trueos::{
    async_fs,
    logl::level,
    ui4_scene::{self, Damage, Font, Frame, SceneTextRow, rgba},
    x86::{AddressSpace, Context, DebugRegisters, ExitKind, ExtendedState, Permissions, Registers},
};
mod asupersync;
mod guest_actor;

use guest_actor::GuestThreadContext;

mod logl {
    // Gate argument evaluation as well as output (some traces inspect guest memory).
    macro_rules! trace {
        ($feature:literal, $level:expr, $message:expr $(,)?) => {
            if cfg!(feature = $feature) {
                $crate::logl::log($level, $message);
            }
        };
    }
    pub(super) use trace;

    #[inline]
    pub fn log(level: u8, message: core::fmt::Arguments<'_>) {
        #[cfg(not(feature = "nolog"))]
        trueos::logl::log(level, message);
        #[cfg(feature = "nolog")]
        let _ = (level, message);
    }
}

use wc3::{
    EXPECTED_SHA256, LAUNCHER_PATH, child_loader,
    imports::WinCall,
    pe32,
    process::{
        CHILD_COMMAND_LINE, CHILD_CRT_HEAP_BASE, CHILD_CRT_HEAP_LIMIT, CHILD_VIRTUAL_ALLOC_BASE,
        CHILD_VIRTUAL_ALLOC_LIMIT, CHILD_WIN_HEAP_BASE, CHILD_WIN_HEAP_LIMIT, CRT_ACMDLN_VA,
        CRT_ARG0_VA, CRT_ARG1_VA, CRT_ARG2_VA, CRT_ARG3_VA, CRT_ARGV_VA, CRT_COMMODE_VA,
        CRT_CONSOLE_APP, CRT_ENVP_VA, CRT_FMODE_VA, CRT_GUI_APP, CRT_UNKNOWN_APP,
        ENVIRONMENT_BLOCK_VA, GuestMemory, HEAP_GENERATE_EXCEPTIONS, HEAP_ZERO_MEMORY,
        PROCESS_DATA_VA, PreparedProcess, ProviderDispatchError, STACK_BASE, STACK_BYTES,
        STACK_TOP, ThreadObject, XP_ANSI_CODE_PAGE, XpProcess, OSVERSIONINFOA_SIZE,
        OSVERSIONINFOEXA_SIZE, VER_NT_WORKSTATION, bmp_file_from_dib, dib_layout,
    },
    session::{
        CompletedWait, CriticalSectionWait, GuestCall, LAUNCHER_PID, LAUNCHER_TID, PersonalityAction, SessionObject,
        SessionRequest, ThreadKey, WINDOW_HANDLE_BASE, Wc3Session, WindowPresentation,
    },
    thunk32,
};

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;
const WAIT_FAILED: u32 = u32::MAX;
const INFINITE: u32 = u32::MAX;
const WC3_REGISTRY_IMAGE_PATH: &[u8] = b"/common/Warcraft III/ok.reg";
const CHILD_IMAGE_ENTRY_HEADROOM: u32 = 0x100;
const CHILD_IMAGE_ENTRY_CALLER_BYTES: usize = 0x40;
const MESSAGE_BOX_WIDTH: u32 = 560;
const MESSAGE_BOX_HEIGHT: u32 = 280;
const MESSAGE_BOX_MAX_ANSI_BYTES: usize = 4096;
const TABLE_CHECKPOINT_DIR: &[u8] = b"/common/Warcraft III/.trueos-wc3";
const TABLE_CHECKPOINT_PATH: &[u8] = b"/common/Warcraft III/.trueos-wc3/table-fill-v1.bin";
const TABLE_CHECKPOINT_FROM_EIP: u32 = 0x0046_14a5;
const TABLE_CHECKPOINT_TO_EIP: u32 = 0x0046_14c5;
const TABLE_CHECKPOINT_TABLE_BYTES: u32 = 0x8000;

fn main() {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("wc3: Tokio runtime failed: {error}"),
            );
            return;
        }
    };
    if let Err(error) = runtime.block_on(run()) {
        let execution = GuestThreadContext::last_execution_diagnostic();
        logl::log(
            level::ERROR,
            format_args!(
                "wc3: {error} last_exec_seq={} last_stage={:?} pid={} tid={}",
                execution.sequence, execution.stage, execution.pid, execution.tid,
            ),
        );
    }
}

fn registry_encoding(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xfe]) {
        "utf16le"
    } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) || bytes.is_ascii() {
        "utf8"
    } else {
        "unknown"
    }
}

async fn ensure_registry_loaded(session: &mut Wc3Session) -> Result<(), String> {
    if matches!(session.registry, wc3::session::RegistryState::Ready(_)) {
        return Ok(());
    }
    let path = String::from_utf8_lossy(WC3_REGISTRY_IMAGE_PATH);
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 REGISTRY LOAD BEGIN path=\"{}\" trigger=\"ADVAPI32!RegOpenKeyExA\"",
            path
        ),
    );
    let metadata = async_fs::metadata(WC3_REGISTRY_IMAGE_PATH)
        .await
        .map_err(|error| {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 REGISTRY LOAD FAILED phase=metadata path=\"{}\" error={error}",
                    path
                ),
            );
            format!("registry metadata: TRUEOSFS error {error}")
        })?;
    logl::log(
        level::IMPORTANT,
        format_args!("WC3 REGISTRY LOAD META bytes={}", metadata.len),
    );
    logl::log(
        level::IMPORTANT,
        format_args!("WC3 REGISTRY LOAD READ BEGIN"),
    );
    let bytes = async_fs::read_file(WC3_REGISTRY_IMAGE_PATH)
        .await
        .map_err(|error| {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 REGISTRY LOAD FAILED phase=read path=\"{}\" error={error}",
                    path
                ),
            );
            format!("registry read: TRUEOSFS error {error}")
        })?;
    logl::log(
        level::IMPORTANT,
        format_args!("WC3 REGISTRY LOAD READ COMPLETE bytes={}", bytes.len()),
    );
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 REGISTRY INDEX BEGIN bytes={} encoding={} mode=keys-only",
            bytes.len(),
            registry_encoding(&bytes)
        ),
    );
    let registry_bytes = bytes.len();
    let registry_encoding = registry_encoding(&bytes);
    let image = wc3::session::RegistryImage::index(bytes, |scanned, keys| {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 REGISTRY INDEX PROGRESS bytes_scanned={} keys={}",
                scanned, keys
            ),
        );
    })
    .map_err(|error| {
        logl::log(
            level::IMPORTANT,
            format_args!("WC3 REGISTRY LOAD FAILED phase=parse error=\"{}\"", error),
        );
        error.to_owned()
    })?;
    let (roots, keys) = image.stats();
    session.registry = wc3::session::RegistryState::Ready(image);
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 REGISTRY INDEX READY path=\"{}\" bytes={} encoding={} roots={} keys={} values=lazy representation=flat-spans backing=host-ram guest_mapped=0",
            path, registry_bytes, registry_encoding, roots, keys
        ),
    );
    Ok(())
}

async fn discover_maps() -> Result<Option<wc3::session::MapCatalog>, String> {
    // The shared selector resolves /common to apps/common on the granted root.
    const FOLDER: &str = "/common/Warcraft III/Maps";
    match async_fs::metadata(FOLDER.as_bytes()).await {
        Err(async_fs::ERR_NOT_FOUND) => {
            logl::log(level::WARN, format_args!("WC3 MAP CATALOG folder={FOLDER:?} status=missing"));
            return Ok(None);
        }
        Ok(info) if info.kind == async_fs::NodeKind::Directory => {}
        Ok(_) => return Err(format!("map catalog folder is not a directory: {FOLDER}")),
        Err(error) => return Err(format!("map catalog folder metadata: TRUEOSFS {error}")),
    }
    let selection = async_fs::select_files(
        FOLDER.as_bytes(), async_fs::ContentTypeId::WARCRAFT3_MAP, 8,
    ).await.map_err(|error| format!("select Warcraft III maps: TRUEOSFS {error}"))?;
    logl::log(level::IMPORTANT, format_args!(
        "WC3 MAP CATALOG folder={FOLDER:?} type=WARCRAFT3_MAP files={} max_depth=8 depth_limited={} truncated={}",
        selection.files.len(), selection.depth_limited, selection.truncated,
    ));
    for path in &selection.files {
        logl::trace!("trace-init", level::IMPORTANT, format_args!("WC3 MAP FILE path={path:?}"));
    }
    Ok(Some(wc3::session::MapCatalog {
        folder: FOLDER.into(), paths: selection.files,
        depth_limited: selection.depth_limited, truncated: selection.truncated,
    }))
}

async fn run() -> Result<(), String> {
    let maps = discover_maps().await?;
    run_x86_extended_state_self_test().await?;
    let bytes = async_fs::read_file(LAUNCHER_PATH.as_bytes())
        .await
        .map_err(|error| format!("read {LAUNCHER_PATH}: TRUEOSFS error {error}"))?;
    if Sha256::digest(&bytes).as_slice() != EXPECTED_SHA256 {
        return Err("launcher SHA-256 does not match Warcraft III RoC 1.00".into());
    }
    let materialized = pe32::materialize(&bytes).map_err(str::to_owned)?;
    let imports = materialized.imports.len();
    let PreparedProcess { mappings, xp } =
        PreparedProcess::new(materialized).map_err(str::to_owned)?;
    let mut session = Wc3Session::new(xp);
    session.maps = maps;
    logl::log(level::IMPORTANT, format_args!("WC3 DIAG BUILD CWEX_V2"));
    let (desktop_width, desktop_height) = ui4_scene::output_dimensions()
        .map_err(|error| format!("query UI4 output dimensions: {error:?}"))?;
    session
        .launcher_mut()
        .xp
        .set_desktop_size(desktop_width, desktop_height);
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 desktop dimensions used by GetClientRect width={} height={}",
            desktop_width, desktop_height
        ),
    );
    let address_space = AddressSpace::create().map_err(|error| error.to_string())?;
    for mapping in &mappings {
        let mut permissions = Permissions::READ | Permissions::WRITE;
        if mapping.executable {
            permissions |= Permissions::EXECUTE;
        }
        address_space
            .map(mapping.address, mapping.bytes.len(), permissions)
            .map_err(|error| format!("map 0x{:08x}: {error}", mapping.address))?;
        let written = address_space
            .write(mapping.address, &mapping.bytes)
            .map_err(|error| format!("write 0x{:08x}: {error}", mapping.address))?;
        if written != mapping.bytes.len() {
            return Err("short x86 mapping write".into());
        }
    }

    let esp = STACK_TOP;
    let registers = Registers {
        esp,
        eip: pe32::IMAGE_BASE + pe32::ENTRY_RVA,
        eflags: 0x202,
        fs_base: wc3::process::TEB_VA,
        ..Registers::default()
    };
    let context = Context::create(&address_space, registers).map_err(|error| error.to_string())?;
    let mut memory = X86Memory(&address_space);
    logl::log(
        level::INFO,
        format_args!(
            "wc3: launcher accepted imports={imports} entry=0x{:08x}; execution owned by Blueprint",
            registers.eip,
        ),
    );

    let mut contexts = vec![GuestContext {
        pid: LAUNCHER_PID,
        tid: LAUNCHER_TID,
        context: GuestThreadContext::spawn(context, LAUNCHER_PID, LAUNCHER_TID)?,
        started: false,
        continuation: None,
        preemption_count: 0,
        last_preemption_page: 0,
        same_page_preemptions: 0,
    }];
    let mut pending_child: Option<PendingChild> = None;
    let mut thread_calls: HashMap<(u32, u32), u32> = HashMap::new();
    let mut default_proc_messages = HashSet::new();
    let mut frames: HashMap<u32, Frame> = HashMap::new();
    let mut window_rgba: HashMap<u32, Vec<u8>> = HashMap::new();
    let mut wait_deadlines: HashMap<ThreadKey, RuntimeWait> = HashMap::new();
    let mut previous_wait_timeout: Option<(ThreadKey, u32, u32)> = None;
    let mut idle_poll_sites: HashMap<IdlePollSite, IdlePollStats> = HashMap::new();
    let mut child_get_command_line_logged = false;
    let mut active_message_box = None;
    let mut active = 0usize;
    let outcome = asupersync::run_loop(
        &address_space,
        memory,
        &mut session,
        &mut contexts,
        &mut pending_child,
        &mut thread_calls,
        &mut default_proc_messages,
        &mut frames,
        &mut window_rgba,
        &mut wait_deadlines,
        &mut previous_wait_timeout,
        &mut idle_poll_sites,
        &mut child_get_command_line_logged,
        &mut active_message_box,
        active,
    )
    .await;
    let execution = GuestThreadContext::last_execution_diagnostic();
    let last_registers = contexts
        .iter()
        .find(|context| context.pid == execution.pid && context.tid == execution.tid)
        .and_then(|context| context.context.registers().ok());
    let live_processes = session
        .processes
        .values()
        .filter(|process| process.exit_code.is_none())
        .count();
    let frames: Vec<_> = frames
        .iter()
        .map(|(hwnd, frame)| format!("0x{hwnd:08x}->{}", frame.window_id()))
        .collect();
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 SESSION RETURN outcome={:?} last_exec_seq={} last_stage={:?} last_pid={} last_tid={} last_eip={} last_esp={} live_processes={} contexts={} frames={:?}",
            outcome,
            execution.sequence,
            execution.stage,
            execution.pid,
            execution.tid,
            last_registers
                .map(|registers| format!("0x{:08x}", registers.eip))
                .unwrap_or_else(|| "<unavailable>".into()),
            last_registers
                .map(|registers| format!("0x{:08x}", registers.esp))
                .unwrap_or_else(|| "<unavailable>".into()),
            live_processes,
            contexts.len(),
            frames,
        ),
    );
    outcome
}

const XSTATE_TEST_CODE_BASE: u32 = 0x0010_0000;
const XSTATE_TEST_DATA_BASE: u32 = 0x0011_0000;

fn emit_fld_m64(code: &mut Vec<u8>, address: u32) {
    code.extend_from_slice(&[0xdd, 0x05]);
    code.extend_from_slice(&address.to_le_bytes());
}

fn emit_fst_m64(code: &mut Vec<u8>, address: u32, pop: bool) {
    code.extend_from_slice(&[0xdd, if pop { 0x1d } else { 0x15 }]);
    code.extend_from_slice(&address.to_le_bytes());
}

fn emit_movdqu_xmm0_from(code: &mut Vec<u8>, address: u32) {
    code.extend_from_slice(&[0xf3, 0x0f, 0x6f, 0x05]);
    code.extend_from_slice(&address.to_le_bytes());
}

fn emit_movdqu_xmm0_to(code: &mut Vec<u8>, address: u32) {
    code.extend_from_slice(&[0xf3, 0x0f, 0x7f, 0x05]);
    code.extend_from_slice(&address.to_le_bytes());
}

fn emit_vmcall(code: &mut Vec<u8>) {
    code.extend_from_slice(&[0x0f, 0x01, 0xc1]);
}

fn require_vmcall(exit: &trueos::x86::Exit, phase: &str) -> Result<(), String> {
    if exit.kind == ExitKind::VmCall {
        Ok(())
    } else {
        Err(format!(
            "x86 xstate self-test {phase}: expected VMCALL, got {:?} detail=0x{:08x} qualification=0x{:016x}",
            exit.kind, exit.detail, exit.qualification
        ))
    }
}

fn read_exact_x86(
    address_space: &AddressSpace,
    address: u32,
    output: &mut [u8],
) -> Result<(), String> {
    let read = address_space
        .read(address, output)
        .map_err(|error| error.to_string())?;
    if read == output.len() {
        Ok(())
    } else {
        Err(format!(
            "x86 xstate self-test short read: {read}/{}",
            output.len()
        ))
    }
}

async fn run_x86_extended_state_self_test() -> Result<(), String> {
    const A_CODE: u32 = XSTATE_TEST_CODE_BASE;
    const B_CODE: u32 = XSTATE_TEST_CODE_BASE + 0x100;
    const DEEP_CODE: u32 = XSTATE_TEST_CODE_BASE + 0x200;
    const BREAKPOINT_CODE: u32 = XSTATE_TEST_CODE_BASE + 0x300;
    const CEIL_ENTRY: u32 = XSTATE_TEST_CODE_BASE + 0x400;
    const CEIL_AFTER_RETURN: u32 = XSTATE_TEST_CODE_BASE + 0x420;
    const A_PATTERN: u32 = XSTATE_TEST_DATA_BASE;
    const B_PATTERN: u32 = XSTATE_TEST_DATA_BASE + 0x10;
    const A_X87_OUT: u32 = XSTATE_TEST_DATA_BASE + 0x20;
    const B_X87_OUT: u32 = XSTATE_TEST_DATA_BASE + 0x28;
    const A_XMM_OUT: u32 = XSTATE_TEST_DATA_BASE + 0x30;
    const B_XMM_OUT: u32 = XSTATE_TEST_DATA_BASE + 0x40;
    const SENTINEL1: u32 = XSTATE_TEST_DATA_BASE + 0x50;
    const SENTINEL0: u32 = XSTATE_TEST_DATA_BASE + 0x58;
    const BASE: u32 = XSTATE_TEST_DATA_BASE + 0x60;
    const EXPONENT: u32 = XSTATE_TEST_DATA_BASE + 0x68;
    const SPILLED_EXPONENT: u32 = XSTATE_TEST_DATA_BASE + 0x70;
    const SPILLED_BASE: u32 = XSTATE_TEST_DATA_BASE + 0x78;
    const RESULT: u32 = XSTATE_TEST_DATA_BASE + 0x80;
    const FINAL_RESULT: u32 = XSTATE_TEST_DATA_BASE + 0x88;
    const FINAL_SENTINEL0: u32 = XSTATE_TEST_DATA_BASE + 0x90;
    const FINAL_SENTINEL1: u32 = XSTATE_TEST_DATA_BASE + 0x98;
    const CEIL_INPUT: u32 = XSTATE_TEST_DATA_BASE + 0xa0;
    const CEIL_FCW: u32 = XSTATE_TEST_DATA_BASE + 0xa8;
    const CEIL_FCW_OUT: u32 = XSTATE_TEST_DATA_BASE + 0xaa;
    const CEIL_RESULT: u32 = XSTATE_TEST_DATA_BASE + 0xb0;
    const CEIL_ESP_OUT: u32 = XSTATE_TEST_DATA_BASE + 0xb8;
    const CEIL_STACK: u32 = XSTATE_TEST_DATA_BASE + 0xc00;

    let address_space = AddressSpace::create().map_err(|error| error.to_string())?;
    address_space
        .map(
            XSTATE_TEST_CODE_BASE,
            0x1000,
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| error.to_string())?;
    address_space
        .map(
            XSTATE_TEST_DATA_BASE,
            0x1000,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| error.to_string())?;
    address_space
        .map(
            thunk32::CHILD_CONTROL_BASE,
            0x1000,
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| error.to_string())?;
    let mut child_controls = [0x90; 0x1000];
    thunk32::install_child_controls(&mut child_controls).map_err(str::to_owned)?;
    address_space
        .write(thunk32::CHILD_CONTROL_BASE, &child_controls)
        .map_err(|error| error.to_string())?;

    let a_pattern = [
        0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe,
        0x0f,
    ];
    let b_pattern = [
        0xf0, 0xde, 0xbc, 0x9a, 0x78, 0x56, 0x34, 0x12, 0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69,
        0x78,
    ];
    address_space
        .write(A_PATTERN, &a_pattern)
        .map_err(|error| error.to_string())?;
    address_space
        .write(B_PATTERN, &b_pattern)
        .map_err(|error| error.to_string())?;

    let context_program = |ones: usize, pattern: u32, x87_out: u32, xmm_out: u32| {
        let mut code = Vec::new();
        for _ in 0..ones {
            code.extend_from_slice(&[0xd9, 0xe8]); // fld1
        }
        for _ in 1..ones {
            code.extend_from_slice(&[0xde, 0xc1]); // faddp st(1), st(0)
        }
        emit_movdqu_xmm0_from(&mut code, pattern);
        emit_vmcall(&mut code);
        emit_fst_m64(&mut code, x87_out, false);
        emit_movdqu_xmm0_to(&mut code, xmm_out);
        emit_vmcall(&mut code);
        code.extend_from_slice(&[0x0f, 0x0b]);
        code
    };
    let a_code = context_program(2, A_PATTERN, A_X87_OUT, A_XMM_OUT);
    let b_code = context_program(3, B_PATTERN, B_X87_OUT, B_XMM_OUT);
    address_space
        .write(A_CODE, &a_code)
        .map_err(|error| error.to_string())?;
    address_space
        .write(B_CODE, &b_code)
        .map_err(|error| error.to_string())?;
    address_space
        .write(BREAKPOINT_CODE, &[0x0f, 0x01, 0xc1]) // vmcall, reached only after #DB
        .map_err(|error| error.to_string())?;

    let registers = |eip| Registers {
        eip,
        eflags: 0x202,
        ..Registers::default()
    };
    let mut a =
        Context::create(&address_space, registers(A_CODE)).map_err(|error| error.to_string())?;
    let mut b =
        Context::create(&address_space, registers(B_CODE)).map_err(|error| error.to_string())?;
    // Arm distinct execution breakpoints at data addresses that neither
    // program executes. This makes both DR0 and DR7 carrier leakage observable
    // without perturbing either xstate program.
    let a_debug = DebugRegisters {
        dr0: A_PATTERN,
        dr1: 0x1111_1111,
        dr2: 0x2222_2222,
        dr3: 0x3333_3333,
        dr6: 0,
        dr7: 0x401, // L0 enable.
    };
    let b_debug = DebugRegisters {
        dr0: B_PATTERN,
        dr1: 0xaaaa_aaaa,
        dr2: 0xbbbb_bbbb,
        dr3: 0xcccc_cccc,
        dr6: 0,
        dr7: 0x402, // G0 enable.
    };
    a.set_debug_registers(a_debug)
        .map_err(|error| error.to_string())?;
    b.set_debug_registers(b_debug)
        .map_err(|error| error.to_string())?;
    let (a_initial, b_initial) = tokio::join!(a.run(), b.run());
    require_vmcall(
        &a_initial.map_err(|error| error.to_string())?,
        "A initialize",
    )?;
    require_vmcall(
        &b_initial.map_err(|error| error.to_string())?,
        "B initialize",
    )?;
    tokio::task::yield_now().await;
    // Reverse submission order while both contexts are runnable. Concurrent
    // jobs hold distinct lane leases and the allocator advances round-robin,
    // so the second pair exercises carrier migration instead of repeatedly
    // resuming both contexts on one favored lane.
    let (b_inspect, a_inspect) = tokio::join!(b.resume(), a.resume());
    require_vmcall(&a_inspect.map_err(|error| error.to_string())?, "A inspect")?;
    require_vmcall(&b_inspect.map_err(|error| error.to_string())?, "B inspect")?;
    // DR6 reserved bits are normalized by hardware and therefore cannot be
    // compared byte-for-byte with the caller's zero. Compare its architectural
    // condition bits while requiring the programmable addresses and DR7 to
    // round-trip exactly.
    let debug_sidecar_matches = |actual: DebugRegisters, expected: DebugRegisters| {
        actual.dr0 == expected.dr0
            && actual.dr1 == expected.dr1
            && actual.dr2 == expected.dr2
            && actual.dr3 == expected.dr3
            && actual.dr6 & 0x0000_e00f == expected.dr6 & 0x0000_e00f
            && actual.dr7 == expected.dr7
    };
    let a_observed = a.debug_registers().map_err(|error| error.to_string())?;
    if !debug_sidecar_matches(a_observed, a_debug) {
        return Err(format!(
            "x86 debug self-test A sidecar mismatch expected={a_debug:?} observed={a_observed:?}",
        ));
    }
    let b_observed = b.debug_registers().map_err(|error| error.to_string())?;
    if !debug_sidecar_matches(b_observed, b_debug) {
        return Err(format!(
            "x86 debug self-test B sidecar mismatch expected={b_debug:?} observed={b_observed:?}",
        ));
    }

    // A real DR0 execute breakpoint must exit as #DB with B0 set before the
    // instruction executes. This is deliberately a generic x86 test rather
    // than a Windows/SEH behavior check.
    let mut breakpoint = Context::create(&address_space, registers(BREAKPOINT_CODE))
        .map_err(|error| error.to_string())?;
    breakpoint
        .set_debug_registers(DebugRegisters {
            dr0: BREAKPOINT_CODE,
            dr7: 0x401, // L0 enable for an instruction-execution breakpoint.
            ..DebugRegisters::default()
        })
        .map_err(|error| error.to_string())?;
    let breakpoint_exit = breakpoint.run().await.map_err(|error| error.to_string())?;
    if breakpoint_exit.kind != ExitKind::Exception
        || breakpoint_exit.detail & 0xff != 1
        || breakpoint_exit.qualification & 1 == 0
    {
        return Err(format!(
            "x86 debug self-test expected #DB/B0, got {:?} detail=0x{:08x} qualification=0x{:016x}",
            breakpoint_exit.kind, breakpoint_exit.detail, breakpoint_exit.qualification,
        ));
    }
    let breakpoint_debug = breakpoint
        .debug_registers()
        .map_err(|error| error.to_string())?;
    if breakpoint_debug.dr0 != BREAKPOINT_CODE
        || breakpoint_debug.dr7 & 0x3 != 1
        || breakpoint_debug.dr6 & 1 == 0
    {
        return Err(format!(
            "x86 debug self-test #DB state mismatch dr0=0x{:08x} dr6=0x{:08x} dr7=0x{:08x}",
            breakpoint_debug.dr0, breakpoint_debug.dr6, breakpoint_debug.dr7,
        ));
    }

    let mut scalar = [0; 8];
    read_exact_x86(&address_space, A_X87_OUT, &mut scalar)?;
    if u64::from_le_bytes(scalar) != 2.0f64.to_bits() {
        return Err("x86 xstate self-test A lost ST0=2.0".into());
    }
    read_exact_x86(&address_space, B_X87_OUT, &mut scalar)?;
    if u64::from_le_bytes(scalar) != 3.0f64.to_bits() {
        return Err("x86 xstate self-test B lost ST0=3.0".into());
    }
    let mut xmm = [0; 16];
    read_exact_x86(&address_space, A_XMM_OUT, &mut xmm)?;
    if xmm != a_pattern {
        return Err("x86 xstate self-test A lost XMM0".into());
    }
    read_exact_x86(&address_space, B_XMM_OUT, &mut xmm)?;
    if xmm != b_pattern {
        return Err("x86 xstate self-test B lost XMM0".into());
    }

    let sentinel0 = 17.0f64;
    let sentinel1 = -29.0f64;
    let base = 2.0f64;
    let exponent = 5.0f64;
    for (address, value) in [
        (SENTINEL1, sentinel1),
        (SENTINEL0, sentinel0),
        (BASE, base),
        (EXPONENT, exponent),
    ] {
        address_space
            .write(address, &value.to_bits().to_le_bytes())
            .map_err(|error| error.to_string())?;
    }
    let mut deep_code = Vec::new();
    for address in [SENTINEL1, SENTINEL0, BASE, EXPONENT] {
        emit_fld_m64(&mut deep_code, address);
    }
    emit_vmcall(&mut deep_code);
    emit_fst_m64(&mut deep_code, SPILLED_EXPONENT, true);
    emit_fst_m64(&mut deep_code, SPILLED_BASE, true);
    emit_vmcall(&mut deep_code);
    emit_fld_m64(&mut deep_code, RESULT);
    emit_vmcall(&mut deep_code);
    emit_fst_m64(&mut deep_code, FINAL_RESULT, true);
    emit_fst_m64(&mut deep_code, FINAL_SENTINEL0, true);
    emit_fst_m64(&mut deep_code, FINAL_SENTINEL1, true);
    emit_vmcall(&mut deep_code);
    deep_code.extend_from_slice(&[0x0f, 0x0b]);
    address_space
        .write(DEEP_CODE, &deep_code)
        .map_err(|error| error.to_string())?;
    let mut deep =
        Context::create(&address_space, registers(DEEP_CODE)).map_err(|error| error.to_string())?;
    require_vmcall(
        &deep.run().await.map_err(|error| error.to_string())?,
        "deep initialize",
    )?;
    tokio::task::yield_now().await;
    require_vmcall(
        &deep.resume().await.map_err(|error| error.to_string())?,
        "deep spill",
    )?;
    let mut spilled = [0; 8];
    read_exact_x86(&address_space, SPILLED_EXPONENT, &mut spilled)?;
    if u64::from_le_bytes(spilled) != exponent.to_bits() {
        return Err("x86 xstate self-test spilled exponent mismatch".into());
    }
    read_exact_x86(&address_space, SPILLED_BASE, &mut spilled)?;
    if u64::from_le_bytes(spilled) != base.to_bits() {
        return Err("x86 xstate self-test spilled base mismatch".into());
    }
    let result = base.powf(exponent);
    address_space
        .write(RESULT, &result.to_bits().to_le_bytes())
        .map_err(|error| error.to_string())?;
    require_vmcall(
        &deep.resume().await.map_err(|error| error.to_string())?,
        "deep restore",
    )?;
    tokio::task::yield_now().await;
    require_vmcall(
        &deep.resume().await.map_err(|error| error.to_string())?,
        "deep inspect",
    )?;
    for (address, expected, label) in [
        (FINAL_RESULT, result, "result"),
        (FINAL_SENTINEL0, sentinel0, "sentinel0"),
        (FINAL_SENTINEL1, sentinel1, "sentinel1"),
    ] {
        read_exact_x86(&address_space, address, &mut spilled)?;
        if u64::from_le_bytes(spilled) != expected.to_bits() {
            return Err(format!(
                "x86 xstate self-test deeper-stack {label} mismatch"
            ));
        }
    }

    // Exercise the exact guest-native MSVCRT ceil helper.  Its input is a
    // cdecl double at ESP+4; the return continuation records both ST(0) and
    // the x87 control word, allowing this to cover value, cdecl stack, and
    // FCW restoration without entering a Rust provider path.
    let mut ceil_entry = vec![0xd9, 0x2d]; // fldcw word ptr [CEIL_FCW]
    ceil_entry.extend_from_slice(&CEIL_FCW.to_le_bytes());
    ceil_entry.push(0xe9);
    ceil_entry.extend_from_slice(
        &thunk32::CHILD_CEIL_ADDRESS
            .wrapping_sub(CEIL_ENTRY + 11)
            .to_le_bytes(),
    );
    address_space
        .write(CEIL_ENTRY, &ceil_entry)
        .map_err(|error| error.to_string())?;
    let mut ceil_after = vec![0x89, 0x25]; // mov [CEIL_ESP_OUT], esp
    ceil_after.extend_from_slice(&CEIL_ESP_OUT.to_le_bytes());
    ceil_after.extend_from_slice(&[0xd9, 0x3d]); // fnstcw [CEIL_FCW_OUT]
    ceil_after.extend_from_slice(&CEIL_FCW_OUT.to_le_bytes());
    ceil_after.extend_from_slice(&[0xdd, 0x1d]); // fstp qword [CEIL_RESULT]
    ceil_after.extend_from_slice(&CEIL_RESULT.to_le_bytes());
    emit_vmcall(&mut ceil_after);
    address_space
        .write(CEIL_AFTER_RETURN, &ceil_after)
        .map_err(|error| error.to_string())?;
    let ceil_fcw = 0x077f_u16; // round-down proves ceil changes RC temporarily.
    for (input, expected) in [(1.25_f64, 2.0_f64), (-1.25, -1.0), (4.0, 4.0)] {
        address_space
            .write(CEIL_INPUT, &input.to_bits().to_le_bytes())
            .map_err(|error| error.to_string())?;
        address_space
            .write(CEIL_FCW, &ceil_fcw.to_le_bytes())
            .map_err(|error| error.to_string())?;
        address_space
            .write(CEIL_STACK, &CEIL_AFTER_RETURN.to_le_bytes())
            .map_err(|error| error.to_string())?;
        let mut frame = [0; 8];
        frame.copy_from_slice(&input.to_bits().to_le_bytes());
        address_space
            .write(CEIL_STACK + 4, &frame)
            .map_err(|error| error.to_string())?;
        let mut ceil_registers = registers(CEIL_ENTRY);
        ceil_registers.esp = CEIL_STACK;
        let mut ceil = Context::create(&address_space, ceil_registers)
            .map_err(|error| error.to_string())?;
        require_vmcall(
            &ceil.run().await.map_err(|error| error.to_string())?,
            "native ceil",
        )?;
        let mut bits = [0; 8];
        read_exact_x86(&address_space, CEIL_RESULT, &mut bits)?;
        if u64::from_le_bytes(bits) != expected.to_bits() {
            return Err(format!(
                "x86 native ceil self-test input={input} expected={expected}"
            ));
        }
        let mut control = [0; 2];
        read_exact_x86(&address_space, CEIL_FCW_OUT, &mut control)?;
        if u16::from_le_bytes(control) != ceil_fcw {
            return Err("x86 native ceil self-test changed x87 control word".into());
        }
        let mut esp = [0; 4];
        read_exact_x86(&address_space, CEIL_ESP_OUT, &mut esp)?;
        if u32::from_le_bytes(esp) != CEIL_STACK + 4 {
            return Err("x86 native ceil self-test violated cdecl stack shape".into());
        }
    }
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 X86 XSTATE SELFTEST PASS contexts=2 x87=pass xmm0=pass migration=debug-sidecars-pass b0=pass deeper_stack=pass ceil=pass"
        ),
    );
    Ok(())
}

fn paint_window_fill_rect(
    request: &wc3::session::WindowFillRectRequest,
    frames: &mut HashMap<u32, Frame>,
    window_rgba: &mut HashMap<u32, Vec<u8>>,
) -> Result<(), String> {
    let frame = frames.get_mut(&request.hwnd)
        .ok_or_else(|| "FillRect destination frame missing".to_owned())?;
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let expected = width.checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "FillRect frame size overflow".to_owned())?;
    let backing = window_rgba.entry(request.hwnd).or_insert_with(|| {
        let mut pixels = vec![0; expected];
        for alpha in pixels[3..].iter_mut().step_by(4) { *alpha = 255; }
        pixels
    });
    if backing.len() != expected {
        return Err("FillRect window backing size mismatch".into());
    }
    let left = request.rect[0].max(0).min(width as i32) as usize;
    let top = request.rect[1].max(0).min(height as i32) as usize;
    let right = request.rect[2].max(0).min(width as i32) as usize;
    let bottom = request.rect[3].max(0).min(height as i32) as usize;
    if left < right && top < bottom {
        for y in top..bottom {
            for x in left..right {
                let offset = (y * width + x) * 4;
                backing[offset..offset + 4].copy_from_slice(&request.rgba);
            }
        }
        frame.begin(rgba(0, 0, 0, 255))
            .and_then(|()| frame.write_opaque_rgba8(backing))
            .and_then(|()| frame.publish(Damage::full(frame.width(), frame.height())))
            .map_err(|error| format!("publish WC3 FillRect: {error:?}"))?;
    }
    logl::log(level::IMPORTANT, format_args!(
        "WC3 UI4 FILLRECT hwnd=0x{:08x} hdc=0x{:08x} rect={:?} published={}",
        request.hwnd, request.hdc, request.rect, u8::from(left < right && top < bottom),
    ));
    Ok(())
}

fn present_window(
    request: WindowPresentation,
    frames: &mut HashMap<u32, Frame>,
    window_rgba: &mut HashMap<u32, Vec<u8>>,
    session: &Wc3Session,
) -> Result<(), String> {
    match request {
        WindowPresentation::Show {
            hwnd,
            x,
            y,
            width,
            height,
        } => {
            if frames.contains_key(&hwnd) {
                return Ok(());
            }
            if hwnd == WINDOW_HANDLE_BASE {
                logl::log(
                    level::IMPORTANT,
                    format_args!("WC3 UI4 ROOT OPEN hwnd=0x{:08x}", hwnd),
                );
            }
            let mut opened = Frame::open(x, y, width, height)
                .map_err(|error| format!("create WC3 UI4 window: {error:?}"))?;
            opened
                .begin(rgba(0, 0, 0, 255))
                .and_then(|()| opened.publish(Damage::full(width, height)))
                .map_err(|error| format!("publish WC3 UI4 window: {error:?}"))?;
            if hwnd == WINDOW_HANDLE_BASE {
                logl::log(
                    level::IMPORTANT,
                    format_args!("WC3 UI4 ROOT INITIAL PUBLISH hwnd=0x{:08x}", hwnd),
                );
            }
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 UI4 FRAME OPEN hwnd=0x{:08x} title={:?} x={} y={} width={} height={}",
                    hwnd,
                    session
                        .windows
                        .get(&hwnd)
                        .map(|window| window.title.as_str())
                        .unwrap_or(""),
                    x,
                    y,
                    width,
                    height
                ),
            );
            frames.insert(hwnd, opened);
        }
        WindowPresentation::Destroy { hwnd } => {
            let frame_dropped = frames.remove(&hwnd).is_some();
            let backing_dropped = window_rgba.remove(&hwnd).is_some();
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 UI4 WINDOW RELEASE hwnd=0x{hwnd:08x} frame_dropped={} backing_dropped={}",
                    frame_dropped as u8, backing_dropped as u8,
                ),
            );
        }
    }
    Ok(())
}

struct GuestContext {
    pid: u32,
    tid: u32,
    context: GuestThreadContext,
    started: bool,
    continuation: Option<GuestContinuation>,
    preemption_count: u64,
    last_preemption_page: u32,
    same_page_preemptions: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingMessageBox {
    caller: ThreadKey,
    owner_hwnd: u32,
    text: String,
    caption: String,
    style: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MessageBoxButton {
    label: &'static str,
    id: u32,
}

struct ActiveMessageBox {
    request: PendingMessageBox,
    frame: Frame,
    buttons: Vec<MessageBoxButton>,
}

fn message_box_buttons(style: u32) -> Option<Vec<MessageBoxButton>> {
    let button = |label, id| MessageBoxButton { label, id };
    Some(match style & 0x0f {
        0 => vec![button("OK", 1)],
        1 => vec![button("OK", 1), button("Cancel", 2)],
        2 => vec![button("Abort", 3), button("Retry", 4), button("Ignore", 5)],
        3 => vec![button("Yes", 6), button("No", 7), button("Cancel", 2)],
        4 => vec![button("Yes", 6), button("No", 7)],
        5 => vec![button("Retry", 4), button("Cancel", 2)],
        6 => vec![
            button("Cancel", 2),
            button("Try Again", 10),
            button("Continue", 11),
        ],
        _ => return None,
    })
}

fn copy_message_box_ansi(memory: &impl GuestMemory, address: u32) -> Result<String, String> {
    if address == 0 {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    for offset in 0..MESSAGE_BOX_MAX_ANSI_BYTES {
        let current = address
            .checked_add(u32::try_from(offset).map_err(|_| "MessageBoxA string offset")?)
            .ok_or_else(|| "MessageBoxA string address overflow".to_owned())?;
        let mut byte = [0];
        memory.read(current, &mut byte).map_err(str::to_owned)?;
        if byte[0] == 0 {
            return Ok(wc3::ThisToThat::cp1252_to_string(&bytes));
        }
        bytes.push(byte[0]);
    }
    Err("MessageBoxA ANSI string exceeds bounded copy".into())
}

fn message_box_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for source_line in text.split('\n') {
        let characters: Vec<_> = source_line.chars().collect();
        if characters.is_empty() {
            lines.push(String::new());
        } else {
            for chunk in characters.chunks(58) {
                lines.push(chunk.iter().collect());
            }
        }
    }
    lines
}

fn open_message_box(
    request: PendingMessageBox,
    buttons: Vec<MessageBoxButton>,
) -> Result<ActiveMessageBox, String> {
    let mut frame = Frame::open(180, 140, MESSAGE_BOX_WIDTH, MESSAGE_BOX_HEIGHT)
        .map_err(|error| format!("create MessageBoxA UI4 frame: {error:?}"))?;
    frame
        .begin(rgba(28, 32, 42, 255))
        .map_err(|error| format!("begin MessageBoxA UI4 frame: {error:?}"))?;
    let caption = SceneTextRow {
        text: request.caption.as_str(),
        x: 20.0,
        y: 18.0,
        font_pixels: 22.0,
    };
    frame
        .stamp_text_scene(
            Font::Default,
            (frame.width(), frame.height()),
            rgba(255, 255, 255, 255),
            core::slice::from_ref(&caption),
        )
        .map_err(|error| format!("stamp MessageBoxA caption: {error:?}"))?;
    let lines = message_box_lines(&request.text);
    let rows: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(index, text)| SceneTextRow {
            text: text.as_str(),
            x: 20.0,
            y: 62.0 + index as f32 * 19.0,
            font_pixels: 16.0,
        })
        .collect();
    frame
        .stamp_text_scene(
            Font::Default,
            (frame.width(), frame.height()),
            rgba(232, 232, 232, 255),
            &rows,
        )
        .map_err(|error| format!("stamp MessageBoxA text: {error:?}"))?;
    let labels = buttons
        .iter()
        .map(|button| format!("[ {} ]", button.label))
        .collect::<Vec<_>>()
        .join("    ");
    let button_row = SceneTextRow {
        text: labels.as_str(),
        x: 20.0,
        y: 238.0,
        font_pixels: 18.0,
    };
    frame
        .stamp_text_scene(
            Font::Default,
            (frame.width(), frame.height()),
            rgba(180, 220, 255, 255),
            core::slice::from_ref(&button_row),
        )
        .map_err(|error| format!("stamp MessageBoxA buttons: {error:?}"))?;
    let damage = Damage::full(frame.width(), frame.height());
    loop {
        match frame.publish(damage) {
            Ok(()) => break,
            Err(ui4_scene::Error::Busy) => {
                trueos::vsys::poll_once();
                trueos::vsys::sleep_ms(1);
            }
            Err(error) => return Err(format!("publish MessageBoxA UI4 frame: {error:?}")),
        }
    }
    Ok(ActiveMessageBox {
        request,
        frame,
        buttons,
    })
}

impl GuestContext {
    fn key(&self) -> ThreadKey {
        ThreadKey {
            pid: self.pid,
            tid: self.tid,
        }
    }
}

struct RuntimeWait {
    deadline: tokio::time::Instant,
    timeout_ms: u32,
    handle: u32,
    resume_registers: Registers,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct IdlePollSite {
    key: ThreadKey,
    caller_return: u32,
    handles: [u32; 2],
    wait_all: u32,
    timeout: u32,
}

struct IdlePollStats {
    polls: u64,
    last_reported_polls: u64,
    last_report: tokio::time::Instant,
}

fn expire_runtime_waits(
    session: &mut Wc3Session,
    contexts: &mut [GuestContext],
    wait_deadlines: &mut HashMap<ThreadKey, RuntimeWait>,
    previous_wait_timeout: &mut Option<(ThreadKey, u32, u32)>,
) -> Result<(), String> {
    let now = tokio::time::Instant::now();
    let expired: Vec<_> = wait_deadlines
        .iter()
        .filter_map(|(key, wait)| (wait.deadline <= now).then_some(*key))
        .collect();
    for key in expired {
        let Some(wait) = wait_deadlines.remove(&key) else {
            continue;
        };
        session.blocked.remove(&key);
        session.enqueue(key);
        let mut registers = wait.resume_registers;
        registers.eax = WAIT_TIMEOUT;
        let index = contexts
            .iter()
            .position(|context| context.key() == key)
            .ok_or_else(|| "timed-out wait context missing".to_owned())?;
        contexts[index]
            .context
            .set_registers(registers)
            .map_err(|error| error.to_string())?;
        let repeated = *previous_wait_timeout == Some((key, wait.handle, wait.timeout_ms));
        if !repeated {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 WAIT TIMEOUT pid={} tid={} handle=0x{:08x} elapsed_ms={} result=0x{:08x}",
                    key.pid, key.tid, wait.handle, wait.timeout_ms, WAIT_TIMEOUT
                ),
            );
        }
        *previous_wait_timeout = Some((key, wait.handle, wait.timeout_ms));
    }
    Ok(())
}

fn context_index(contexts: &[GuestContext], key: ThreadKey) -> Option<usize> {
    contexts.iter().position(|context| context.key() == key)
}

fn pop_runnable_context(session: &mut Wc3Session, contexts: &[GuestContext]) -> Option<usize> {
    let key = session.take_highest_runnable(|key| context_index(contexts, key).is_some())?;
    context_index(contexts, key)
}

struct GuestContinuation {
    import_resume_eip: u32,
    import_esp: u32,
    completion_eax: u32,
    wndproc: u32,
    hwnd: u32,
    message: u32,
}

fn create_thread_context(
    address_space: &AddressSpace,
    thread: &ThreadObject,
) -> Result<GuestContext, String> {
    // Each logical thread gets private x86 stack state. The context remains a
    // logical Blueprint object; every run/resume may use a different carrier.
    let stack_bytes = usize::try_from(thread.requested_stack_size.max(0x1000))
        .map_err(|_| "thread stack size")?;
    let stack_bytes = stack_bytes.next_multiple_of(0x1000);
    let stack_top = STACK_BASE
        .checked_sub(
            thread
                .tid
                .checked_mul(0x0010_0000)
                .ok_or("thread stack index")?,
        )
        .ok_or("thread stack address")?;
    let stack_base = stack_top
        .checked_sub(u32::try_from(stack_bytes).map_err(|_| "thread stack range")?)
        .ok_or("thread stack address")?;
    let teb = thread_teb_va(thread.tid)?;
    address_space
        .map(teb, 0x1000, Permissions::READ | Permissions::WRITE)
        .map_err(|error| error.to_string())?;
    let initialized_exception_list = u32::MAX.to_le_bytes();
    if address_space
        .write(teb, &initialized_exception_list)
        .map_err(|error| error.to_string())?
        != initialized_exception_list.len()
    {
        return Err("short x86 TEB write".into());
    }
    address_space
        .map(
            stack_base,
            stack_bytes,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| error.to_string())?;
    let esp = stack_top - 8;
    let mut entry_frame = [0; 8];
    entry_frame[..4].copy_from_slice(&thunk32::THREAD_EXIT_ADDRESS.to_le_bytes());
    entry_frame[4..].copy_from_slice(&thread.parameter.to_le_bytes());
    if address_space
        .write(esp, &entry_frame)
        .map_err(|error| error.to_string())?
        != entry_frame.len()
    {
        return Err("short x86 thread stack write".into());
    }
    let registers = Registers {
        esp,
        eip: thread.start_address,
        eflags: 0x202,
        fs_base: teb,
        ..Registers::default()
    };
    let context = Context::create(address_space, registers).map_err(|error| error.to_string())?;
    Ok(GuestContext {
        pid: LAUNCHER_PID,
        tid: thread.tid,
        context: GuestThreadContext::spawn(context, LAUNCHER_PID, thread.tid)?,
        started: false,
        continuation: None,
        preemption_count: 0,
        last_preemption_page: 0,
        same_page_preemptions: 0,
    })
}

fn create_child_primary_context(
    child: &mut PendingChild,
    native_index: usize,
) -> Result<GuestContext, String> {
    if child.execution != (ChildExecutionState::DllInitReady { native_index }) {
        return Err("child primary context requires matching DLL-init-ready state".into());
    }
    validate_child_crt_heap_range(child)?;
    validate_child_virtual_alloc_range(child)?;
    validate_child_win_heap_range(child)?;
    let module = child
        .native_modules
        .get(native_index)
        .ok_or_else(|| "child primary context native index".to_owned())?;
    let entry = module
        .image
        .image_base
        .checked_add(module.image.entry_rva)
        .ok_or_else(|| "child primary context entry overflow".to_owned())?;
    let teb = thread_teb_va(child.tid)?;
    child
        .address_space
        .map(teb, 0x1000, Permissions::READ | Permissions::WRITE)
        .map_err(|error| format!("map child TEB: {error}"))?;
    let exception_list = u32::MAX.to_le_bytes();
    if child
        .address_space
        .write(teb, &exception_list)
        .map_err(|error| error.to_string())?
        != exception_list.len()
    {
        return Err("short child TEB write".into());
    }
    child
        .address_space
        .map(
            STACK_BASE,
            STACK_BYTES,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| format!("map child primary stack: {error}"))?;
    let esp = STACK_TOP - 0x10;
    if child.static_load_reserved == 0 {
        child.static_load_reserved = STACK_TOP - 0x20;
        let marker = 0x5743_3344u32.to_le_bytes();
        if child
            .address_space
            .write(child.static_load_reserved, &marker)
            .map_err(|error| error.to_string())?
            != marker.len()
        {
            return Err("short child static-load marker write".into());
        }
    }
    let reserved = child.static_load_reserved;
    let mut frame = [0u8; 16];
    frame[0..4].copy_from_slice(&thunk32::CHILD_DLL_RETURN_ADDRESS.to_le_bytes());
    frame[4..8].copy_from_slice(&module.image.image_base.to_le_bytes());
    frame[8..12].copy_from_slice(&1u32.to_le_bytes());
    frame[12..16].copy_from_slice(&reserved.to_le_bytes());
    if child
        .address_space
        .write(esp, &frame)
        .map_err(|error| error.to_string())?
        != frame.len()
    {
        return Err("short child DLL frame write".into());
    }

    let mut actual_exception_list = [0; 4];
    let mut actual_frame = [0; 16];
    child
        .address_space
        .read(teb, &mut actual_exception_list)
        .map_err(|error| error.to_string())?;
    child
        .address_space
        .read(esp, &mut actual_frame)
        .map_err(|error| error.to_string())?;
    if u32::from_le_bytes(actual_exception_list) != u32::MAX
        || u32::from_le_bytes(
            actual_frame[0..4]
                .try_into()
                .map_err(|_| "child frame return")?,
        ) != thunk32::CHILD_DLL_RETURN_ADDRESS
        || u32::from_le_bytes(
            actual_frame[4..8]
                .try_into()
                .map_err(|_| "child frame hinst")?,
        ) != module.image.image_base
        || u32::from_le_bytes(
            actual_frame[8..12]
                .try_into()
                .map_err(|_| "child frame reason")?,
        ) != 1
        || u32::from_le_bytes(
            actual_frame[12..16]
                .try_into()
                .map_err(|_| "child frame reserved")?,
        ) != reserved
    {
        return Err("child primary context frame verification failed".into());
    }
    let registers = Registers {
        eip: entry,
        esp,
        eflags: 0x202,
        fs_base: teb,
        ..Registers::default()
    };
    let context =
        Context::create(&child.address_space, registers).map_err(|error| error.to_string())?;
    let actual_registers = context.registers().map_err(|error| error.to_string())?;
    if actual_registers.eip != entry
        || actual_registers.esp != esp
        || actual_registers.fs_base != teb
    {
        return Err("child primary context register verification failed".into());
    }
    Ok(GuestContext {
        pid: child.pid,
        tid: child.tid,
        context: GuestThreadContext::spawn(context, child.pid, child.tid)?,
        started: false,
        continuation: None,
        preemption_count: 0,
        last_preemption_page: 0,
        same_page_preemptions: 0,
    })
}

fn create_child_runtime_context(
    child: &PendingChild,
    tid: u32,
    stack_size: u32,
    start_address: u32,
    parameter: u32,
) -> Result<GuestContext, String> {
    if child.execution != ChildExecutionState::ImageEntryRunning {
        return Err("child thread requires image-entry execution".into());
    }
    let stack_bytes = (if stack_size == 0 {
        STACK_BYTES as u32
    } else {
        stack_size.max(0x1000)
    })
        .checked_add(0xfff)
        .ok_or("child thread stack size overflow")?
        & !0xfff;
    if stack_bytes > STACK_BYTES as u32 {
        return Err("child thread stack size frontier".into());
    }
    let stack_top = STACK_BASE
        .checked_sub(tid.checked_mul(0x0010_0000).ok_or("child thread stack index")?)
        .ok_or("child thread stack address")?;
    let stack_base = stack_top
        .checked_sub(stack_bytes)
        .ok_or("child thread stack address")?;
    let teb = thread_teb_va(tid)?;
    child
        .address_space
        .map(teb, 0x1000, Permissions::READ | Permissions::WRITE)
        .map_err(|error| format!("map child thread TEB: {error}"))?;
    if child
        .address_space
        .write(teb, &u32::MAX.to_le_bytes())
        .map_err(|error| error.to_string())?
        != 4
    {
        return Err("short child thread TEB write".into());
    }
    child
        .address_space
        .map(
            stack_base,
            stack_bytes as usize,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| format!("map child thread stack: {error}"))?;
    let esp = stack_top - 8;
    let mut frame = [0u8; 8];
    frame[..4].copy_from_slice(&thunk32::CHILD_THREAD_EXIT_ADDRESS.to_le_bytes());
    frame[4..].copy_from_slice(&parameter.to_le_bytes());
    if child
        .address_space
        .write(esp, &frame)
        .map_err(|error| error.to_string())?
        != frame.len()
    {
        return Err("short child thread entry frame write".into());
    }
    let registers = Registers {
        eip: start_address,
        esp,
        eflags: 0x202,
        fs_base: teb,
        ..Registers::default()
    };
    let context = Context::create(&child.address_space, registers)
        .map_err(|error| error.to_string())?;
    Ok(GuestContext {
        pid: child.pid,
        tid,
        context: GuestThreadContext::spawn(context, child.pid, tid)?,
        started: false,
        continuation: None,
        preemption_count: 0,
        last_preemption_page: 0,
        same_page_preemptions: 0,
    })
}

fn create_child_guest_thread(
    child: &PendingChild,
    session: &mut Wc3Session,
    stack_size: u32,
    start_address: u32,
    parameter: u32,
) -> Result<(GuestContext, ThreadKey, u32), String> {
    let proposed_tid = session.next_tid;
    let guest = create_child_runtime_context(
        child,
        proposed_tid,
        stack_size,
        start_address,
        parameter,
    )?;
    let (key, handle) = session
        .create_child_thread(child.pid)
        .map_err(str::to_owned)?;
    if key.tid != proposed_tid {
        return Err("child thread TID reservation changed".into());
    }
    Ok((guest, key, handle))
}

fn arm_existing_child_dll_init(
    child: &mut PendingChild,
    guest: &mut GuestContext,
    native_index: usize,
    returned: Registers,
) -> Result<(String, u32, u32), String> {
    if guest.pid != child.pid || guest.tid != child.tid {
        return Err("child DLL rearm context identity mismatch".into());
    }
    if !guest.started {
        return Err("child DLL rearm requires a started context".into());
    }
    if child.execution != (ChildExecutionState::DllInitReady { native_index }) {
        return Err("child DLL rearm requires matching DLL-init-ready state".into());
    }
    let module = child
        .native_modules
        .get(native_index)
        .ok_or_else(|| "child DLL rearm native index".to_owned())?;
    let entry = module
        .image
        .image_base
        .checked_add(module.image.entry_rva)
        .ok_or_else(|| "DLL entry overflow".to_owned())?;
    let returned_esp = returned.esp;
    if returned_esp < STACK_BASE
        || returned_esp > STACK_TOP
        || returned_esp % 4 != 0
        || returned_esp < STACK_BASE + 16
    {
        return Err("child DLL rearm returned ESP is outside the child stack".into());
    }
    let frame_esp = returned_esp
        .checked_sub(16)
        .ok_or_else(|| "DLL frame stack underflow".to_owned())?;
    if child.static_load_reserved == 0 {
        return Err("child DLL rearm static-load marker is unavailable".into());
    }
    let mut frame = [0u8; 16];
    frame[0..4].copy_from_slice(&thunk32::CHILD_DLL_RETURN_ADDRESS.to_le_bytes());
    frame[4..8].copy_from_slice(&module.image.image_base.to_le_bytes());
    frame[8..12].copy_from_slice(&1u32.to_le_bytes());
    frame[12..16].copy_from_slice(&child.static_load_reserved.to_le_bytes());
    if child
        .address_space
        .write(frame_esp, &frame)
        .map_err(|error| error.to_string())?
        != frame.len()
    {
        return Err("short child DLL rearm frame write".into());
    }
    let mut actual_frame = [0u8; 16];
    if child
        .address_space
        .read(frame_esp, &mut actual_frame)
        .map_err(|error| error.to_string())?
        != actual_frame.len()
        || actual_frame != frame
    {
        return Err("child DLL rearm frame verification failed".into());
    }
    let mut registers = returned;
    registers.eip = entry;
    registers.esp = frame_esp;
    guest
        .context
        .set_registers(registers)
        .map_err(|error| error.to_string())?;
    child.execution = ChildExecutionState::DllInitRunning { native_index };
    Ok((module.stored.clone(), entry, frame_esp))
}

fn arm_existing_child_image_entry(
    child: &mut PendingChild,
    guest: &mut GuestContext,
    returned: Registers,
) -> Result<(u32, u32), String> {
    if guest.pid != child.pid || guest.tid != child.tid {
        return Err("child image-entry context identity mismatch".into());
    }
    if !guest.started {
        return Err("child image-entry requires a started context".into());
    }
    if child.execution != ChildExecutionState::ImageEntryReady {
        return Err("child image-entry requires image-entry-ready state".into());
    }
    if child
        .native_modules
        .iter()
        .any(|module| !module.initialized)
    {
        return Err("child image-entry requires all native modules initialized".into());
    }
    let entry = child
        .image
        .image_base
        .checked_add(child.image.entry_rva)
        .ok_or_else(|| "War3 entry overflow".to_owned())?;
    if returned.esp < STACK_BASE + 4 || returned.esp > STACK_TOP || returned.esp % 4 != 0 {
        return Err("child image-entry returned ESP is outside the child stack".into());
    }
    let frame_esp = returned
        .esp
        .checked_sub(CHILD_IMAGE_ENTRY_HEADROOM)
        .ok_or_else(|| "child image-entry stack underflow".to_owned())?;
    let caller_bytes = u32::try_from(CHILD_IMAGE_ENTRY_CALLER_BYTES)
        .map_err(|_| "child image-entry caller bytes")?;
    let caller_end = frame_esp
        .checked_add(caller_bytes)
        .ok_or_else(|| "child image-entry caller range overflow".to_owned())?;
    if frame_esp < STACK_BASE || caller_end > returned.esp || caller_end > STACK_TOP {
        return Err("child image-entry caller frame outside stack".into());
    }
    let mut caller = [0; CHILD_IMAGE_ENTRY_CALLER_BYTES];
    caller[..4].copy_from_slice(&thunk32::CHILD_IMAGE_RETURN_ADDRESS.to_le_bytes());
    if child
        .address_space
        .write(frame_esp, &caller)
        .map_err(|error| error.to_string())?
        != caller.len()
    {
        return Err("short child image-entry frame write".into());
    }
    let mut actual = [0; CHILD_IMAGE_ENTRY_CALLER_BYTES];
    if child
        .address_space
        .read(frame_esp, &mut actual)
        .map_err(|error| error.to_string())?
        != actual.len()
        || actual != caller
    {
        return Err("child image-entry frame verification failed".into());
    }
    let mut registers = returned;
    registers.eip = entry;
    registers.esp = frame_esp;
    guest
        .context
        .set_registers(registers)
        .map_err(|error| error.to_string())?;
    child.execution = ChildExecutionState::ImageEntryRunning;
    Ok((entry, frame_esp))
}

fn validate_child_crt_heap_range(child: &PendingChild) -> Result<(), String> {
    validate_child_private_arena_range(
        child,
        (CHILD_CRT_HEAP_BASE, CHILD_CRT_HEAP_LIMIT),
        "child CRT heap",
    )
}

fn validate_child_virtual_alloc_range(child: &PendingChild) -> Result<(), String> {
    validate_child_private_arena_range(
        child,
        (CHILD_VIRTUAL_ALLOC_BASE, CHILD_VIRTUAL_ALLOC_LIMIT),
        "child VirtualAlloc arena",
    )
}

fn validate_child_win_heap_range(child: &PendingChild) -> Result<(), String> {
    validate_child_private_arena_range(
        child,
        (CHILD_WIN_HEAP_BASE, CHILD_WIN_HEAP_LIMIT),
        "child Win32 heap",
    )
}

fn validate_child_private_arena_range(
    child: &PendingChild,
    arena: (u32, u32),
    arena_name: &str,
) -> Result<(), String> {
    let mut ranges = vec![
        (
            "child control",
            thunk32::CHILD_CONTROL_BASE,
            thunk32::CHILD_CONTROL_BASE + 0x1000,
        ),
        (
            "provider thunk",
            thunk32::THUNK_BASE,
            thunk32::THUNK_BASE
                .checked_add(
                    u32::try_from(child.provider_thunk_bytes)
                        .map_err(|_| "provider thunk bytes")?,
                )
                .ok_or("provider thunk range")?,
        ),
        (
            "child TEB",
            thread_teb_va(child.tid)?,
            thread_teb_va(child.tid)?
                .checked_add(0x1000)
                .ok_or("child TEB range")?,
        ),
        ("child stack", STACK_BASE, STACK_TOP),
        ("child GDI arena", 0x0500_0000, 0x0600_0000),
        (
            "War3 image",
            child.image.image_base,
            child
                .image
                .image_base
                .checked_add(
                    u32::try_from(child.image.image.len()).map_err(|_| "War3 image range")?,
                )
                .ok_or("War3 image range")?,
        ),
    ];
    for module in &child.native_modules {
        ranges.push((
            "native module",
            module.image.image_base,
            module
                .image
                .image_base
                .checked_add(
                    u32::try_from(module.image.image.len()).map_err(|_| "native image range")?,
                )
                .ok_or("native image range")?,
        ));
    }
    if ranges
        .iter()
        .any(|(_, start, end)| arena.0 < *end && *start < arena.1)
    {
        return Err(format!(
            "{arena_name} overlaps an established child mapping"
        ));
    }
    Ok(())
}

fn ensure_child_crt_allocation_mapped(
    child: &mut PendingChild,
    pointer: u32,
    requested: u32,
) -> Result<u32, String> {
    let mapped_end = child_crt_mapping_end(child.crt_heap_mapped_end, pointer, requested)?;
    if mapped_end > child.crt_heap_mapped_end {
        child
            .address_space
            .map(
                child.crt_heap_mapped_end,
                usize::try_from(mapped_end - child.crt_heap_mapped_end)
                    .map_err(|_| "child CRT mapping length")?,
                Permissions::READ | Permissions::WRITE,
            )
            .map_err(|error| format!("map child CRT heap: {error}"))?;
        child.crt_heap_mapped_end = mapped_end;
    }
    Ok(child.crt_heap_mapped_end)
}

fn ensure_child_win_heap_mapped(
    child: &mut PendingChild,
    allocation_end: u32,
) -> Result<u32, String> {
    let mapped_end = allocation_end
        .checked_add(0xfff)
        .ok_or_else(|| "child Win32 heap mapping end overflow".to_owned())?
        & !0xfff;
    if mapped_end > CHILD_WIN_HEAP_LIMIT {
        return Err("child Win32 heap mapping exceeds arena".into());
    }
    if mapped_end > child.win_heap_mapped_end {
        child
            .address_space
            .map(
                child.win_heap_mapped_end,
                usize::try_from(mapped_end - child.win_heap_mapped_end)
                    .map_err(|_| "child Win32 heap mapping length")?,
                Permissions::READ | Permissions::WRITE,
            )
            .map_err(|error| format!("map child Win32 heap: {error}"))?;
        child.win_heap_mapped_end = mapped_end;
    }
    Ok(child.win_heap_mapped_end)
}

fn child_dllonexit(
    child: &mut PendingChild,
    process: &mut XpProcess,
    pid: u32,
    tid: u32,
    during: &str,
    provider_id: u32,
    esp: u32,
) -> Result<u32, String> {
    const MAX_CRT_CALLBACKS: u32 = 65_536;

    let frame = read_guest_words(&X86Memory(&child.address_space), esp, 4)?;
    let (caller_ret, func, start_ref, end_ref) = (frame[0], frame[1], frame[2], frame[3]);
    let start = if start_ref == 0 {
        0
    } else {
        read_guest_words(&X86Memory(&child.address_space), start_ref, 1)?[0]
    };
    let end = if end_ref == 0 {
        0
    } else {
        read_guest_words(&X86Memory(&child.address_space), end_ref, 1)?[0]
    };
    let entries = end
        .checked_sub(start)
        .filter(|bytes| bytes % 4 == 0)
        .map(|bytes| bytes / 4)
        .unwrap_or(0);
    logl::trace!("trace-init",
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD CRT DLLONEXIT pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} caller_ret=0x{:08x} func=0x{:08x} start_ref=0x{:08x} end_ref=0x{:08x} start=0x{:08x} end=0x{:08x} entries={}",
            pid,
            tid,
            during,
            provider_id,
            caller_ret,
            func,
            start_ref,
            end_ref,
            start,
            end,
            entries
        ),
    );
    let fail = |reason: &str| {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD CRT DLLONEXIT RESULT pid={} tid={} result=0x00000000 reason={}",
                pid, tid, reason
            ),
        );
        0
    };
    if start_ref == 0 || end_ref == 0 {
        return Ok(fail("null-reference"));
    }
    if start == 0 || end == 0 {
        return Ok(fail("null-table"));
    }
    if end < start || start % 4 != 0 || end % 4 != 0 {
        return Ok(fail("malformed-table"));
    }
    let used_bytes = end
        .checked_sub(start)
        .ok_or_else(|| "child __dllonexit range underflow".to_owned())?;
    let old_entries = used_bytes / 4;
    if old_entries > MAX_CRT_CALLBACKS {
        return Ok(fail("table-too-large"));
    }
    let required_bytes = used_bytes
        .checked_add(4)
        .ok_or_else(|| "child __dllonexit required size overflow".to_owned())?;
    let Some(old_capacity) = process.crt_allocation_capacity(start) else {
        return Ok(fail("untracked-table"));
    };
    if used_bytes > old_capacity {
        return Ok(fail("table-exceeds-capacity"));
    }

    let (new_start, new_end, moved, mapped_end) = if required_bytes <= old_capacity {
        let slot = start
            .checked_add(used_bytes)
            .ok_or_else(|| "child __dllonexit slot overflow".to_owned())?;
        write_child_u32(child, slot, func)?;
        write_child_u32(child, start_ref, start)?;
        let new_end = start
            .checked_add(required_bytes)
            .ok_or_else(|| "child __dllonexit end overflow".to_owned())?;
        write_child_u32(child, end_ref, new_end)?;
        (start, new_end, false, None)
    } else {
        let Some(resize) = process
            .crt_grow_callback_table(start, used_bytes, required_bytes)
            .map_err(str::to_owned)?
        else {
            return Ok(fail("oom"));
        };
        let old_mapped_end = child.crt_heap_mapped_end;
        let current_mapped_end =
            ensure_child_crt_allocation_mapped(child, resize.pointer, resize.required_bytes)?;
        let bytes = read_guest_bytes(
            &X86Memory(&child.address_space),
            start,
            usize::try_from(used_bytes).map_err(|_| "child __dllonexit copy length")?,
        )?;
        if !bytes.is_empty()
            && child
                .address_space
                .write(resize.pointer, &bytes)
                .map_err(|error| format!("copy child __dllonexit callbacks: {error}"))?
                != bytes.len()
        {
            return Err("short child __dllonexit callback copy".into());
        }
        let slot = resize
            .pointer
            .checked_add(used_bytes)
            .ok_or_else(|| "child __dllonexit replacement slot overflow".to_owned())?;
        write_child_u32(child, slot, func)?;
        let new_end = resize
            .pointer
            .checked_add(required_bytes)
            .ok_or_else(|| "child __dllonexit replacement end overflow".to_owned())?;
        write_child_u32(child, start_ref, resize.pointer)?;
        write_child_u32(child, end_ref, new_end)?;
        if !process.retire_crt_allocation(start) {
            return Err("child __dllonexit old allocation disappeared".into());
        }
        (
            resize.pointer,
            new_end,
            resize.moved,
            (current_mapped_end != old_mapped_end).then_some(current_mapped_end),
        )
    };
    logl::trace!("trace-init",
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD CRT DLLONEXIT RESULT pid={} tid={} func=0x{:08x} old_start=0x{:08x} old_end=0x{:08x} new_start=0x{:08x} new_end=0x{:08x} entries_before={} entries_after={} moved={} return_eax=0x{:08x}",
            pid,
            tid,
            func,
            start,
            end,
            new_start,
            new_end,
            old_entries,
            old_entries + 1,
            moved as u8,
            func
        ),
    );
    if let Some(mapped_end) = mapped_end {
        logl::trace!("trace-init",
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD CRT DLLONEXIT RESULT mapped_end=0x{:08x}",
                mapped_end
            ),
        );
    }
    Ok(func)
}

fn write_child_u32(child: &mut PendingChild, address: u32, value: u32) -> Result<(), String> {
    if child
        .address_space
        .write(address, &value.to_le_bytes())
        .map_err(|error| format!("write child u32 0x{address:08x}: {error}"))?
        != 4
    {
        return Err("short child u32 write".into());
    }
    Ok(())
}

fn child_crt_mapping_end(current: u32, pointer: u32, requested: u32) -> Result<u32, String> {
    let allocation_end = pointer
        .checked_add(requested)
        .ok_or_else(|| "child CRT allocation end overflow".to_owned())?;
    let mapped_end = allocation_end
        .checked_add(0xfff)
        .ok_or_else(|| "child CRT map alignment overflow".to_owned())?
        & !0xfff;
    if current < CHILD_CRT_HEAP_BASE || mapped_end > CHILD_CRT_HEAP_LIMIT {
        return Err("child CRT mapping exceeds heap limit".into());
    }
    Ok(current.max(mapped_end))
}

fn verify_child_primary_context(
    child: &PendingChild,
    context: &GuestContext,
    native_index: usize,
) -> Result<(), String> {
    if context.key()
        != (ThreadKey {
            pid: child.pid,
            tid: child.tid,
        })
    {
        return Err("child primary context identity mismatch".into());
    }
    let module = child
        .native_modules
        .get(native_index)
        .ok_or_else(|| "child primary context native index".to_owned())?;
    let entry = module
        .image
        .image_base
        .checked_add(module.image.entry_rva)
        .ok_or_else(|| "child primary context entry overflow".to_owned())?;
    let teb = thread_teb_va(child.tid)?;
    let esp = STACK_TOP - 0x10;
    let reserved = STACK_TOP - 0x20;
    let registers = context
        .context
        .registers()
        .map_err(|error| error.to_string())?;
    let mut frame = [0; 16];
    child
        .address_space
        .read(esp, &mut frame)
        .map_err(|error| error.to_string())?;
    if registers.eip != entry
        || registers.esp != esp
        || registers.fs_base != teb
        || u32::from_le_bytes(frame[0..4].try_into().map_err(|_| "child frame return")?)
            != thunk32::CHILD_DLL_RETURN_ADDRESS
        || u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "child frame hinst")?)
            != module.image.image_base
        || u32::from_le_bytes(frame[8..12].try_into().map_err(|_| "child frame reason")?) != 1
        || u32::from_le_bytes(
            frame[12..16]
                .try_into()
                .map_err(|_| "child frame reserved")?,
        ) != reserved
    {
        return Err("child primary context preservation mismatch".into());
    }
    Ok(())
}

fn thread_teb_va(tid: u32) -> Result<u32, String> {
    wc3::process::TEB_VA
        .checked_add(
            tid.checked_sub(1)
                .ok_or_else(|| "invalid x86 thread id".to_owned())?
                .checked_mul(0x1000)
                .ok_or_else(|| "x86 TEB index overflow".to_owned())?,
        )
        .filter(|address| *address < wc3::process::HEAP_VA)
        .ok_or_else(|| "x86 TEB address space exhausted".to_owned())
}

struct PendingChild {
    pid: u32,
    tid: u32,
    active_thread_tid: u32,
    parked_threads: HashMap<u32, ChildThreadRuntime>,
    image: pe32::PeImage,
    /// The exact on-disk bytes parsed to create `image`.  File APIs for the
    /// child main image must serve this immutable snapshot, rather than issue
    /// a second TRUEOSFS read later in execution.
    self_image_bytes: Arc<Vec<u8>>,
    native_modules: Vec<PendingNativeModule>,
    address_space: AddressSpace,
    crt_heap_mapped_end: u32,
    win_heap_mapped_end: u32,
    provider_thunk_bytes: usize,
    static_load_reserved: u32,
    initterm: Option<ChildInitterm>,
    load_library_call: Option<ChildLoadLibraryCall>,
    window_callback: Option<ChildWindowCallback>,
    cipow: Option<ChildCiPow>,
    cipow_diagnostic_logged: bool,
    get_system_info_consumer_logged: bool,
    seh_handler_dumped: bool,
    seh3_diagnostic_logged: bool,
    seh: Option<ChildSehDispatch>,
    seh3_call: Option<ChildSeh3Call>,
    unhandled_filter_call: Option<ChildUnhandledFilterCall>,
    repeated_null_call: Option<NullLoopWatch>,
    repeated_divide_fault: Option<DivideLoopWatch>,
    single_step_count: u64,
    scan_progress: Option<ScanProgress>,
    scan_heartbeat_source: Option<u32>,
    dword_scan_watch: Option<DwordScanWatch>,
    table_checkpoint_capture: Option<TableCheckpointCapture>,
    loop_checkpoint_capture: Option<asupersync::loop_checkpoint::LoopCheckpointCapture>,
    loop_checkpoint_attempted: u8,
    loader: ChildLoaderState,
    execution: ChildExecutionState,
}

#[derive(Default)]
struct ChildThreadRuntime {
    cipow: Option<ChildCiPow>,
    seh_handler_dumped: bool,
    seh3_diagnostic_logged: bool,
    seh: Option<ChildSehDispatch>,
    seh3_call: Option<ChildSeh3Call>,
    unhandled_filter_call: Option<ChildUnhandledFilterCall>,
    window_callback: Option<ChildWindowCallback>,
    repeated_null_call: Option<NullLoopWatch>,
    repeated_divide_fault: Option<DivideLoopWatch>,
}

impl PendingChild {
    /// Existing dispatch code uses these fields as the currently admitted
    /// thread's scratch state. Swap them at the serialized scheduler boundary.
    fn activate_thread(&mut self, tid: u32) {
        if self.active_thread_tid == tid {
            return;
        }
        let previous = ChildThreadRuntime {
            cipow: self.cipow.take(),
            seh_handler_dumped: std::mem::take(&mut self.seh_handler_dumped),
            seh3_diagnostic_logged: std::mem::take(&mut self.seh3_diagnostic_logged),
            seh: self.seh.take(),
            seh3_call: self.seh3_call.take(),
            unhandled_filter_call: self.unhandled_filter_call.take(),
            window_callback: self.window_callback.take(),
            repeated_null_call: self.repeated_null_call.take(),
            repeated_divide_fault: self.repeated_divide_fault.take(),
        };
        self.parked_threads.insert(self.active_thread_tid, previous);
        let next = self.parked_threads.remove(&tid).unwrap_or_default();
        self.cipow = next.cipow;
        self.seh_handler_dumped = next.seh_handler_dumped;
        self.seh3_diagnostic_logged = next.seh3_diagnostic_logged;
        self.seh = next.seh;
        self.seh3_call = next.seh3_call;
        self.unhandled_filter_call = next.unhandled_filter_call;
        self.window_callback = next.window_callback;
        self.repeated_null_call = next.repeated_null_call;
        self.repeated_divide_fault = next.repeated_divide_fault;
        self.active_thread_tid = tid;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NullLoopSignature {
    pid: u32,
    tid: u32,
    return_address: u32,
    slot: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NullLoopWatch {
    signature: NullLoopSignature,
    count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DivideLoopSignature {
    pid: u32,
    tid: u32,
    eip: u32,
    esp: u32,
    eax: u32,
    ecx: u32,
    edx: u32,
    progress: DivideLoopProgress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DivideLoopProgress {
    table_base: u32,
    index: u32,
    state_ptr: u32,
    input: u8,
    accumulator: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DivideLoopWatch {
    signature: DivideLoopSignature,
    count: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScanProgress {
    stage: Option<u8>,
    source: Option<u32>,
    checksum: Option<u8>,
    index: Option<u32>,
    bound: Option<u32>,
    reset: Option<u32>,
    gate: Option<u32>,
    tf: bool,
    dr7: u32,
}

use wc3::checkpoint::DwordScanWatch;

#[derive(Clone, Debug)]
struct CachedPage {
    va: u32,
    before_sha256: [u8; 32],
    before: Vec<u8>,
}

#[derive(Clone, Debug)]
struct TableCheckpointCapture {
    table_base: u32,
    before_registers: Registers,
    before_debug_registers: DebugRegisters,
    before_extended_state: ExtendedState,
    pages: Vec<CachedPage>,
    call_count: u32,
    provider_import_count: usize,
    session_object_count: usize,
    process_handle_count: usize,
    provider_thunk_bytes: usize,
    crt_heap_mapped_end: u32,
    win_heap_mapped_end: u32,
}

#[derive(Clone, Debug)]
struct ChildSehDispatch {
    original_registers: Registers,
    registration: u32,
    next_registration: u32,
    handler: u32,
    exception_record_va: u32,
    context_va: u32,
    exception_pointers_va: u32,
    preserved_fs_base: u32,
    depth: u32,
    quiet: bool,
    boring_single_step: bool,
    scan_single_step: bool,
    dword_scan_single_step: bool,
}
#[derive(Clone, Debug)]
enum ChildSeh3CallbackKind {
    Filter {
        level: i32,
        previous: i32,
        start_level: i32,
    },
    Finally {
        next_level: i32,
        selected_level: i32,
    },
}

#[derive(Clone, Debug)]
struct ChildSeh3Call {
    provider_resume_eip: u32,
    provider_esp: u32,
    frame: u32,
    scope: u32,
    kind: ChildSeh3CallbackKind,
}
#[derive(Clone, Debug)]
enum ChildUnhandledFilterContinuation {
    Provider { resume_eip: u32 },
    TerminalSeh { seh: ChildSehDispatch },
}

#[derive(Clone, Debug)]
struct ChildUnhandledFilterCall {
    return_esp: u32,
    filter: u32,
    continuation: ChildUnhandledFilterContinuation,
}

struct ChildCiPow {
    provider_esp: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChildInitterm {
    provider_resume_eip: u32,
    provider_esp: u32,
    begin: u32,
    cursor: u32,
    end: u32,
    callbacks_invoked: u32,
    rust_callbacks: u32,
}

#[derive(Clone, Debug)]
struct ChildLoadLibraryCall {
    provider_resume_eip: u32,
    provider_esp: u32,
    native_index: usize,
    module_handle: u32,
    load_library_handle: u32,
    remaining_native_indices: Vec<usize>,
}

#[derive(Clone, Debug)]
struct ChildWindowCallback {
    provider_resume_eip: u32,
    provider_esp: u32,
    reason: &'static str,
    return_policy: WindowCallbackReturn,
    hwnd: u32,
    wndproc: u32,
    message: u32,
}

#[derive(Clone, Copy, Debug)]
enum WindowCallbackReturn {
    Fixed(u32),
    WndProc,
}

impl WindowCallbackReturn {
    fn api_result(self, wndproc_eax: u32) -> u32 {
        match self {
            Self::Fixed(value) => value,
            Self::WndProc => wndproc_eax,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InittermAdvance {
    CallbackScheduled,
    Complete,
}

// Child PE images are mapped RWX as a whole by map_child_image. Restrict the
// optimization further to their declared sections: code for fetches, non-code
// data for operands. Dynamic/stack/control mappings never qualify.
fn initializer_image_range(child: &PendingChild, address: u32, len: usize, access: u32) -> bool {
    let Some(end) = u32::try_from(len).ok().and_then(|len| address.checked_add(len)) else {
        return false;
    };
    std::iter::once(&child.image).chain(child.native_modules.iter().map(|module| &module.image))
        .any(|image| image.sections.iter().any(|section| {
            let Some(start) = image.image_base.checked_add(section.virtual_address) else { return false; };
            let Some(limit) = start.checked_add(section.virtual_size) else { return false; };
            address >= start && end <= limit
                && section.characteristics & access == access
                && (access & 0x2000_0000 != 0 || section.characteristics & 0x2000_0000 == 0)
        }))
}

fn try_rust_initializer(
    child: &PendingChild,
    context: &mut GuestContext,
    target: u32,
    callback_esp: u32,
    registers: &mut Registers,
) -> Result<bool, String> {
    // Do not erase single stepping, breakpoints, alignment faults, or observable
    // interleavings. Normal child images/stack have flat segments and RWX/RW maps.
    if !child.parked_threads.is_empty() || registers.eflags & 0x0007_4100 != 0 {
        return Ok(false);
    }
    let debug = context.context.debug_registers().map_err(|error| error.to_string())?;
    if debug.dr7 & 0x23ff != 0 { return Ok(false); }
    let mut trap = [0; 3];
    if child.address_space.read(thunk32::CHILD_CALLBACK_RETURN_ADDRESS, &mut trap).ok() != Some(3)
        || trap != [0x0f, 0x01, 0xc1]
    { return Ok(false); }
    let Some(effect) = wc3::initterm::plan(target, |address, output, access| {
        let permission = match access {
            wc3::initterm::Access::Code => 0x2000_0000,
            wc3::initterm::Access::Data => 0x4000_0000,
        };
        initializer_image_range(child, address, output.len(), permission)
            && child.address_space.read(address, output).ok() == Some(output.len())
    }) else { return Ok(false); };
    if let Some((destination, value)) = effect.store {
        // A single-page kernel write validates the whole range before copying.
        // On denial it has made no changes, so the guest can take its own fault.
        if destination & 0xfff > 0xffc
            || !initializer_image_range(child, destination, 4, 0xc000_0000)
        { return Ok(false); }
        match child.address_space.write(destination, &value.to_le_bytes()) {
            Ok(4) => (),
            Err(_) => return Ok(false),
            Ok(_) => return Err("short semantic initializer write".into()),
        }
    }
    if let Some(eax) = effect.eax { registers.eax = eax; }
    registers.esp = callback_esp + 4;
    registers.eip = thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL;
    context.context.set_registers(*registers).map_err(|error| error.to_string())?;
    Ok(true)
}

fn advance_child_initterm(
    child: &mut PendingChild,
    context: &mut GuestContext,
) -> Result<InittermAdvance, String> {
    loop {
        let state = child
            .initterm
            .as_mut()
            .ok_or_else(|| "child _initterm continuation missing".to_owned())?;
        if state.cursor == state.end {
            let complete = child
                .initterm
                .take()
                .ok_or_else(|| "child _initterm completion state missing".to_owned())?;
            logl::log(level::IMPORTANT, format_args!(
                "WC3 CHILD CRT INITTERM COMPLETE pid={} tid={} callbacks={} rust_callbacks={}",
                child.pid, child.tid, complete.callbacks_invoked, complete.rust_callbacks,
            ));
            let mut registers = context
                .context
                .registers()
                .map_err(|error| error.to_string())?;
            registers.eip = complete.provider_resume_eip;
            registers.esp = complete.provider_esp;
            registers.eax = 0;
            context
                .context
                .set_registers(registers)
                .map_err(|error| error.to_string())?;
            return Ok(InittermAdvance::Complete);
        }
        if state.cursor > state.end {
            return Err("child _initterm cursor exceeded range".into());
        }
        let slot = state.cursor;
        state.cursor = state
            .cursor
            .checked_add(4)
            .ok_or_else(|| "child _initterm cursor overflow".to_owned())?;
        let mut target = [0; 4];
        child
            .address_space
            .read(slot, &mut target)
            .map_err(|error| format!("read child _initterm slot 0x{slot:08x}: {error}"))?;
        let target = u32::from_le_bytes(target);
        if target == 0 {
            continue;
        }
        let callback_esp = state
            .provider_esp
            .checked_sub(4)
            .ok_or_else(|| "child _initterm callback stack underflow".to_owned())?;
        let callback_return = thunk32::CHILD_CALLBACK_RETURN_ADDRESS.to_le_bytes();
        if child
            .address_space
            .write(callback_esp, &callback_return)
            .map_err(|error| format!("write child _initterm callback return: {error}"))?
            != callback_return.len()
        {
            return Err("short child _initterm callback return write".into());
        }
        state.callbacks_invoked = state
            .callbacks_invoked
            .checked_add(1)
            .ok_or_else(|| "child _initterm callback count overflow".to_owned())?;
        let index = slot
            .checked_sub(state.begin)
            .ok_or_else(|| "child _initterm index underflow".to_owned())?
            / 4;
        logl::trace!("trace-init",
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD CRT INITTERM CALL pid={} tid={} index={} slot=0x{:08x} target=0x{:08x}",
                child.pid, child.tid, index, slot, target
            ),
        );
        let mut registers = context
            .context
            .registers()
            .map_err(|error| error.to_string())?;
        if !cfg!(feature = "guest-initterm")
            && try_rust_initializer(child, context, target, callback_esp, &mut registers)?
        {
            child.initterm.as_mut().expect("active initializer").rust_callbacks += 1;
            continue;
        }
        registers.eip = target;
        registers.esp = callback_esp;
        context
            .context
            .set_registers(registers)
            .map_err(|error| error.to_string())?;
        return Ok(InittermAdvance::CallbackScheduled);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChildExecutionState {
    Loader,
    DllInitReady { native_index: usize },
    DllInitRunning { native_index: usize },
    ImageEntryReady,
    ImageEntryRunning,
}

struct ChildLoaderState {
    prepared: bool,
    native_requests: Vec<child_loader::NativeModuleRequest>,
    next_native: usize,
}

struct PendingNativeModule {
    requested: String,
    stored: String,
    image: pe32::PeImage,
    initialized: bool,
}

fn child_execution_module(
    child: &PendingChild,
) -> Result<(usize, &PendingNativeModule), &'static str> {
    let native_index = match child.execution {
        ChildExecutionState::DllInitReady { native_index }
        | ChildExecutionState::DllInitRunning { native_index } => native_index,
        ChildExecutionState::Loader
        | ChildExecutionState::ImageEntryReady
        | ChildExecutionState::ImageEntryRunning => {
            return Err("child execution has no native module");
        }
    };
    child
        .native_modules
        .get(native_index)
        .map(|module| (native_index, module))
        .ok_or("child execution native index")
}

fn child_execution_scope(child: &PendingChild) -> Result<String, &'static str> {
    if let Some(call) = &child.load_library_call {
        let module = child
            .native_modules
            .get(call.native_index)
            .ok_or("runtime LoadLibrary native index")?;
        return Ok(format!("{}:DLL_PROCESS_ATTACH", module.stored));
    }
    match child.execution {
        ChildExecutionState::DllInitRunning { .. } => {
            let (_, module) = child_execution_module(child)?;
            Ok(format!("{}:DLL_PROCESS_ATTACH", module.stored))
        }
        ChildExecutionState::ImageEntryRunning => Ok("War3.exe:ENTRY".to_owned()),
        ChildExecutionState::Loader => Err("child provider trap while execution=loader"),
        ChildExecutionState::DllInitReady { .. } | ChildExecutionState::ImageEntryReady => {
            Err("child provider trap while execution is not running")
        }
    }
}

fn log_child_fault_precursor(
    child: &PendingChild,
    storm: &PendingNativeModule,
    process: &wc3::session::Wc3Process,
    pid: u32,
    tid: u32,
    returned_eax: u32,
) -> Result<(), String> {
    const CALL_EIP: u32 = 0x1503_62da;
    const IAT_RVA: u32 = 0x0003_d1e0;
    let iat_va = storm
        .image
        .image_base
        .checked_add(IAT_RVA)
        .ok_or_else(|| "Storm precursor IAT address overflow".to_owned())?;
    let import = storm
        .image
        .imports
        .iter()
        .find(|import| import.iat_rva == IAT_RVA)
        .ok_or_else(|| "Storm precursor IAT has no parsed import".to_owned())?;
    let mut target_bytes = [0; 4];
    child
        .address_space
        .read(iat_va, &mut target_bytes)
        .map_err(|error| error.to_string())?;
    let target = u32::from_le_bytes(target_bytes);
    let provider = target
        .checked_sub(thunk32::THUNK_BASE)
        .filter(|offset| *offset % thunk32::THUNK_BYTES as u32 == 0)
        .map(|offset| offset / thunk32::THUNK_BYTES as u32)
        .filter(|provider_id| (*provider_id as usize) < process.xp.provider_import_count())
        .and_then(|provider_id| {
            process
                .xp
                .provider_import(provider_id)
                .map(|provider| (provider_id, provider))
        });

    let import_symbol = pe_import_symbol_label(&import.symbol);
    let mut detail = format!(
        "WC3 CHILD FAULT PRECURSOR pid={} tid={} module=\"Storm.dll\" call_eip=0x{:08x} iat_va=0x{:08x} iat_rva=0x{:08x} import_module=\"{}\" {} argument0=0x00000080 returned_eax=0x{:08x} target=0x{:08x}",
        pid, tid, CALL_EIP, iat_va, IAT_RVA, import.module, import_symbol, returned_eax, target,
    );
    if let Some((provider_id, provider)) = provider {
        let provider_symbol = provider_symbol_label(&provider.symbol);
        detail.push_str(&format!(
            " provider_id={} provider_module=\"{}\" {}",
            provider_id, provider.module, provider_symbol,
        ));
        if !provider_matches_import(provider, import) {
            logl::log(
                level::IMPORTANT,
                format_args!(
                    "WC3 CHILD IAT BINDING MISMATCH iat_rva=0x{:08x} pe_module=\"{}\" pe_{} provider_module=\"{}\" provider_{} provider_id={} target=0x{:08x}",
                    IAT_RVA,
                    import.module,
                    import_symbol,
                    provider.module,
                    provider_symbol,
                    provider_id,
                    target,
                ),
            );
            return Err("child IAT binding mismatch".into());
        }
    }
    logl::log(level::IMPORTANT, format_args!("{detail}"));
    Ok(())
}

fn pe_import_symbol_label(symbol: &pe32::ImportSymbol) -> String {
    match symbol {
        pe32::ImportSymbol::Name(name) => format!("symbol=\"{}\"", name),
        pe32::ImportSymbol::Ordinal(ordinal) => format!("ordinal={}", ordinal),
    }
}

fn provider_symbol_label(symbol: &child_loader::ProviderSymbol) -> String {
    match symbol {
        child_loader::ProviderSymbol::Name(name) => format!("symbol=\"{}\"", name),
        child_loader::ProviderSymbol::Ordinal(ordinal) => format!("ordinal={}", ordinal),
    }
}

fn provider_matches_import(
    provider: &child_loader::ProviderImport,
    import: &pe32::ImportDescriptor,
) -> bool {
    provider.module.eq_ignore_ascii_case(&import.module)
        && match (&provider.symbol, &import.symbol) {
            (child_loader::ProviderSymbol::Name(provider), pe32::ImportSymbol::Name(import)) => {
                provider == import
            }
            (
                child_loader::ProviderSymbol::Ordinal(provider),
                pe32::ImportSymbol::Ordinal(import),
            ) => provider == import,
            _ => false,
        }
}

fn begin_child_dll_init(child: &mut PendingChild) -> Result<(usize, u32, u32), &'static str> {
    begin_child_dll_init_state(&mut child.execution, &child.native_modules)
}

fn begin_child_dll_init_state(
    execution: &mut ChildExecutionState,
    native_modules: &[PendingNativeModule],
) -> Result<(usize, u32, u32), &'static str> {
    let ChildExecutionState::DllInitReady { native_index } = *execution else {
        return Err("child DLL init not ready");
    };
    let module = native_modules
        .get(native_index)
        .ok_or("child execution native index")?;
    let entry = module
        .image
        .image_base
        .checked_add(module.image.entry_rva)
        .ok_or("child DLL entry")?;
    *execution = ChildExecutionState::DllInitRunning { native_index };
    Ok((native_index, module.image.image_base, entry))
}

fn map_child_image(address_space: &AddressSpace, image: &pe32::PeImage) -> Result<(), String> {
    address_space
        .map(
            image.image_base,
            image.image.len(),
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| format!("map child image: {error}"))?;
    let written = address_space
        .write(image.image_base, &image.image)
        .map_err(|error| format!("write child image: {error}"))?;
    if written != image.image.len() {
        return Err("short child image write".into());
    }
    Ok(())
}

fn map_child_thunks(address_space: &AddressSpace, thunks: &[u8]) -> Result<(), String> {
    address_space
        .map(
            thunk32::THUNK_BASE,
            thunks.len(),
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| format!("map child provider thunks: {error}"))?;
    let written = address_space
        .write(thunk32::THUNK_BASE, thunks)
        .map_err(|error| format!("write child provider thunks: {error}"))?;
    if written != thunks.len() {
        return Err("short child provider thunk write".into());
    }
    Ok(())
}

fn map_child_controls(address_space: &AddressSpace) -> Result<(), String> {
    let mut page = vec![0x90; 0x1000];
    thunk32::install_child_controls(&mut page).map_err(str::to_owned)?;
    address_space
        .map(
            thunk32::CHILD_CONTROL_BASE,
            page.len(),
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| format!("map child controls: {error}"))?;
    let written = address_space
        .write(thunk32::CHILD_CONTROL_BASE, &page)
        .map_err(|error| format!("write child controls: {error}"))?;
    if written != page.len() {
        return Err("short child control write".into());
    }
    Ok(())
}

fn log_native_child_image(
    requested: &str,
    stored: &str,
    image: &pe32::PeImage,
    listing: &async_fs::DirListing,
) -> Result<(), &'static str> {
    let entry_va = image
        .image_base
        .checked_add(image.entry_rva)
        .ok_or("native entry overflow")?;
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD NATIVE DLL parent=\"War3.exe\" requested=\"{}\" stored=\"{}\" image_base=0x{:08x} entry_rva=0x{:08x} entry_va=0x{:08x} size_of_image={} sections={} imports={} relocations={}",
            requested,
            stored,
            image.image_base,
            image.entry_rva,
            entry_va,
            image.size_of_image,
            image.sections.len(),
            image.imports.len(),
            image.relocations.len(),
        ),
    );
    let mut dependencies: Vec<(String, usize, usize, usize)> = Vec::new();
    for import in &image.imports {
        if let Some((_, count, named, ordinal)) = dependencies
            .iter_mut()
            .find(|(module, _, _, _)| module == &import.module)
        {
            *count += 1;
            match &import.symbol {
                pe32::ImportSymbol::Name(_) => *named += 1,
                pe32::ImportSymbol::Ordinal(_) => *ordinal += 1,
            }
        } else {
            dependencies.push((
                import.module.clone(),
                1,
                usize::from(matches!(&import.symbol, pe32::ImportSymbol::Name(_))),
                usize::from(matches!(&import.symbol, pe32::ImportSymbol::Ordinal(_))),
            ));
        }
    }
    for (index, (module, imports, named, ordinal)) in dependencies.into_iter().enumerate() {
        let local = child_loader::resolve_file(listing, &module)?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD NATIVE DLL DEPENDENCY parent=\"{}\" index={} module=\"{}\" imports={} named={} ordinal={} local={}{}",
                stored,
                index,
                module,
                imports,
                named,
                ordinal,
                usize::from(local.is_some()),
                local
                    .as_ref()
                    .map(|value| format!(" stored=\"{}\"", value))
                    .unwrap_or_default(),
            ),
        );
    }
    Ok(())
}

fn log_child_handles(session: &Wc3Session, pid: u32) {
    let Some(process) = session.process(pid) else {
        return;
    };
    logl::log(
        level::IMPORTANT,
        format_args!(
            "WC3 CHILD HANDLES pid={} count={}",
            pid,
            process.handles.len()
        ),
    );
    let mut handles: Vec<_> = process.handles.iter().collect();
    handles.sort_unstable_by_key(|(handle, _)| **handle);
    for (handle, entry) in handles {
        let kind = match session.objects.get(&entry.object) {
            Some(SessionObject::Event(_)) => "event",
            Some(SessionObject::Mutex(_)) => "mutex",
            Some(SessionObject::Process(_)) => "process",
            Some(SessionObject::Thread(_)) => "thread",
            Some(SessionObject::IoCompletionPort(_)) => "io-completion-port",
            None => "unknown",
        };
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 CHILD HANDLE handle=0x{:08x} object_id={} kind={}",
                handle, entry.object, kind
            ),
        );
    }
}

struct X86Memory<'a>(&'a AddressSpace);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ChildException {
    vector: Option<u32>,
    interruption_type: Option<u32>,
    valid: bool,
    error_valid: Option<bool>,
    error: Option<u32>,
    fault_linear: Option<u32>,
    debug_status: Option<u32>,
    name: &'static str,
}

fn decode_child_exception(detail: u32, qualification: u64) -> ChildException {
    let valid = detail & (1 << 31) != 0;
    let vector = valid.then_some(detail & 0xff);
    let error_valid = valid.then_some(detail & (1 << 11) != 0);
    ChildException {
        vector,
        interruption_type: valid.then_some((detail >> 8) & 7),
        valid,
        error_valid,
        error: error_valid.unwrap_or(false).then_some(qualification as u32),
        fault_linear: (valid && vector == Some(14)).then_some((qualification >> 32) as u32),
        debug_status: (valid && vector == Some(1)).then_some(qualification as u32),
        name: vector
            .map(child_exception_name)
            .unwrap_or("invalid-interruption-info"),
    }
}

fn child_exception_fault_detail(exception: ChildException) -> String {
    if exception.vector != Some(14) {
        return "linear=-".into();
    }
    let error = exception.error.unwrap_or(0);
    let linear = exception.fault_linear.unwrap_or(0);
    format!(
        "linear=0x{linear:08x} error=0x{error:08x} present={} write={} user={} reserved={} instruction_fetch={}",
        error & 1,
        (error >> 1) & 1,
        (error >> 2) & 1,
        (error >> 3) & 1,
        (error >> 4) & 1,
    )
}

fn seh_registration_head(address_space: &AddressSpace, fs_base: u32) -> String {
    let mut bytes = [0; 4];
    match address_space.read(fs_base, &mut bytes) {
        Ok(4) => format!("0x{:08x}", u32::from_le_bytes(bytes)),
        _ => "<unreadable>".into(),
    }
}

fn child_exception_name(vector: u32) -> &'static str {
    match vector {
        0 => "#DE",
        1 => "#DB",
        3 => "#BP",
        4 => "#OF",
        5 => "#BR",
        6 => "#UD",
        7 => "#NM",
        8 => "#DF",
        10 => "#TS",
        11 => "#NP",
        12 => "#SS",
        13 => "#GP",
        14 => "#PF",
        16 => "#MF",
        17 => "#AC",
        19 => "#XM",
        _ => "unknown",
    }
}

fn exception_code_window(address_space: &AddressSpace, eip: u32) -> String {
    let mut bytes = [0; 32];
    match address_space.read(eip, &mut bytes) {
        Ok(32) => bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" "),
        _ => "<unreadable>".into(),
    }
}

use wc3::process::read_guest_words;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct VirtualAllocFrame {
    caller_ret: u32,
    address: u32,
    size: u32,
    allocation_type: u32,
    protect: u32,
}

fn decode_virtual_alloc_frame(
    memory: &impl GuestMemory,
    esp: u32,
) -> Result<VirtualAllocFrame, String> {
    let frame = read_guest_words(memory, esp, 5)?;
    Ok(VirtualAllocFrame {
        caller_ret: frame[0],
        address: frame[1],
        size: frame[2],
        allocation_type: frame[3],
        protect: frame[4],
    })
}

fn virtual_alloc_protect_name(protect: u32) -> &'static str {
    match protect {
        0x01 => "PAGE_NOACCESS",
        0x02 => "PAGE_READONLY",
        0x04 => "PAGE_READWRITE",
        0x08 => "PAGE_WRITECOPY",
        0x10 => "PAGE_EXECUTE",
        0x20 => "PAGE_EXECUTE_READ",
        0x40 => "PAGE_EXECUTE_READWRITE",
        0x80 => "PAGE_EXECUTE_WRITECOPY",
        _ => "UNKNOWN",
    }
}

fn read_guest_bytes(
    memory: &impl GuestMemory,
    address: u32,
    count: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0; count];
    memory.read(address, &mut bytes).map_err(str::to_owned)?;
    Ok(bytes)
}

fn diagnostic_ansi_string(memory: &impl GuestMemory, address: u32) -> Result<String, String> {
    let mut bytes = Vec::new();
    for offset in 0..256u32 {
        let Some(current) = address.checked_add(offset) else {
            return Err(format!("address overflow at +0x{offset:x}"));
        };
        let mut byte = [0];
        if let Err(error) = memory.read(current, &mut byte) {
            return Err(error.to_owned());
        }
        if byte[0] == 0 {
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        bytes.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn hex_digest(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

impl GuestMemory for X86Memory<'_> {
    fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
        match self.0.read(address, output) {
            Ok(read) if read == output.len() => Ok(()),
            _ => Err("x86 guest read"),
        }
    }

    fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
        match self.0.write(address, input) {
            Ok(written) if written == input.len() => Ok(()),
            _ => Err("x86 guest write"),
        }
    }
}

#[cfg(test)]
crate::wc3_main_tests_1!();
