{
                if exit.registers.eip == thunk32::THREAD_EXIT_AFTER_VMCALL {
                    if contexts[active].tid != LAUNCHER_TID {
                        let Some(next) = terminate_launcher_thread(
                            &mut session,
                            &mut contexts,
                            active,
                            exit.registers.eax,
                            thread_calls,
                            &mut wait_deadlines,
                        )?
                        else {
                            return Ok(());
                        };
                        active = next;
                        continue;
                    }
                    let exited = contexts.remove(active);
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
                logl::trace!(
                    "trace-api",
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
                                        let text = wc3::ThisToThat::cp1252_to_string(&bytes);
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
                if call_kind == WinCall::MessageBoxA {
                    if active_message_box.is_some() {
                        return Err("nested MessageBoxA modal frontier".into());
                    }
                    let frame = read_guest_words(&memory, exit.registers.esp, 5)?;
                    let text = copy_message_box_ansi(&memory, frame[2])?;
                    let caption = copy_message_box_ansi(&memory, frame[3])?;
                    let buttons = message_box_buttons(frame[4]).ok_or_else(|| {
                        format!(
                            "MessageBoxA unsupported button type=0x{:x}",
                            frame[4] & 0x0f
                        )
                    })?;
                    let request = PendingMessageBox {
                        caller: active_key,
                        owner_hwnd: frame[1],
                        text,
                        caption,
                        style: frame[4],
                    };
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 MESSAGEBOXA OPEN pid={} tid={} owner=0x{:08x} text={:?} caption={:?} type=0x{:08x} buttons=[{}] topmost={} state=modal-blocked",
                            request.caller.pid,
                            request.caller.tid,
                            request.owner_hwnd,
                            request.text,
                            request.caption,
                            request.style,
                            buttons
                                .iter()
                                .map(|button| button.label)
                                .collect::<Vec<_>>()
                                .join(","),
                            u32::from(request.style & 0x0004_0000 != 0),
                        ),
                    );
                    *active_message_box = Some(open_message_box(request, buttons)?);
                    continue;
                }
                if WinCall::from_import(&import) == WinCall::Unsupported {
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 LAUNCHER UNSUPPORTED pid={} tid={} module={} symbol={} esp=0x{:08x} eip=0x{:08x} stack[0..16 dwords]={:?}",
                            active_key.pid,
                            active_key.tid,
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
                let draw_text_input = if WinCall::from_import(&import) == WinCall::DrawTextA {
                    let frame = read_guest_words(&memory, exit.registers.esp, 6)?;
                    let rect = read_guest_words(&memory, frame[4], 4)?;
                    Some((frame, rect))
                } else {
                    None
                };
                let end_paint_input = if WinCall::from_import(&import) == WinCall::EndPaint {
                    let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                    let hdc = read_guest_words(&memory, frame[2], 1)?[0];
                    let target = match session.launcher().xp.dc_target(hdc) {
                        Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                        Some(None) => "MEMORY".to_owned(),
                        None => "UNKNOWN".to_owned(),
                    };
                    Some((frame, hdc, target))
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
                    PersonalityAction::OpenFile(_) => {
                        return Err("launcher requested child-only async file open".into());
                    }
                    PersonalityAction::Return(value) => {
                        if WinCall::from_import(&import) == WinCall::TlsGetValue {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 TLSGETVALUE pid={} tid={} slot={} value=0x{:08x} result=0x{:08x}",
                                    active_key.pid, active_key.tid, frame[1], value, value,
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetLastError {
                            let frame = read_guest_words(&memory, exit.registers.esp, 2)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SETLASTERROR pid={} tid={} value=0x{:08x}",
                                    active_key.pid, active_key.tid, frame[1],
                                ),
                            );
                        }
                        if let Some((frame, hdc, target)) = end_paint_input.as_ref() {
                            if value == 1 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 EndPaint hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target={} retired=1",
                                        frame[1], frame[2], hdc, target
                                    ),
                                );
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 paint lifecycle active_paints={} paint_hdc_0x{:08x}_live={}",
                                        session.launcher().xp.active_paint_count(),
                                        hdc,
                                        u32::from(session.launcher().xp.gdi_live(*hdc))
                                    ),
                                );
                            }
                        }
                        if WinCall::from_import(&import) == WinCall::SetTextColor {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let new_color = frame[2] & 0x00ff_ffff;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetTextColor hdc=0x{:08x} target={} old=0x{:08x} new=0x{:08x} rgb=[{},{},{}]",
                                    frame[1],
                                    target,
                                    value,
                                    new_color,
                                    new_color & 0xff,
                                    (new_color >> 8) & 0xff,
                                    (new_color >> 16) & 0xff
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetBkColor {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let new_color = frame[2] & 0x00ff_ffff;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetBkColor hdc=0x{:08x} target={} old=0x{:08x} new=0x{:08x} rgb=[{},{},{}]",
                                    frame[1],
                                    target,
                                    value,
                                    new_color,
                                    new_color & 0xff,
                                    (new_color >> 8) & 0xff,
                                    (new_color >> 16) & 0xff
                                ),
                            );
                        }
                        if WinCall::from_import(&import) == WinCall::SetBkMode {
                            let frame = read_guest_words(&memory, exit.registers.esp, 3)?;
                            let target = match session.launcher().xp.dc_target(frame[1]) {
                                Some(Some(hwnd)) => format!("WINDOW_PAINT hwnd=0x{hwnd:08x}"),
                                Some(None) => "MEMORY".to_owned(),
                                None => "UNKNOWN".to_owned(),
                            };
                            let mode_name = |mode| match mode {
                                1 => "TRANSPARENT",
                                2 => "OPAQUE",
                                _ => "UNKNOWN",
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 SetBkMode hdc=0x{:08x} target={} old={} new={} old_name={} new_name={}",
                                    frame[1],
                                    target,
                                    value,
                                    frame[2],
                                    mode_name(value),
                                    mode_name(frame[2])
                                ),
                            );
                            if let Some((text_color, bk_color, bk_mode)) =
                                session.launcher().xp.text_state(frame[1])
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 TEXT DC hdc=0x{:08x} text_color=0x{:08x} bk_color=0x{:08x} bk_mode={}",
                                        frame[1],
                                        text_color,
                                        bk_color,
                                        mode_name(bk_mode)
                                    ),
                                );
                            }
                        }
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
                        window_rgba.insert(request.hwnd, request.rgba.clone());
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
                        1
                    }
                    PersonalityAction::WindowText(request) => {
                        let backing = window_rgba
                            .get(&request.hwnd)
                            .ok_or_else(|| "DrawTextA window backing missing".to_owned())?;
                        let frame = frames
                            .get_mut(&request.hwnd)
                            .ok_or_else(|| "DrawTextA destination frame missing".to_owned())?;
                        let expected_bytes = (frame.width() as usize)
                            .checked_mul(frame.height() as usize)
                            .and_then(|pixels| pixels.checked_mul(4))
                            .ok_or_else(|| "DrawTextA frame size overflow".to_owned())?;
                        if backing.len() != expected_bytes {
                            return Err("DrawTextA window backing size mismatch".into());
                        }
                        frame
                            .begin(rgba(0, 0, 0, 255))
                            .map_err(|error| format!("begin WC3 DrawTextA frame: {error:?}"))?;
                        frame
                            .write_opaque_rgba8(backing)
                            .map_err(|error| format!("restore WC3 DrawTextA backing: {error:?}"))?;
                        let row = SceneTextRow {
                            text: request.text.as_str(),
                            x: request.rect[0] as f32,
                            y: request.rect[1] as f32,
                            font_pixels: request.height as f32,
                        };
                        frame
                            .stamp_text_scene(
                                Font::Default,
                                (frame.width(), frame.height()),
                                rgba(240, 200, 0, 255),
                                core::slice::from_ref(&row),
                            )
                            .map_err(|error| format!("stamp WC3 DrawTextA text: {error:?}"))?;
                        let damage = Damage::full(frame.width(), frame.height());
                        loop {
                            match frame.publish(damage) {
                                Ok(()) => break,
                                Err(ui4_scene::Error::Busy) => {
                                    trueos::vsys::poll_once();
                                    trueos::vsys::sleep_ms(1);
                                }
                                Err(error) => {
                                    return Err(format!("publish WC3 DrawTextA text: {error:?}"));
                                }
                            }
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 UI4 TEXT hwnd=0x{:08x} hdc=0x{:08x} rect=[{},{},{},{}] height={} published=1",
                                request.hwnd,
                                request.hdc,
                                request.rect[0],
                                request.rect[1],
                                request.rect[2],
                                request.rect[3],
                                request.height
                            ),
                        );
                        16
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
                        let child_file_bytes = Arc::new(child_bytes);
                        let created = session.create_child();
                        // CREATE_SUSPENDED was not requested.  The logical
                        // child thread is runnable, but its native entry is
                        // held at the loader frontier until dependencies are
                        // prepared.
                        session.enqueue(ThreadKey {
                            pid: created.pid,
                            tid: created.tid,
                        });
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
                        log_child_handles(&session, created.pid);
                        let child_address_space =
                            AddressSpace::create().map_err(|error| error.to_string())?;
                        child_address_space
                            .map(
                                PROCESS_DATA_VA,
                                0x1000,
                                Permissions::READ | Permissions::WRITE,
                            )
                            .map_err(|error| format!("map child process data: {error}"))?;
                        let written = child_address_space
                            .write(PROCESS_DATA_VA, CHILD_COMMAND_LINE)
                            .map_err(|error| format!("write child command line: {error}"))?;
                        if written != CHILD_COMMAND_LINE.len() {
                            return Err("short child command-line write".into());
                        }
                        let fmode_written = child_address_space
                            .write(CRT_FMODE_VA, &[0, 0, 0, 0])
                            .map_err(|error| format!("write child CRT _fmode: {error}"))?;
                        if fmode_written != 4 {
                            return Err("short child CRT _fmode write".into());
                        }
                        let commode_written = child_address_space
                            .write(CRT_COMMODE_VA, &[0, 0, 0, 0])
                            .map_err(|error| format!("write child CRT _commode: {error}"))?;
                        if commode_written != 4 {
                            return Err("short child CRT _commode write".into());
                        }
                        let acmdln_written = child_address_space
                            .write(CRT_ACMDLN_VA, &PROCESS_DATA_VA.to_le_bytes())
                            .map_err(|error| format!("write child CRT _acmdln: {error}"))?;
                        if acmdln_written != 4 {
                            return Err("short child CRT _acmdln write".into());
                        }
                        let argv = [CRT_ARG0_VA, CRT_ARG1_VA, CRT_ARG2_VA, CRT_ARG3_VA, 0];
                        let mut argv_bytes = [0u8; 20];
                        for (index, pointer) in argv.into_iter().enumerate() {
                            argv_bytes[index * 4..index * 4 + 4]
                                .copy_from_slice(&pointer.to_le_bytes());
                        }
                        if child_address_space
                            .write(CRT_ARGV_VA, &argv_bytes)
                            .map_err(|error| format!("write child CRT argv: {error}"))?
                            != argv_bytes.len()
                        {
                            return Err("short child CRT argv write".into());
                        }
                        for (address, value) in [
                            (CRT_ARG0_VA, b"war3.exe\0".as_slice()),
                            (CRT_ARG1_VA, b"-opengl\0".as_slice()),
                            (CRT_ARG2_VA, b"-nosound\0".as_slice()),
                            (CRT_ARG3_VA, b"-swtnl\0".as_slice()),
                        ] {
                            if child_address_space
                                .write(address, value)
                                .map_err(|error| format!("write child CRT argv string: {error}"))?
                                != value.len()
                            {
                                return Err("short child CRT argv string write".into());
                            }
                        }
                        if child_address_space
                            .write(CRT_ENVP_VA, &[0, 0, 0, 0])
                            .map_err(|error| format!("write child CRT envp: {error}"))?
                            != 4
                        {
                            return Err("short child CRT envp write".into());
                        }
                        let environment_written = child_address_space
                            .write(ENVIRONMENT_BLOCK_VA, &[0, 0, 0, 0])
                            .map_err(|error| format!("write child environment block: {error}"))?;
                        if environment_written != 4 {
                            return Err("short child environment-block write".into());
                        }
                        *pending_child = Some(PendingChild {
                            pid: created.pid,
                            tid: created.tid,
                            active_thread_tid: created.tid,
                            parked_threads: HashMap::new(),
                            image: child,
                            self_image_bytes: child_file_bytes,
                            native_modules: Vec::new(),
                            address_space: child_address_space,
                            crt_heap_mapped_end: CHILD_CRT_HEAP_BASE,
                            win_heap_mapped_end: CHILD_WIN_HEAP_BASE,
                            provider_thunk_bytes: 0,
                            static_load_reserved: 0,
                            initterm: None,
                            load_library_call: None,
                            cipow: None,
                            cipow_diagnostic_logged: false,
                            get_system_info_consumer_logged: false,
                            seh_handler_dumped: false,
                            seh3_diagnostic_logged: false,
                            seh: None,
                            seh3_call: None,
                            unhandled_filter_call: None,
                            repeated_null_call: None,
                            repeated_divide_fault: None,
                            single_step_count: 0,
                            scan_progress: None,
                            scan_heartbeat_source: None,
                            dword_scan_watch: None,
                            table_checkpoint_capture: None,
                            loop_checkpoint_capture: None,
                            loop_checkpoint_attempted: 0,
                            loader: ChildLoaderState {
                                prepared: false,
                                native_requests: Vec::new(),
                                next_native: 0,
                            },
                            execution: ChildExecutionState::Loader,
                        });
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
                    PersonalityAction::Session(SessionRequest::GetExitCodeProcess(request)) => {
                        match session.get_exit_code_process(request.pid, request.handle) {
                            Ok((target_pid, exit_code)) => {
                                memory
                                    .write(request.exit_code_pointer, &exit_code.to_le_bytes())
                                    .map_err(str::to_owned)?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GETEXITCODEPROCESS pid={} tid={} handle=0x{:08x} target_pid={} exit_code=0x{:08x} result=1",
                                        request.pid,
                                        request.tid,
                                        request.handle,
                                        target_pid,
                                        exit_code,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(request.pid)
                                    .ok_or_else(|| "GetExitCodeProcess caller missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_key.tid, 6);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 GETEXITCODEPROCESS pid={} tid={} handle=0x{:08x} target_pid=- exit_code=- result=0 error=6",
                                        request.pid, request.tid, request.handle,
                                    ),
                                );
                                0
                            }
                        }
                    }
                    PersonalityAction::Session(SessionRequest::CreateEvent(request)) => {
                        let (handle, already_exists) = session.create_event(LAUNCHER_PID, request);
                        session.launcher_mut().xp.set_last_error_for_thread(
                            active_key.tid,
                            if handle == 0 { 6 } else if already_exists { 183 } else { 0 },
                        );
                        handle
                    }
                    PersonalityAction::Session(request @ SessionRequest::CreateMutex { .. })
                    | PersonalityAction::Session(request @ SessionRequest::ReleaseMutex { .. })
                    | PersonalityAction::Session(request @ SessionRequest::ResetEvent { .. }) => {
                        service_sync_request(
                            &mut session,
                            LAUNCHER_PID,
                            active_key.tid,
                            request,
                            &mut contexts,
                            &mut wait_deadlines,
                        )?
                    }
                    PersonalityAction::Session(SessionRequest::SetEvent { pid, tid, handle }) => {
                        match session.set_event(pid, handle) {
                            Ok(outcome) => {
                                let waiters_woken = outcome.woken.len();
                                resume_completed_waiters(
                                    &outcome.woken,
                                    "set-event",
                                    &mut contexts,
                                    &mut wait_deadlines,
                                )?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SETEVENT pid={} tid={} handle=0x{:08x} manual_reset={} was_signaled={} waiters_woken={} result=1",
                                        pid,
                                        tid,
                                        handle,
                                        outcome.manual_reset as u8,
                                        outcome.was_signaled as u8,
                                        waiters_woken,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(pid)
                                    .ok_or_else(|| "SetEvent caller missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_key.tid, 6);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 SETEVENT pid={} tid={} handle=0x{:08x} result=0 error=6",
                                        pid, tid, handle,
                                    ),
                                );
                                0
                            }
                        }
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
                    PersonalityAction::Session(SessionRequest::DestroyWindow { pid, hwnd }) => {
                        let frame_present = frames.contains_key(&hwnd);
                        match session.destroy_window(pid, hwnd) {
                            Ok(result) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 DESTROYWINDOW pid={} tid={} hwnd=0x{:08x} frame_present={} focused={} result=1",
                                        pid,
                                        active_key.tid,
                                        hwnd,
                                        frame_present as u8,
                                        result.was_focused as u8,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(pid)
                                    .ok_or_else(|| "DestroyWindow caller missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_key.tid, 1400);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 DESTROYWINDOW pid={} tid={} hwnd=0x{:08x} frame_present={} focused=- result=0 error=1400",
                                        pid, active_key.tid, hwnd, frame_present as u8,
                                    ),
                                );
                                0
                            }
                        }
                    }
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
                            session.launcher_mut().xp.set_last_error_for_thread(active_key.tid, 6);
                            0
                        }
                    }
                    PersonalityAction::Block(request) => {
                        let is_single = import.symbol == "WaitForSingleObject";
                        if is_single {
                            let handle = request.handles[0];
                            let repeated_wait_timeout = *previous_wait_timeout
                                == Some((request.key, handle, request.timeout));
                            let description = session.describe_handle(request.key.pid, handle);
                            let state = session.event_state(request.key.pid, handle);
                            if !repeated_wait_timeout {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WaitForSingleObject pid={} tid={} ret=0x{:08x} handle=0x{:08x} timeout={} object={}",
                                        request.key.pid,
                                        request.key.tid,
                                        request.return_address,
                                        handle,
                                        request.timeout,
                                        description
                                    ),
                                );
                                if let Some((manual_reset, signaled)) = state {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WaitForSingleObject state manual_reset={} signaled={}",
                                            u32::from(manual_reset),
                                            u32::from(signaled)
                                        ),
                                    );
                                }
                            }
                            if let Some(wait_result) =
                                session.poll_wait(&request).map_err(str::to_owned)?
                            {
                                if wait_result != WAIT_TIMEOUT {
                                    *previous_wait_timeout = None;
                                }
                                let consumed = state
                                    .map(|(manual_reset, signaled)| signaled && !manual_reset)
                                    .unwrap_or(false);
                                if wait_result == WAIT_OBJECT_0 {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT SIGNALED pid={} tid={} handle=0x{:08x} auto_reset_consumed={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            handle,
                                            u32::from(consumed),
                                            wait_result
                                        ),
                                    );
                                } else if wait_result == WAIT_TIMEOUT && !repeated_wait_timeout {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT TIMEOUT pid={} tid={} handle=0x{:08x} elapsed_ms=0 result=0x{:08x}",
                                            request.key.pid, request.key.tid, handle, wait_result
                                        ),
                                    );
                                } else if wait_result == WAIT_FAILED {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT FAILED pid={} tid={} handle=0x{:08x} result=0x{:08x}",
                                            request.key.pid, request.key.tid, handle, wait_result
                                        ),
                                    );
                                }
                                let mut registers = exit.registers;
                                registers.eax = wait_result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                if !repeated_wait_timeout {
                                    logl::trace!(
                                        "trace-api",
                                        level::INFO,
                                        format_args!(
                                            "wc3: return #{} KERNEL32.dll!WaitForSingleObject eax=0x{:08x}",
                                            session.launcher().xp.call_count,
                                            wait_result
                                        ),
                                    );
                                }
                                continue;
                            }
                            if !repeated_wait_timeout {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WAIT BLOCK pid={} tid={} handle=0x{:08x} timeout_ms={}",
                                        request.key.pid, request.key.tid, handle, request.timeout
                                    ),
                                );
                            }
                        } else {
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
                                    "WC3 WAIT MULTIPLE pid={} tid={} call=#{} ret=0x{:08x} count={} handles_ptr=0x{:08x} handle0=0x{:08x} handle1=0x{:08x} wait_all={} timeout=0x{:08x}",
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
                            logl::trace!(
                                "trace-api",
                                level::INFO,
                                format_args!(
                                    "wc3: wait handle0 {}",
                                    session.describe_handle(LAUNCHER_PID, request.handles[0])
                                ),
                            );
                            logl::trace!(
                                "trace-api",
                                level::INFO,
                                format_args!(
                                    "wc3: wait handle1 {}",
                                    session.describe_handle(LAUNCHER_PID, request.handles[1])
                                ),
                            );
                            if request.count == 0 || request.count > 2 || request.wait_all > 1 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 WAIT FRONTIER reason=unsupported-shape count={} wait_all={}",
                                        request.count, request.wait_all,
                                    ),
                                );
                                return Ok(());
                            }
                            if let Some(wait_result) =
                                session.poll_wait(&request).map_err(str::to_owned)?
                            {
                                let index = wait_result.saturating_sub(WAIT_OBJECT_0);
                                if wait_result >= WAIT_OBJECT_0 && index < request.count {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT SIGNALED pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} index={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.handles[0],
                                            request.handles[1],
                                            request.count,
                                            request.wait_all,
                                            index,
                                            wait_result,
                                        ),
                                    );
                                } else if wait_result == WAIT_TIMEOUT {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT TIMEOUT pid={} tid={} count={} wait_all={} elapsed_ms=0 result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.count,
                                            request.wait_all,
                                            wait_result,
                                        ),
                                    );
                                } else {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 WAIT FAILED pid={} tid={} count={} wait_all={} result=0x{:08x}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.count,
                                            request.wait_all,
                                            wait_result,
                                        ),
                                    );
                                }
                                let mut registers = exit.registers;
                                registers.eax = wait_result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 WAIT BLOCK pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} count={} wait_all={} timeout_ms={}",
                                    request.key.pid,
                                    request.key.tid,
                                    request.handles[0],
                                    request.handles[1],
                                    request.count,
                                    request.wait_all,
                                    request.timeout,
                                ),
                            );
                        }
                        session.block_wait(request.clone()).map_err(str::to_owned)?;
                        if request.timeout != INFINITE {
                            wait_deadlines.insert(
                                request.key,
                                RuntimeWait {
                                    deadline: tokio::time::Instant::now()
                                        + Duration::from_millis(request.timeout as u64),
                                    timeout_ms: request.timeout,
                                    handle: request.handles[0],
                                    resume_registers: exit.registers,
                                },
                            );
                        }
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        if let Some(key) =
                            session.runnable.iter().find(|key| key.pid != LAUNCHER_PID)
                        {
                            let child = pending_child
                                .as_mut()
                                .filter(|child| child.pid == key.pid && child.tid == key.tid)
                                .ok_or_else(|| "runnable child missing pending image".to_owned())?;
                            let listing = async_fs::list_dir(b"/common/Warcraft III")
                                .await
                                .map_err(|error| {
                                    format!("list Warcraft III directory: TRUEOSFS {error}")
                                })?;
                            if listing.truncated {
                                return Err("Warcraft III directory listing truncated".into());
                            }
                            let loaded =
                                session
                                    .assets
                                    .preload_war3(&listing)
                                    .await
                                    .map_err(|error| {
                                        logl::log(
                                            level::ERROR,
                                            format_args!(
                                    "WC3 RAM ASSET FAILED asset=\"war3.mpq\" error={error:?}"
                                ),
                                        );
                                        error
                                    })?;
                            if loaded {
                                let asset = session.assets.war3_mpq().expect("resident MPQ");
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 RAM ASSET READY asset=\"war3.mpq\" stored=\"{}\" bytes={} backing=host-ram guest_mapped=0 copies=1",
                                        asset.stored_path(),
                                        asset.len(),
                                    ),
                                );
                            }
                            let war3_mpq = session
                                .assets
                                .lookup("war3.mpq")
                                .ok_or_else(|| "resident War3.mpq missing after preload".to_owned())?;
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing for War3.mpq install".to_owned())?
                                .xp
                                .install_war3_mpq(war3_mpq);
                            let surface = child_loader::prepare(&mut child.image, &listing)
                                .map_err(str::to_owned)?;
                            child.loader.native_requests = surface.native.clone();
                            child.loader.prepared = true;
                            map_child_image(&child.address_space, &child.image)?;
                            install_war3_event_pool_trap(child)?;
                            log_child_slot_xrefs(child, WAR3_REPEATED_NULL_CALL_SLOT);
                            log_child_slot_xrefs(child, WAR3_SCAN_INDEX);
                            log_child_slot_xrefs(child, WAR3_SCAN_SOURCE);
                            log_child_slot_xrefs(child, WAR3_SCAN_BOUND);
                            log_child_slot_xrefs(child, WAR3_SCAN_COUNT);
                            let mut bytes = [0u8; 0xb0];
                            if child
                                .address_space
                                .read(WAR3_HOTLOOP_START, &mut bytes)
                                .ok()
                                == Some(bytes.len())
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD HOTLOOP CODE start=0x{WAR3_HOTLOOP_START:08x} bytes=\"{}\"",
                                        diagnostic_hex_bytes(&bytes),
                                    ),
                                );
                            }
                            map_child_thunks(&child.address_space, &surface.thunks)?;
                            map_child_controls(&child.address_space)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD PROVIDERS READY pid={} modules={} imports={} named={} ordinal={} thunk_base=0x{:08x} thunk_bytes={} patched_iat={}",
                                    child.pid,
                                    surface.external_modules,
                                    surface.imports.len(),
                                    surface.named,
                                    surface.ordinal,
                                    thunk32::THUNK_BASE,
                                    surface.thunks.len(),
                                    surface.imports.len(),
                                ),
                            );
                            let initial_provider_thunk_bytes = surface.thunks.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .try_install_provider_surface(
                                    surface.imports,
                                    surface.thunks,
                                    surface.providers,
                                )
                                .map_err(str::to_owned)?;
                            child.provider_thunk_bytes = initial_provider_thunk_bytes;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXECUTION ROUTER READY pid={} provider_imports={} provider_thunk_bytes={} control_base=0x{:08x} provider_namespace=child memory_space=child",
                                    child.pid,
                                    session
                                        .process(child.pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .provider_import_count(),
                                    4096,
                                    thunk32::CHILD_CONTROL_BASE,
                                ),
                            );
                            let native = child
                                .loader
                                .native_requests
                                .get(child.loader.next_native)
                                .cloned()
                                .ok_or_else(|| {
                                    "child has no local native direct module".to_owned()
                                })?;
                            let path = format!("/common/Warcraft III/{}", native.stored);
                            let bytes = async_fs::read_file(path.as_bytes())
                                .await
                                .map_err(|error| format!("read {path}: TRUEOSFS error {error}"))?;
                            let image = pe32::parse(&bytes).map_err(str::to_owned)?;
                            child
                                .address_space
                                .map(
                                    image.image_base,
                                    image.image.len(),
                                    Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
                                )
                                .map_err(|_| "preferred-base-unavailable".to_owned())?;
                            let written = child
                                .address_space
                                .write(image.image_base, &image.image)
                                .map_err(|error| error.to_string())?;
                            if written != image.image.len() {
                                return Err("short native image write".into());
                            }
                            if native.stored.eq_ignore_ascii_case("Storm.dll") {
                                install_storm_record_expand_trap(&child.address_space)?;
                            }
                            let storm_imports: Vec<_> = image
                                .imports
                                .iter()
                                .map(|import| child_loader::ProviderImport {
                                    module: import.module.clone(),
                                    symbol: match &import.symbol {
                                        pe32::ImportSymbol::Name(name) => {
                                            child_loader::ProviderSymbol::Name(name.clone())
                                        }
                                        pe32::ImportSymbol::Ordinal(ordinal) => {
                                            child_loader::ProviderSymbol::Ordinal(*ordinal)
                                        }
                                    },
                                    iat_rva: import.iat_rva,
                                })
                                .collect();
                            let external_modules = storm_imports
                                .iter()
                                .map(|import| import.module.as_str())
                                .collect::<std::collections::HashSet<_>>()
                                .len();
                            let (addresses, old_bytes, new_bytes, updated_from, updated) = session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .append_provider_imports(storm_imports)
                                .map_err(str::to_owned)?;
                            let provider_thunks_total = session
                                .process(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_import_count();
                            child.provider_thunk_bytes = new_bytes;
                            if new_bytes > old_bytes {
                                child
                                    .address_space
                                    .map(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        new_bytes - old_bytes,
                                        Permissions::READ
                                            | Permissions::WRITE
                                            | Permissions::EXECUTE,
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
                            let existing_update =
                                old_bytes.saturating_sub(updated_from).min(updated.len());
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
                            for (import, address) in image.imports.iter().zip(addresses) {
                                child
                                    .address_space
                                    .write(
                                        image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE IMPORTS READY pid={} module=\"{}\" external_modules={} external_imports={} patched_iat={} provider_thunks_total={} thunk_bytes={}",
                                    child.pid,
                                    native.stored,
                                    external_modules,
                                    image.imports.len(),
                                    image.imports.len(),
                                    provider_thunks_total,
                                    new_bytes
                                ),
                            );
                            let export_named = image
                                .exports
                                .iter()
                                .filter(|export| export.name.is_some())
                                .count();
                            let export_forwarders = image
                                .exports
                                .iter()
                                .filter(|export| {
                                    matches!(export.target, pe32::ExportTarget::Forwarder(_))
                                })
                                .count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE EXPORTS module=\"{}\" exports={} named={} ordinal_only={} forwarders={}",
                                    native.stored,
                                    image.exports.len(),
                                    export_named,
                                    image.exports.len() - export_named,
                                    export_forwarders
                                ),
                            );
                            let parent_imports: Vec<_> = child
                                .image
                                .imports
                                .iter()
                                .filter(|import| {
                                    import.module.eq_ignore_ascii_case(&native.requested)
                                })
                                .collect();
                            let mut resolved = 0usize;
                            let mut parent_named = 0usize;
                            let mut parent_ordinal = 0usize;
                            for import in &parent_imports {
                                let export = match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => {
                                        parent_named += 1;
                                        image.exports.iter().find(|export| export.name.as_deref() == Some(name.as_str()))
                                    }
                                    pe32::ImportSymbol::Ordinal(ordinal) => {
                                        parent_ordinal += 1;
                                        image.exports.iter().find(|export| export.ordinal == u32::from(*ordinal))
                                    }
                                }.ok_or_else(|| match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" symbol=\"{}\"", native.stored, name),
                                    pe32::ImportSymbol::Ordinal(ordinal) => format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" ordinal={}", native.stored, ordinal),
                                })?;
                                let pe32::ExportTarget::Rva(rva) = &export.target else {
                                    let forwarder = match &export.target {
                                        pe32::ExportTarget::Forwarder(value) => value,
                                        _ => unreachable!(),
                                    };
                                    return Err(format!(
                                        "WC3 CHILD NATIVE EXPORT FORWARDER FRONTIER parent=\"War3.exe\" module=\"{}\" forwarder=\"{}\"",
                                        native.stored, forwarder
                                    ));
                                };
                                if *rva >= image.size_of_image {
                                    return Err("Storm export RVA outside image".into());
                                }
                                let address = image
                                    .image_base
                                    .checked_add(*rva)
                                    .ok_or_else(|| "Storm export VA overflow".to_owned())?;
                                child
                                    .address_space
                                    .write(
                                        child.image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(child.image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("War3 Storm IAT readback mismatch".into());
                                }
                                resolved += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE BIND parent=\"War3.exe\" module=\"{}\" imports={} resolved={} named={} ordinal={} forwarded=0",
                                    native.stored,
                                    parent_imports.len(),
                                    resolved,
                                    parent_named,
                                    parent_ordinal
                                ),
                            );
                            if resolved != parent_imports.len() {
                                return Err("incomplete War3 Storm binding".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MAP pid={} module=\"{}\" preferred_base=0x{:08x} mapped_base=0x{:08x} size=0x{:08x} relocation_delta=0 relocations_applied=0",
                                    child.pid,
                                    native.stored,
                                    image.image_base,
                                    image.image_base,
                                    image.size_of_image
                                ),
                            );
                            log_native_child_image(
                                &native.requested,
                                &native.stored,
                                &image,
                                &listing,
                            )
                            .map_err(str::to_owned)?;
                            let native_base = image.image_base;
                            let native_imports = image.imports.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .register_native_module(
                                    &native.requested,
                                    &native.stored,
                                    native_base,
                                )
                                .map_err(str::to_owned)?;
                            child.native_modules.push(PendingNativeModule {
                                requested: native.requested,
                                stored: native.stored.clone(),
                                image,
                                initialized: false,
                            });
                            let initialized = child
                                .native_modules
                                .last()
                                .ok_or_else(|| "stored Storm missing".to_owned())?
                                .initialized;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MODULE READY pid={} module=\"{}\" base=0x{:08x} imports_bound={} parent_imports_resolved={} initialized={}",
                                    child.pid,
                                    native.stored,
                                    native_base,
                                    native_imports,
                                    resolved,
                                    initialized as u8
                                ),
                            );
                            child.loader.next_native += 1;
                            let next_native = child
                                .loader
                                .native_requests
                                .get(child.loader.next_native)
                                .ok_or_else(|| "no unresolved native child module".to_owned())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE ADVANCE pid={} from=\"{}\" to=\"{}\"",
                                    child.pid, native.stored, next_native.stored
                                ),
                            );
                            let mss = next_native.clone();
                            let mss_path = format!("/common/Warcraft III/{}", mss.stored);
                            let mss_bytes =
                                async_fs::read_file(mss_path.as_bytes()).await.map_err(
                                    |error| format!("read {mss_path}: TRUEOSFS error {error}"),
                                )?;
                            let mss_image = pe32::parse(&mss_bytes).map_err(str::to_owned)?;
                            child
                                .address_space
                                .map(
                                    mss_image.image_base,
                                    mss_image.image.len(),
                                    Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
                                )
                                .map_err(|_| "preferred-base-unavailable".to_owned())?;
                            let mss_written = child
                                .address_space
                                .write(mss_image.image_base, &mss_image.image)
                                .map_err(|error| error.to_string())?;
                            if mss_written != mss_image.image.len() {
                                return Err("short Mss image write".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MAP pid={} module=\"{}\" preferred_base=0x{:08x} mapped_base=0x{:08x} size=0x{:08x} relocation_delta=0 relocations_applied=0",
                                    child.pid,
                                    mss.stored,
                                    mss_image.image_base,
                                    mss_image.image_base,
                                    mss_image.size_of_image
                                ),
                            );
                            let mss_export_named = mss_image
                                .exports
                                .iter()
                                .filter(|export| export.name.is_some())
                                .count();
                            let mss_forwarders = mss_image
                                .exports
                                .iter()
                                .filter(|export| {
                                    matches!(export.target, pe32::ExportTarget::Forwarder(_))
                                })
                                .count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE EXPORTS module=\"{}\" exports={} named={} ordinal_only={} forwarders={}",
                                    mss.stored,
                                    mss_image.exports.len(),
                                    mss_export_named,
                                    mss_image.exports.len() - mss_export_named,
                                    mss_forwarders
                                ),
                            );
                            let mss_parent_imports: Vec<_> = child
                                .image
                                .imports
                                .iter()
                                .filter(|import| import.module.eq_ignore_ascii_case(&mss.requested))
                                .collect();
                            let mut mss_resolved = 0usize;
                            for import in &mss_parent_imports {
                                let pe32::ImportSymbol::Name(name) = &import.symbol else {
                                    return Err("unexpected ordinal War3 Mss import".into());
                                };
                                let export = mss_image.exports.iter().find(|export| export.name.as_deref() == Some(name.as_str()))
                                    .ok_or_else(|| format!("WC3 CHILD NATIVE EXPORT MISSING parent=\"War3.exe\" module=\"{}\" symbol=\"{}\"", mss.stored, name))?;
                                let pe32::ExportTarget::Rva(rva) = &export.target else {
                                    let pe32::ExportTarget::Forwarder(forwarder) = &export.target
                                    else {
                                        unreachable!()
                                    };
                                    return Err(format!(
                                        "WC3 CHILD NATIVE EXPORT FORWARDER FRONTIER parent=\"War3.exe\" module=\"{}\" symbol=\"{}\" forwarder=\"{}\"",
                                        mss.stored, name, forwarder
                                    ));
                                };
                                if *rva >= mss_image.size_of_image {
                                    return Err("Mss export RVA outside image".into());
                                }
                                let address = mss_image
                                    .image_base
                                    .checked_add(*rva)
                                    .ok_or_else(|| "Mss export VA overflow".to_owned())?;
                                child
                                    .address_space
                                    .write(
                                        child.image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(child.image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("War3 Mss IAT readback mismatch".into());
                                }
                                mss_resolved += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE BIND parent=\"War3.exe\" module=\"{}\" imports={} resolved={} named={} ordinal=0 forwarded=0",
                                    mss.stored,
                                    mss_parent_imports.len(),
                                    mss_resolved,
                                    mss_parent_imports.len()
                                ),
                            );
                            if mss_resolved != mss_parent_imports.len() {
                                return Err("incomplete War3 Mss binding".into());
                            }
                            let mss_providers: Vec<_> = mss_image
                                .imports
                                .iter()
                                .map(|import| child_loader::ProviderImport {
                                    module: import.module.clone(),
                                    symbol: match &import.symbol {
                                        pe32::ImportSymbol::Name(name) => {
                                            child_loader::ProviderSymbol::Name(name.clone())
                                        }
                                        pe32::ImportSymbol::Ordinal(ordinal) => {
                                            child_loader::ProviderSymbol::Ordinal(*ordinal)
                                        }
                                    },
                                    iat_rva: import.iat_rva,
                                })
                                .collect();
                            let mss_external_modules = mss_providers
                                .iter()
                                .map(|import| import.module.as_str())
                                .collect::<std::collections::HashSet<_>>()
                                .len();
                            let (mss_addresses, old_bytes, new_bytes, updated_from, updated) =
                                session
                                    .process_mut(child.pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .append_provider_imports(mss_providers)
                                    .map_err(str::to_owned)?;
                            child.provider_thunk_bytes = new_bytes;
                            if new_bytes > old_bytes {
                                child
                                    .address_space
                                    .map(
                                        thunk32::THUNK_BASE + old_bytes as u32,
                                        new_bytes - old_bytes,
                                        Permissions::READ
                                            | Permissions::WRITE
                                            | Permissions::EXECUTE,
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
                            let existing_update =
                                old_bytes.saturating_sub(updated_from).min(updated.len());
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
                            for (import, address) in mss_image.imports.iter().zip(mss_addresses) {
                                child
                                    .address_space
                                    .write(
                                        mss_image.image_base + import.iat_rva,
                                        &address.to_le_bytes(),
                                    )
                                    .map_err(|error| error.to_string())?;
                                let mut readback = [0; 4];
                                child
                                    .address_space
                                    .read(mss_image.image_base + import.iat_rva, &mut readback)
                                    .map_err(|error| error.to_string())?;
                                if u32::from_le_bytes(readback) != address {
                                    return Err("Mss provider IAT readback mismatch".into());
                                }
                            }
                            let mss_provider_total = session
                                .process(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_import_count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE IMPORTS READY pid={} module=\"{}\" external_modules={} external_imports={} patched_iat={} provider_thunks_total={} thunk_bytes={}",
                                    child.pid,
                                    mss.stored,
                                    mss_external_modules,
                                    mss_image.imports.len(),
                                    mss_image.imports.len(),
                                    mss_provider_total,
                                    new_bytes
                                ),
                            );
                            let mut winmm_total = 0usize;
                            let mut winmm_named = 0usize;
                            let mut winmm_ordinal = 0usize;
                            for import in mss_image
                                .imports
                                .iter()
                                .filter(|import| import.module.eq_ignore_ascii_case("WINMM.dll"))
                            {
                                let index = winmm_total;
                                let (symbol, kind) = match &import.symbol {
                                    pe32::ImportSymbol::Name(name) => {
                                        winmm_named += 1;
                                        (name.clone(), "name")
                                    }
                                    pe32::ImportSymbol::Ordinal(value) => {
                                        winmm_ordinal += 1;
                                        (format!("#{value}"), "ordinal")
                                    }
                                };
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD WINMM IMPORT index={} symbol=\"{}\" iat_rva=0x{:08x} kind={}",
                                        index, symbol, import.iat_rva, kind
                                    ),
                                );
                                winmm_total += 1;
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WINMM SURFACE module=\"Mss32.dll\" imports={} named={} ordinal={}",
                                    winmm_total, winmm_named, winmm_ordinal
                                ),
                            );
                            log_native_child_image(
                                &mss.requested,
                                &mss.stored,
                                &mss_image,
                                &listing,
                            )
                            .map_err(str::to_owned)?;
                            let mss_base = mss_image.image_base;
                            let mss_imports = mss_image.imports.len();
                            session
                                .process_mut(child.pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .register_native_module(&mss.requested, &mss.stored, mss_base)
                                .map_err(str::to_owned)?;
                            child.native_modules.push(PendingNativeModule {
                                requested: mss.requested,
                                stored: mss.stored.clone(),
                                image: mss_image,
                                initialized: false,
                            });
                            let initialized = child
                                .native_modules
                                .last()
                                .ok_or_else(|| "stored Mss missing".to_owned())?
                                .initialized;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE MODULE READY pid={} module=\"{}\" base=0x{:08x} imports_bound={} parent_imports_resolved={} initialized={}",
                                    child.pid,
                                    mss.stored,
                                    mss_base,
                                    mss_imports,
                                    mss_resolved,
                                    initialized as u8
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD NATIVE LOAD COMPLETE pid={} modules={} initialized=0",
                                    child.pid,
                                    child.native_modules.len()
                                ),
                            );
                            let storm_index = child
                                .native_modules
                                .iter()
                                .position(|module| module.stored.eq_ignore_ascii_case("Storm.dll"))
                                .ok_or_else(|| "loaded Storm missing".to_owned())?;
                            child.execution = ChildExecutionState::DllInitReady {
                                native_index: storm_index,
                            };
                            let storm = child
                                .native_modules
                                .get(storm_index)
                                .ok_or_else(|| "loaded Storm missing".to_owned())?;
                            let storm_name = storm.stored.clone();
                            let storm_base = storm.image.image_base;
                            let storm_entry = storm
                                .image
                                .image_base
                                .checked_add(storm.image.entry_rva)
                                .ok_or_else(|| "Storm entry overflow".to_owned())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD EXECUTION STATE pid={} tid={} state=dll-init-ready module=\"{}\" context_created=0",
                                    child.pid, child.tid, storm_name
                                ),
                            );
                            let child_key = ThreadKey {
                                pid: child.pid,
                                tid: child.tid,
                            };
                            if context_index(&contexts, child_key).is_some() {
                                return Err("child primary context already exists".into());
                            }
                            let child_context = create_child_primary_context(child, storm_index)?;
                            let child_esp = STACK_TOP - 0x10;
                            let child_teb = thread_teb_va(child.tid)?;
                            let reserved = STACK_TOP - 0x20;
                            contexts.push(child_context);
                            if contexts
                                .iter()
                                .filter(|context| context.key() == child_key)
                                .count()
                                != 1
                            {
                                return Err("child primary context insertion mismatch".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CONTEXT READY pid={} tid={} state=dll-init-ready module=\"{}\" context_created=1 started=0 scheduled=0 eip=0x{:08x} esp=0x{:08x} teb=0x{:08x} stack_base=0x{:08x} stack_top=0x{:08x} return_va=0x{:08x}",
                                    child.pid,
                                    child.tid,
                                    storm_name,
                                    storm_entry,
                                    child_esp,
                                    child_teb,
                                    STACK_BASE,
                                    STACK_TOP,
                                    thunk32::CHILD_DLL_RETURN_ADDRESS
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL FRAME pid={} tid={} module=\"{}\" return=0x{:08x} hinst=0x{:08x} reason=1 reserved=0x{:08x}",
                                    child.pid,
                                    child.tid,
                                    storm_name,
                                    thunk32::CHILD_DLL_RETURN_ADDRESS,
                                    storm_base,
                                    reserved
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SCHEDULER READY pid={} tid={} context_identity=thread-key runnable_selection=process-aware wait_resume=process-aware control_routing=process-aware context_created=1",
                                    child.pid, child.tid
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT FRONTIER pid={} tid={} module=\"{}\" entry_va=0x{:08x} reason=context-ready-not-scheduled",
                                    child.pid, child.tid, storm_name, storm_entry,
                                ),
                            );
                            let child_context_index = context_index(&contexts, child_key)
                                .ok_or_else(|| {
                                    "child primary context missing after insertion".to_owned()
                                })?;
                            let matching_contexts = contexts
                                .iter()
                                .filter(|context| context.key() == child_key)
                                .count();
                            if matching_contexts != 1 {
                                return Err("child primary context is not unique".into());
                            }
                            if contexts[child_context_index].started {
                                return Err("child primary context unexpectedly started".into());
                            }
                            if child.native_modules[storm_index].initialized {
                                return Err(
                                    "Storm unexpectedly initialized before scheduling".into()
                                );
                            }
                            verify_child_primary_context(
                                child,
                                &contexts[child_context_index],
                                storm_index,
                            )?;
                            let (_, _, scheduled_entry) =
                                begin_child_dll_init(child).map_err(str::to_owned)?;
                            if scheduled_entry != storm_entry {
                                return Err("child DLL scheduling entry mismatch".into());
                            }
                            let runnable_count = session
                                .runnable
                                .iter()
                                .filter(|key| **key == child_key)
                                .count();
                            if runnable_count != 1 {
                                return Err(
                                    "child runnable queue entry missing or duplicated".into()
                                );
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT SCHEDULE pid={} tid={} module=\"{}\" state=dll-init-running eip=0x{:08x} esp=0x{:08x}",
                                    child.pid, child.tid, storm_name, storm_entry, child_esp
                                ),
                            );
                            let selected = pop_runnable_context(&mut session, &contexts)
                                .ok_or_else(|| {
                                    "child runnable context was not selected".to_owned()
                                })?;
                            if contexts[selected].key() != child_key {
                                return Err("ordinary scheduler selected non-child context".into());
                            }
                            active = selected;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SCHEDULED pid={} tid={} module=\"{}\" started=0",
                                    child.pid, child.tid, storm_name
                                ),
                            );
                            continue;
                        }
                        let Some(deadline) =
                            wait_deadlines.values().map(|wait| wait.deadline).min()
                        else {
                            return Err("no runnable thread after launcher block".into());
                        };
                        tokio::time::sleep_until(deadline).await;
                        expire_runtime_waits(
                            &mut session,
                            &mut contexts,
                            &mut wait_deadlines,
                            &mut previous_wait_timeout,
                        )?;
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        return Err("expired wait did not produce runnable launcher context".into());
                    }
                    PersonalityAction::CallGuest(call) => {
                        callback = Some(call);
                        0
                    }
                    PersonalityAction::ExitThread(exit_code) => {
                        let Some(next) = terminate_launcher_thread(
                            &mut session,
                            &mut contexts,
                            active,
                            exit_code,
                            thread_calls,
                            &mut wait_deadlines,
                        )?
                        else {
                            return Ok(());
                        };
                        active = next;
                        continue;
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
                if let Some((frame, input)) =
                    draw_text_input.filter(|(frame, _)| frame[5] == 0x0000_0411)
                {
                    let output = read_guest_words(&memory, frame[4], 4)?;
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 DrawTextA CALCRECT hdc=0x{:08x} count={} format=0x{:08x} input=[{},{},{},{}] output=[{},{},{},{}] height={}",
                            frame[1],
                            frame[3],
                            frame[5],
                            input[0] as i32,
                            input[1] as i32,
                            input[2] as i32,
                            input[3] as i32,
                            output[0] as i32,
                            output[1] as i32,
                            output[2] as i32,
                            output[3] as i32,
                            output[3].wrapping_sub(output[1])
                        ),
                    );
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 DrawTextA placement client=500x400 measured={}x{} expected_final_rect=[47,376,453,392]",
                            output[2].wrapping_sub(output[0]),
                            output[3].wrapping_sub(output[1])
                        ),
                    );
                }
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
                if import.symbol != "PeekMessageA" {
                    logl::trace!(
                        "trace-api",
                        level::INFO,
                        format_args!(
                            "wc3: return #{} {}!{} eax=0x{result:08x}",
                            session.launcher().xp.call_count,
                            import.module,
                            import.symbol,
                        ),
                    );
                }
                if let Some(thread) = session.absorb_runnable_thread() {
                    contexts.push(create_thread_context(&address_space, &thread)?);
                }
                if let Some(request) = session.take_window_presentation() {
                    present_window(request, &mut frames, window_rgba, &session)?;
                }
                if contexts[active].tid == LAUNCHER_TID {
                    active = 0;
                }
                }
