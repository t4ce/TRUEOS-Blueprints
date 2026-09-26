                    {
                    if operation.is_generic_process_local() {
                        let global_memory_status = if operation
                            == child_loader::ProviderOp::GlobalMemoryStatus
                        {
                            Some(read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                2,
                            )?[1])
                        } else {
                            None
                        };
                        let interlocked_exchange = if operation
                            == child_loader::ProviderOp::InterlockedExchange
                        {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                3,
                            )?;
                            let old =
                                read_guest_words(&X86Memory(&child.address_space), frame[1], 1)?[0];
                            Some((frame[1], frame[2], old))
                        } else {
                            None
                        };
                        let interlocked_add = if matches!(
                            operation,
                            child_loader::ProviderOp::InterlockedIncrement
                                | child_loader::ProviderOp::InterlockedDecrement
                        ) {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                2,
                            )?;
                            let old =
                                read_guest_words(&X86Memory(&child.address_space), frame[1], 1)?[0];
                            Some((frame[1], old))
                        } else {
                            None
                        };
                        let crt_memmove = if operation == child_loader::ProviderOp::CrtMemmove {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                4,
                            )?;
                            Some((frame[1], frame[2], frame[3]))
                        } else {
                            None
                        };
                        let process_memory = matches!(
                            operation,
                            child_loader::ProviderOp::ReadProcessMemory
                                | child_loader::ProviderOp::WriteProcessMemory
                        )
                        .then(|| {
                            read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                6,
                            )
                        })
                        .transpose()?;
                        let set_file_attributes = if operation
                            == child_loader::ProviderOp::SetFileAttributesA
                        {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                3,
                            )?;
                            let [_, filename, attributes] = frame.as_slice() else {
                                unreachable!("SetFileAttributesA frame has three words")
                            };
                            let path = if *filename == 0 {
                                "<null>".to_owned()
                            } else {
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    *filename,
                                    1024,
                                )
                                .map_err(|error| format!("SetFileAttributesA filename: {error}"))?
                            };
                            Some((path, *attributes))
                        } else {
                            None
                        };
                        let get_drive_type_root = if operation
                            == child_loader::ProviderOp::GetDriveTypeA
                        {
                            let [_, root_ptr] = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                2,
                            )?[..] else {
                                unreachable!("GetDriveTypeA frame has two words")
                            };
                            Some(if root_ptr == 0 {
                                None
                            } else {
                                Some(
                                    wc3::process::read_c_string(
                                        &X86Memory(&child.address_space),
                                        root_ptr,
                                        1024,
                                    )
                                    .map_err(|error| format!("GetDriveTypeA root: {error}"))?,
                                )
                            })
                        } else {
                            None
                        };
                        let crt_toupper = if operation == child_loader::ProviderOp::CrtToUpper {
                            Some(
                                read_guest_words(
                                    &X86Memory(&child.address_space),
                                    exit.registers.esp,
                                    2,
                                )?[1],
                            )
                        } else {
                            None
                        };
                        let crt_decimal = if cfg!(feature = "trace-api") && matches!(
                            operation,
                            child_loader::ProviderOp::CrtAtoi | child_loader::ProviderOp::CrtAtol
                        ) {
                            let string = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                2,
                            )?[1];
                            Some(
                                wc3::process::crt_parse_decimal_i32(
                                    &X86Memory(&child.address_space),
                                    string,
                                    if operation == child_loader::ProviderOp::CrtAtoi { "atoi" } else { "atol" },
                                )
                                .map_err(|error| format!("atol input: {error}"))?,
                            )
                        } else {
                            None
                        };
                        let crt_srand = if operation == child_loader::ProviderOp::CrtSrand {
                            Some(
                                read_guest_words(
                                    &X86Memory(&child.address_space),
                                    exit.registers.esp,
                                    2,
                                )?[1],
                            )
                        } else {
                            None
                        };
                        let crt_rand = operation == child_loader::ProviderOp::CrtRand;
                        let get_volume_information = if operation
                            == child_loader::ProviderOp::GetVolumeInformationA
                        {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                9,
                            )?;
                            let [
                                _,
                                root_ptr,
                                volume_name,
                                volume_cap,
                                serial,
                                max_component,
                                fs_flags,
                                fs_name,
                                fs_name_cap,
                            ] = frame.as_slice()
                            else {
                                unreachable!("GetVolumeInformationA frame has nine words")
                            };
                            let root = if *root_ptr == 0 {
                                None
                            } else {
                                Some(
                                    wc3::process::read_c_string(
                                        &X86Memory(&child.address_space),
                                        *root_ptr,
                                        1024,
                                    )
                                    .map_err(|error| {
                                        format!("GetVolumeInformationA root: {error}")
                                    })?,
                                )
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETVOLUMEINFORMATIONA CALL pid={} tid={} root={:?} volume_name=0x{:08x} volume_cap={} serial=0x{:08x} max_component=0x{:08x} fs_flags=0x{:08x} fs_name=0x{:08x} fs_name_cap={} caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    root.as_deref().unwrap_or("<current-directory>"),
                                    volume_name,
                                    volume_cap,
                                    serial,
                                    max_component,
                                    fs_flags,
                                    fs_name,
                                    fs_name_cap,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                            Some(root)
                        } else {
                            None
                        };
                        let get_disk_free_space = if operation
                            == child_loader::ProviderOp::GetDiskFreeSpaceA
                        {
                            let frame = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                6,
                            )?;
                            let [
                                _,
                                root_ptr,
                                sectors_per_cluster,
                                bytes_per_sector,
                                free_clusters,
                                total_clusters,
                            ] = frame.as_slice()
                            else {
                                unreachable!("GetDiskFreeSpaceA frame has six words")
                            };
                            let root = if *root_ptr == 0 {
                                None
                            } else {
                                Some(
                                    wc3::process::read_c_string(
                                        &X86Memory(&child.address_space),
                                        *root_ptr,
                                        1024,
                                    )
                                    .map_err(|error| format!("GetDiskFreeSpaceA root: {error}"))?,
                                )
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETDISKFREESPACEA CALL pid={} tid={} root={:?} sectors_per_cluster=0x{:08x} bytes_per_sector=0x{:08x} free_clusters=0x{:08x} total_clusters=0x{:08x} caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    root.as_deref().unwrap_or("<current-directory>"),
                                    sectors_per_cluster,
                                    bytes_per_sector,
                                    free_clusters,
                                    total_clusters,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                            Some(root)
                        } else {
                            None
                        };
                        if operation == child_loader::ProviderOp::FindFirstFileA {
                            let [_, pattern, find_data] = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                3,
                            )?[..] else {
                                unreachable!("FindFirstFileA frame has three words")
                            };
                            let pattern_text = if pattern == 0 {
                                "<null>".to_owned()
                            } else {
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    pattern,
                                    1024,
                                )
                                .map_err(|error| format!("FindFirstFileA pattern: {error}"))?
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD FINDFIRSTFILEA CALL pid={} tid={} during=\"{}\" pattern={:?} find_data=0x{:08x} caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    pattern_text,
                                    find_data,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                        }
                        let disable_thread_library_calls =
                            if operation == child_loader::ProviderOp::DisableThreadLibraryCalls {
                                let module = read_guest_words(
                                    &X86Memory(&child.address_space),
                                    exit.registers.esp,
                                    2,
                                )?[1];
                                let was_disabled = session
                                    .process(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .thread_library_calls_disabled(module)
                                    .unwrap_or(false);
                                Some((module, was_disabled))
                            } else {
                                None
                            };
                        let gl_needs_frame = matches!(operation, child_loader::ProviderOp::WglSwapLayerBuffers|child_loader::ProviderOp::GlFinish)
                            || (operation == child_loader::ProviderOp::GlDrawElements
                                && session.process(active_pid).is_some_and(|p|p.xp.gl_preview_pending(active_tid))
                                && read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 3)?[2] != 0)
                            || (operation == child_loader::ProviderOp::GlClear
                                && read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 2)?[1] & 0x0000_4000 != 0);
                        let gl_draw_frame = if gl_needs_frame {
                            let (_hglrc, hwnd, _mode) = session.process(active_pid)
                                .ok_or_else(|| "GL process missing".to_owned())?
                                .xp.gl_context_diagnostic(active_tid)
                                .ok_or_else(|| "GL call has no current context".to_owned())?;
                            let dimensions=session.windows.get(&hwnd).map(|w|(w.width,w.height)).ok_or("GL window missing")?;
                            let frame = frames.get_mut(&hwnd)
                                .ok_or_else(|| format!("GL call hwnd=0x{hwnd:08x} has no UI4 frame"))?;
                            frame.begin_gpu_frame().map_err(|error| format!("GL begin UI4 frame: {error:?}"))?;
                            let window_id = frame.window_id();
                            session.process_mut(active_pid)
                                .ok_or_else(|| "GL process missing".to_owned())?
                                .xp.bind_gl_ui4_window(hwnd, window_id, dimensions.0, dimensions.1);
                            Some(hwnd)
                        } else {
                            None
                        };
                        let dispatch = {
                            let mut child_memory = X86Memory(&child.address_space);
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .dispatch_provider_for_process_typed_with_self_image(
                                    active_pid,
                                    active_tid,
                                    provider_id,
                                    exit.registers.esp,
                                    &mut child_memory,
                                    Some(child.self_image_bytes.as_slice()),
                                )
                        };
                        match dispatch {
                            Ok(PersonalityAction::OpenFile(request)) => {
                                let listing = async_fs::list_dir(b"/common/Warcraft III")
                                    .await
                                    .map_err(|error| {
                                        format!(
                                            "list Warcraft III directory for CreateFileA: TRUEOSFS {error}"
                                        )
                                    })?;
                                if listing.truncated {
                                    return Err("Warcraft III directory listing truncated".into());
                                }
                                let stored = child_loader::resolve_file(&listing, &request.path)
                                    .map_err(str::to_owned)?;
                                let win_path = format!(r"C:\Warcraft III\{}", request.path);
                                let result = if let Some(stored) = stored {
                                    let trueos_path = format!("/common/Warcraft III/{stored}");
                                    let bytes = async_fs::read_file(trueos_path.as_bytes())
                                        .await
                                        .map_err(|error| {
                                            format!(
                                                "read {trueos_path} for CreateFileA: TRUEOSFS {error}"
                                            )
                                        })?;
                                    let bytes = Arc::new(bytes);
                                    let byte_len = bytes.len();
                                    let handle = session
                                        .process_mut(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .admit_trueos_file(
                                            format!(r"C:\Warcraft III\{stored}"),
                                            trueos_path.clone(),
                                            bytes,
                                            request.desired_access,
                                            request.share_mode,
                                        )
                                        .map_err(str::to_owned)?;
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CREATEFILE TRUEOSFS pid={} tid={} win_path={:?} mount=\"C:\\Warcraft III\" trueos_dir=\"/common/Warcraft III\" stored={:?} trueos_path={:?} bytes={} disposition=OPEN_EXISTING result=0x{:08x} last_error=0",
                                            active_pid, active_tid, win_path, stored, trueos_path,
                                            byte_len, handle,
                                        ),
                                    );
                                    handle
                                } else {
                                    session
                                        .process_mut(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .set_last_error_for_thread(active_tid, ERROR_FILE_NOT_FOUND);
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CREATEFILE TRUEOSFS pid={} tid={} win_path={:?} mount=\"C:\\Warcraft III\" trueos_dir=\"/common/Warcraft III\" stored=None disposition=OPEN_EXISTING result=0xffffffff last_error={}",
                                            active_pid, active_tid, win_path, ERROR_FILE_NOT_FOUND,
                                        ),
                                    );
                                    u32::MAX
                                };
                                let mut registers = exit.registers;
                                registers.eax = result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            Ok(PersonalityAction::Return(result)) => {
                                if operation == child_loader::ProviderOp::WglMakeCurrent && result != 0 {
                                    if let Some((_,hwnd,_))=session.process(active_pid).and_then(|p|p.xp.gl_context_diagnostic(active_tid)) {
                                        if let (Some(window),Some(frame))=(session.windows.get(&hwnd),frames.get(&hwnd)) {
                                            let (width,height,window_id)=(window.width,window.height,frame.window_id());
                                            session.process_mut(active_pid).ok_or("GL process missing")?.xp.bind_gl_ui4_window(hwnd,window_id,width,height);
                                        }
                                    }
                                }
                                if let Some(hwnd) = gl_draw_frame {
                                    let window = session.windows.get(&hwnd)
                                        .ok_or_else(|| "GL window missing".to_owned())?;
                                    frames.get_mut(&hwnd)
                                        .ok_or_else(|| "GL UI4 frame missing".to_owned())?
                                        .publish(Damage::full(window.width, window.height))
                                        .map_err(|error| format!("GL publish UI4 frame: {error:?}"))?;
                                }
                                if operation == child_loader::ProviderOp::GetSystemInfo {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD PROVIDER RETURN pid={} tid={} module=\"{}\" symbol=\"GetSystemInfo\" eax=0x{:08x} cleanup=4-by-thunk",
                                            active_pid, active_tid, provider.module, result,
                                        ),
                                    );
                                    if !child.get_system_info_consumer_logged {
                                        let output = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp + 4,
                                            1,
                                        )?[0];
                                        let bytes = read_guest_bytes(
                                            &X86Memory(&child.address_space),
                                            0x0040_2a5d,
                                            96,
                                        )?;
                                        child.get_system_info_consumer_logged = true;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GETSYSTEMINFO CONSUMER output=0x{:08x} return=0x00402a5d bytes=\"{}\"",
                                                output,
                                                diagnostic_hex_bytes(&bytes),
                                            ),
                                        );
                                    }
                                }
                                if let Some(buffer) = global_memory_status {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GLOBALMEMORYSTATUS pid={} tid={} buffer=0x{:08x} length=32 memory_load={} total_phys={} avail_phys={} total_pagefile={} avail_pagefile={} total_virtual={} avail_virtual={} cleanup={}-by-thunk",
                                            active_pid,
                                            active_tid,
                                            buffer,
                                            wc3::process::XP_MEMORY_LOAD,
                                            wc3::process::XP_TOTAL_PHYS,
                                            wc3::process::XP_AVAIL_PHYS,
                                            wc3::process::XP_TOTAL_PAGEFILE,
                                            wc3::process::XP_AVAIL_PAGEFILE,
                                            wc3::process::XP_TOTAL_VIRTUAL,
                                            wc3::process::XP_AVAIL_VIRTUAL,
                                            operation.stack_cleanup_bytes(),
                                        ),
                                    );
                                }
                                if let Some((target, value, old)) = interlocked_exchange {
                                    let after = read_guest_words(
                                        &X86Memory(&child.address_space),
                                        target,
                                        1,
                                    )?[0];
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD INTERLOCKEDEXCHANGE pid={} tid={} target=0x{:08x} old=0x{:08x} value=0x{:08x} after=0x{:08x} eax=0x{:08x} cleanup=8-by-thunk",
                                            active_pid,
                                            active_tid,
                                            target,
                                            old,
                                            value,
                                            after,
                                            result,
                                        ),
                                    );
                                }
                                if let Some((target, old)) = interlocked_add {
                                    let after = read_guest_words(
                                        &X86Memory(&child.address_space),
                                        target,
                                        1,
                                    )?[0];
                                    let api = match operation {
                                        child_loader::ProviderOp::InterlockedIncrement => {
                                            "INTERLOCKEDINCREMENT"
                                        }
                                        child_loader::ProviderOp::InterlockedDecrement => {
                                            "INTERLOCKEDDECREMENT"
                                        }
                                        _ => unreachable!(
                                            "interlocked add capture has an add operation"
                                        ),
                                    };
                                    let cleanup = operation.stack_cleanup_bytes();
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD {} pid={} tid={} target=0x{:08x} old=0x{:08x} after=0x{:08x} eax=0x{:08x} cleanup={}-by-thunk",
                                            api, active_pid, active_tid, target, old, after, result, cleanup,
                                        ),
                                    );
                                }
                                if let Some((destination, source, count)) = crt_memmove {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT MEMMOVE pid={} tid={} destination=0x{:08x} source=0x{:08x} count={} overlap={} return_eax=0x{:08x} cleanup=0-by-thunk",
                                            active_pid,
                                            active_tid,
                                            destination,
                                            source,
                                            count,
                                            ranges_overlap(destination, source, count) as u8,
                                            result,
                                        ),
                                    );
                                }
                                if let Some((path, attributes)) = set_file_attributes {
                                    let last_error = session
                                        .process(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .last_error();
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD SETFILEATTRIBUTESA path={path:?} attributes=0x{attributes:08x} exists={} result={} last_error={}",
                                            u8::from(result != 0),
                                            result,
                                            last_error,
                                        ),
                                    );
                                }
                                if let Some(root) = get_drive_type_root {
                                    let kind = match result {
                                        wc3::process::DRIVE_FIXED => "DRIVE_FIXED",
                                        wc3::process::DRIVE_NO_ROOT_DIR => "DRIVE_NO_ROOT_DIR",
                                        _ => "UNKNOWN",
                                    };
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GETDRIVETYPEA pid={} tid={} root={:?} result={} kind={} cleanup=4-by-thunk",
                                            active_pid,
                                            active_tid,
                                            root.as_deref().unwrap_or("<current-directory>"),
                                            result,
                                            kind,
                                        ),
                                    );
                                }
                                if let Some(character) = crt_toupper {
                                    logl::trace!(
                                        "trace-api",
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT TOUPPER pid={} tid={} character=0x{:08x} result=0x{:08x} cleanup=0-by-thunk",
                                            active_pid, active_tid, character, result,
                                        ),
                                    );
                                }
                                if let Some((_, overflow, input)) = crt_decimal {
                                    let api = if operation == child_loader::ProviderOp::CrtAtoi {
                                        "ATOI"
                                    } else {
                                        "ATOL"
                                    };
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT {} pid={} tid={} input={:?} result={} eax=0x{:08x} overflow={} cleanup=0-by-thunk",
                                            api,
                                            active_pid,
                                            active_tid,
                                            input,
                                            result as i32,
                                            result,
                                            overflow as u8,
                                        ),
                                    );
                                }
                                if let Some(seed) = crt_srand {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT SRAND pid={} tid={} seed=0x{:08x} cleanup=0-by-thunk",
                                            active_pid, active_tid, seed,
                                        ),
                                    );
                                }
                                if cfg!(feature = "trace-api") && crt_rand {
                                    let state = session
                                        .process(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .crt_rng_seed();
                                    logl::trace!(
                                        "trace-api",
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT RAND pid={} tid={} result={} eax=0x{:08x} state=0x{:08x} cleanup=0-by-thunk",
                                            active_pid, active_tid, result, result, state,
                                        ),
                                    );
                                }
                                if let Some(root) = get_volume_information {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GETVOLUMEINFORMATIONA RESULT root={:?} volume={:?} serial=0x{:08x} max_component={} fs_flags=0x{:08x} fs={:?} result={} cleanup=32-by-thunk",
                                            root.as_deref().unwrap_or("<current-directory>"),
                                            wc3::process::XP_C_VOLUME_NAME,
                                            wc3::process::XP_C_VOLUME_SERIAL,
                                            wc3::process::XP_C_MAX_COMPONENT,
                                            wc3::process::XP_C_FS_FLAGS,
                                            wc3::process::XP_C_FILE_SYSTEM_NAME,
                                            result,
                                        ),
                                    );
                                }
                                if let Some(root) = get_disk_free_space {
                                    let geometry = wc3::process::XP_C_DISK_GEOMETRY;
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GETDISKFREESPACEA RESULT root={:?} sectors_per_cluster={} bytes_per_sector={} free_clusters={} total_clusters={} capacity_bytes={} result={} cleanup=20-by-thunk",
                                            root.as_deref().unwrap_or("<current-directory>"),
                                            geometry.sectors_per_cluster,
                                            geometry.bytes_per_sector,
                                            geometry.free_clusters,
                                            geometry.total_clusters,
                                            wc3::process::XP_C_DISK_BYTES,
                                            result,
                                        ),
                                    );
                                }
                                if let Some((module, was_disabled)) = disable_thread_library_calls {
                                    let now_disabled = session
                                        .process(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .thread_library_calls_disabled(module)
                                        .unwrap_or(false);
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD DISABLETHREADLIBRARYCALLS pid={} tid={} during={:?} module=0x{:08x} was_disabled={} now_disabled={} result={}",
                                            active_pid,
                                            active_tid,
                                            running_module_name,
                                            module,
                                            was_disabled as u8,
                                            now_disabled as u8,
                                            result,
                                        ),
                                    );
                                }
                                if let Some(frame) = process_memory {
                                    let [_, process, first, second, size, bytes_transferred] =
                                        frame.as_slice()
                                    else {
                                        unreachable!("process-memory frame has six words")
                                    };
                                    match operation {
                                        child_loader::ProviderOp::ReadProcessMemory => logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD READPROCESSMEMORY pid={} tid={} process=0x{:08x} source=0x{:08x} destination=0x{:08x} size={} bytes_read=0x{:08x} result={}",
                                                active_pid,
                                                active_tid,
                                                process,
                                                first,
                                                second,
                                                size,
                                                bytes_transferred,
                                                result,
                                            ),
                                        ),
                                        child_loader::ProviderOp::WriteProcessMemory => logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD WRITEPROCESSMEMORY pid={} tid={} process=0x{:08x} destination=0x{:08x} source=0x{:08x} size={} bytes_written=0x{:08x} result={}",
                                                active_pid,
                                                active_tid,
                                                process,
                                                first,
                                                second,
                                                size,
                                                bytes_transferred,
                                                result,
                                            ),
                                        ),
                                        _ => unreachable!(
                                            "process-memory operation was classified before dispatch"
                                        ),
                                    }
                                }
                                match operation {
                                    child_loader::ProviderOp::RegisterClassA if result != 0 => {
                                        let [_, structure] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("RegisterClassA frame has two words")
                                        };
                                        let fields = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            structure,
                                            10,
                                        )?;
                                        let class = wc3::process::read_c_string(
                                            &X86Memory(&child.address_space),
                                            fields[9],
                                            256,
                                        )?;
                                        let menu = if fields[8] == 0 || fields[8] >> 16 == 0 {
                                            None
                                        } else {
                                            Some(wc3::process::read_c_string(
                                                &X86Memory(&child.address_space),
                                                fields[8],
                                                256,
                                            )?)
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD REGISTERCLASSA RESULT pid={} tid={} class={:?} style=0x{:08x} wndproc=0x{:08x} cls_extra={} wnd_extra={} instance=0x{:08x} icon=0x{:08x} cursor=0x{:08x} background=0x{:08x} menu={:?} atom={} result=success cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                class,
                                                fields[0],
                                                fields[1],
                                                fields[2] as i32,
                                                fields[3] as i32,
                                                fields[4],
                                                fields[5],
                                                fields[6],
                                                fields[7],
                                                menu,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::RegisterClassExA if result != 0 => {
                                        let [_, structure] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("RegisterClassExA frame has two words")
                                        };
                                        let fields = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            structure,
                                            12,
                                        )?;
                                        let class = wc3::process::read_c_string(
                                            &X86Memory(&child.address_space),
                                            fields[10],
                                            256,
                                        )?;
                                        let menu = if fields[9] == 0 || fields[9] >> 16 == 0 {
                                            None
                                        } else {
                                            Some(wc3::process::read_c_string(
                                                &X86Memory(&child.address_space),
                                                fields[9],
                                                256,
                                            )?)
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD REGISTERCLASSEXA pid={} tid={} cb_size={} class={:?} style=0x{:08x} wndproc=0x{:08x} instance=0x{:08x} icon=0x{:08x} cursor=0x{:08x} background=0x{:08x} menu={:?} icon_sm=0x{:08x} atom={} result=success cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                fields[0],
                                                class,
                                                fields[1],
                                                fields[2],
                                                fields[5],
                                                fields[6],
                                                fields[7],
                                                fields[8],
                                                menu,
                                                fields[11],
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::LoadCursorA => {
                                        let cursor = session
                                            .process(active_pid)
                                            .and_then(|process| process.xp.user_image_cursor_result(result))
                                            .ok_or_else(|| "LoadCursorA result handle missing".to_owned())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD LOADCURSORA RESULT pid={} tid={} module={:?} name={:?} selected={}x{} cursor_resource_id={} hotspot={},{} resource_bytes={} handle=0x{:08x} shared={} result=success cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                session.process(active_pid).and_then(|process| process.xp.loaded_module_name(cursor.module)).unwrap_or("<unknown>"),
                                                cursor.name,
                                                cursor.width,
                                                cursor.height,
                                                cursor.resource_id,
                                                cursor.hotspot_x,
                                                cursor.hotspot_y,
                                                cursor.resource_bytes,
                                                cursor.handle,
                                                cursor.shared as u8,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::LoadImageA => {
                                        let [_, module, name_ptr, image_type, cx, cy, flags] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            7,
                                        )?[..]
                                        else {
                                            unreachable!("LoadImageA frame has seven words")
                                        };
                                        let image = session
                                            .process(active_pid)
                                            .and_then(|process| process.xp.user_image_load_result(result))
                                            .ok_or_else(|| "LoadImageA result handle missing".to_owned())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD LOADIMAGEA RESULT pid={} tid={} module={:?} name={:?} type=IMAGE_ICON requested=default selected={}x{} image_resource_id={} handle=0x{:08x} result=success cleanup=24-by-thunk",
                                                active_pid,
                                                active_tid,
                                                session.process(active_pid).and_then(|process| process.xp.loaded_module_name(module)).unwrap_or("<unknown>"),
                                                image.name,
                                                image.width,
                                                image.height,
                                                image.resource_id,
                                                result,
                                            ),
                                        );
                                        let _ = (name_ptr, image_type, cx, cy, flags);
                                    }
                                    child_loader::ProviderOp::D3D8Release => {
                                        let [caller_ret, this] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("D3D8 Release frame has two words")
                                        };
                                        let references_after = result;
                                        let references_before = references_after
                                            .checked_add(1)
                                            .expect("successful D3D8 Release has a prior reference");
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD D3D8 RELEASE pid={} tid={} this=0x{:08x} references_before={} references_after={} result={} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                this,
                                                references_before,
                                                references_after,
                                                result,
                                                caller_ret,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::D3D8GetAdapterIdentifier => {
                                        let [_, this, adapter, flags, output] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            5,
                                        )?[..]
                                        else {
                                            unreachable!("D3D8 GetAdapterIdentifier frame has five words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD D3D8 GETADAPTERIDENTIFIER pid={} tid={} this=0x{:08x} adapter={} flags=0x{:08x} output=0x{:08x} vendor=0x{:04x} device=0x{:04x} subsys=0x{:08x} revision=0x{:02x} description=\"Intel(R) UHD Graphics 770\" result=0x{:08x} cleanup=16-by-thunk",
                                                active_pid,
                                                active_tid,
                                                this,
                                                adapter,
                                                flags,
                                                output,
                                                wc3::process::TRUEOS_D3D8_VENDOR_ID,
                                                wc3::process::TRUEOS_D3D8_DEVICE_ID,
                                                wc3::process::TRUEOS_D3D8_SUBSYSTEM_ID,
                                                wc3::process::TRUEOS_D3D8_REVISION,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::EnumDisplayDevicesA => {
                                        let [_, device_ptr, index, output, flags] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            5,
                                        )?[..]
                                        else {
                                            unreachable!("EnumDisplayDevicesA frame has five words")
                                        };
                                        // A null output buffer returns FALSE before the provider
                                        // dereferences lpDevice, so preserve that ABI behavior in
                                        // diagnostics as well.
                                        let device = if output == 0 || device_ptr == 0 {
                                            None
                                        } else {
                                            Some(
                                                wc3::process::read_c_string(
                                                    &X86Memory(&child.address_space),
                                                    device_ptr,
                                                    32,
                                                )
                                                .map_err(|error| {
                                                    format!("EnumDisplayDevicesA device: {error}")
                                                })?,
                                            )
                                        };
                                        let (kind, name, state) = match (result, device.as_deref()) {
                                            (1, None) => (
                                                "adapter",
                                                r"\\.\DISPLAY1",
                                                0x0000_0005u32,
                                            ),
                                            (1, Some(name))
                                                if name.eq_ignore_ascii_case(r"\\.\DISPLAY1") => (
                                                "monitor",
                                                r"\\.\DISPLAY1\Monitor0",
                                                0x0000_0001,
                                            ),
                                            _ => ("none", "", 0),
                                        };
                                        let cb = (output != 0)
                                            .then(|| {
                                                read_guest_words(
                                                    &X86Memory(&child.address_space),
                                                    output,
                                                    1,
                                                )
                                            })
                                            .transpose()?
                                            .map(|words| words[0]);
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD ENUMDISPLAYDEVICES pid={} tid={} device={:?} index={} output=0x{:08x} cb={:?} flags=0x{:08x} kind={} name={:?} state=0x{:08x} result={} cleanup=16-by-thunk",
                                                active_pid,
                                                active_tid,
                                                device.as_deref().unwrap_or("<null>"),
                                                index,
                                                output,
                                                cb,
                                                flags,
                                                kind,
                                                name,
                                                state,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::EnumDisplaySettingsA => {
                                        let [_, device_ptr, mode_num, output] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            4,
                                        )?[..]
                                        else {
                                            unreachable!("EnumDisplaySettingsA frame has four words")
                                        };
                                        let device = if output == 0 || device_ptr == 0 {
                                            None
                                        } else {
                                            Some(
                                                wc3::process::read_c_string(
                                                    &X86Memory(&child.address_space),
                                                    device_ptr,
                                                    32,
                                                )
                                                .map_err(|error| {
                                                    format!("EnumDisplaySettingsA device: {error}")
                                                })?,
                                            )
                                        };
                                        let mode_kind = match mode_num {
                                            wc3::process::ENUM_CURRENT_SETTINGS => "current",
                                            wc3::process::ENUM_REGISTRY_SETTINGS => "registry",
                                            0 => "enumerated",
                                            _ => "unsupported",
                                        };
                                        let dm_size = (result == 1)
                                            .then(|| {
                                                read_guest_words(
                                                    &X86Memory(&child.address_space),
                                                    output + 0x24,
                                                    1,
                                                )
                                            })
                                            .transpose()?
                                            .map(|words| words[0] & 0xffff);
                                        let (width, height) = if result == 1 {
                                            session
                                                .process(active_pid)
                                                .ok_or_else(|| "child process missing".to_owned())?
                                                .xp
                                                .desktop_size()
                                        } else {
                                            (0, 0)
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD ENUMDISPLAYSETTINGS pid={} tid={} device={:?} mode=0x{:08x} mode_kind={} output=0x{:08x} dm_size={:?} width={} height={} bpp=32 frequency=60 result={} cleanup=12-by-thunk",
                                                active_pid,
                                                active_tid,
                                                device.as_deref().unwrap_or("<null>"),
                                                mode_num,
                                                mode_kind,
                                                output,
                                                dm_size,
                                                width,
                                                height,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::ChangeDisplaySettingsExA => {
                                        let [_, device_ptr, _devmode, _hwnd, flags, _lparam] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            6,
                                        )?[..]
                                        else {
                                            unreachable!("ChangeDisplaySettingsExA frame has six words")
                                        };
                                        let device = wc3::process::read_c_string(
                                            &X86Memory(&child.address_space),
                                            device_ptr,
                                            32,
                                        )?;
                                        let (width, height) = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .desktop_size();
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD CHANGEDISPLAYSETTINGSEXA RESULT pid={} tid={} device={:?} mode={}x{}x32@60 flags={} mode_change=none-current-mode ui4_frame_unchanged=1 result=DISP_CHANGE_SUCCESSFUL cleanup=20-by-thunk",
                                                active_pid,
                                                active_tid,
                                                device,
                                                width,
                                                height,
                                                if flags == 0x0000_0004 { "CDS_FULLSCREEN" } else { "UNKNOWN" },
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GetDeviceCaps => {
                                        let [_, hdc, index] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            3,
                                        )?[..]
                                        else {
                                            unreachable!("GetDeviceCaps frame has three words")
                                        };
                                        let capability = match index {
                                            2 => "TECHNOLOGY",
                                            8 => "HORZRES",
                                            10 => "VERTRES",
                                            12 => "BITSPIXEL",
                                            14 => "PLANES",
                                            24 => "NUMCOLORS",
                                            38 => "RASTERCAPS",
                                            88 => "LOGPIXELSX",
                                            90 => "LOGPIXELSY",
                                            104 => "SIZEPALETTE",
                                            106 => "NUMRESERVED",
                                            108 => "COLORRES",
                                            116 => "VREFRESH",
                                            117 => "DESKTOPVERTRES",
                                            118 => "DESKTOPHORZRES",
                                            _ => "UNKNOWN",
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GETDEVICECAPS RESULT pid={} tid={} hdc=0x{:08x} index={} capability={} result=0x{:08x} cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hdc,
                                                index,
                                                capability,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::SetPixelFormat => {
                                        let [_, hdc, format, ppfd] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            4,
                                        )?[..]
                                        else {
                                            unreachable!("SetPixelFormat frame has four words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD SETPIXELFORMAT RESULT pid={} tid={} hdc=0x{:08x} format={} ppfd=0x{:08x} window_format={} result={} cleanup=12-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hdc,
                                                format,
                                                ppfd,
                                                format,
                                                if result != 0 { "TRUE" } else { "FALSE" },
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::WglCreateContext => {
                                        let [_, hdc] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("wglCreateContext frame has two words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD WGLCREATECONTEXT RESULT pid={} tid={} hdc=0x{:08x} hglrc=0x{:08x} vgpu_device=opened render_queue=created ui4_surface=not-acquired current=0 result=success cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hdc,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::ReleaseDC => {
                                        let [_, hwnd, hdc] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            3,
                                        )?[..]
                                        else {
                                            unreachable!("ReleaseDC frame has three words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD RELEASEDC RESULT pid={} tid={} hwnd=0x{:08x} hdc=0x{:08x} persistent=1 retained=1 result={} cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hwnd,
                                                hdc,
                                                if result != 0 { "TRUE" } else { "FALSE" },
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::EndPaint => {
                                        let [_, hwnd, paint_struct] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            3,
                                        )?[..]
                                        else {
                                            unreachable!("EndPaint frame has three words")
                                        };
                                        let hdc = if paint_struct == 0 {
                                            0
                                        } else {
                                            read_guest_words(
                                                &X86Memory(&child.address_space),
                                                paint_struct,
                                                1,
                                            )?[0]
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD ENDPAINT pid={} tid={} hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target=WINDOW_PAINT retired={} result={} cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hwnd,
                                                paint_struct,
                                                hdc,
                                                result as u8,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::ClipCursor if cfg!(feature = "trace-api") || result == 0 => {
                                        let [_, rect_ptr] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("ClipCursor frame has two words")
                                        };
                                        let rect = if rect_ptr == 0 {
                                            None
                                        } else {
                                            let words = read_guest_words(
                                                &X86Memory(&child.address_space),
                                                rect_ptr,
                                                4,
                                            )?;
                                            Some([
                                                words[0] as i32,
                                                words[1] as i32,
                                                words[2] as i32,
                                                words[3] as i32,
                                            ])
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD CLIPCURSOR RESULT pid={} tid={} rect_ptr=0x{:08x} rect={:?} trueos_cursor_action=ignored result={} cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                rect_ptr,
                                                rect,
                                                if result != 0 { "TRUE" } else { "FALSE" },
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::WglMakeCurrent => {
                                        let [_, hdc, hglrc] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            3,
                                        )?[..]
                                        else {
                                            unreachable!("wglMakeCurrent frame has three words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD WGLMAKECURRENT RESULT pid={} tid={} hdc=0x{:08x} hglrc=0x{:08x} pixel_format=1 current_tid={} ui4_surface=not-acquired result={} cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hdc,
                                                hglrc,
                                                active_tid,
                                                if result != 0 { "TRUE" } else { "FALSE" },
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GlGetString => {
                                        let [_, name] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("glGetString frame has two words")
                                        };
                                        let name_label = match name {
                                            0x0000_1f02 => "GL_VERSION",
                                            0x0000_1f03 => "GL_EXTENSIONS",
                                            _ => "UNKNOWN",
                                        };
                                        let value = if result != 0 {
                                            wc3::process::read_c_string(
                                                &X86Memory(&child.address_space),
                                                result,
                                                4096,
                                            )?
                                        } else {
                                            "<null>".into()
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GLGETSTRING RESULT pid={} tid={} name={} value={:?} pointer=0x{:08x} result=success cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                name_label,
                                                value,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GlMatrixMode => {
                                        let [_, mode] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..]
                                        else {
                                            unreachable!("glMatrixMode frame has two words")
                                        };
                                        let (hglrc, _hwnd, current_mode) = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .gl_context_diagnostic(active_tid)
                                            .ok_or_else(|| "current GL context missing after glMatrixMode".to_owned())?;
                                        let mode_name = match mode {
                                            0x0000_1700 => "GL_MODELVIEW",
                                            0x0000_1701 => "GL_PROJECTION",
                                            0x0000_1702 => "GL_TEXTURE",
                                            _ => "UNKNOWN",
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GLMATRIXMODE RESULT pid={} tid={} hglrc=0x{:08x} mode={} stored_mode=0x{:08x} ui4_surface=not-acquired result=void cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hglrc,
                                                mode_name,
                                                current_mode,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GlLightModelfv => {
                                        let (hglrc, ambient) = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .gl_light_model_ambient_diagnostic(active_tid)
                                            .ok_or_else(|| "current GL context missing after glLightModelfv".to_owned())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GLLIGHTMODELFV RESULT pid={} tid={} hglrc=0x{:08x} pname=GL_LIGHT_MODEL_AMBIENT stored={:?} result=void cleanup=8-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hglrc,
                                                ambient,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GlLightfv => {
                                        let [_, _light, pname, params] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            4,
                                        )?[..]
                                        else {
                                            unreachable!("glLightfv frame has four words")
                                        };
                                        let mut raw = [0u8; 16];
                                        X86Memory(&child.address_space).read(params, &mut raw)?;
                                        let source = core::array::from_fn::<f32, 4, _>(|index| {
                                            f32::from_le_bytes(
                                                raw[index * 4..index * 4 + 4].try_into().unwrap(),
                                            )
                                        });
                                        let process = &session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp;
                                        let (hglrc, stored, pname_name, modelview_applied) = match pname {
                                            0x0000_1200 => {
                                                let (hglrc, stored) = process
                                                    .gl_light0_ambient_diagnostic(active_tid)
                                                    .ok_or_else(|| "current GL context missing after glLightfv".to_owned())?;
                                                (hglrc, stored, "GL_AMBIENT", 0)
                                            }
                                            0x0000_1201 => {
                                                let (hglrc, stored) = process
                                                    .gl_light0_diffuse_diagnostic(active_tid)
                                                    .ok_or_else(|| "current GL context missing after glLightfv".to_owned())?;
                                                (hglrc, stored, "GL_DIFFUSE", 0)
                                            }
                                            0x0000_1202 => {
                                                let (hglrc, stored) = process
                                                    .gl_light0_specular_diagnostic(active_tid)
                                                    .ok_or_else(|| "current GL context missing after glLightfv".to_owned())?;
                                                (hglrc, stored, "GL_SPECULAR", 0)
                                            }
                                            0x0000_1203 => {
                                                let (hglrc, stored) = process
                                                    .gl_light0_position_eye_diagnostic(active_tid)
                                                    .ok_or_else(|| "current GL context missing after glLightfv".to_owned())?;
                                                (hglrc, stored, "GL_POSITION", 1)
                                            }
                                            _ => return Err("unexpected glLightfv pname after dispatch".into()),
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GLLIGHTFV RESULT pid={} tid={} hglrc=0x{:08x} light=GL_LIGHT0 pname={} source={:?} stored={:?} modelview_applied={} result=void cleanup=12-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hglrc,
                                                pname_name,
                                                source,
                                                stored,
                                                modelview_applied,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GlDisable => {
                                        let (hglrc, enabled) = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .gl_light0_enabled_diagnostic(active_tid)
                                            .ok_or_else(|| "current GL context missing after glDisable".to_owned())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GLDISABLE RESULT pid={} tid={} hglrc=0x{:08x} cap=GL_LIGHT0 previous_enabled=0 enabled={} light_parameters_preserved=1 result=void cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                hglrc,
                                                enabled as u8,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::CrtSscanf => {
                                        let [_, input, format, major, minor] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            5,
                                        )?[..]
                                        else {
                                            unreachable!("sscanf frame has five words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD SSCANF RESULT pid={} tid={} input=0x{:08x} format=0x{:08x} major=0x{:08x} minor=0x{:08x} assignments={} cleanup=0-by-thunk",
                                                active_pid,
                                                active_tid,
                                                input,
                                                format,
                                                major,
                                                minor,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::FormatMessageA => {
                                        let frame = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            8,
                                        )?;
                                        let [
                                            _,
                                            flags,
                                            source,
                                            message_id,
                                            language_id,
                                            buffer,
                                            _capacity,
                                            _arguments,
                                        ] = frame.as_slice()
                                        else {
                                            unreachable!("FormatMessageA frame has eight words")
                                        };
                                        let encoding = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .last_format_message_encoding()
                                            .map(|value| match value {
                                                wc3::process::MessageResourceEncoding::Ansi => {
                                                    "ansi"
                                                }
                                                wc3::process::MessageResourceEncoding::Unicode => {
                                                    "unicode"
                                                }
                                            })
                                            .unwrap_or("none");
                                        let source_kind = match *flags {
                                            0x0000_0800 => "FROM_HMODULE",
                                            0x0000_1000 => "FROM_SYSTEM",
                                            _ => "UNKNOWN",
                                        };
                                        let (resolved_language_id, language_kind) = if *flags == 0x0000_1000 {
                                            (0, "ignored")
                                        } else {
                                            wc3::process::format_message_language_resolution(
                                                *language_id,
                                            )
                                        };
                                        let message_name = match *message_id {
                                            2 => "ERROR_FILE_NOT_FOUND",
                                            _ => "UNKNOWN",
                                        };
                                        let last_error = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .last_error();
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD FORMATMESSAGEA LANGUAGE pid={} tid={} flags={} requested=0x{:04x} resolved=0x{:04x} kind={}",
                                                active_pid,
                                                active_tid,
                                                source_kind,
                                                language_id,
                                                resolved_language_id,
                                                language_kind,
                                            ),
                                        );
                                        let text = if result == 0 {
                                            String::new()
                                        } else {
                                            let bytes = read_guest_bytes(
                                                &X86Memory(&child.address_space),
                                                *buffer,
                                                result as usize,
                                            )?;
                                            wc3::ThisToThat::cp1252_to_string(&bytes)
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD FORMATMESSAGEA RESULT pid={} tid={} flags={} source=0x{:08x} message_id=0x{:08x} message_name={} language_id=0x{:04x} encoding={} chars={} text={:?} eax=0x{:08x} last_error={} cleanup=28-by-thunk",
                                                active_pid,
                                                active_tid,
                                                source_kind,
                                                source,
                                                message_id,
                                                message_name,
                                                language_id,
                                                encoding,
                                                result,
                                                text,
                                                result,
                                                last_error,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GetProcessHeap => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GETPROCESSHEAP pid={} tid={} during=\"{}\" handle=0x{:08x}",
                                            active_pid, active_tid, running_module_name, result,
                                        ),
                                    ),
                                    child_loader::ProviderOp::GetTickCount => {
                                        let caller_return = u32::from_le_bytes(caller_ret);
                                        let caller_module = child_pc_owner(child, caller_return)
                                            .map(|(owner, _)| owner.to_owned())
                                            .unwrap_or_else(|| running_module_name.clone());
                                        logl::trace!(
                                            "trace-api",
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD GETTICKCOUNT pid={} tid={} during=\"{}\" milliseconds={}",
                                                active_pid, active_tid, caller_module, result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::Sleep => {
                                        let [_, milliseconds] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..] else {
                                            unreachable!("Sleep frame has two words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD SLEEP pid={} tid={} during=\"{}\" requested_ms={} applied_delay_ms=0 effect=yield cleanup=4-by-thunk",
                                                active_pid,
                                                active_tid,
                                                running_module_name,
                                                milliseconds,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::GetCurrentThreadId => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD GETCURRENTTHREADID pid={} tid={} during=\"{}\" result={}",
                                            active_pid, active_tid, running_module_name, result,
                                        ),
                                    ),
                                    child_loader::ProviderOp::TlsAlloc => logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD TLS ALLOC pid={} tid={} during=\"{}\" slot={} result=0x{:08x}",
                                            active_pid,
                                            active_tid,
                                            running_module_name,
                                            result,
                                            result,
                                        ),
                                    ),
                                    child_loader::ProviderOp::TlsSetValue if cfg!(feature = "trace-api") || result == 0 => {
                                        let [_, slot, value] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            3,
                                        )?[..] else {
                                            unreachable!("TlsSetValue frame has three words")
                                        };
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD TLS SET pid={} tid={} during=\"{}\" slot={} value=0x{:08x} result={}",
                                                active_pid,
                                                active_tid,
                                                running_module_name,
                                                slot,
                                                value,
                                                result,
                                            ),
                                        );
                                    }
                                    child_loader::ProviderOp::TlsGetValue if cfg!(feature = "trace-api") => {
                                        let [_, slot] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..] else {
                                            unreachable!("TlsGetValue frame has two words")
                                        };
                                        logl::trace!(
                                            "trace-api",
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD TLS GET pid={} tid={} during=\"{}\" slot={} value=0x{:08x}",
                                                active_pid,
                                                active_tid,
                                                running_module_name,
                                                slot,
                                                result,
                                            ),
                                        );
                                    }
                                    _ => {}
                                }
                                logl::trace!(
                                    "trace-api",
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER RETURN pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eax=0x{:08x} cleanup={}-by-thunk",
                                        active_pid,
                                        active_tid,
                                        running_module_name,
                                        provider_id,
                                        provider.module,
                                        symbol(),
                                        result,
                                        operation.stack_cleanup_bytes(),
                                    ),
                                );
                                let mut registers = exit.registers;
                                registers.eax = result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                if operation == child_loader::ProviderOp::Sleep {
                                    tokio::task::yield_now().await;
                                }
                                continue;
                            }
                            Ok(_) => {
                                return Err("pure child provider requested a runtime effect".into());
                            }
                            Err(ProviderDispatchError::Unsupported) => {}
                            Err(ProviderDispatchError::Fault(error)) => {
                                return Err(format!("child provider semantic fault: {error}"));
                            }
                            Err(ProviderDispatchError::Frontier { api, detail }) => {
                                if provider.module.eq_ignore_ascii_case("OPENGL32.dll") {
                                    if let child_loader::ProviderSymbol::Name(name) = &provider.symbol {
                                        if let Some(signature) = wc3::gl_signatures::signature(name) {
                                            match wc3::gl_signatures::decode_call(
                                                signature,
                                                &X86Memory(&child.address_space),
                                                exit.registers.esp,
                                            ) {
                                                Ok(call) => {
                                                    logl::log(
                                                        level::IMPORTANT,
                                                        format_args!(
                                                            "WC3 CHILD GL SIGNATURE pid={} tid={} symbol={} arguments={} caller_ret=0x{:08x} disposition={:?}",
                                                            active_pid,
                                                            active_tid,
                                                            signature.symbol,
                                                            call.description,
                                                            u32::from_le_bytes(caller_ret),
                                                            signature.effect,
                                                        ),
                                                    );
                                                    if signature.effect == wc3::gl_signatures::GlEffect::NoteWrite
                                                        && !call.description.contains("<null>")
                                                    {
                                                        let noted = session
                                                            .process_mut(active_pid)
                                                            .ok_or_else(|| "child process missing".to_owned())?
                                                            .xp
                                                            .note_gl_write(
                                                                active_tid,
                                                                signature.symbol,
                                                                call.words,
                                                                call.description,
                                                            );
                                                        if let Ok((hglrc, note_count)) = noted {
                                                            logl::log(
                                                                level::IMPORTANT,
                                                                format_args!(
                                                                    "WC3 CHILD GL WRITE NOTE pid={} tid={} hglrc=0x{:08x} symbol={} notes={} modeled=0 source_frontier={:?} result=void cleanup={}-by-thunk",
                                                                    active_pid,
                                                                    active_tid,
                                                                    hglrc,
                                                                    signature.symbol,
                                                                    note_count,
                                                                    detail,
                                                                    operation.stack_cleanup_bytes(),
                                                                ),
                                                            );
                                                            let mut registers = exit.registers;
                                                            registers.eax = 0;
                                                            contexts[active]
                                                                .context
                                                                .set_registers(registers)
                                                                .map_err(|error| error.to_string())?;
                                                            continue;
                                                        }
                                                    }
                                                }
                                                Err(error) => logl::log(
                                                    level::IMPORTANT,
                                                    format_args!(
                                                        "WC3 CHILD GL SIGNATURE DECODE pid={} tid={} symbol={} error={:?}",
                                                        active_pid, active_tid, signature.symbol, error,
                                                    ),
                                                ),
                                            }
                                        }
                                    }
                                }
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x} api=\"{}\" detail={:?}",
                                        active_pid,
                                        active_tid,
                                        running_module_name,
                                        provider_id,
                                        provider.module,
                                        symbol(),
                                        exit.registers.eip,
                                        exit.registers.esp,
                                        u32::from_le_bytes(caller_ret),
                                        api,
                                        detail,
                                    ),
                                );
                                return Ok(());
                            }
                        }
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "FormatMessageA"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            8,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD FORMATMESSAGEA CALL pid={} tid={} during=\"{}\" provider_id={} caller_ret=0x{:08x} flags=0x{:08x} source=0x{:08x} message_id=0x{:08x} language_id=0x{:08x} buffer=0x{:08x} capacity={} arguments=0x{:08x} cleanup=28-by-thunk",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                frame[0],
                                frame[1],
                                frame[2],
                                frame[3],
                                frame[4],
                                frame[5],
                                frame[6],
                                frame[7],
                            ),
                        );
                    }
                    logl::log(
                        level::IMPORTANT,
                        format_args!(
                            "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" {} eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x}",
                            active_pid,
                            active_tid,
                            running_module_name,
                            provider_id,
                            provider.module,
                            symbol(),
                            exit.registers.eip,
                            exit.registers.esp,
                            u32::from_le_bytes(caller_ret)
                        ),
                    );
                    return Ok(());
                    }
