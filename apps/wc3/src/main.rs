use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use trueos::{
    async_fs,
    logl::{self, level},
    ui4_scene::{self, Damage, Frame, rgba},
    x86::{AddressSpace, Context, ExitKind, Permissions, Registers},
};
use wc3::{
    EXPECTED_SHA256, LAUNCHER_PATH,
    imports::WinCall,
    pe32,
    process::{
        GuestMemory, PreparedProcess, STACK_BASE, STACK_BYTES, STACK_TOP, TEB_VA, ThreadObject,
        bmp_file_from_dib, dib_layout,
    },
    session::{
        GuestCall, LAUNCHER_PID, LAUNCHER_TID, PersonalityAction, SessionRequest,
        WINDOW_HANDLE_BASE, Wc3Session, WindowPresentation,
    },
    thunk32,
};

fn main() {
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
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
        logl::log(level::ERROR, format_args!("wc3: {error}"));
    }
}

async fn run() -> Result<(), String> {
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
        tid: LAUNCHER_TID,
        context,
        started: false,
        continuation: None,
    }];
    let mut child_runtime: Option<RuntimeProcess> = None;
    let mut thread_calls: HashMap<(u32, u32), u32> = HashMap::new();
    let mut default_proc_messages = HashSet::new();
    let mut frames: HashMap<u32, Frame> = HashMap::new();
    let mut active = 0usize;
    loop {
        let exit = if contexts[active].started {
            contexts[active].context.resume().await
        } else {
            contexts[active].started = true;
            contexts[active].context.run().await
        }
        .map_err(|error| error.to_string())?;
        match exit.kind {
            // A transient VMCS always starts with VMLAUNCH.  Its preemption
            // timer is therefore a Blueprint scheduling boundary, not an x86
            // program stop: Context::resume() restores the logical context
            // into a fresh VMCS on whichever Tokio carrier runs next.
            ExitKind::Other if exit.detail == 52 => {
                tokio::task::yield_now().await;
                continue;
            }
            ExitKind::VmCall => {
                if exit.registers.eip == thunk32::THREAD_EXIT_AFTER_VMCALL {
                    let exited = contexts.remove(active);
                    if exited.tid != LAUNCHER_TID {
                        session.signal_thread(
                            wc3::session::ThreadKey {
                                pid: LAUNCHER_PID,
                                tid: exited.tid,
                            },
                            exit.registers.eax,
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 TID2 EXIT pid={} tid={} code=0x{:08x}",
                                LAUNCHER_PID, exited.tid, exit.registers.eax
                            ),
                        );
                        return Ok(());
                    }
                    session
                        .launcher_mut()
                        .xp
                        .exit_thread(exited.tid, exit.registers.eax)
                        .map_err(str::to_owned)?;
                    logl::log(
                        level::INFO,
                        format_args!(
                            "wc3: x86 ThreadProc exited tid={} code=0x{:08x}",
                            exited.tid, exit.registers.eax,
                        ),
                    );
                    if contexts.is_empty() {
                        return Ok(());
                    }
                    active %= contexts.len();
                    continue;
                }
                if exit.registers.eip == thunk32::GUEST_RETURN_AFTER_VMCALL {
                    let continuation = contexts[active]
                        .continuation
                        .take()
                        .ok_or_else(|| "guest return without continuation".to_owned())?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CALL_GUEST RETURN tid={} hwnd=0x{:08x} message=0x{:08x} wndproc=0x{:08x} result=0x{:08x}",
                            contexts[active].tid,
                            continuation.hwnd,
                            continuation.message,
                            continuation.wndproc,
                            exit.registers.eax
                        ),
                    );
                    if contexts[active].tid == 1 && continuation.hwnd == WINDOW_HANDLE_BASE {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CALL_GUEST RETURN tid=1 hwnd=0x{:08x} wndproc=0x{:08x} message=WM_PAINT result=0x{:08x}",
                                continuation.hwnd, continuation.wndproc, exit.registers.eax
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 ROOT WM_PAINT RETURN hwnd=0x{:08x}",
                                continuation.hwnd
                            ),
                        );
                    }
                    let mut registers = exit.registers;
                    registers.eip = continuation.import_resume_eip;
                    registers.esp = continuation.import_esp;
                    registers.eax = continuation.completion_eax;
                    contexts[active]
                        .context
                        .set_registers(registers)
                        .map_err(|error| error.to_string())?;
                    continue;
                }
                let import_id = exit.registers.eax;
                let import = session
                    .launcher()
                    .xp
                    .import(import_id)
                    .cloned()
                    .ok_or_else(|| format!("unknown import trap id={import_id}"))?;
                let thread_call = thread_calls
                    .entry((LAUNCHER_PID, contexts[active].tid))
                    .and_modify(|call| *call += 1)
                    .or_insert(1);
                let sequence = session.note();
                let process_call = session.launcher().xp.call_count + 1;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3[seq={sequence} p=launcher pid={} tid={} pcall={process_call} tcall={}] {}!{}",
                        LAUNCHER_PID,
                        contexts[active].tid,
                        *thread_call,
                        import.module,
                        import.symbol
                    ),
                );
                let call_kind = WinCall::from_import(&import);
                if call_kind == WinCall::BitBlt {
                    let frame = read_guest_words(&memory, exit.registers.esp, 10)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 BitBlt ret=0x{:08x} dst=0x{:08x} dst_xy={},{} size={}x{} src=0x{:08x} src_xy={},{} rop=0x{:08x}",
                            frame[0],
                            frame[1],
                            frame[2],
                            frame[3],
                            frame[4],
                            frame[5],
                            frame[6],
                            frame[7],
                            frame[8],
                            frame[9]
                        ),
                    );
                }
                if call_kind == WinCall::DefWindowProcA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                    if default_proc_messages.insert(frame[2]) {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 DefWindowProcA hwnd=0x{:08x} message=0x{:08x} wparam=0x{:08x} lparam=0x{:08x}",
                                frame[1], frame[2], frame[3], frame[4]
                            ),
                        );
                    }
                }
                if call_kind == WinCall::RegisterClassA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                    let fields = read_guest_words(&memory, frame[1], 10)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC0 tid={} ptr={:#010x}",
                            contexts[active].tid, frame[1]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC1 style={:#010x} wndproc={:#010x}",
                            fields[0], fields[1]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 RC2 cls_extra={} wnd_extra={}", fields[2], fields[3]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC3 instance={:#010x} icon={:#010x}",
                            fields[4], fields[5]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC4 cursor={:#010x} background={:#010x}",
                            fields[6], fields[7]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 RC5 menu_ptr={:#010x} class_ptr={:#010x}",
                            fields[8], fields[9]
                        ),
                    );
                    if fields[9] != 0 && fields[9] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, fields[9]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 RCCLASS {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 RCCLASS decode-failed ptr={:#010x}", fields[9]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 RCCLASS atom=0x{:04x}", fields[9] & 0xffff),
                        );
                    }
                    if fields[8] == 0 {
                        logl::log(level::IMPORTANT, format_args!("WC3 RCMENU <null>"));
                    } else if fields[8] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, fields[8]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 RCMENU {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 RCMENU decode-failed ptr={:#010x}", fields[8]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 RCMENU atom=0x{:04x}", fields[8] & 0xffff),
                        );
                    }
                }
                if call_kind == WinCall::CreateWindowExA {
                    let a = read_guest_words(&memory, exit.registers.esp, 13)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CW0 tid={} esp={:#010x} ret={:#010x}",
                            contexts[active].tid, exit.registers.esp, a[0]
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW1 ex={:#010x} style={:#010x}", a[1], a[4]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW2 class_ptr={:#010x} title_ptr={:#010x}", a[2], a[3]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW3 x={} y={}", a[5] as i32, a[6] as i32),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW4 width={} height={}", a[7], a[8]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW5 parent={:#010x} menu={:#010x}", a[9], a[10]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CW6 instance={:#010x} param={:#010x}", a[11], a[12]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWA a0..a4={:?}", &a[0..5]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWB a5..a8={:?}", &a[5..9]),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!("WC3 CWC a9..a12={:?}", &a[9..13]),
                    );
                    if a[2] != 0 && a[2] >> 16 != 0 {
                        match diagnostic_ansi_string(&memory, a[2]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 CWCLASS {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 CWCLASS decode-failed ptr={:#010x}", a[2]),
                            ),
                        }
                    } else {
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 CWCLASS atom=0x{:04x}", a[2] & 0xffff),
                        );
                    }
                    if a[3] == 0 {
                        logl::log(level::IMPORTANT, format_args!("WC3 CWTITLE <null>"));
                    } else {
                        match diagnostic_ansi_string(&memory, a[3]) {
                            Ok(value) => {
                                logl::log(level::IMPORTANT, format_args!("WC3 CWTITLE {:?}", value))
                            }
                            Err(_) => logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 CWTITLE decode-failed ptr={:#010x}", a[3]),
                            ),
                        }
                    }
                }
                if contexts[active].tid == 2 && import.symbol == "TlsSetValue" {
                    let raw = read_guest_words(&memory, exit.registers.esp, 3)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 TID2 TlsSetValue esp=0x{:08x} return_address=0x{:08x} slot={} value=0x{:08x}",
                            exit.registers.esp, raw[0], raw[1], raw[2]
                        ),
                    );
                }
                if import.symbol == "DrawTextA" {
                    match read_guest_words(&memory, exit.registers.esp, 6) {
                        Ok(a) => {
                            logl::log(
                                level::IMPORTANT,
                                format_args!("WC3 DT0 ret=0x{:08x} hdc=0x{:08x}", a[0], a[1]),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DT1 text_ptr=0x{:08x} count={}",
                                    a[2], a[3] as i32
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DT2 rect_ptr=0x{:08x} format=0x{:08x}",
                                    a[4], a[5]
                                ),
                            );
                            if (a[3] as i32) >= 0 {
                                match read_guest_bytes(&memory, a[2], a[3] as usize) {
                                    Ok(bytes) => {
                                        let text = diagnostic_cp1252(&bytes);
                                        let digest = Sha256::digest(&bytes);
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 DTTEXT {:?} bytes_sha256={}",
                                                text,
                                                hex_digest(&digest)
                                            ),
                                        );
                                    }
                                    Err(error) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTTEXT decode-failed ptr=0x{:08x} count={} error={}",
                                            a[2], a[3], error
                                        ),
                                    ),
                                }
                            }
                            if a[4] != 0 {
                                match read_guest_words(&memory, a[4], 4) {
                                    Ok(rect) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTRECT in=[{},{},{},{}]",
                                            rect[0] as i32,
                                            rect[1] as i32,
                                            rect[2] as i32,
                                            rect[3] as i32
                                        ),
                                    ),
                                    Err(error) => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 DTRECT decode-failed ptr=0x{:08x} error={}",
                                            a[4], error
                                        ),
                                    ),
                                }
                            }
                            let flags = a[5];
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DTFLAGS raw=0x{:08x} center={} vcenter={} wordbreak={} singleline={} calcrect={} noprefix={}",
                                    flags,
                                    u32::from(flags & 0x0001 != 0),
                                    u32::from(flags & 0x0004 != 0),
                                    u32::from(flags & 0x0010 != 0),
                                    u32::from(flags & 0x0020 != 0),
                                    u32::from(flags & 0x0400 != 0),
                                    u32::from(flags & 0x0800 != 0)
                                ),
                            );
                        }
                        Err(error) => logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 DT0 decode-failed esp=0x{:08x} error={}",
                                exit.registers.esp, error
                            ),
                        ),
                    }
                }
                if WinCall::from_import(&import) == WinCall::LoadImageA {
                    let raw = read_guest_words(&memory, exit.registers.esp, 7)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 LoadImageA RAW esp=0x{:08x} ret=0x{:08x} module=0x{:08x} name=0x{:08x} type={} cx={} cy={} flags=0x{:08x}",
                            exit.registers.esp,
                            raw[0],
                            raw[1],
                            raw[2],
                            raw[3],
                            raw[4],
                            raw[5],
                            raw[6]
                        ),
                    );
                }
                if WinCall::from_import(&import) == WinCall::Unsupported {
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 TID2 UNSUPPORTED module={} symbol={} esp=0x{:08x} eip=0x{:08x} stack[0..16 dwords]={:?}",
                            import.module,
                            import.symbol,
                            exit.registers.esp,
                            exit.registers.eip,
                            read_guest_words(&memory, exit.registers.esp, 16)?
                        ),
                    );
                    return Ok(());
                }
                let result = session.launcher().xp.selected_bitmap(
                    if WinCall::from_import(&import) == WinCall::DeleteDC {
                        read_guest_words(&memory, exit.registers.esp, 2)?[1]
                    } else {
                        0
                    },
                );
                let delete_dc_selected = (WinCall::from_import(&import) == WinCall::DeleteDC)
                    .then_some((read_guest_words(&memory, exit.registers.esp, 2)?[1], result));
                let delete_object_before = if WinCall::from_import(&import) == WinCall::DeleteObject
                {
                    let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                    Some((
                        frame[1],
                        session.launcher().xp.bitmap_info(frame[1]),
                        session.launcher().xp.bitmap_stock(frame[1]),
                        session.launcher().xp.bitmap_selected_in_dc(frame[1]),
                    ))
                } else {
                    None
                };
                let result = session
                    .launcher_mut()
                    .xp
                    .dispatch_for_process(
                        LAUNCHER_PID,
                        contexts[active].tid,
                        import_id,
                        exit.registers.esp,
                        &mut memory,
                    )
                    .map_err(|error| {
                        format!(
                            "call #{} {}!{}: {error}",
                            session.launcher().xp.call_count,
                            import.module,
                            import.symbol
                        )
                    })?;
                let mut callback = None;
                let result = match result {
                    PersonalityAction::Return(value) => {
                        if let Some((handle, Some(info), Some(stock), selected_in_dc)) =
                            delete_object_before
                        {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteObject handle=0x{:08x} kind=BITMAP stock={} selected_in_dc={} bits_va=0x{:08x} bits_len={}",
                                    handle,
                                    u32::from(stock),
                                    u32::from(selected_in_dc),
                                    info.bits_va,
                                    info.bits_len
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteObject complete handle=0x{:08x} bitmap_live={} palette_0x57437003_live={} stock_bitmap_live={}",
                                    handle,
                                    u32::from(session.launcher().xp.gdi_live(handle)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7003)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7f01))
                                ),
                            );
                        }
                        if let Some((hdc, Some(selected))) = delete_dc_selected {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteDC hdc=0x{:08x} kind=MEMORY_DC selected_bitmap=0x{:08x}",
                                    hdc, selected
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 DeleteDC complete hdc=0x{:08x} remaining_bitmap_0x57437001={} remaining_stock_bitmap={} ",
                                    hdc,
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7001)),
                                    u32::from(session.launcher().xp.gdi_live(0x5743_7f01))
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::CreatePalette && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let header = read_guest_bytes(&memory, frame[1], 4)?;
                            let entries = u16::from_le_bytes([header[2], header[3]]) as usize;
                            let exact = read_guest_bytes(&memory, frame[1] + 4, entries * 4)?;
                            let digest = Sha256::digest(&exact);
                            let allocation =
                                session.launcher().xp.allocation_size(frame[1]).unwrap_or(0);
                            let version = u16::from_le_bytes([header[0], header[1]]);
                            let entry0 = exact.get(..4).unwrap_or(&[]);
                            let entry255 = exact.get(255 * 4..256 * 4).unwrap_or(&[]);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CreatePalette LOGPALETTE ptr=0x{:08x} version=0x{:04x} entries={} required_bytes={} allocation_bytes={} entry0={:?} entry255={:?} palette_sha256={} hpalette=0x{:08x}",
                                    frame[1],
                                    version,
                                    entries,
                                    4 + entries * 4,
                                    allocation,
                                    entry0,
                                    entry255,
                                    hex_digest(&digest),
                                    value
                                ),
                            );
                            if let Some((stored_version, stored_entries)) =
                                session.launcher().xp.palette_info(value)
                            {
                                logl::log(
                                    level::INFO,
                                    format_args!(
                                        "wc3: logical palette admitted version=0x{:04x} entries={}",
                                        stored_version, stored_entries
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::GetDIBColorTable {
                            let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                            if value != 0 {
                                let bytes =
                                    read_guest_bytes(&memory, frame[4], value as usize * 4)?;
                                let digest = Sha256::digest(&bytes);
                                if let Some(bitmap) =
                                    session.launcher().xp.selected_bitmap(frame[1])
                                {
                                    if let Some(info) = session.launcher().xp.bitmap_info(bitmap) {
                                        let source_offset = (info.height.unsigned_abs() as usize
                                            - 1)
                                            * info.row_stride as usize;
                                        let mut source_index = [0; 1];
                                        memory
                                            .read(
                                                info.bits_va + source_offset as u32,
                                                &mut source_index,
                                            )
                                            .map_err(str::to_owned)?;
                                        let palette_offset = source_index[0] as usize * 4;
                                        let visual = bytes
                                            .get(palette_offset..palette_offset + 3)
                                            .ok_or_else(|| {
                                                "palette index outside returned table".to_owned()
                                            })?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 GetDIBColorTable hdc=0x{:08x} bitmap=0x{:08x} start={} requested={} copied={} output=0x{:08x} entry0={:?} entry255={:?} palette_sha256={} top_left_index={} top_left_rgb=[{},{},{}]",
                                                frame[1],
                                                bitmap,
                                                frame[2],
                                                frame[3],
                                                value,
                                                frame[4],
                                                bytes.get(..4).unwrap_or(&[]),
                                                bytes.get(255 * 4..256 * 4).unwrap_or(&[]),
                                                hex_digest(&digest),
                                                source_index[0],
                                                visual[2],
                                                visual[1],
                                                visual[0]
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SelectObject {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            if let Some(info) = session.launcher().xp.bitmap_info(frame[2]) {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SelectObject hdc=0x{:08x} new=0x{:08x} old=0x{:08x} new_type=BITMAP width={} height={} bpp={} bits_va=0x{:08x}",
                                        frame[1],
                                        frame[2],
                                        value,
                                        info.width,
                                        info.height,
                                        info.bit_count,
                                        info.bits_va
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::CreateCompatibleDC {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let source = frame[1];
                            let (source_kind, source_hwnd) = if source == 0 {
                                ("DISPLAY", None)
                            } else {
                                match session.launcher().xp.dc_target(source) {
                                    Some(Some(hwnd)) => ("WINDOW_PAINT", Some(hwnd)),
                                    Some(None) => ("MEMORY_DC", None),
                                    None => ("UNKNOWN", None),
                                }
                            };
                            if let Some((selected, width, height, bpp)) =
                                session.launcher().xp.compatible_dc_info(value)
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CreateCompatibleDC source=0x{:08x} source_kind={} source_hwnd={} compatibility=DISPLAY hdc=0x{:08x} target=MEMORY selected_bitmap=0x{:08x} selected_bitmap_shape={}x{}x{}",
                                        source,
                                        source_kind,
                                        source_hwnd.map_or_else(
                                            || "-".to_owned(),
                                            |hwnd| format!("0x{hwnd:08x}")
                                        ),
                                        value,
                                        selected,
                                        width,
                                        height,
                                        bpp
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SelectPalette && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 4)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let entries =
                                session.launcher().xp.palette_entries(frame[2]).unwrap_or(0);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SelectPalette hdc=0x{:08x} target={} new=0x{:08x} old=0x{:08x} force_background={} entries={}",
                                    frame[1],
                                    target,
                                    frame[2],
                                    value,
                                    frame[3] != 0,
                                    entries
                                ),
                            );
                            if let (Some(paint_palette), Some(memory_bitmap)) = (
                                session.launcher().xp.selected_palette(0x5743_7004),
                                session.launcher().xp.selected_bitmap(0x5743_7008),
                            ) {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 palette state paint_hdc=0x57437004 selected_palette=0x{:08x} memory_hdc=0x57437008 selected_bitmap=0x{:08x}",
                                        paint_palette, memory_bitmap
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::RealizePalette {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let palette = session.launcher().xp.selected_palette(frame[1]);
                            let entries = palette
                                .and_then(|handle| session.launcher().xp.palette_entries(handle))
                                .unwrap_or(0);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 RealizePalette hdc=0x{:08x} target={} palette={} entries={} first_realization={} mapped={}",
                                    frame[1],
                                    target,
                                    palette
                                        .map_or_else(|| "-".to_owned(), |v| format!("0x{v:08x}")),
                                    entries,
                                    u32::from(value != 0 && value != u32::MAX),
                                    value
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::GetObjectA && value != 0 {
                            let frame = read_guest_words(&memory, exit.registers.esp, 4)?;
                            if let Some(info) = session.launcher().xp.bitmap_info(frame[1]) {
                                let mut first = [0; 1];
                                memory
                                    .read(info.bits_va, &mut first)
                                    .map_err(str::to_owned)?;
                                let rgba = &info.rgba[..4];
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GetObjectA BITMAP hbitmap=0x{:08x} buffer=0x{:08x} bytes={} width={} height={} width_bytes={} planes={} bpp={} bits_va=0x{:08x} bits_len={} first_index={} first_visual_rgba=[{},{},{},{}]",
                                        frame[1],
                                        frame[3],
                                        frame[2],
                                        info.width,
                                        info.height,
                                        info.row_stride,
                                        info.planes,
                                        info.bit_count,
                                        info.bits_va,
                                        info.bits_len,
                                        first[0],
                                        rgba[0],
                                        rgba[1],
                                        rgba[2],
                                        rgba[3]
                                    ),
                                );
                            }
                        }
                        value
                    }
                    PersonalityAction::Session(SessionRequest::LoadImage(request)) => {
                        let layout = dib_layout(&request.dib).map_err(str::to_owned)?;
                        let bits_va = session
                            .launcher_mut()
                            .xp
                            .allocate_gdi_bits(layout.bits_len)
                            .map_err(str::to_owned)?;
                        let mapped_len = (layout.bits_len + 0xfff) & !0xfff;
                        address_space
                            .map(bits_va, mapped_len, Permissions::READ | Permissions::WRITE)
                            .map_err(|error| format!("map DIB bits: {error}"))?;
                        let bits_end = layout
                            .pixel_offset
                            .checked_add(layout.bits_len)
                            .ok_or_else(|| "DIB bits range overflow".to_owned())?;
                        address_space
                            .write(bits_va, &request.dib[layout.pixel_offset..bits_end])
                            .map_err(|error| format!("write DIB bits: {error}"))?;
                        let bmp = bmp_file_from_dib(&request.dib).map_err(str::to_owned)?;
                        let decoded = match trueos::vmedia::decode(
                            trueos::vmedia::ImageFormat::Bmp,
                            &bmp,
                        )
                        .await
                        {
                            Ok(decoded) => decoded,
                            Err(error) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 BITMAP DECODE_REJECTED resource={} width={} height={} planes={} bpp={} compression={} dib_bytes={} decoder_error={}",
                                        request.resource_id,
                                        request.width,
                                        request.height,
                                        request.planes,
                                        request.bit_count,
                                        request.compression,
                                        request.dib.len(),
                                        error
                                    ),
                                );
                                return Ok(());
                            }
                        };
                        let value = session
                            .launcher_mut()
                            .xp
                            .admit_bitmap(request, decoded.rgba, bits_va, layout)
                            .map_err(str::to_owned)?;
                        let info = session
                            .launcher()
                            .xp
                            .bitmap_info(value)
                            .ok_or_else(|| "admitted bitmap disappeared".to_owned())?;
                        let rgba_top_left = info
                            .rgba
                            .get(..4)
                            .ok_or_else(|| "decoded bitmap has no top-left pixel".to_owned())?;
                        let digest = Sha256::digest(&info.rgba);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BITMAP resource={} width={} height={} planes={} bpp={} compression={} dib_bytes={} rgba_bytes={} rgba_top_left=[{},{},{},{}] rgba_sha256={} hbitmap=0x{:08x}",
                                info.resource_id,
                                info.width,
                                info.height,
                                info.planes,
                                info.bit_count,
                                info.compression,
                                info.dib_bytes,
                                info.rgba.len(),
                                rgba_top_left[0],
                                rgba_top_left[1],
                                rgba_top_left[2],
                                rgba_top_left[3],
                                hex_digest(&digest),
                                value
                            ),
                        );
                        value
                    }
                    PersonalityAction::WindowBlit(request) => {
                        let digest = Sha256::digest(&request.rgba);
                        let frame = frames
                            .get_mut(&request.hwnd)
                            .ok_or_else(|| "BitBlt destination frame missing".to_owned())?;
                        frame
                            .begin(rgba(0, 0, 0, 255))
                            .and_then(|()| frame.write_opaque_rgba8(&request.rgba))
                            .and_then(|()| {
                                frame.publish(Damage::full(request.width, request.height))
                            })
                            .map_err(|error| format!("publish WC3 BitBlt: {error:?}"))?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BitBlt SRCCOPY dst_hdc=0x{:08x} dst_hwnd=0x{:08x} dst=[{},{} {}x{}] src_hdc=0x{:08x} src_bitmap=0x{:08x} src=[0,0] rop=0x00cc0020 bits_va=0x{:08x} bpp=8 bottom_up={} rgba_bytes={} rgba_sha256={}",
                                request.dst_hdc,
                                request.hwnd,
                                request.dst_x,
                                request.dst_y,
                                request.width,
                                request.height,
                                request.src_hdc,
                                request.source_bitmap,
                                request.bits_va,
                                u32::from(request.bottom_up),
                                request.rgba.len(),
                                hex_digest(&digest)
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 BLIT hwnd=0x{:08x} width={} height={} source_bitmap=0x{:08x} published=1",
                                request.hwnd, request.width, request.height, request.source_bitmap
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 UI4 BLIT LIVE confirm=anykey"),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 UI4 BLIT STOPPED awaiting operator input"),
                        );
                        // Keep the local Frame alive after the visual checkpoint. Returning
                        // from run() would drop `frames` and close the UI4 surface.
                        std::future::pending::<()>().await;
                        unreachable!("WC3 UI4 checkpoint future unexpectedly completed");
                    }
                    PersonalityAction::Session(SessionRequest::CreateProcess(request)) => {
                        let frame = request.frame;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BLUEPRINT FRONTIER: CreateProcessA call #{} esp=0x{:08x} ret=0x{:08x} command_line=0x{:08x} startup=0x{:08x} process_info=0x{:08x}",
                                session.launcher().xp.call_count,
                                exit.registers.esp,
                                frame.return_address,
                                frame.command_line,
                                frame.startup_info,
                                frame.process_information,
                            ),
                        );
                        let child_bytes = async_fs::read_file(b"/common/Warcraft III/War3.exe")
                            .await
                            .map_err(|error| {
                                format!(
                                    "read /common/Warcraft III/War3.exe: TRUEOSFS error {error}"
                                )
                            })?;
                        let child = pe32::parse(&child_bytes).map_err(str::to_owned)?;
                        let created = session.create_child();
                        memory
                            .write(
                                frame.process_information,
                                &created.process_handle.to_le_bytes(),
                            )
                            .map_err(str::to_owned)?;
                        memory
                            .write(
                                frame.process_information + 4,
                                &created.thread_handle.to_le_bytes(),
                            )
                            .map_err(str::to_owned)?;
                        memory
                            .write(frame.process_information + 8, &created.pid.to_le_bytes())
                            .map_err(str::to_owned)?;
                        memory
                            .write(frame.process_information + 12, &created.tid.to_le_bytes())
                            .map_err(str::to_owned)?;
                        child_runtime = Some(create_child_runtime(&child)?);
                        logl::log(
                            level::INFO,
                            format_args!(
                                "wc3: CreateProcessA succeeded pid={} tid={} process_handle=0x{:08x} thread_handle=0x{:08x}",
                                created.pid,
                                created.tid,
                                created.process_handle,
                                created.thread_handle
                            ),
                        );
                        1
                    }
                    PersonalityAction::Session(SessionRequest::CreateEvent(request)) => {
                        let (handle, already_exists) = session.create_event(LAUNCHER_PID, request);
                        session.launcher_mut().xp.set_last_error(if already_exists {
                            183
                        } else {
                            0
                        });
                        handle
                    }
                    PersonalityAction::Session(SessionRequest::CreateWindow(request)) => {
                        let hwnd = session
                            .create_window(request.clone())
                            .map_err(str::to_owned)?;
                        let window = session.windows.get(&hwnd).ok_or("created window missing")?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CreateWindowExA hwnd=0x{:08x} owner=pid{}/tid{} class={:?} title={:?} wndproc=0x{:08x} geometry={},{} {}x{} style=0x{:08x} ex_style=0x{:08x} visible={} ui4_frame=0",
                                hwnd,
                                window.owner.pid,
                                window.owner.tid,
                                window.class,
                                window.title,
                                window.wndproc,
                                window.x,
                                window.y,
                                window.width,
                                window.height,
                                window.style,
                                window.ex_style,
                                u32::from(window.visible)
                            ),
                        );
                        hwnd
                    }
                    PersonalityAction::Session(SessionRequest::ShowWindow {
                        pid: _,
                        hwnd,
                        show,
                    }) => session.show_window(hwnd, show).map_err(str::to_owned)?,
                    PersonalityAction::Session(SessionRequest::UpdateWindow { pid: _, hwnd }) => {
                        let pending = session.update_window(hwnd).map_err(str::to_owned)?;
                        if pending != 0 {
                            let window = session.windows.get(&hwnd).ok_or("window disappeared")?;
                            callback = Some(GuestCall {
                                address: window.wndproc,
                                arguments: [hwnd, 0x000f, 0, 0],
                                completion_eax: 1,
                            });
                        }
                        pending
                    }
                    PersonalityAction::Session(SessionRequest::BeginPaint {
                        pid,
                        hwnd,
                        paint_struct,
                    }) => {
                        let (width, height) = session
                            .begin_paint_window(pid, hwnd)
                            .map_err(str::to_owned)?;
                        let hdc = session
                            .process_mut(pid)
                            .ok_or("BeginPaint process missing")?
                            .xp
                            .begin_paint(hwnd, paint_struct, width, height, &mut memory)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BeginPaint hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target=WINDOW client={}x{} rcPaint=[0,0,{},{}] erase=0",
                                hwnd, paint_struct, hdc, width, height, width, height
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 PAINT HANDLES hwnd=0x{:08x} hdc=0x{:08x} distinct={}",
                                hwnd,
                                hdc,
                                u32::from(hwnd != hdc)
                            ),
                        );
                        hdc
                    }
                    PersonalityAction::Session(SessionRequest::SetFocus { pid: _, hwnd }) => {
                        session.set_focus(hwnd).map_err(str::to_owned)?
                    }
                    PersonalityAction::Session(SessionRequest::CloseHandle { pid, handle }) => {
                        if session.close_handle(pid, handle) {
                            1
                        } else {
                            session.launcher_mut().xp.set_last_error(6);
                            0
                        }
                    }
                    PersonalityAction::Block(request) => {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 RAW #90 WaitForMultipleObjects esp=0x{:08x} ret=0x{:08x} count={} handles_ptr=0x{:08x} wait_all={} timeout=0x{:08x}",
                                exit.registers.esp,
                                request.return_address,
                                request.count,
                                request.handles_pointer,
                                request.wait_all,
                                request.timeout,
                            ),
                        );
                        if request.handles_pointer != 0 && request.count <= 1024 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 RAW #90 handles handle0=0x{:08x} handle1=0x{:08x}",
                                    request.handles[0], request.handles[1],
                                ),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 BLUEPRINT FRONTIER: WaitForMultipleObjects process=launcher pid={} tid={} call=#{} ret=0x{:08x} count={} handles_ptr=0x{:08x} handle0=0x{:08x} handle1=0x{:08x} wait_all={} timeout=0x{:08x}",
                                LAUNCHER_PID,
                                request.key.tid,
                                session.launcher().xp.call_count,
                                request.return_address,
                                request.count,
                                request.handles_pointer,
                                request.handles[0],
                                request.handles[1],
                                request.wait_all,
                                request.timeout,
                            ),
                        );
                        logl::log(
                            level::INFO,
                            format_args!(
                                "wc3: wait handle0 {}",
                                session.describe_handle(LAUNCHER_PID, request.handles[0])
                            ),
                        );
                        logl::log(
                            level::INFO,
                            format_args!(
                                "wc3: wait handle1 {}",
                                session.describe_handle(LAUNCHER_PID, request.handles[1])
                            ),
                        );
                        if request.key.tid != LAUNCHER_TID {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 TID2 BLOCK pid={} tid={} pcall={} tcall={}",
                                    request.key.pid, request.key.tid, process_call, *thread_call
                                ),
                            );
                            return Ok(());
                        }
                        session.block_wait(request.clone()).map_err(str::to_owned)?;
                        let next = session
                            .runnable
                            .pop_front()
                            .ok_or_else(|| "no runnable thread after launcher block".to_owned())?;
                        active = contexts
                            .iter()
                            .position(|context| context.tid == next.tid)
                            .ok_or_else(|| {
                                format!("no runtime context for pid={} tid={}", next.pid, next.tid)
                            })?;
                        continue;
                    }
                    PersonalityAction::CallGuest(call) => {
                        callback = Some(call);
                        0
                    }
                    PersonalityAction::ExitThread(_) => {
                        if contexts[active].tid != LAUNCHER_TID {
                            return Ok(());
                        }
                        return Err(
                            "wc3: ExitThread action is not wired into the launcher loop".into()
                        );
                    }
                    PersonalityAction::ExitProcess(_) => {
                        if contexts[active].tid != LAUNCHER_TID {
                            return Ok(());
                        }
                        return Err(
                            "wc3: ExitProcess action is not wired into the launcher loop".into(),
                        );
                    }
                };
                if let Some(call) = callback {
                    let callback_esp = exit
                        .registers
                        .esp
                        .checked_sub(20)
                        .ok_or("callback stack underflow")?;
                    for (offset, value) in [
                        (0, thunk32::GUEST_RETURN_ADDRESS),
                        (4, call.arguments[0]),
                        (8, call.arguments[1]),
                        (12, call.arguments[2]),
                        (16, call.arguments[3]),
                    ] {
                        memory
                            .write(callback_esp + offset, &value.to_le_bytes())
                            .map_err(str::to_owned)?;
                    }
                    contexts[active].continuation = Some(GuestContinuation {
                        import_resume_eip: exit.registers.eip,
                        import_esp: exit.registers.esp,
                        completion_eax: call.completion_eax,
                        wndproc: call.address,
                        hwnd: call.arguments[0],
                        message: call.arguments[1],
                    });
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CALL_GUEST tid={} reason=UpdateWindow/WM_PAINT hwnd=0x{:08x} wndproc=0x{:08x} message=0x{:08x} wparam=0x{:08x} lparam=0x{:08x}",
                            contexts[active].tid,
                            call.arguments[0],
                            call.address,
                            call.arguments[1],
                            call.arguments[2],
                            call.arguments[3]
                        ),
                    );
                    if let Some(window) = session.windows.get(&call.arguments[0]) {
                        if call.arguments[0] == WINDOW_HANDLE_BASE && call.arguments[1] == 0x000f {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 UI4 ROOT WM_PAINT ENTER hwnd=0x{:08x}",
                                    call.arguments[0]
                                ),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!("WC3 UI4 PAINT BEGIN hwnd=0x{:08x}", call.arguments[0]),
                        );
                        let _ = window;
                    }
                    let mut registers = exit.registers;
                    registers.eip = call.address;
                    registers.esp = callback_esp;
                    contexts[active]
                        .context
                        .set_registers(registers)
                        .map_err(|error| error.to_string())?;
                    continue;
                }
                let mut registers = exit.registers;
                registers.eax = result;
                contexts[active]
                    .context
                    .set_registers(registers)
                    .map_err(|error| error.to_string())?;
                logl::log(
                    level::INFO,
                    format_args!(
                        "wc3: return #{} {}!{} eax=0x{result:08x}",
                        session.launcher().xp.call_count,
                        import.module,
                        import.symbol,
                    ),
                );
                if let Some(thread) = session.absorb_runnable_thread() {
                    contexts.push(create_thread_context(&address_space, &thread)?);
                }
                if let Some(request) = session.take_window_presentation() {
                    present_window(request, &mut frames, &session)?;
                }
                if contexts[active].tid == LAUNCHER_TID {
                    active = 0;
                }
            }
            ExitKind::Halted => {
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
                return Err(format!(
                    "x86 context stopped: kind={kind:?} detail={} qualification=0x{:x} eip=0x{:08x}",
                    exit.detail, exit.qualification, exit.registers.eip,
                ));
            }
        }
    }
}

fn present_window(
    request: WindowPresentation,
    frames: &mut HashMap<u32, Frame>,
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
        WindowPresentation::Hide { hwnd } => {
            frames.remove(&hwnd);
            if hwnd == WINDOW_HANDLE_BASE {
                logl::log(
                    level::IMPORTANT,
                    format_args!("WC3 UI4 ROOT CLOSE hwnd=0x{:08x}", hwnd),
                );
            }
        }
    }
    Ok(())
}

struct GuestContext {
    tid: u32,
    context: Context,
    started: bool,
    continuation: Option<GuestContinuation>,
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
        tid: thread.tid,
        context,
        started: false,
        continuation: None,
    })
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

struct RuntimeProcess {
    _address_space: AddressSpace,
    _primary: Context,
}

fn create_child_runtime(image: &pe32::PeImage) -> Result<RuntimeProcess, String> {
    let address_space = AddressSpace::create().map_err(|error| error.to_string())?;
    address_space
        .map(
            image.image_base,
            image.image.len(),
            Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
        )
        .map_err(|error| format!("map child image: {error}"))?;
    if address_space
        .write(image.image_base, &image.image)
        .map_err(|error| format!("write child image: {error}"))?
        != image.image.len()
    {
        return Err("short child image write".into());
    }
    address_space
        .map(TEB_VA, 0x1000, Permissions::READ | Permissions::WRITE)
        .map_err(|error| format!("map child TEB: {error}"))?;
    address_space
        .map(
            STACK_BASE,
            STACK_BYTES,
            Permissions::READ | Permissions::WRITE,
        )
        .map_err(|error| format!("map child stack: {error}"))?;
    let registers = Registers {
        esp: STACK_TOP,
        eip: image
            .image_base
            .checked_add(image.entry_rva)
            .ok_or("child entry overflow")?,
        eflags: 0x202,
        fs_base: TEB_VA,
        ..Registers::default()
    };
    let primary = Context::create(&address_space, registers).map_err(|error| error.to_string())?;
    Ok(RuntimeProcess {
        _address_space: address_space,
        _primary: primary,
    })
}

struct X86Memory<'a>(&'a AddressSpace);

fn read_guest_words(memory: &impl GuestMemory, esp: u32, count: usize) -> Result<Vec<u32>, String> {
    (0..count)
        .map(|index| {
            let address = esp
                .checked_add((index as u32) * 4)
                .ok_or_else(|| "guest stack address overflow".to_owned())?;
            let mut bytes = [0; 4];
            memory.read(address, &mut bytes).map_err(str::to_owned)?;
            Ok(u32::from_le_bytes(bytes))
        })
        .collect()
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

fn diagnostic_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            0x80 => '\u{20ac}',
            0x82 => '\u{201a}',
            0x83 => '\u{192}',
            0x84 => '\u{201e}',
            0x85 => '\u{2026}',
            0x86 => '\u{2020}',
            0x87 => '\u{2021}',
            0x91 => '\u{2018}',
            0x92 => '\u{2019}',
            0x93 => '\u{201c}',
            0x94 => '\u{201d}',
            0x96 => '\u{2013}',
            0x97 => '\u{2014}',
            byte => char::from(*byte),
        })
        .collect()
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
