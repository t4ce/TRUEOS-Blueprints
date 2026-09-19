use std::collections::HashMap;

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
        WindowRequest, bmp_file_from_dib, dib_layout,
    },
    session::{LAUNCHER_PID, LAUNCHER_TID, PersonalityAction, SessionRequest, Wc3Session},
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
    let (desktop_width, desktop_height) = ui4_scene::output_dimensions()
        .map_err(|error| format!("query UI4 output dimensions: {error:?}"))?;
    session
        .launcher_mut()
        .xp
        .set_desktop_size(desktop_width, desktop_height);
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
    }];
    let mut child_runtime: Option<RuntimeProcess> = None;
    let mut thread_calls: HashMap<(u32, u32), u32> = HashMap::new();
    let mut window_frame: Option<Frame> = None;
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
                let result = match result {
                    PersonalityAction::Return(value) => {
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
                            if let Some((selected, width, height, bpp)) =
                                session.launcher().xp.compatible_dc_info(value)
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CreateCompatibleDC source=DISPLAY hdc=0x{:08x} selected_bitmap=0x{:08x} selected_bitmap_shape={}x{}x{}",
                                        value, selected, width, height, bpp
                                    ),
                                );
                            }
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
                    PersonalityAction::CallGuest(_) => {
                        if contexts[active].tid != LAUNCHER_TID {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 TID2 CALL_GUEST pid={} tid={}",
                                    LAUNCHER_PID, contexts[active].tid
                                ),
                            );
                            return Ok(());
                        }
                        return Err(
                            "wc3: CallGuest action is not wired into the launcher loop".into()
                        );
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
                session.sync_launcher_focus();
                if let Some(request) = session.launcher_mut().xp.take_window_request() {
                    present_window(request, &mut window_frame)?;
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

fn present_window(request: WindowRequest, frame: &mut Option<Frame>) -> Result<(), String> {
    match request {
        WindowRequest::Show {
            x,
            y,
            width,
            height,
        } => {
            if frame.is_some() {
                return Ok(());
            }
            let mut opened = Frame::open(x, y, width, height)
                .map_err(|error| format!("create WC3 UI4 window: {error:?}"))?;
            opened
                .begin(rgba(0, 0, 0, 255))
                .and_then(|()| opened.publish(Damage::full(width, height)))
                .map_err(|error| format!("publish WC3 UI4 window: {error:?}"))?;
            *frame = Some(opened);
        }
        WindowRequest::Hide => {
            *frame = None;
        }
    }
    Ok(())
}

struct GuestContext {
    tid: u32,
    context: Context,
    started: bool,
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
