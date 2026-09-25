                if active_pid != LAUNCHER_PID {
                    let child = pending_child
                        .as_mut()
                        .filter(|child| child.pid == active_pid)
                        .ok_or_else(|| "active child address space missing".to_owned())?;
                    if exit.registers.eip == STORM_EXPAND_AFTER_TRAP {
                        complete_storm_record_expand(child, &session, &mut contexts[active], exit.registers)?;
                        continue;
                    }
                    if exit.registers.eip == WAR3_EVENT_POOL_AFTER_TRAP {
                        complete_war3_event_pool(child, &mut session, &mut contexts[active], exit.registers)?;
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_UEF_RETURN_AFTER_VMCALL {
                        let pending = child
                            .unhandled_filter_call
                            .take()
                            .ok_or("UEF return without pending filter call")?;
                        if exit.registers.esp != pending.return_esp {
                            return Err(format!(
                                "UEF callback ESP mismatch expected=0x{:08x} actual=0x{:08x}",
                                pending.return_esp, exit.registers.esp,
                            ));
                        }
                        match pending.continuation {
                            ChildUnhandledFilterContinuation::Provider { resume_eip } => {
                                let result = session
                                    .process(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .complete_unhandled_exception_filter(Some(exit.registers.eax))
                                    .map_err(str::to_owned)?;
                                let mut registers = exit.registers;
                                registers.eip = resume_eip;
                                registers.esp = pending.return_esp;
                                registers.eax = result;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD UEF FILTER RETURN pid={} tid={} filter=0x{:08x} filter_result=0x{:08x} uef_result=0x{:08x}",
                                        active_pid,
                                        active_tid,
                                        pending.filter,
                                        exit.registers.eax,
                                        result,
                                    ),
                                );
                                continue;
                            }
                            ChildUnhandledFilterContinuation::TerminalSeh { seh } => {
                                match exit.registers.eax {
                                    wc3::process::EXCEPTION_FILTER_CONTINUE_EXECUTION => {
                                        let mut bytes = [0; wc3::seh::X86_CONTEXT_BYTES];
                                        if child
                                            .address_space
                                            .read(seh.context_va, &mut bytes)
                                            .map_err(|error| error.to_string())?
                                            != bytes.len()
                                        {
                                            return Err("short terminal UEF context read".into());
                                        }
                                        let restored = wc3::seh::decode_x86_context(
                                            &bytes,
                                            seh.preserved_fs_base,
                                        )
                                        .map_err(str::to_owned)?;
                                        let restored_debug =
                                            wc3::seh::decode_x86_debug_registers(&bytes)
                                                .map_err(str::to_owned)?;
                                        contexts[active]
                                            .context
                                            .set_registers(restored)
                                            .map_err(|error| error.to_string())?;
                                        contexts[active]
                                            .context
                                            .set_debug_registers(restored_debug)
                                            .map_err(|error| error.to_string())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD TOPLEVEL FILTER CONTINUE pid={} tid={} filter_result=0xffffffff old_eip=0x{:08x} new_eip=0x{:08x}",
                                                active_pid,
                                                active_tid,
                                                seh.original_registers.eip,
                                                restored.eip,
                                            ),
                                        );
                                        continue;
                                    }
                                    wc3::process::EXCEPTION_FILTER_CONTINUE_SEARCH
                                    | wc3::process::EXCEPTION_FILTER_EXECUTE_HANDLER => {
                                        return Err(format!(
                                            "WC3 CHILD UNHANDLED EXCEPTION filter_result=0x{:08x} exception_eip=0x{:08x}",
                                            exit.registers.eax, seh.original_registers.eip,
                                        ));
                                    }
                                    value => {
                                        return Err(format!(
                                            "WC3 CHILD UEF FRONTIER reason=invalid-filter-result value=0x{value:08x}"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    if exit.registers.eip == thunk32::CHILD_SEH_RETURN_AFTER_VMCALL {
                        let seh = child
                            .seh
                            .take()
                            .ok_or("SEH return without pending dispatch")?;
                        if !seh.quiet {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH RETURN pid={} tid={} registration=0x{:08x} handler=0x{:08x} disposition={}",
                                    active_pid,
                                    active_tid,
                                    seh.registration,
                                    seh.handler,
                                    exit.registers.eax,
                                ),
                            );
                        }
                        if exit.registers.eax == wc3::seh::DISPOSITION_CONTINUE_SEARCH {
                            if seh.next_registration == u32::MAX {
                                let filter = session
                                    .process(active_pid)
                                    .ok_or_else(|| format!("missing process {active_pid}"))?
                                    .xp
                                    .unhandled_exception_filter();
                                if filter == 0 {
                                    return Err(
                                        "WC3 CHILD UEF FRONTIER reason=no-top-level-filter".into(),
                                    );
                                }

                                let exception_pointers_va = seh.exception_pointers_va;
                                let exception_pointers = [seh.exception_record_va, seh.context_va];
                                let mut exception_pointers_bytes = [0; 8];
                                for (index, value) in exception_pointers.into_iter().enumerate() {
                                    exception_pointers_bytes[index * 4..index * 4 + 4]
                                        .copy_from_slice(&value.to_le_bytes());
                                }
                                if child
                                    .address_space
                                    .write(exception_pointers_va, &exception_pointers_bytes)
                                    .map_err(|error| error.to_string())?
                                    != exception_pointers_bytes.len()
                                {
                                    return Err(
                                        "short terminal UEF exception pointers write".into()
                                    );
                                }

                                let return_esp = exit.registers.esp;
                                let callback_esp = return_esp
                                    .checked_sub(8)
                                    .ok_or("terminal UEF callback stack underflow")?;
                                let callback_frame =
                                    [thunk32::CHILD_UEF_RETURN_ADDRESS, exception_pointers_va];
                                let mut callback_frame_bytes = [0; 8];
                                for (index, value) in callback_frame.into_iter().enumerate() {
                                    callback_frame_bytes[index * 4..index * 4 + 4]
                                        .copy_from_slice(&value.to_le_bytes());
                                }
                                if child
                                    .address_space
                                    .write(callback_esp, &callback_frame_bytes)
                                    .map_err(|error| error.to_string())?
                                    != callback_frame_bytes.len()
                                {
                                    return Err("short terminal UEF callback frame write".into());
                                }

                                child.unhandled_filter_call = Some(ChildUnhandledFilterCall {
                                    return_esp,
                                    filter,
                                    continuation: ChildUnhandledFilterContinuation::TerminalSeh {
                                        seh,
                                    },
                                });
                                let mut registers = exit.registers;
                                registers.eip = filter;
                                registers.esp = callback_esp;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SEH TOPLEVEL FILTER CALL pid={} tid={} filter=0x{:08x} exception_pointers=0x{:08x}",
                                        active_pid, active_tid, filter, exception_pointers_va,
                                    ),
                                );
                                continue 'child_run;
                            }
                            if seh.next_registration == 0 {
                                return Err(
                                    "WC3 CHILD SEH FRONTIER reason=unhandled-next-registration"
                                        .into(),
                                );
                            }
                            let registration =
                                read_seh_registration(&child.address_space, seh.next_registration)?;
                            let handler_frame = [
                                thunk32::CHILD_SEH_RETURN_ADDRESS,
                                seh.exception_record_va,
                                registration.frame,
                                seh.context_va,
                                0,
                            ];
                            let mut frame_bytes = [0; 20];
                            for (index, value) in handler_frame.into_iter().enumerate() {
                                frame_bytes[index * 4..index * 4 + 4]
                                    .copy_from_slice(&value.to_le_bytes());
                            }
                            if child
                                .address_space
                                .write(exit.registers.esp, &frame_bytes)
                                .map_err(|error| error.to_string())?
                                != frame_bytes.len()
                            {
                                return Err("short continued SEH handler frame write".into());
                            }
                            let handler_registers = wc3::seh::exception_handler_registers(
                                seh.original_registers,
                                registration.handler,
                                exit.registers.esp,
                            );
                            child.seh = Some(ChildSehDispatch {
                                original_registers: seh.original_registers,
                                registration: registration.frame,
                                next_registration: registration.next,
                                handler: registration.handler,
                                exception_record_va: seh.exception_record_va,
                                context_va: seh.context_va,
                                exception_pointers_va: seh.exception_pointers_va,
                                preserved_fs_base: seh.preserved_fs_base,
                                depth: seh.depth.saturating_add(1),
                                quiet: seh.quiet,
                                boring_single_step: seh.boring_single_step,
                                scan_single_step: seh.scan_single_step,
                                dword_scan_single_step: seh.dword_scan_single_step,
                            });
                            contexts[active]
                                .context
                                .set_registers(handler_registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH CONTINUE_SEARCH pid={} tid={} registration=0x{:08x} handler=0x{:08x} disposition=1",
                                    active_pid,
                                    active_tid,
                                    registration.frame,
                                    registration.handler
                                ),
                            );
                            continue 'child_run;
                        }
                        if exit.registers.eax != wc3::seh::DISPOSITION_CONTINUE_EXECUTION {
                            return Err(format!(
                                "WC3 CHILD SEH FRONTIER reason=unsupported-disposition value={}",
                                exit.registers.eax
                            ));
                        }
                        let mut bytes = [0; wc3::seh::X86_CONTEXT_BYTES];
                        if child
                            .address_space
                            .read(seh.context_va, &mut bytes)
                            .map_err(|error| error.to_string())?
                            != bytes.len()
                        {
                            return Err("short SEH context readback".into());
                        }
                        let mut restored =
                            wc3::seh::decode_x86_context(&bytes, seh.preserved_fs_base)
                                .map_err(str::to_owned)?;
                        let mut restored_debug =
                            wc3::seh::decode_x86_debug_registers(&bytes).map_err(str::to_owned)?;
                        loop_checkpoint::boundary(
                            child,
                            &mut contexts[active].context,
                            &mut restored,
                            &mut restored_debug,
                        )
                        .await?;
                        if restored.eip == TABLE_CHECKPOINT_FROM_EIP
                            && child_read_u32(child, WAR3_DWORD_SCAN_INDEX) == Some(0)
                        {
                            let restored_checkpoint = try_restore_table_checkpoint(
                                child,
                                &mut contexts[active].context,
                                &mut restored,
                                &mut restored_debug,
                            )
                            .await?;
                            if !restored_checkpoint {
                                begin_table_checkpoint_capture(
                                    child,
                                    &contexts[active].context,
                                    restored,
                                    restored_debug,
                                    &session,
                                )?;
                            }
                        }
                        if seh.boring_single_step {
                            let control_changed = restored.eip != seh.original_registers.eip
                                || restored.esp != seh.original_registers.esp
                                || ((restored.eflags ^ seh.original_registers.eflags)
                                    & wc3::seh::X86_EFLAGS_TF)
                                    != 0;
                            if cfg!(feature = "trace-seh") && control_changed {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SINGLESTEP TRANSITION old_eip=0x{:08x} new_eip=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x} old_eflags=0x{:08x} new_eflags=0x{:08x} dr6=0x{:08x} dr7=0x{:08x}",
                                        seh.original_registers.eip,
                                        restored.eip,
                                        seh.original_registers.esp,
                                        restored.esp,
                                        seh.original_registers.eflags,
                                        restored.eflags,
                                        restored_debug.dr6,
                                        restored_debug.dr7,
                                    ),
                                );
                            }
                            child.single_step_count = child.single_step_count.saturating_add(1);
                            if child.single_step_count == 1 || child.single_step_count % 0x1000 == 0
                            {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SINGLESTEP HEARTBEAT count={} eip=0x{:08x} tf={} dr6=0x{:08x} dr7=0x{:08x}",
                                        child.single_step_count,
                                        restored.eip,
                                        u32::from(restored.eflags & wc3::seh::X86_EFLAGS_TF != 0),
                                        restored_debug.dr6,
                                        restored_debug.dr7,
                                    ),
                                );
                            }
                        }
                        if !seh.quiet
                            && matches!(
                                seh.original_registers.eip,
                                0x0045_af51 | 0x0045_af54 | 0x0045_af5a
                            )
                        {
                            let get = |offset: usize| {
                                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH DEBUG RETURN eip=0x{:08x} dr0=0x{:08x} dr1=0x{:08x} dr2=0x{:08x} dr3=0x{:08x} dr6=0x{:08x} dr7=0x{:08x}",
                                    seh.original_registers.eip,
                                    get(0x04),
                                    get(0x08),
                                    get(0x0c),
                                    get(0x10),
                                    get(0x14),
                                    get(0x18),
                                ),
                            );
                        }
                        if seh.scan_single_step {
                            log_war3_scan_progress(
                                child,
                                seh.original_registers.eip,
                                restored,
                                restored_debug,
                            );
                        }
                        if seh.dword_scan_single_step {
                            observe_war3_dword_scan(child, seh.original_registers.eip);
                        }
                        if !seh.quiet {
                            let raw_ecx = u32::from_le_bytes(
                                bytes[wc3::seh::ECX_OFFSET..wc3::seh::ECX_OFFSET + 4]
                                    .try_into()
                                    .unwrap(),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH CONTEXT RETURN pid={} tid={} context=0x{:08x} old_ecx=0x{:08x} saved_ecx=0x{:08x} restored_ecx=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    seh.context_va,
                                    seh.original_registers.ecx,
                                    raw_ecx,
                                    restored.ecx,
                                ),
                            );
                        }
                        if restored.eip == TABLE_CHECKPOINT_TO_EIP
                            && child_read_u32(child, WAR3_DWORD_SCAN_INDEX)
                                == Some(WAR3_TABLE_FILL_BOUND)
                            && child.table_checkpoint_capture.is_some()
                        {
                            finish_table_checkpoint_capture(
                                child,
                                &contexts[active].context,
                                restored,
                                restored_debug,
                                &session,
                            )
                            .await?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD TABLE CHECKPOINT CONTINUE reason=artifact-verified"
                                ),
                            );
                        }
                        contexts[active]
                            .context
                            .set_registers(restored)
                            .map_err(|error| error.to_string())?;
                        contexts[active]
                            .context
                            .set_debug_registers(restored_debug)
                            .map_err(|error| error.to_string())?;
                        if !seh.quiet {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD SEH CONTINUE pid={} tid={} old_eip=0x{:08x} new_eip=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    seh.original_registers.eip,
                                    restored.eip,
                                    seh.original_registers.esp,
                                    restored.esp,
                                ),
                            );
                        }
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_DLL_RETURN_AFTER_VMCALL {
                        let native_index = match child.execution {
                            ChildExecutionState::DllInitRunning { native_index } => native_index,
                            ChildExecutionState::Loader => {
                                return Err("child DLL return while execution=loader".into());
                            }
                            ChildExecutionState::DllInitReady { .. } => {
                                return Err("child DLL return while DLL init is not running".into());
                            }
                            ChildExecutionState::ImageEntryReady
                            | ChildExecutionState::ImageEntryRunning => {
                                return Err("child DLL return while image entry is active".into());
                            }
                        };
                        let module = child
                            .native_modules
                            .get(native_index)
                            .ok_or_else(|| "child DLL return native index".to_owned())?;
                        let module_name = module.stored.clone();
                        let success = exit.registers.eax != 0;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL INIT RETURN pid={} tid={} module=\"{}\" eax=0x{:08x} success={}",
                                active_pid,
                                active_tid,
                                module_name,
                                exit.registers.eax,
                                success as u8
                            ),
                        );
                        if !success {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD DLL INIT FAILED pid={} module=\"{}\" reason=DLL_PROCESS_ATTACH-returned-FALSE",
                                    active_pid, module_name
                                ),
                            );
                            return Ok(());
                        }
                        child.native_modules[native_index].initialized = true;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD NATIVE MODULE INITIALIZED pid={} module=\"{}\" base=0x{:08x} initialized=1",
                                active_pid,
                                module_name,
                                child.native_modules[native_index].image.image_base
                            ),
                        );
                        let next_index = child
                            .native_modules
                            .iter()
                            .position(|module| !module.initialized);
                        let Some(next_index) = next_index else {
                            let initialized = child
                                .native_modules
                                .iter()
                                .filter(|module| module.initialized)
                                .count();
                            if initialized != child.native_modules.len() {
                                return Err("child loader completion count mismatch".into());
                            }
                            child.execution = ChildExecutionState::ImageEntryReady;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADER COMPLETE pid={} tid={} modules={} initialized={}",
                                    child.pid,
                                    child.tid,
                                    child.native_modules.len(),
                                    initialized
                                ),
                            );
                            let (entry, frame_esp) = arm_existing_child_image_entry(
                                child,
                                &mut contexts[active],
                                exit.registers,
                            )?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD IMAGE ENTRY RESUME pid={} tid={} image=\"War3.exe\" base=0x{:08x} entry=0x{:08x} esp=0x{:08x} return=0x{:08x} caller_headroom={} caller_bytes={} stack_top=0x{:08x} context_reused=1 teb_preserved=1 xstate_preserved=1",
                                    active_pid,
                                    active_tid,
                                    child.image.image_base,
                                    entry,
                                    frame_esp,
                                    thunk32::CHILD_IMAGE_RETURN_ADDRESS,
                                    CHILD_IMAGE_ENTRY_HEADROOM,
                                    CHILD_IMAGE_ENTRY_CALLER_BYTES,
                                    STACK_TOP,
                                ),
                            );
                            continue;
                        };
                        let previous_name = module_name;
                        child.execution = ChildExecutionState::DllInitReady {
                            native_index: next_index,
                        };
                        let (next_name, next_entry, frame_esp) = arm_existing_child_dll_init(
                            child,
                            &mut contexts[active],
                            next_index,
                            exit.registers,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL REARM pid={} tid={} previous=\"{}\" next=\"{}\" context_reused=1 xstate_preserved=1 teb_preserved=1 entry=0x{:08x} esp=0x{:08x}",
                                active_pid,
                                active_tid,
                                previous_name,
                                next_name,
                                next_entry,
                                frame_esp
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL FRAME pid={} tid={} module=\"{}\" return=0x{:08x} hinst=0x{:08x} reason=1 reserved=0x{:08x}",
                                active_pid,
                                active_tid,
                                next_name,
                                thunk32::CHILD_DLL_RETURN_ADDRESS,
                                child.native_modules[next_index].image.image_base,
                                child.static_load_reserved,
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DLL INIT RESUME pid={} tid={} module=\"{}\" state=dll-init-running context_started=1",
                                active_pid, active_tid, next_name
                            ),
                        );
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_IMAGE_RETURN_AFTER_VMCALL {
                        if child.execution != ChildExecutionState::ImageEntryRunning {
                            return Err(
                                "child image return outside image-entry-running state".into()
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD IMAGE RETURN pid={} tid={} eax=0x{:08x}",
                                active_pid, active_tid, exit.registers.eax
                            ),
                        );
                        return Ok(());
                    }
                    if exit.registers.eip == thunk32::CHILD_THREAD_EXIT_AFTER_VMCALL {
                        if active_tid == child.tid {
                            return Err("child primary thread returned through worker exit".into());
                        }
                        let exit_code = exit.registers.eax;
                        let key = active_key;
                        child.activate_thread(child.tid);
                        child.parked_threads.remove(&active_tid);
                        contexts.remove(active);
                        let woken = session
                            .signal_thread(key, exit_code)
                            .map_err(str::to_owned)?;
                        wait_deadlines.remove(&key);
                        resume_completed_waiters(
                            &woken,
                            "child-thread-exit",
                            &mut contexts,
                            &mut wait_deadlines,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD THREAD EXIT pid={} tid={} exit_code=0x{:08x} waiters_woken={}",
                                active_pid, active_tid, exit_code, woken.len(),
                            ),
                        );
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        return Err("child thread exited with no runnable guest".into());
                    }
                    if exit.registers.eip == thunk32::CHILD_CIPOW_SPILL_AFTER_VMCALL {
                        let pending = child
                            .cipow
                            .take()
                            .ok_or_else(|| "CIPOW spill without pending state".to_owned())?;
                        if exit.registers.esp != pending.provider_esp {
                            return Err("CIPOW spill ESP mismatch".into());
                        }
                        let exponent = read_guest_words(
                            &X86Memory(&child.address_space),
                            thunk32::CHILD_CIPOW_EXPONENT_ADDRESS,
                            2,
                        )?;
                        let base = read_guest_words(
                            &X86Memory(&child.address_space),
                            thunk32::CHILD_CIPOW_BASE_ADDRESS,
                            2,
                        )?;
                        let exponent =
                            f64::from_bits((exponent[0] as u64) | ((exponent[1] as u64) << 32));
                        let base = f64::from_bits((base[0] as u64) | ((base[1] as u64) << 32));
                        if !child.cipow_diagnostic_logged {
                            child.cipow_diagnostic_logged = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT CIPOW base={} exponent={}",
                                    base, exponent
                                ),
                            );
                        }
                        if !base.is_finite() || !exponent.is_finite() || base <= 0.0 {
                            return Ok(());
                        }
                        let result = base.powf(exponent);
                        if !result.is_finite() {
                            return Ok(());
                        }
                        child
                            .address_space
                            .write(
                                thunk32::CHILD_CIPOW_RESULT_ADDRESS,
                                &result.to_bits().to_le_bytes(),
                            )
                            .map_err(|error| error.to_string())?;
                        let mut registers = exit.registers;
                        registers.eip = thunk32::CHILD_CIPOW_RESTORE_ADDRESS;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL {
                        if let Some(pending) = child.seh3_call.take() {
                            let callback_arg_bytes = match &pending.kind {
                                ChildSeh3CallbackKind::Filter { .. } => 4u32,
                                ChildSeh3CallbackKind::Finally { .. } => 0u32,
                            };
                            let expected_esp = pending
                                .provider_esp
                                .checked_sub(callback_arg_bytes)
                                .ok_or("SEH3 callback expected ESP underflow")?;
                            if exit.registers.esp != expected_esp {
                                return Err(format!(
                                    "SEH3 callback ESP mismatch expected=0x{:08x} actual=0x{:08x} arg_bytes={}",
                                    expected_esp, exit.registers.esp, callback_arg_bytes,
                                ));
                            }
                            let mut callback_return = exit.registers;
                            callback_return.esp = pending.provider_esp;
                            let anchor =
                                pending.frame.checked_add(0x10).ok_or("SEH3 EBP overflow")?;
                            match pending.kind {
                                ChildSeh3CallbackKind::Finally {
                                    mut next_level,
                                    selected_level,
                                } => loop {
                                    if next_level == selected_level {
                                        let (_, _, handler) = child_seh3_scope_entry(
                                            child,
                                            pending.scope,
                                            selected_level,
                                        )?;
                                        if handler == 0 {
                                            return Err("SEH3 selected handler is null".into());
                                        }
                                        let (_, selected_prev, _) = child_seh3_scope_entry(
                                            child,
                                            pending.scope,
                                            selected_level,
                                        )?;
                                        write_child_u32(
                                            child,
                                            pending.frame + 0x0c,
                                            selected_prev as u32,
                                        )?;
                                        let mut registers = callback_return;
                                        registers.eip = handler;
                                        registers.esp = pending.provider_esp;
                                        registers.ebp = anchor;
                                        contexts[active]
                                            .context
                                            .set_registers(registers)
                                            .map_err(|error| error.to_string())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD CRT EH3 HANDLER pid={} tid={} level={} handler=0x{:08x}",
                                                active_pid, active_tid, selected_level, handler
                                            ),
                                        );
                                        continue 'child_run;
                                    }
                                    if next_level < 0 {
                                        return Err(
                                            "SEH3 unwind reached end before selected level".into(),
                                        );
                                    }
                                    let (previous, filter, handler) =
                                        child_seh3_scope_entry(child, pending.scope, next_level)?;
                                    write_child_u32(child, pending.frame + 0x0c, previous as u32)?;
                                    next_level = previous;
                                    if filter == 0 && handler != 0 {
                                        let call = ChildSeh3Call {
                                            provider_resume_eip: pending.provider_resume_eip,
                                            provider_esp: pending.provider_esp,
                                            frame: pending.frame,
                                            scope: pending.scope,
                                            kind: ChildSeh3CallbackKind::Finally {
                                                next_level,
                                                selected_level,
                                            },
                                        };
                                        schedule_child_seh3_finally(
                                            child,
                                            &mut contexts[active],
                                            callback_return,
                                            call,
                                            handler,
                                        )?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD CRT EH3 FINALLY pid={} tid={} handler=0x{:08x}",
                                                active_pid, active_tid, handler
                                            ),
                                        );
                                        continue 'child_run;
                                    }
                                },
                                ChildSeh3CallbackKind::Filter {
                                    level,
                                    previous,
                                    start_level,
                                } => {
                                    let result =
                                        i32::from_le_bytes(callback_return.eax.to_le_bytes());
                                    if result == -1 {
                                        let mut registers = callback_return;
                                        registers.eip = pending.provider_resume_eip;
                                        registers.esp = pending.provider_esp;
                                        registers.eax = wc3::seh::DISPOSITION_CONTINUE_EXECUTION;
                                        contexts[active]
                                            .context
                                            .set_registers(registers)
                                            .map_err(|error| error.to_string())?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD CRT EH3 FILTER RETURN pid={} tid={} level={} result=-1 disposition=0",
                                                active_pid, active_tid, level
                                            ),
                                        );
                                        continue 'child_run;
                                    }
                                    if result != 0 && result != 1 {
                                        return Err(format!(
                                            "WC3 CHILD CRT EH3 FRONTIER reason=filter-result value={}",
                                            result
                                        ));
                                    }
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT EH3 FILTER RETURN pid={} tid={} level={} result={}",
                                            active_pid, active_tid, level, result,
                                        ),
                                    );
                                    let selected = if result == 1 { Some(level) } else { None };
                                    let mut current =
                                        if result == 1 { start_level } else { previous };
                                    if result == 0 {
                                        write_child_u32(
                                            child,
                                            pending.frame + 0x0c,
                                            previous as u32,
                                        )?;
                                    }
                                    loop {
                                        if let Some(selected_level) = selected {
                                            if current == selected_level {
                                                let (_, selected_prev, handler) =
                                                    child_seh3_scope_entry(
                                                        child,
                                                        pending.scope,
                                                        selected_level,
                                                    )?;
                                                if handler == 0 {
                                                    return Err(
                                                        "SEH3 selected handler is null".into()
                                                    );
                                                }
                                                write_child_u32(
                                                    child,
                                                    pending.frame + 0x0c,
                                                    selected_prev as u32,
                                                )?;
                                                let mut registers = callback_return;
                                                registers.eip = handler;
                                                registers.esp = pending.provider_esp;
                                                registers.ebp = anchor;
                                                contexts[active]
                                                    .context
                                                    .set_registers(registers)
                                                    .map_err(|error| error.to_string())?;
                                                logl::log(
                                                    level::IMPORTANT,
                                                    format_args!(
                                                        "WC3 CHILD CRT EH3 HANDLER pid={} tid={} level={} handler=0x{:08x}",
                                                        active_pid,
                                                        active_tid,
                                                        selected_level,
                                                        handler
                                                    ),
                                                );
                                                continue 'child_run;
                                            }
                                        }
                                        if current < 0 {
                                            if selected.is_some() {
                                                return Err(
                                                    "SEH3 selected level was not found".into()
                                                );
                                            }
                                            let mut registers = callback_return;
                                            registers.eip = pending.provider_resume_eip;
                                            registers.esp = pending.provider_esp;
                                            registers.eax = wc3::seh::DISPOSITION_CONTINUE_SEARCH;
                                            contexts[active]
                                                .context
                                                .set_registers(registers)
                                                .map_err(|error| error.to_string())?;
                                            logl::log(
                                                level::IMPORTANT,
                                                format_args!(
                                                    "WC3 CHILD CRT EH3 SEARCH pid={} tid={} disposition=1",
                                                    active_pid, active_tid
                                                ),
                                            );
                                            continue 'child_run;
                                        }
                                        let (previous, filter, handler) =
                                            child_seh3_scope_entry(child, pending.scope, current)?;
                                        write_child_u32(
                                            child,
                                            pending.frame + 0x0c,
                                            previous as u32,
                                        )?;
                                        if let Some(selected_level) = selected {
                                            current = previous;
                                            if filter == 0 && handler != 0 {
                                                let call = ChildSeh3Call {
                                                    provider_resume_eip: pending
                                                        .provider_resume_eip,
                                                    provider_esp: pending.provider_esp,
                                                    frame: pending.frame,
                                                    scope: pending.scope,
                                                    kind: ChildSeh3CallbackKind::Finally {
                                                        next_level: current,
                                                        selected_level,
                                                    },
                                                };
                                                schedule_child_seh3_finally(
                                                    child,
                                                    &mut contexts[active],
                                                    callback_return,
                                                    call,
                                                    handler,
                                                )?;
                                                logl::log(
                                                    level::IMPORTANT,
                                                    format_args!(
                                                        "WC3 CHILD CRT EH3 FINALLY pid={} tid={} handler=0x{:08x}",
                                                        active_pid, active_tid, handler
                                                    ),
                                                );
                                                continue 'child_run;
                                            }
                                            continue;
                                        }
                                        let level = current;
                                        current = previous;
                                        if filter != 0 {
                                            let call = ChildSeh3Call {
                                                provider_resume_eip: pending.provider_resume_eip,
                                                provider_esp: pending.provider_esp,
                                                frame: pending.frame,
                                                scope: pending.scope,
                                                kind: ChildSeh3CallbackKind::Filter {
                                                    level,
                                                    previous,
                                                    start_level,
                                                },
                                            };
                                            schedule_child_seh3_filter(
                                                child,
                                                &mut contexts[active],
                                                callback_return,
                                                call,
                                                filter,
                                            )?;
                                            continue 'child_run;
                                        }
                                    }
                                }
                            }
                        }
                        // _initterm runs inside the runtime DLL's DllMain, so its
                        // callback owns this return before the outer loader does.
                        if let Some(initterm) = child.initterm.as_ref() {
                            if exit.registers.esp != initterm.provider_esp {
                                return Err(format!(
                                    "child _initterm callback ESP mismatch expected=0x{:08x} actual=0x{:08x}",
                                    initterm.provider_esp, exit.registers.esp
                                ));
                            }
                            logl::trace!("trace-init",
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT INITTERM RETURN pid={} tid={} completed={} eax=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    initterm.callbacks_invoked,
                                    exit.registers.eax
                                ),
                            );
                            match advance_child_initterm(child, &mut contexts[active])? {
                                InittermAdvance::CallbackScheduled | InittermAdvance::Complete => {
                                    continue;
                                }
                            }
                        }
                        if let Some(pending) = child.load_library_call.as_ref() {
                            if exit.registers.esp != pending.provider_esp {
                                return Err(format!(
                                    "LoadLibrary DllMain ESP mismatch expected=0x{:08x} actual=0x{:08x}",
                                    pending.provider_esp, exit.registers.esp
                                ));
                            }
                        }
                        if let Some(pending) = child.load_library_call.take() {
                            if exit.registers.eax == 0 {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD LOADLIBRARY FRONTIER reason=dllmain-returned-false handle=0x{:08x}",
                                        pending.module_handle,
                                    ),
                                );
                                return Ok(());
                            }
                            let module = child
                                .native_modules
                                .get_mut(pending.native_index)
                                .ok_or_else(|| "runtime LoadLibrary native index".to_owned())?;
                            module.initialized = true;
                            let stored = module.stored.clone();
                            if let Some((&next_native_index, remaining_native_indices)) =
                                pending.remaining_native_indices.split_first()
                            {
                                let next_module =
                                    child.native_modules.get(next_native_index).ok_or_else(
                                        || "runtime LoadLibrary next native index".to_owned(),
                                    )?;
                                let next_module_handle = next_module.image.image_base;
                                let entry = next_module_handle
                                    .checked_add(next_module.image.entry_rva)
                                    .ok_or("runtime dependency DLL entry overflow")?;
                                let callback_esp = pending
                                    .provider_esp
                                    .checked_sub(16)
                                    .ok_or("runtime dependency DllMain stack underflow")?;
                                let frame = [
                                    thunk32::CHILD_CALLBACK_RETURN_ADDRESS,
                                    next_module_handle,
                                    1,
                                    0,
                                ];
                                let mut frame_bytes = [0u8; 16];
                                for (index, value) in frame.into_iter().enumerate() {
                                    frame_bytes[index * 4..index * 4 + 4]
                                        .copy_from_slice(&value.to_le_bytes());
                                }
                                if child
                                    .address_space
                                    .write(callback_esp, &frame_bytes)
                                    .map_err(|error| error.to_string())?
                                    != frame_bytes.len()
                                {
                                    return Err(
                                        "short runtime dependency DllMain frame write".into()
                                    );
                                }
                                child.load_library_call = Some(ChildLoadLibraryCall {
                                    provider_resume_eip: pending.provider_resume_eip,
                                    provider_esp: pending.provider_esp,
                                    native_index: next_native_index,
                                    module_handle: next_module_handle,
                                    load_library_handle: pending.load_library_handle,
                                    remaining_native_indices: remaining_native_indices.to_vec(),
                                });
                                let mut registers = exit.registers;
                                registers.eip = entry;
                                registers.esp = callback_esp;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD RUNTIME NATIVE DEPENDENCY ATTACH pid={} tid={} module={:?} handle=0x{:08x}",
                                        active_pid,
                                        active_tid,
                                        next_module.stored,
                                        next_module_handle,
                                    ),
                                );
                                continue;
                            }
                            let mut registers = exit.registers;
                            registers.eip = pending.provider_resume_eip;
                            registers.esp = pending.provider_esp;
                            registers.eax = pending.load_library_handle;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY RETURN pid={} tid={} module={:?} handle=0x{:08x} dllmain=TRUE cleanup=4-by-thunk",
                                    active_pid, active_tid, stored, pending.load_library_handle,
                                ),
                            );
                            continue;
                        }
                        let scope = child_execution_scope(child).map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CONTROL FRONTIER pid={} tid={} during=\"{}\" kind=callback-return",
                                active_pid, active_tid, scope
                            ),
                        );
                        return Ok(());
                    }
                    let running_scope = child_execution_scope(child).map_err(str::to_owned)?;
                    let running_module_name = running_scope.clone();
                    let provider_id = exit.registers.eax;
                    let provider = session
                        .process(active_pid)
                        .and_then(|process| process.xp.provider_import(provider_id))
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "unknown child provider trap pid={} tid={} id={}",
                                active_pid, active_tid, provider_id
                            )
                        })?;
                    let mut caller_ret = [0; 4];
                    child
                        .address_space
                        .read(exit.registers.esp, &mut caller_ret)
                        .map_err(|error| error.to_string())?;
                    let symbol = match &provider.symbol {
                        child_loader::ProviderSymbol::Name(name) => format!("symbol=\"{}\"", name),
                        child_loader::ProviderSymbol::Ordinal(ordinal) => {
                            format!("ordinal={}", ordinal)
                        }
                    };
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "LoadImageA"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            7,
                        )?;
                        let module_name = session
                            .process(active_pid)
                            .and_then(|process| process.xp.loaded_module_name(frame[1]))
                            .unwrap_or("<unknown>");
                        let name = if frame[2] & 0xffff_0000 == 0 {
                            format!("MAKEINTRESOURCE({})", frame[2] & 0xffff)
                        } else {
                            wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                frame[2],
                                256,
                            )
                            .map_err(|error| format!("LoadImageA name: {error}"))?
                        };
                        let type_name = match frame[3] {
                            0 => "IMAGE_BITMAP",
                            1 => "IMAGE_ICON",
                            2 => "IMAGE_CURSOR",
                            _ => "UNKNOWN",
                        };
                        let flags_name = if frame[6] == 0x40 {
                            "LR_DEFAULTSIZE"
                        } else {
                            "UNKNOWN"
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LOADIMAGEA pid={} tid={} module=0x{:08x} module_name={:?} name_ptr=0x{:08x} name={:?} type={} type_name={} cx={} cy={} flags=0x{:08x} flags_name={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                module_name,
                                frame[2],
                                name,
                                frame[3],
                                type_name,
                                frame[4],
                                frame[5],
                                frame[6],
                                flags_name,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("GDI32.dll")
                                && name == "GetDeviceCaps"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETDEVICECAPS CALL pid={} tid={} hdc=0x{:08x} index={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("GDI32.dll")
                                && name == "SetPixelFormat"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETPIXELFORMAT CALL pid={} tid={} hdc=0x{:08x} format={} ppfd=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[3],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "wglCreateContext"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD WGLCREATECONTEXT CALL pid={} tid={} hdc=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "wglMakeCurrent"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD WGLMAKECURRENT CALL pid={} tid={} hdc=0x{:08x} hglrc=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glGetString"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLGETSTRING CALL pid={} tid={} name=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glMatrixMode"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let mode_name = match frame[1] {
                            0x0000_1700 => "GL_MODELVIEW",
                            0x0000_1701 => "GL_PROJECTION",
                            0x0000_1702 => "GL_TEXTURE",
                            _ => "UNKNOWN",
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLMATRIXMODE CALL pid={} tid={} mode=0x{:08x} mode_name={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                mode_name,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glLightModelfv"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let pname = frame[1];
                        let params = frame[2];
                        let pname_name = match pname {
                            0x0000_0b51 => "GL_LIGHT_MODEL_LOCAL_VIEWER",
                            0x0000_0b52 => "GL_LIGHT_MODEL_TWO_SIDE",
                            0x0000_0b53 => "GL_LIGHT_MODEL_AMBIENT",
                            _ => "UNKNOWN",
                        };
                        let mut first = [0u8; 4];
                        X86Memory(&child.address_space).read(params, &mut first)?;
                        let p0 = f32::from_le_bytes(first);

                        if pname == 0x0000_0b53 {
                            let mut raw = [0u8; 16];
                            X86Memory(&child.address_space).read(params, &mut raw)?;
                            let values = core::array::from_fn::<f32, 4, _>(|i| {
                                f32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
                            });
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GLLIGHTMODELFV CALL pid={} tid={} pname=0x{:08x} pname_name={} params=0x{:08x} values={:?} caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    pname,
                                    pname_name,
                                    params,
                                    values,
                                    frame[0],
                                ),
                            );
                        } else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GLLIGHTMODELFV CALL pid={} tid={} pname=0x{:08x} pname_name={} params=0x{:08x} value={} caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    pname,
                                    pname_name,
                                    params,
                                    p0,
                                    frame[0],
                                ),
                            );
                        }
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "sscanf"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            8,
                        )?;
                        let input = wc3::process::read_c_string(
                            &X86Memory(&child.address_space),
                            frame[1],
                            256,
                        )?;
                        let format = wc3::process::read_c_string(
                            &X86Memory(&child.address_space),
                            frame[2],
                            256,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SSCANF CALL pid={} tid={} input_ptr=0x{:08x} input={:?} format_ptr=0x{:08x} format={:?} args=[0x{:08x},0x{:08x},0x{:08x},0x{:08x},0x{:08x}] caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                input,
                                frame[2],
                                format,
                                frame[3],
                                frame[4],
                                frame[5],
                                frame[6],
                                frame[7],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glLightfv"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let light = frame[1];
                        let pname = frame[2];
                        let params = frame[3];
                        let light_name = match light {
                            0x4000 => "GL_LIGHT0",
                            0x4001 => "GL_LIGHT1",
                            0x4002 => "GL_LIGHT2",
                            0x4003 => "GL_LIGHT3",
                            0x4004 => "GL_LIGHT4",
                            0x4005 => "GL_LIGHT5",
                            0x4006 => "GL_LIGHT6",
                            0x4007 => "GL_LIGHT7",
                            _ => "UNKNOWN",
                        };
                        let (pname_name, count) = match pname {
                            0x1200 => ("GL_AMBIENT", 4),
                            0x1201 => ("GL_DIFFUSE", 4),
                            0x1202 => ("GL_SPECULAR", 4),
                            0x1203 => ("GL_POSITION", 4),
                            0x1204 => ("GL_SPOT_DIRECTION", 3),
                            0x1205 => ("GL_SPOT_EXPONENT", 1),
                            0x1206 => ("GL_SPOT_CUTOFF", 1),
                            0x1207 => ("GL_CONSTANT_ATTENUATION", 1),
                            0x1208 => ("GL_LINEAR_ATTENUATION", 1),
                            0x1209 => ("GL_QUADRATIC_ATTENUATION", 1),
                            _ => ("UNKNOWN", 1),
                        };
                        let mut values = [0.0f32; 4];
                        for (index, value) in values[..count].iter_mut().enumerate() {
                            let mut raw = [0u8; 4];
                            X86Memory(&child.address_space).read(
                                params + (index as u32) * 4,
                                &mut raw,
                            )?;
                            *value = f32::from_le_bytes(raw);
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLLIGHTFV CALL pid={} tid={} light=0x{:08x} light_name={} pname=0x{:08x} pname_name={} params=0x{:08x} values={:?} count={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                light,
                                light_name,
                                pname,
                                pname_name,
                                params,
                                &values[..count],
                                count,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glDisable"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let cap_name = match frame[1] {
                            0x0b44 => "GL_CULL_FACE",
                            0x0b50 => "GL_LIGHTING",
                            0x0b57 => "GL_COLOR_MATERIAL",
                            0x0b60 => "GL_FOG",
                            0x0b71 => "GL_DEPTH_TEST",
                            0x0ba1 => "GL_NORMALIZE",
                            0x0bc0 => "GL_ALPHA_TEST",
                            0x0bd0 => "GL_DITHER",
                            0x0be2 => "GL_BLEND",
                            0x0c11 => "GL_SCISSOR_TEST",
                            0x0de1 => "GL_TEXTURE_2D",
                            0x4000 => "GL_LIGHT0",
                            0x4001 => "GL_LIGHT1",
                            0x8037 => "GL_POLYGON_OFFSET_FILL",
                            _ => "UNKNOWN",
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLDISABLE CALL pid={} tid={} cap=0x{:08x} cap_name={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                cap_name,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("OPENGL32.dll")
                                && name == "glLoadMatrixf"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let mut raw = [0u8; 64];
                        X86Memory(&child.address_space).read(frame[1], &mut raw)?;
                        let matrix = core::array::from_fn::<f32, 16, _>(|index| {
                            f32::from_le_bytes(
                                raw[index * 4..index * 4 + 4].try_into().unwrap(),
                            )
                        });
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLLOADMATRIXF CALL pid={} tid={} matrix=0x{:08x} values={:?} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                matrix,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "ReleaseDC"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD RELEASEDC CALL pid={} tid={} hwnd=0x{:08x} hdc=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "SetWindowPos"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            8,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETWINDOWPOS CALL pid={} tid={} hwnd=0x{:08x} insert_after=0x{:08x} x={} y={} width={} height={} flags=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[3] as i32,
                                frame[4] as i32,
                                frame[5],
                                frame[6],
                                frame[7],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "ChangeDisplaySettingsExA"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            6,
                        )?;
                        let device = if frame[1] == 0 {
                            None
                        } else {
                            Some(wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                frame[1],
                                32,
                            )?)
                        };
                        let mode = if frame[2] == 0 {
                            None
                        } else {
                            let size_extra = read_guest_words(
                                &X86Memory(&child.address_space),
                                frame[2] + 0x24,
                                1,
                            )?[0];
                            let dm_size = size_extra & 0xffff;
                            let dm_driver_extra = size_extra >> 16;
                            let dm_fields = read_guest_words(
                                &X86Memory(&child.address_space),
                                frame[2] + 0x28,
                                1,
                            )?[0];
                            let values = read_guest_words(
                                &X86Memory(&child.address_space),
                                frame[2] + 0x68,
                                5,
                            )?;
                            Some((
                                dm_size,
                                dm_driver_extra,
                                dm_fields,
                                values[0],
                                values[1],
                                values[2],
                                values[3],
                                values[4],
                            ))
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CHANGEDISPLAYSETTINGSEXA pid={} tid={} device={:?} devmode=0x{:08x} mode={:?} hwnd=0x{:08x} flags=0x{:08x} lparam=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                device.as_deref().unwrap_or("<null>"),
                                frame[2],
                                mode,
                                frame[3],
                                frame[4],
                                frame[5],
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "LoadCursorA"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let resolved = if frame[2] & 0xffff_0000 == 0 {
                            format!("system-id={}", frame[2] & 0xffff)
                        } else {
                            let name = wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                frame[2],
                                256,
                            )
                            .map_err(|error| format!("LoadCursorA name: {error}"))?;
                            format!("resource-name={name:?}")
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LOADCURSORA pid={} tid={} module=0x{:08x} name=0x{:08x} caller_ret=0x{:08x} resolved={}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[0],
                                resolved,
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("USER32.dll")
                                && name == "CreateWindowExA"
                    ) {
                        let a = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            13,
                        )?;
                        let class = if a[2] != 0 && a[2] >> 16 != 0 {
                            Some(wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                a[2],
                                256,
                            )?)
                        } else {
                            None
                        };
                        let title = if a[3] != 0 {
                            Some(wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                a[3],
                                256,
                            )?)
                        } else {
                            None
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CREATEWINDOWEXA CALL pid={} tid={} ex_style=0x{:08x} class_ptr=0x{:08x} class={:?} title_ptr=0x{:08x} title={:?} style=0x{:08x} x={} y={} width={} height={} parent=0x{:08x} menu=0x{:08x} instance=0x{:08x} param=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                a[1],
                                a[2],
                                class,
                                a[3],
                                title,
                                a[4],
                                a[5] as i32,
                                a[6] as i32,
                                a[7],
                                a[8],
                                a[9],
                                a[10],
                                a[11],
                                a[12],
                                a[0],
                            ),
                        );
                    }
                    let is_heap_create = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapCreate"
                    );
                    if is_heap_create {
                        let mut child_memory = X86Memory(&child.address_space);
                        let heap = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .create_win_heap(exit.registers.esp, &mut child_memory)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP CREATE pid={} tid={} during=\"{}\" provider_id={} options=0x{:08x} initial_size=0x{:08x} maximum_size=0x{:08x} handle=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap.options,
                                heap.initial_size,
                                heap.maximum_size,
                                heap.handle,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = heap.handle;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP CREATE RESULT pid={} tid={} handle=0x{:08x} resume_eip=0x{:08x} esp=0x{:08x} cleanup=12-by-thunk",
                                active_pid, active_tid, heap.handle, registers.eip, registers.esp,
                            ),
                        );
                        continue;
                    }
                    let operation = child_loader::provider_op(&provider);
                    if operation == child_loader::ProviderOp::MessageBoxA {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            5,
                        )?;

                        let owner = frame[1];
                        let text_ptr = frame[2];
                        let caption_ptr = frame[3];
                        let style = frame[4];

                        let text =
                            copy_message_box_ansi(&X86Memory(&child.address_space), text_ptr)?;
                        let caption =
                            copy_message_box_ansi(&X86Memory(&child.address_space), caption_ptr)?;

                        let buttons = message_box_buttons(style)
                            .map(|buttons| {
                                buttons
                                    .iter()
                                    .map(|button| button.label)
                                    .collect::<Vec<_>>()
                                    .join(",")
                            })
                            .unwrap_or_else(|| "<unsupported>".to_owned());

                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD MESSAGEBOXA FRONTIER pid={} tid={} during=\"{}\" provider_id={} caller_ret=0x{:08x} owner=0x{:08x} text_ptr=0x{:08x} caption_ptr=0x{:08x} text={:?} caption={:?} style=0x{:08x} button_type=0x{:x} buttons=[{}] topmost={}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                frame[0],
                                owner,
                                text_ptr,
                                caption_ptr,
                                text,
                                caption,
                                style,
                                style & 0x0f,
                                buttons,
                                u32::from(style & 0x0004_0000 != 0),
                            ),
                        );

                        return Ok(());
                    }
                    let is_global_alloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GlobalAlloc"
                    );
                    if is_global_alloc {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let flags = frame[1];
                        let bytes = frame[2];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GLOBALALLOC CALL pid={} tid={} during=\"{}\" provider_id={} flags=0x{:08x} bytes={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                flags,
                                bytes,
                                frame[0],
                            ),
                        );
                        let allocation = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .alloc_global_fixed(flags, bytes);
                        let allocation = match allocation {
                            Ok(value) => value,
                            Err(ProviderDispatchError::Frontier { api, detail }) => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}\" provider_id={} module=\"{}\" symbol=\"GlobalAlloc\" api=\"{}\" detail={:?}",
                                        active_pid,
                                        active_tid,
                                        running_module_name,
                                        provider_id,
                                        provider.module,
                                        api,
                                        detail,
                                    ),
                                );
                                return Ok(());
                            }
                            Err(error) => {
                                return Err(format!("GlobalAlloc semantic fault: {error}"));
                            }
                        };
                        let pointer = if let Some(allocation) = allocation {
                            let mapped_end = ensure_child_win_heap_mapped(child, allocation.end)?;
                            if allocation.flags & 0x40 != 0 {
                                let zeroes = vec![0; allocation.requested as usize];
                                if child
                                    .address_space
                                    .write(allocation.pointer, &zeroes)
                                    .map_err(|error| error.to_string())?
                                    != zeroes.len()
                                {
                                    return Err("short GlobalAlloc zero write".into());
                                }
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GLOBALALLOC RESULT pid={} tid={} flags=0x{:08x} requested={} pointer=0x{:08x} end=0x{:08x} mapped_end=0x{:08x} zeroed={} cleanup=8-by-thunk",
                                    active_pid,
                                    active_tid,
                                    allocation.flags,
                                    allocation.requested,
                                    allocation.pointer,
                                    allocation.end,
                                    mapped_end,
                                    u8::from(allocation.flags & 0x40 != 0),
                                ),
                            );
                            allocation.pointer
                        } else {
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = pointer;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_heap_alloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapAlloc"
                    );
                    if is_heap_alloc {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let heap = frame[1];
                        let flags = frame[2];
                        let bytes = frame[3];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP ALLOC CALL pid={} tid={} during=\"{}\" provider_id={} heap=0x{:08x} flags=0x{:08x} bytes={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap,
                                flags,
                                bytes,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let allocation = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .alloc_win_heap(exit.registers.esp, &X86Memory(&child.address_space))
                            .map_err(str::to_owned)?;
                        let pointer = if let Some(allocation) = allocation {
                            let mapped_end = ensure_child_win_heap_mapped(child, allocation.end)?;
                            if allocation.flags & HEAP_ZERO_MEMORY != 0 {
                                let zeroes = vec![0; allocation.requested.max(1) as usize];
                                let written = child
                                    .address_space
                                    .write(allocation.pointer, &zeroes)
                                    .map_err(|error| error.to_string())?;
                                if written != zeroes.len() {
                                    return Err("short child HeapAlloc zero write".into());
                                }
                            }
                            if allocation.pointer < CHILD_WIN_HEAP_BASE
                                || allocation.pointer >= CHILD_WIN_HEAP_LIMIT
                                || allocation.pointer % 8 != 0
                                || allocation.end > mapped_end
                            {
                                return Err("child HeapAlloc pointer verification failed".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP ALLOC RESULT pid={} tid={} heap=0x{:08x} flags=0x{:08x} requested={} pointer=0x{:08x} end=0x{:08x} mapped_end=0x{:08x} zeroed={} cleanup=12-by-thunk",
                                    active_pid,
                                    active_tid,
                                    allocation.heap,
                                    allocation.flags,
                                    allocation.requested,
                                    allocation.pointer,
                                    allocation.end,
                                    mapped_end,
                                    (allocation.flags & HEAP_ZERO_MEMORY != 0) as u8,
                                ),
                            );
                            allocation.pointer
                        } else if flags & HEAP_GENERATE_EXCEPTIONS != 0 {
                            return Err(format!(
                                "WC3 CHILD HEAPALLOC FRONTIER reason=generate-exceptions-allocation-failure heap=0x{heap:08x} flags=0x{flags:08x} bytes={bytes}"
                            ));
                        } else {
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = pointer;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_heap_free = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "HeapFree"
                    );
                    if is_heap_free {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let heap = frame[1];
                        let flags = frame[2];
                        let pointer = frame[3];
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD HEAP FREE CALL pid={} tid={} during=\"{}\" provider_id={} heap=0x{:08x} flags=0x{:08x} pointer=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                heap,
                                flags,
                                pointer,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if flags != 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE FRONTIER pid={} tid={} reason=unsupported-flags flags=0x{:08x}",
                                    active_pid, active_tid, flags,
                                ),
                            );
                            return Ok(());
                        }
                        let freed = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .free_win_heap(exit.registers.esp, &X86Memory(&child.address_space))
                            .map_err(str::to_owned)?;
                        let result = if let Some(allocation) = freed {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE RESULT pid={} tid={} heap=0x{:08x} pointer=0x{:08x} requested={} end=0x{:08x} freed=1 cleanup=12-by-thunk",
                                    active_pid,
                                    active_tid,
                                    allocation.heap,
                                    allocation.pointer,
                                    allocation.requested,
                                    allocation.end,
                                ),
                            );
                            1
                        } else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD HEAP FREE RESULT pid={} tid={} heap=0x{:08x} pointer=0x{:08x} freed=0 cleanup=12-by-thunk",
                                    active_pid, active_tid, heap, pointer,
                                ),
                            );
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_command_line_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetCommandLineA"
                    );
                    if is_get_command_line_a {
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("GetCommandLineA child provider did not return".into());
                        };
                        if result != PROCESS_DATA_VA {
                            return Err("child command-line pointer mismatch".into());
                        }
                        let mut bytes = [0; CHILD_COMMAND_LINE.len()];
                        child
                            .address_space
                            .read(result, &mut bytes)
                            .map_err(|error| error.to_string())?;
                        if bytes != CHILD_COMMAND_LINE {
                            return Err("child command-line backing mismatch".into());
                        }
                        let command_line =
                            String::from_utf8_lossy(&bytes[..bytes.len().saturating_sub(1)]);
                        if !*child_get_command_line_logged {
                            *child_get_command_line_logged = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETCOMMANDLINEA RESULT pid={} tid={} pointer=0x{:08x} value={:?} process_private=1 stack_cleanup=none",
                                    active_pid, active_tid, result, command_line,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_environment_strings_w = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetEnvironmentStringsW"
                    );
                    if is_get_environment_strings_w {
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err(
                                "GetEnvironmentStringsW child provider did not return".into()
                            );
                        };
                        if result != ENVIRONMENT_BLOCK_VA {
                            return Err("child environment pointer mismatch".into());
                        }
                        let mut terminator = [0u8; 4];
                        child
                            .address_space
                            .read(result, &mut terminator)
                            .map_err(|error| error.to_string())?;
                        if terminator != [0, 0, 0, 0] {
                            return Err(
                                "child wide environment block is not double-NUL terminated".into(),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETENVIRONMENTSTRINGSW RESULT pid={} tid={} pointer=0x{:08x} environment=empty encoding=utf16le process_private=1 stack_cleanup=none",
                                active_pid, active_tid, result,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_wide_char_to_multi_byte = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "WideCharToMultiByte"
                    );
                    if is_wide_char_to_multi_byte {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            9,
                        )?;
                        let code_page = frame[1];
                        let flags = frame[2];
                        let source = frame[3];
                        let count = frame[4];
                        let output = frame[5];
                        let capacity = frame[6];
                        let default_char = frame[7];
                        let used_default_char = frame[8];
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD WIDECHARTOMULTIBYTE CALL pid={} tid={} during=\"{}\" provider_id={} code_page={} flags=0x{:08x} source=0x{:08x} count={} output=0x{:08x} capacity={} default_char=0x{:08x} used_default_char=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                code_page,
                                flags,
                                source,
                                count as i32,
                                output,
                                capacity,
                                default_char,
                                used_default_char,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if code_page != 0 && code_page != XP_ANSI_CODE_PAGE {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE FRONTIER pid={} tid={} reason=unsupported-code-page code_page={}",
                                    active_pid, active_tid, code_page,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("WideCharToMultiByte child provider did not return".into());
                        };
                        if output == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE RESULT pid={} tid={} mode=size-query eax={} cleanup=32-by-thunk",
                                    active_pid, active_tid, result,
                                ),
                            );
                        } else {
                            let byte_count = usize::try_from(result)
                                .map_err(|_| "WideCharToMultiByte result too large".to_owned())?;
                            let mut bytes = vec![0; byte_count];
                            let read = child
                                .address_space
                                .read(output, &mut bytes)
                                .map_err(|error| error.to_string())?;
                            if read != bytes.len() {
                                return Err("short WideCharToMultiByte output read".into());
                            }
                            let mut preview = bytes
                                .iter()
                                .take(64)
                                .map(|byte| format!("{byte:02x}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            if bytes.len() > 64 {
                                preview.push_str(" ...");
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD WIDECHARTOMULTIBYTE RESULT pid={} tid={} mode=convert eax={} output=0x{:08x} bytes=\"{}\" cleanup=32-by-thunk",
                                    active_pid, active_tid, result, output, preview,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_get_version_ex_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetVersionExA"
                    );
                    if is_get_version_ex_a {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let info = frame[1];
                        let size = read_guest_words(&X86Memory(&child.address_space), info, 1)?[0];
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSIONEXA CALL pid={} tid={} during=\"{}\" provider_id={} info=0x{:08x} size=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                info,
                                size,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        if !matches!(size, OSVERSIONINFOA_SIZE | OSVERSIONINFOEXA_SIZE) {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETVERSIONEXA FRONTIER pid={} tid={} reason=unsupported-structure-size size=0x{:08x}",
                                    active_pid, active_tid, size,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("GetVersionExA child provider did not return".into());
                        };
                        let version = read_guest_words(&X86Memory(&child.address_space), info, 5)?;
                        if result != 1 || version != [size, 5, 1, 2600, 2] {
                            return Err("GetVersionExA child result verification failed".into());
                        }
                        let mut version_tail = [0; 8];
                        if size == OSVERSIONINFOEXA_SIZE {
                            if child
                                .address_space
                                .read(info + OSVERSIONINFOA_SIZE, &mut version_tail)
                                .map_err(|error| error.to_string())?
                                != version_tail.len()
                            {
                                return Err("short OSVERSIONINFOEXA tail read".into());
                            }
                            let service_pack_major = u16::from_le_bytes(version_tail[0..2].try_into().unwrap());
                            let service_pack_minor = u16::from_le_bytes(version_tail[2..4].try_into().unwrap());
                            let suite_mask = u16::from_le_bytes(version_tail[4..6].try_into().unwrap());
                            if service_pack_major != 0
                                || service_pack_minor != 0
                                || suite_mask != 0
                                || version_tail[6] != VER_NT_WORKSTATION
                                || version_tail[7] != 0
                            {
                                return Err("GetVersionExA extended result verification failed".into());
                            }
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSIONEXA RESULT pid={} tid={} size=0x{:08x} eax={} major={} minor={} build={} platform={} service_pack={}.{} suite=0x{:04x} product_type={} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                size,
                                result,
                                version[1],
                                version[2],
                                version[3],
                                version[4],
                                u16::from_le_bytes(version_tail[0..2].try_into().unwrap()),
                                u16::from_le_bytes(version_tail[2..4].try_into().unwrap()),
                                u16::from_le_bytes(version_tail[4..6].try_into().unwrap()),
                                version_tail[6],
                            ),
                        );
                        continue;
                    }
                    let is_get_version = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "GetVersion"
                    );
                    if is_get_version {
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"GetVersion\" esp=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("GetVersion child provider did not return".into());
                        };
                        if value != 0x0a28_0105 {
                            return Err(format!(
                                "GetVersion result mismatch expected=0x0a280105 actual=0x{value:08x}"
                            ));
                        }
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETVERSION RESULT pid={} tid={} eax=0x{:08x} stack_cleanup=none",
                                active_pid, active_tid, value
                            ),
                        );
                        continue;
                    }
                    let is_initialize_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "InitializeCriticalSection"
                    );
                    if is_initialize_critical_section {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut critical_section = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut critical_section)
                            .map_err(|error| error.to_string())?;
                        let critical_section = u32::from_le_bytes(critical_section);
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"InitializeCriticalSection\" esp=0x{:08x} critical_section=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                critical_section
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err(
                                "InitializeCriticalSection child provider did not return".into()
                            );
                        };
                        let mut initialized = [0; 0x18];
                        child
                            .address_space
                            .read(critical_section, &mut initialized)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(
                            initialized[4..8]
                                .try_into()
                                .map_err(|_| "critical-section lock count")?,
                        );
                        let recursion = u32::from_le_bytes(
                            initialized[8..12]
                                .try_into()
                                .map_err(|_| "critical-section recursion")?,
                        );
                        let owner = u32::from_le_bytes(
                            initialized[12..16]
                                .try_into()
                                .map_err(|_| "critical-section owner")?,
                        );
                        if lock_count != u32::MAX || recursion != 0 || owner != 0 {
                            return Err(
                                "child critical-section initialization verification failed".into(),
                            );
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRITICAL SECTION INIT pid={} address=0x{:08x} lock_count=0xffffffff recursion=0 owner=0 caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                                active_pid,
                                critical_section,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip,
                                value
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_enter_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "EnterCriticalSection"
                    );
                    if is_enter_critical_section {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut critical_section = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut critical_section)
                            .map_err(|error| error.to_string())?;
                        let critical_section = u32::from_le_bytes(critical_section);
                        let known = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .has_critical_section(critical_section);
                        if !known {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRITICAL SECTION FRONTIER pid={} tid={} operation=enter reason=unknown-critical-section address=0x{:08x}",
                                    active_pid, active_tid, critical_section
                                ),
                            );
                            return Ok(());
                        }
                        let mut before = [0; 16];
                        child
                            .address_space
                            .read(critical_section, &mut before)
                            .map_err(|error| error.to_string())?;
                        let lock_count_before =
                            u32::from_le_bytes(before[4..8].try_into().unwrap());
                        let recursion_before =
                            u32::from_le_bytes(before[8..12].try_into().unwrap());
                        let owner_before = u32::from_le_bytes(before[12..16].try_into().unwrap());
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"EnterCriticalSection\" esp=0x{:08x} critical_section=0x{:08x} caller_ret=0x{:08x} lock_count_before=0x{:08x} recursion_before={} owner_before={}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                critical_section,
                                u32::from_le_bytes(caller_ret),
                                lock_count_before,
                                recursion_before,
                                owner_before,
                            ),
                        );
                        if owner_before != 0 && owner_before != active_tid {
                            session
                                .block_critical_section(CriticalSectionWait {
                                    key: active_key,
                                    address: critical_section,
                                    provider_id,
                                    esp: exit.registers.esp,
                                })
                                .map_err(str::to_owned)?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRITICAL SECTION BLOCK pid={} tid={} address=0x{:08x} owner={}",
                                    active_pid, active_tid, critical_section, owner_before,
                                ),
                            );
                            if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                                active = next;
                                continue;
                            }
                            return Err("critical section deadlock: no runnable guest".into());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("EnterCriticalSection child provider did not return".into());
                        };
                        let mut after = [0; 16];
                        child
                            .address_space
                            .read(critical_section, &mut after)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(after[4..8].try_into().unwrap());
                        let recursion = u32::from_le_bytes(after[8..12].try_into().unwrap());
                        let owner = u32::from_le_bytes(after[12..16].try_into().unwrap());
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRITICAL SECTION ENTER pid={} tid={} address=0x{:08x} lock_count=0x{:08x} recursion={} owner={} caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                                active_pid,
                                active_tid,
                                critical_section,
                                lock_count,
                                recursion,
                                owner,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip,
                                value,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_leave_critical_section = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "LeaveCriticalSection"
                    );
                    if is_leave_critical_section {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut address = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut address)
                            .map_err(|error| error.to_string())?;
                        let address = u32::from_le_bytes(address);
                        if !session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .has_critical_section(address)
                        {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRITICAL SECTION FRONTIER pid={} tid={} operation=leave reason=unknown-critical-section address=0x{:08x}",
                                    active_pid, active_tid, address
                                ),
                            );
                            return Ok(());
                        }
                        let mut before = [0; 16];
                        child
                            .address_space
                            .read(address, &mut before)
                            .map_err(|error| error.to_string())?;
                        let lock_before = u32::from_le_bytes(before[4..8].try_into().unwrap());
                        let recursion_before =
                            u32::from_le_bytes(before[8..12].try_into().unwrap());
                        let owner_before = u32::from_le_bytes(before[12..16].try_into().unwrap());
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"LeaveCriticalSection\" esp=0x{:08x} critical_section=0x{:08x} caller_ret=0x{:08x} lock_count_before=0x{:08x} recursion_before={} owner_before={}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                address,
                                u32::from_le_bytes(caller_ret),
                                lock_before,
                                recursion_before,
                                owner_before
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(value) = action else {
                            return Err("LeaveCriticalSection child provider did not return".into());
                        };
                        let mut after = [0; 16];
                        child
                            .address_space
                            .read(address, &mut after)
                            .map_err(|error| error.to_string())?;
                        let lock_count = u32::from_le_bytes(after[4..8].try_into().unwrap());
                        let recursion = u32::from_le_bytes(after[8..12].try_into().unwrap());
                        let owner = u32::from_le_bytes(after[12..16].try_into().unwrap());
                        if owner == 0 {
                            if let Some(wait) = session.take_critical_waiter(active_pid, address) {
                                let acquired = session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .dispatch_provider_for_process(
                                        active_pid,
                                        wait.key.tid,
                                        wait.provider_id,
                                        wait.esp,
                                        &mut X86Memory(&child.address_space),
                                    )
                                    .map_err(str::to_owned)?;
                                let PersonalityAction::Return(acquired) = acquired else {
                                    return Err("critical section waiter did not acquire".into());
                                };
                                let index = context_index(&contexts, wait.key)
                                    .ok_or("critical section waiter context missing")?;
                                let mut resumed = contexts[index]
                                    .context
                                    .registers()
                                    .map_err(|error| error.to_string())?;
                                resumed.eax = acquired;
                                contexts[index]
                                    .context
                                    .set_registers(resumed)
                                    .map_err(|error| error.to_string())?;
                                session.enqueue(wait.key);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD CRITICAL SECTION WAKE pid={} tid={} address=0x{:08x} previous_owner={}",
                                        active_pid, wait.key.tid, address, active_tid,
                                    ),
                                );
                            }
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRITICAL SECTION LEAVE pid={} tid={} address=0x{:08x} lock_count=0x{:08x} recursion={} owner={} caller_ret=0x{:08x} resume_eip=0x{:08x} return_eax=0x{:08x}",
                                active_pid,
                                active_tid,
                                address,
                                lock_count,
                                recursion,
                                owner,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip,
                                value
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_set_last_error = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "SetLastError"
                    );
                    if is_set_last_error {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut value = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut value)
                            .map_err(|error| error.to_string())?;
                        let value = u32::from_le_bytes(value);
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROVIDER CALL pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"SetLastError\" esp=0x{:08x} value=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.module,
                                exit.registers.esp,
                                value
                            ),
                        );
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("SetLastError child provider did not return".into());
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LAST ERROR SET pid={} tid={} value=0x{:08x}",
                                active_pid, active_tid, value
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_cipow = matches!(&provider.symbol, child_loader::ProviderSymbol::Name(name) if provider.module.eq_ignore_ascii_case("MSVCRT.dll") && name == "_CIpow");
                    if is_cipow {
                        if child.cipow.is_some() {
                            return Err("nested CIPOW".into());
                        }
                        child.cipow = Some(ChildCiPow {
                            provider_esp: exit.registers.esp,
                        });
                        let mut registers = exit.registers;
                        registers.eip = thunk32::CHILD_CIPOW_SPILL_ADDRESS;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_unhandled_filter = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "SetUnhandledExceptionFilter"
                    );
                    if is_unhandled_filter {
                        let filter = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?[1];
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(previous) = action else {
                            return Err(
                                "SetUnhandledExceptionFilter child provider did not return".into(),
                            );
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD UNHANDLED FILTER SET pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" filter=0x{:08x} previous=0x{:08x} caller_ret=0x{:08x} resume_eip=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                filter,
                                previous,
                                u32::from_le_bytes(caller_ret),
                                exit.registers.eip
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = previous;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_set_app_type = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "__set_app_type"
                    );
                    if is_crt_set_app_type {
                        let app_type = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?[1];
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(return_value) = action else {
                            return Err("__set_app_type child provider did not return".into());
                        };
                        let kind = match app_type {
                            CRT_UNKNOWN_APP => "unknown",
                            CRT_CONSOLE_APP => "console",
                            CRT_GUI_APP => "gui",
                            _ => "unobserved",
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT SET APP TYPE pid={} tid={} app_type={} kind={} cleanup=0-by-thunk",
                                active_pid, active_tid, app_type, kind,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = return_value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_get_main_args = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "__getmainargs"
                    );
                    if is_crt_get_main_args {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            6,
                        )?;
                        let new_mode = if frame[5] == 0 {
                            0
                        } else {
                            read_guest_words(&X86Memory(&child.address_space), frame[5], 1)?[0]
                        };
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(return_value) = action else {
                            return Err("__getmainargs child provider did not return".into());
                        };
                        let argc =
                            read_guest_words(&X86Memory(&child.address_space), frame[1], 1)?[0];
                        let argv =
                            read_guest_words(&X86Memory(&child.address_space), frame[2], 1)?[0];
                        let envp =
                            read_guest_words(&X86Memory(&child.address_space), frame[3], 1)?[0];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT GETMAINARGS pid={} tid={} argc={} argv=0x{:08x} envp=0x{:08x} wildcard={} new_mode={} cleanup=0-by-thunk",
                                active_pid, active_tid, argc, argv, envp, frame[4], new_mode,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = return_value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_onexit = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "_onexit"
                    );
                    if is_crt_onexit {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let mut child_memory = X86Memory(&child.address_space);
                        let (action, entries) = {
                            let process = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?;
                            let action = process
                                .xp
                                .dispatch_provider_for_process(
                                    active_pid,
                                    active_tid,
                                    provider_id,
                                    exit.registers.esp,
                                    &mut child_memory,
                                )
                                .map_err(str::to_owned)?;
                            (action, process.xp.crt_onexit_count())
                        };
                        let PersonalityAction::Return(return_value) = action else {
                            return Err("_onexit child provider did not return".into());
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT ONEXIT pid={} tid={} caller_ret=0x{:08x} func=0x{:08x} entries={} return_eax=0x{:08x} cleanup=0-by-thunk",
                                active_pid, active_tid, frame[0], frame[1], entries, return_value,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = return_value;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if matches!(
                        operation,
                        child_loader::ProviderOp::CrtControlFp
                            | child_loader::ProviderOp::CrtControl87
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let new_control = frame[1];
                        let mask = frame[2];
                        let mut state = contexts[active]
                            .context
                            .extended_state()
                            .map_err(|error| error.to_string())?;
                        let before_fcw = u16::from_le_bytes(state.bytes[0..2].try_into().unwrap());
                        let old_control = msvcrt_control_from_x87(before_fcw);
                        let supported_mask = if operation == child_loader::ProviderOp::CrtControlFp {
                            MSVCRT_X87_CONTROL_MASK & !MSVCRT_EM_DENORMAL
                        } else {
                            MSVCRT_X87_CONTROL_MASK
                        };
                        let effective_mask = mask & supported_mask;
                        let updated_control =
                            (old_control & !effective_mask) | (new_control & effective_mask);
                        let after_fcw = x87_control_from_msvcrt(before_fcw, updated_control);
                        state.bytes[0..2].copy_from_slice(&after_fcw.to_le_bytes());
                        let mut xstate_bv =
                            u64::from_le_bytes(state.bytes[512..520].try_into().unwrap());
                        xstate_bv |= 1 << 0;
                        state.bytes[512..520].copy_from_slice(&xstate_bv.to_le_bytes());
                        contexts[active]
                            .context
                            .set_extended_state(&state)
                            .map_err(|error| error.to_string())?;
                        let mut registers = exit.registers;
                        registers.eax = updated_control;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT {} pid={} tid={} new=0x{:08x} mask=0x{:08x} effective_mask=0x{:08x} x87_before=0x{:04x} x87_after=0x{:04x} eax=0x{:08x} cleanup=0-by-thunk",
                                if operation == child_loader::ProviderOp::CrtControlFp { "CONTROLFP" } else { "CONTROL87" },
                                active_pid,
                                active_tid,
                                new_control,
                                mask,
                                effective_mask,
                                before_fcw,
                                after_fcw,
                                updated_control,
                            ),
                        );
                        continue;
                    }
                    let is_crt_clear_fp = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "_clearfp"
                    );
                    if is_crt_clear_fp {
                        let caller_ret = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            1,
                        )?[0];
                        let mut state = contexts[active]
                            .context
                            .extended_state()
                            .map_err(|error| error.to_string())?;
                        let before_fsw = u16::from_le_bytes(state.bytes[2..4].try_into().unwrap());
                        // FNCLEx clears x87 exception flags, the exception and
                        // stack-fault summaries, and the busy bit.  It leaves
                        // the condition-code and TOP fields intact.
                        let after_fsw = before_fsw & !0x80ff;
                        state.bytes[2..4].copy_from_slice(&after_fsw.to_le_bytes());
                        let mut xstate_bv =
                            u64::from_le_bytes(state.bytes[512..520].try_into().unwrap());
                        xstate_bv |= 1 << 0;
                        state.bytes[512..520].copy_from_slice(&xstate_bv.to_le_bytes());
                        contexts[active]
                            .context
                            .set_extended_state(&state)
                            .map_err(|error| error.to_string())?;
                        let result = msvcrt_status_from_x87(before_fsw);
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT CLEARFP pid={} tid={} caller_ret=0x{:08x} x87_before=0x{:04x} x87_after=0x{:04x} eax=0x{:08x} cleanup=0-by-thunk",
                                active_pid,
                                active_tid,
                                caller_ret,
                                before_fsw,
                                after_fsw,
                                result,
                            ),
                        );
                        continue;
                    }
                    let is_crt_ftol = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "_ftol"
                    );
                    if is_crt_ftol {
                        let caller_ret = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            1,
                        )?[0];
                        let mut state = contexts[active]
                            .context
                            .extended_state()
                            .map_err(|error| error.to_string())?;
                        let result = wc3::ThisToThat::x87_ftol(&mut state.bytes);
                        let mut xstate_bv =
                            u64::from_le_bytes(state.bytes[512..520].try_into().unwrap());
                        xstate_bv |= 1 << 0;
                        state.bytes[512..520].copy_from_slice(&xstate_bv.to_le_bytes());
                        contexts[active]
                            .context
                            .set_extended_state(&state)
                            .map_err(|error| error.to_string())?;
                        let mut registers = exit.registers;
                        registers.eax = result as u32;
                        registers.edx = (result >> 32) as u32;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT FTOL pid={} tid={} caller_ret=0x{:08x} result={} eax=0x{:08x} edx=0x{:08x} cleanup=0-by-thunk",
                                active_pid,
                                active_tid,
                                caller_ret,
                                result,
                                result as u32,
                                (result >> 32) as u32,
                            ),
                        );
                        continue;
                    }
                    let is_crt_malloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "malloc"
                    );
                    if is_crt_malloc {
                        let argument =
                            exit.registers.esp.checked_add(4).ok_or_else(|| {
                                "child provider argument address overflow".to_owned()
                            })?;
                        let mut size = [0; 4];
                        child
                            .address_space
                            .read(argument, &mut size)
                            .map_err(|error| error.to_string())?;
                        let size = u32::from_le_bytes(size);
                        if size == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD PROVIDER FRONTIER pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} module=\"{}\" symbol=\"malloc\" reason=zero-size-unobserved",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    provider_id,
                                    provider.module,
                                ),
                            );
                            return Ok(());
                        }
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(pointer) = action else {
                            return Err("malloc child provider did not return".into());
                        };
                        if pointer == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT MALLOC pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" size={} pointer=0x00000000 result=out-of-memory",
                                    active_pid, active_tid, running_module_name, size,
                                ),
                            );
                        } else {
                            let mapped_end =
                                ensure_child_crt_allocation_mapped(child, pointer, size)?;
                            if pointer == 1
                                || pointer % 8 != 0
                                || pointer < CHILD_CRT_HEAP_BASE
                                || pointer
                                    .checked_add(size)
                                    .filter(|end| {
                                        *end <= CHILD_CRT_HEAP_LIMIT && *end <= mapped_end
                                    })
                                    .is_none()
                            {
                                return Err("child CRT malloc pointer verification failed".into());
                            }
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT MALLOC pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" size={} pointer=0x{:08x} mapped_end=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    size,
                                    pointer,
                                    mapped_end,
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = pointer;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_dllonexit = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "__dllonexit"
                    );
                    if is_crt_dllonexit {
                        let result = {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            child_dllonexit(
                                child,
                                process,
                                active_pid,
                                active_tid,
                                &running_module_name,
                                provider_id,
                                exit.registers.esp,
                            )?
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_crt_initterm = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "_initterm"
                    );
                    if is_crt_initterm {
                        if child.initterm.is_some() {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT INITTERM FRONTIER pid={} tid={} reason=nested-initterm",
                                    active_pid, active_tid
                                ),
                            );
                            return Ok(());
                        }
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let caller_ret = frame[0];
                        let begin = frame[1];
                        let end = frame[2];
                        if running_module_name.eq_ignore_ascii_case("Storm.dll")
                            && caller_ret != 0x1503_630a
                        {
                            return Err(format!(
                                "Storm _initterm caller mismatch expected=0x1503630a actual=0x{caller_ret:08x}"
                            ));
                        }
                        if begin > end || begin % 4 != 0 || end % 4 != 0 {
                            return Err(format!(
                                "malformed child _initterm range begin=0x{begin:08x} end=0x{end:08x}"
                            ));
                        }
                        let bytes = end
                            .checked_sub(begin)
                            .ok_or_else(|| "child _initterm range underflow".to_owned())?;
                        if bytes % 4 != 0 {
                            return Err("child _initterm byte range is not pointer-aligned".into());
                        }
                        let entries = bytes / 4;
                        if entries > 65_536 {
                            return Err(format!(
                                "child _initterm range exceeds defensive bound entries={entries}"
                            ));
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT INITTERM pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} begin=0x{:08x} end=0x{:08x} entries={} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                begin,
                                end,
                                entries,
                                caller_ret
                            ),
                        );
                        child.initterm = Some(ChildInitterm {
                            provider_resume_eip: exit.registers.eip,
                            provider_esp: exit.registers.esp,
                            begin,
                            cursor: begin,
                            end,
                            callbacks_invoked: 0,
                            rust_callbacks: 0,
                        });
                        match advance_child_initterm(child, &mut contexts[active])? {
                            InittermAdvance::CallbackScheduled | InittermAdvance::Complete => {
                                continue;
                            }
                        }
                    }
                    let is_reg_open_key_ex_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("ADVAPI32.dll")
                                && name == "RegOpenKeyExA"
                    );
                    if is_reg_open_key_ex_a {
                        let child_memory = X86Memory(&child.address_space);
                        let frame = wc3::reg::decode_open_key_ex_a(&child_memory, exit.registers.esp)?;
                        if frame.caller_ret != u32::from_le_bytes(caller_ret) {
                            return Err("RegOpenKeyExA caller return mismatch".into());
                        }
                        let subkey = frame.subkey.as_deref().unwrap_or("");
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY OPEN pid={} tid={} root=\"{}\" subkey=[redacted] subkey_bytes={} sam=0x{:08x}",
                                active_pid,
                                active_tid,
                                wc3::reg::format_root_name(frame.hkey),
                                subkey.len(),
                                frame.sam
                            ),
                        );
                        ensure_registry_loaded(&mut session).await?;
                        let registry = match &session.registry {
                            wc3::session::RegistryState::Ready(registry) => registry,
                            wc3::session::RegistryState::Unloaded => {
                                return Err("registry remained unloaded".into());
                            }
                        };
                        let start = registry.root(frame.hkey).or_else(|| {
                            session
                                .process(active_pid)
                                .and_then(|process| process.xp.registry_handle_node(frame.hkey))
                        });
                        let node = (frame.options == 0)
                            .then_some(start)
                            .flatten()
                            .and_then(|node| registry.child_path(node, subkey));
                        let (result, handle) = if let Some(node) = node {
                            let handle = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .open_registry_key(node, frame.sam)
                                .map_err(str::to_owned)?;
                            child
                                .address_space
                                .write(frame.result_ptr, &handle.to_le_bytes())
                                .map_err(|error| error.to_string())?;
                            (0u32, Some(handle))
                        } else {
                            (2u32, None)
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY OPEN RESULT pid={} exists={} result={} handle={}",
                                active_pid,
                                node.is_some() as u8,
                                result,
                                handle
                                    .map(|handle| format!("0x{handle:08x}"))
                                    .unwrap_or_else(|| "-".into())
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_reg_query_value_ex_a = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("ADVAPI32.dll")
                                && name == "RegQueryValueExA"
                    );
                    if is_reg_query_value_ex_a {
                        let child_memory = X86Memory(&child.address_space);
                        let frame = wc3::reg::decode_query_value_ex_a(&child_memory, exit.registers.esp)?;
                        if frame.caller_ret != u32::from_le_bytes(caller_ret) {
                            return Err("RegQueryValueExA caller return mismatch".into());
                        }
                        let input_capacity = (frame.size_ptr != 0)
                            .then(|| read_guest_words(&child_memory, frame.size_ptr, 1))
                            .transpose()?
                            .map(|words| words[0]);
                        let handle_node = session
                            .process(active_pid)
                            .and_then(|process| process.xp.registry_handle_node(frame.hkey));
                        ensure_registry_loaded(&mut session).await?;
                        let (node, key_path, loaded_before, value, value_source) = match &mut session.registry {
                            wc3::session::RegistryState::Ready(registry) => {
                                let node = registry.root(frame.hkey).or(handle_node);
                                let key_path = node.and_then(|node| registry.key_identity(node));
                                let loaded_before =
                                    node.is_some_and(|node| registry.values_loaded(node));
                                let design_value = key_path.as_ref().and_then(|(root, path)| {
                                    wc3::reg::lookup(
                                        *root,
                                        path,
                                        frame.value_name.as_deref().unwrap_or(""),
                                    )
                                });
                                let (value, value_source) = if let Some(value) = design_value {
                                    (Some((value.ty, value.bytes.to_vec())), "design")
                                } else if let Some(node) = node {
                                    registry.ensure_values_loaded(node).map_err(str::to_owned)?;
                                    let value = registry
                                        .value(node, frame.value_name.as_deref().unwrap_or(""))
                                        .map(|value| (value.ty, value.bytes.clone()));
                                    (value, "backing")
                                } else {
                                    (None, "absent")
                                };
                                (node, key_path, loaded_before, value, value_source)
                            }
                            wc3::session::RegistryState::Unloaded => {
                                return Err("registry remained unloaded".into());
                            }
                        };
                        let value_name = frame.value_name.as_deref().unwrap_or("");
                        let (value_type, value_bytes) = value
                            .as_ref()
                            .map(|(ty, bytes)| (format!("0x{ty:08x}"), bytes.len().to_string()))
                            .unwrap_or_else(|| ("-".into(), "-".into()));
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY QUERY CALL pid={} tid={} hkey=0x{:08x} node={} key_path={:?} value_name={:?} reserved=0x{:08x} type_ptr=0x{:08x} data_ptr=0x{:08x} size_ptr=0x{:08x} input_capacity={} values_loaded_before={} source={} present={} value_type={} value_bytes={} caller_ret=0x{:08x} cleanup=24-by-thunk",
                                active_pid,
                                active_tid,
                                frame.hkey,
                                node.map(|node| format!("0x{node:08x}")).unwrap_or_else(|| "-".into()),
                                key_path.as_ref().map(|(_, path)| path.as_str()).unwrap_or("-"),
                                value_name,
                                frame.reserved,
                                frame.type_ptr,
                                frame.data_ptr,
                                frame.size_ptr,
                                input_capacity.map(|value| value.to_string()).unwrap_or_else(|| "-".into()),
                                loaded_before as u8,
                                value_source,
                                value.is_some() as u8,
                                value_type,
                                value_bytes,
                                frame.caller_ret,
                            ),
                        );
                        let result = if frame.reserved != 0 {
                            ERROR_INVALID_PARAMETER
                        } else if node.is_none() {
                            ERROR_INVALID_HANDLE
                        } else if value.is_none() {
                            ERROR_FILE_NOT_FOUND
                        } else {
                            let (ty, bytes) = value.as_ref().unwrap();
                            let required = u32::try_from(bytes.len())
                                .map_err(|_| "registry value size overflow")?;
                            if frame.type_ptr != 0 {
                                child
                                    .address_space
                                    .write(frame.type_ptr, &ty.to_le_bytes())
                                    .map_err(|error| error.to_string())?;
                            }
                            if frame.data_ptr != 0 && frame.size_ptr == 0 {
                                ERROR_INVALID_PARAMETER
                            } else {
                                if frame.size_ptr != 0 {
                                    child
                                        .address_space
                                        .write(frame.size_ptr, &required.to_le_bytes())
                                        .map_err(|error| error.to_string())?;
                                }
                                if frame.data_ptr == 0 {
                                    ERROR_SUCCESS
                                } else if input_capacity.unwrap_or(0) < required {
                                    ERROR_MORE_DATA
                                } else {
                                    child
                                        .address_space
                                        .write(frame.data_ptr, bytes)
                                        .map_err(|error| error.to_string())?;
                                    ERROR_SUCCESS
                                }
                            }
                        };
                        let (result_type, required, dword) = value
                            .as_ref()
                            .map(|(ty, bytes)| {
                                let kind = match *ty {
                                    3 => "REG_BINARY",
                                    4 => "REG_DWORD",
                                    _ => "REG_UNKNOWN",
                                };
                                let dword = (*ty == 4 && bytes.len() == 4).then(|| {
                                    u32::from_le_bytes(bytes[..4].try_into().unwrap())
                                });
                                (kind, bytes.len(), dword)
                            })
                            .unwrap_or(("-", 0, None));
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY QUERY RESULT pid={} tid={} hkey=0x{:08x} value_name={:?} source={} present={} type={} required={} capacity={} dword={} result={} cleanup=24-by-thunk",
                                active_pid,
                                active_tid,
                                frame.hkey,
                                value_name,
                                value_source,
                                value.is_some() as u8,
                                result_type,
                                required,
                                input_capacity.map(|value| value.to_string()).unwrap_or_else(|| "-".into()),
                                dword.map(|value| format!("0x{value:08x}")).unwrap_or_else(|| "-".into()),
                                result,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_reg_close_key = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("ADVAPI32.dll")
                                && name == "RegCloseKey"
                    );
                    if is_reg_close_key {
                        let child_memory = X86Memory(&child.address_space);
                        let frame = read_guest_words(&child_memory, exit.registers.esp, 2)?;
                        let frame_caller_ret = frame[0];
                        let hkey = frame[1];
                        if frame_caller_ret != u32::from_le_bytes(caller_ret) {
                            return Err("RegCloseKey caller return mismatch".into());
                        }
                        let predefined = wc3::reg::is_predefined_root(hkey);
                        let closed = if predefined {
                            false
                        } else {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .close_registry_handle(hkey)
                        };
                        let result = if predefined || closed {
                            ERROR_SUCCESS
                        } else {
                            ERROR_INVALID_HANDLE
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD REGISTRY CLOSE RESULT pid={} tid={} hkey=0x{:08x} kind={} result={} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                hkey,
                                if predefined { "predefined" } else { "opened" },
                                result,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_virtual_alloc = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "VirtualAlloc"
                    );
                    if is_virtual_alloc {
                        let frame = decode_virtual_alloc_frame(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                        )?;
                        if frame.size == 0 {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error_for_thread(active_tid, 87);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        let supported_reserve = frame.address == 0
                            && frame.size != 0
                            && frame.allocation_type == 0x0000_2000
                            && frame.protect == 0x01;
                        let supported_commit = frame.address != 0
                            && frame.size != 0
                            && frame.allocation_type == 0x0000_1000
                            && frame.protect == 0x04;
                        let supported_null_commit = frame.address == 0
                            && frame.size != 0
                            && frame.allocation_type == 0x0000_1000
                            && frame.protect == 0x04;
                        if !supported_reserve && !supported_commit && !supported_null_commit {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC FRONTIER pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" provider_id={} caller_ret=0x{:08x} address=0x{:08x} size=0x{:08x} allocation_type=0x{:08x} protect=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    provider_id,
                                    frame.caller_ret,
                                    frame.address,
                                    frame.size,
                                    frame.allocation_type,
                                    frame.protect,
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC FLAGS commit={} reserve={} top_down={} protect_name=\"{}\"",
                                    (frame.allocation_type & 0x0000_1000 != 0) as u8,
                                    (frame.allocation_type & 0x0000_2000 != 0) as u8,
                                    (frame.allocation_type & 0x0010_0000 != 0) as u8,
                                    virtual_alloc_protect_name(frame.protect),
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC FRONTIER reason=unsupported-observed-shape"
                                ),
                            );
                            return Ok(());
                        }
                        if supported_null_commit {
                            let request = session
                                .process(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .virtual_prepare_null_commit(frame.size)
                                .map_err(str::to_owned)?;
                            let Some(request) = request else {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_tid, 8);
                                let mut registers = exit.registers;
                                registers.eax = 0;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            };
                            child
                                .address_space
                                .map(
                                    request.base,
                                    usize::try_from(request.size)
                                        .map_err(|_| "VirtualAlloc null commit size")?,
                                    Permissions::READ | Permissions::WRITE,
                                )
                                .map_err(|error| {
                                    format!("map VirtualAlloc null commit: {error}")
                                })?;
                            let zeroes = vec![
                                0;
                                usize::try_from(request.size)
                                    .map_err(|_| "VirtualAlloc null commit zero size")?
                            ];
                            if child
                                .address_space
                                .write(request.base, &zeroes)
                                .map_err(|error| error.to_string())?
                                != zeroes.len()
                            {
                                return Err("short VirtualAlloc null commit initialization".into());
                            }
                            let (reservations, reserve_next, committed_ranges, committed_bytes) = {
                                let process = &mut session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp;
                                process
                                    .virtual_finish_null_commit(request)
                                    .map_err(str::to_owned)?;
                                let (reservations, reserve_next) = process.virtual_reservation_state();
                                let (committed_ranges, committed_bytes) = process.virtual_commit_state();
                                (reservations, reserve_next, committed_ranges, committed_bytes)
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC NULL COMMIT pid={} tid={} requested_size=0x{:08x} region_size=0x{:08x} allocation_base=0x{:08x} protect=PAGE_READWRITE permissions=RW guest_mapped=1 zero_initialized=1 return_eax=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    frame.size,
                                    request.size,
                                    request.base,
                                    request.base,
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} committed_ranges={} committed_bytes={} reserve_next=0x{:08x}",
                                    active_pid,
                                    reservations,
                                    committed_ranges,
                                    committed_bytes,
                                    reserve_next,
                                ),
                            );
                            let mut registers = exit.registers;
                            registers.eax = request.base;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        if supported_commit {
                            let request = {
                                let process = &session
                                    .process(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp;
                                match process.virtual_prepare_commit(frame.address, frame.size) {
                                    Ok(Some(request)) => request,
                                    Ok(None) => {
                                        session
                                            .process_mut(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .set_last_error_for_thread(active_tid, 487);
                                        let mut registers = exit.registers;
                                        registers.eax = 0;
                                        contexts[active]
                                            .context
                                            .set_registers(registers)
                                            .map_err(|error| error.to_string())?;
                                        continue;
                                    }
                                    Err("VirtualAlloc overlapping commit") => {
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD VIRTUALALLOC FRONTIER reason=overlapping-commit"
                                            ),
                                        );
                                        return Ok(());
                                    }
                                    Err(error) => return Err(error.into()),
                                }
                            };
                            child
                                .address_space
                                .map(
                                    request.base,
                                    usize::try_from(request.size)
                                        .map_err(|_| "VirtualAlloc commit size")?,
                                    Permissions::READ | Permissions::WRITE,
                                )
                                .map_err(|error| format!("map VirtualAlloc commit: {error}"))?;
                            let zeroes = vec![
                                0;
                                usize::try_from(request.size)
                                    .map_err(|_| "VirtualAlloc zero size")?
                            ];
                            if child
                                .address_space
                                .write(request.base, &zeroes)
                                .map_err(|error| error.to_string())?
                                != zeroes.len()
                            {
                                return Err("short VirtualAlloc commit initialization".into());
                            }
                            let (reservations, reserve_next, committed_ranges, committed_bytes) = {
                                let process = &mut session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp;
                                process
                                    .virtual_finish_commit(request)
                                    .map_err(str::to_owned)?;
                                let (reservations, reserve_next) =
                                    process.virtual_reservation_state();
                                let (committed_ranges, committed_bytes) =
                                    process.virtual_commit_state();
                                (
                                    reservations,
                                    reserve_next,
                                    committed_ranges,
                                    committed_bytes,
                                )
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC COMMIT pid={} tid={} address=0x{:08x} requested_size=0x{:08x} commit_size=0x{:08x} reservation_base=0x{:08x} reservation_size=0x{:08x} protect=PAGE_READWRITE permissions=RW guest_mapped=1 zero_initialized=1 return_eax=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    frame.address,
                                    frame.size,
                                    request.size,
                                    request.reservation_base,
                                    request.reservation_size,
                                    request.base
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} committed_ranges={} committed_bytes={} reserve_next=0x{:08x}",
                                    active_pid,
                                    reservations,
                                    committed_ranges,
                                    committed_bytes,
                                    reserve_next
                                ),
                            );
                            let mut registers = exit.registers;
                            registers.eax = request.base;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        let (reservation, reservation_count, reserve_next) = {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            let reservation = match process.virtual_reserve_null(frame.size) {
                                Ok(reservation) => reservation,
                                Err(_) => None,
                            };
                            if reservation.is_none() {
                                process.set_last_error_for_thread(active_tid, 8);
                            }
                            let (reservation_count, reserve_next) =
                                process.virtual_reservation_state();
                            (reservation, reservation_count, reserve_next)
                        };
                        let result = reservation.as_ref().map(|value| value.base).unwrap_or(0);
                        if let Some(reservation) = reservation {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALALLOC RESERVE pid={} tid={} during=\"{}:DLL_PROCESS_ATTACH\" requested_address=0x00000000 requested_size=0x{:08x} reserved_base=0x{:08x} reserved_size=0x{:08x} allocation_type=MEM_RESERVE protect=PAGE_NOACCESS committed=0 guest_mapped=0",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    frame.size,
                                    reservation.base,
                                    reservation.size,
                                ),
                            );
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} reserve_next=0x{:08x}",
                                    active_pid, reservation_count, reserve_next
                                ),
                            );
                        }
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let is_virtual_free = matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "VirtualFree"
                    );
                    if is_virtual_free {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let [frame_caller_ret, address, size, free_type] =
                            <[u32; 4]>::try_from(frame).map_err(|_| "VirtualFree frame")?;
                        if frame_caller_ret != u32::from_le_bytes(caller_ret) {
                            return Err("VirtualFree caller return mismatch".into());
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD VIRTUALFREE CALL pid={} tid={} during=\"{}\" provider_id={} address=0x{:08x} size=0x{:08x} free_type=0x{:08x} mem_decommit={} mem_release={} caller_ret=0x{:08x} cleanup=12-by-thunk",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                address,
                                size,
                                free_type,
                                (free_type & 0x0000_4000 != 0) as u8,
                                (free_type & 0x0000_8000 != 0) as u8,
                                frame_caller_ret,
                            ),
                        );
                        if free_type != MEM_RELEASE {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD VIRTUALFREE FRONTIER reason=unsupported-free-type address=0x{address:08x} size=0x{size:08x} free_type=0x{free_type:08x}"
                                ),
                            );
                            return Ok(());
                        }
                        if size != 0 {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error_for_thread(active_tid, ERROR_INVALID_ADDRESS);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
                        let reservation = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .virtual_prepare_release(address, size);
                        let Some(reservation) = reservation else {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error_for_thread(active_tid, ERROR_INVALID_ADDRESS);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        };
                        let committed_ranges = reservation.committed.len();
                        let committed_bytes = reservation
                            .committed
                            .iter()
                            .try_fold(0u32, |total, commit| total.checked_add(commit.size))
                            .ok_or("VirtualFree committed size overflow")?;
                        for commit in &reservation.committed {
                            child
                                .address_space
                                .unmap(
                                    commit.base,
                                    usize::try_from(commit.size)
                                        .map_err(|_| "VirtualFree committed size")?,
                                )
                                .map_err(|error| {
                                    format!(
                                        "VirtualFree unmap base=0x{:08x} size=0x{:08x}: {error}",
                                        commit.base, commit.size
                                    )
                                })?;
                        }
                        let (reservations, reserve_next, remaining_ranges, remaining_bytes) = {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            process
                                .virtual_finish_release(reservation.base, reservation.size)
                                .map_err(str::to_owned)?;
                            let (reservations, reserve_next) = process.virtual_reservation_state();
                            let (remaining_ranges, remaining_bytes) = process.virtual_commit_state();
                            (reservations, reserve_next, remaining_ranges, remaining_bytes)
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD VIRTUALFREE RELEASE pid={} tid={} base=0x{:08x} reservation_size=0x{:08x} committed_ranges={} committed_bytes={} unmapped_bytes={} result=1 cleanup=12-by-thunk",
                                active_pid,
                                active_tid,
                                reservation.base,
                                reservation.size,
                                committed_ranges,
                                committed_bytes,
                                committed_bytes,
                            ),
                        );
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD VIRTUAL MEMORY pid={} reservations={} committed_ranges={} committed_bytes={} reserve_next=0x{:08x}",
                                active_pid,
                                reservations,
                                remaining_ranges,
                                remaining_bytes,
                                reserve_next,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = 1;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    let operation = child_loader::provider_op(&provider);
                    if operation == child_loader::ProviderOp::CreateThread {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            7,
                        )?;
                        let [caller_ret, security, stack_size, start, argument, flags, tid_ptr] =
                            <[u32; 7]>::try_from(frame).map_err(|_| "CreateThread frame")?;
                        if security != 0 || flags != 0 || start == 0 {
                            return Err(format!(
                                "CreateThread argument frontier security=0x{security:08x} flags=0x{flags:08x} start=0x{start:08x}"
                            ));
                        }
                        let (guest, key, handle) = create_child_guest_thread(
                            child,
                            &mut session,
                            stack_size,
                            start,
                            argument,
                        )?;
                        if tid_ptr != 0
                            && child
                                .address_space
                                .write(tid_ptr, &key.tid.to_le_bytes())
                                .map_err(|error| error.to_string())?
                                != 4
                        {
                            return Err("short CreateThread TID write".into());
                        }
                        contexts.push(guest);
                        session.enqueue(key);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CREATETHREAD pid={} caller_tid={} tid={} handle=0x{:08x} start=0x{:08x} parameter=0x{:08x} stack_size={} flags=0x{:08x} tid_ptr=0x{:08x} caller_ret=0x{:08x} cleanup=24-by-thunk",
                                active_pid, active_tid, key.tid, handle, start, argument,
                                stack_size, flags, tid_ptr, caller_ret,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = handle;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::GetThreadPriority {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let [caller_ret, handle] =
                            <[u32; 2]>::try_from(frame).map_err(|_| "GetThreadPriority frame")?;
                        let mut registers = exit.registers;
                        match session.get_thread_priority(active_key, handle) {
                            Ok((target, priority, base)) => {
                                registers.eax = priority as u32;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD GETTHREADPRIORITY pid={} caller_tid={} handle=0x{:08x} target_tid={} priority={} base_priority={} result=0x{:08x} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                        active_pid, active_tid, handle, target.tid, priority,
                                        base, priority as u32, caller_ret,
                                    ),
                                );
                            }
                            Err(error) => {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_tid, error);
                                registers.eax = 0x7fff_ffff;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD GETTHREADPRIORITY pid={} caller_tid={} handle=0x{:08x} result=THREAD_PRIORITY_ERROR_RETURN last_error={} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                        active_pid, active_tid, handle, error, caller_ret,
                                    ),
                                );
                            }
                        }
                        continue;
                    }
                    if operation == child_loader::ProviderOp::SetThreadPriority {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let [caller_ret, handle, priority] =
                            <[u32; 3]>::try_from(frame).map_err(|_| "SetThreadPriority frame")?;
                        let result = session.set_thread_priority(
                            active_key,
                            handle,
                            priority as i32,
                        );
                        let mut registers = exit.registers;
                        let preempt = match result {
                            Ok((target, old, base)) => {
                                registers.eax = 1;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                let current_priority = session.thread_base_priority(active_key).unwrap_or(8);
                                let best_runnable_priority = session
                                    .runnable
                                    .iter()
                                    .copied()
                                    .filter(|key| context_index(&contexts, *key).is_some())
                                    .filter_map(|key| session.thread_base_priority(key))
                                    .max()
                                    .unwrap_or(0);
                                let preempt = best_runnable_priority > current_priority;
                                if preempt {
                                    session.enqueue(active_key);
                                    active = pop_runnable_context(&mut session, &contexts)
                                        .ok_or_else(|| "priority preemption lost runnable context".to_owned())?;
                                }
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SETTHREADPRIORITY pid={} caller_tid={} handle=0x{:08x} target_tid={} priority={} old_priority={} base_priority={} result=1 preempt={} caller_ret=0x{:08x} cleanup=8-by-thunk",
                                        active_pid, active_tid, handle, target.tid, priority as i32,
                                        old, base, preempt as u8, caller_ret,
                                    ),
                                );
                                preempt
                            }
                            Err(error) => {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_tid, error);
                                registers.eax = 0;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD SETTHREADPRIORITY pid={} caller_tid={} handle=0x{:08x} priority={} result=0 last_error={} caller_ret=0x{:08x} cleanup=8-by-thunk",
                                        active_pid, active_tid, handle, priority as i32, error, caller_ret,
                                    ),
                                );
                                false
                            }
                        };
                        if preempt {
                            tokio::task::yield_now().await;
                        }
                        continue;
                    }
                    if operation == child_loader::ProviderOp::CrtBeginThreadEx {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            7,
                        )?;
                        let [caller_ret, security, stack_size, start, argument, flags, tid_ptr] =
                            <[u32; 7]>::try_from(frame).map_err(|_| "_beginthreadex frame")?;
                        if security != 0 || flags != 0 || start == 0 {
                            return Err(format!(
                                "_beginthreadex argument frontier security=0x{security:08x} flags=0x{flags:08x} start=0x{start:08x}"
                            ));
                        }
                        let (guest, key, handle) = create_child_guest_thread(
                            child,
                            &mut session,
                            stack_size,
                            start,
                            argument,
                        )?;
                        if tid_ptr != 0 {
                            if child
                                .address_space
                                .write(tid_ptr, &key.tid.to_le_bytes())
                                .map_err(|error| error.to_string())?
                                != 4
                            {
                                return Err("short _beginthreadex TID write".into());
                            }
                        }
                        contexts.push(guest);
                        session.enqueue(key);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT BEGINTHREADEX pid={} caller_tid={} tid={} handle=0x{:08x} start=0x{:08x} argument=0x{:08x} stack_size={} flags=0x{:08x} tid_ptr=0x{:08x} caller_ret=0x{:08x} cleanup=0-by-thunk",
                                active_pid, active_tid, key.tid, handle, start, argument,
                                stack_size, flags, tid_ptr, caller_ret,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = handle;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::ExitProcess {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let exit_code = frame[1];
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD EXITPROCESS CALL pid={} tid={} during=\"{}\" exit_code=0x{:08x} caller_ret=0x{:08x}",
                                active_pid, active_tid, running_module_name, exit_code, frame[0],
                            ),
                        );
                        let action = {
                            let mut child_memory = X86Memory(&child.address_space);
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .dispatch_provider_for_process(
                                    active_pid,
                                    active_tid,
                                    provider_id,
                                    exit.registers.esp,
                                    &mut child_memory,
                                )
                                .map_err(str::to_owned)?
                        };
                        let PersonalityAction::ExitProcess(dispatched_code) = action else {
                            return Err(
                                "ExitProcess child provider returned a non-exit action".into()
                            );
                        };
                        if dispatched_code != exit_code {
                            return Err("ExitProcess child provider exit code mismatch".into());
                        }
                        let threads_signaled = session
                            .objects
                            .values()
                            .filter(|object| {
                                matches!(object, SessionObject::Thread(thread) if thread.key.pid == active_pid)
                            })
                            .count();
                        let woken = session
                            .terminate_process(active_pid, exit_code)
                            .map_err(str::to_owned)?;
                        wait_deadlines.retain(|key, _| key.pid != active_pid);
                        resume_completed_waiters(
                            &woken,
                            "process-exit",
                            &mut contexts,
                            &mut wait_deadlines,
                        )?;
                        thread_calls.retain(|(pid, _), _| *pid != active_pid);
                        let before = contexts.len();
                        contexts.retain(|context| context.pid != active_pid);
                        let contexts_removed = before - contexts.len();
                        let terminated = pending_child
                            .take()
                            .ok_or_else(|| "ExitProcess child state missing".to_owned())?;
                        if terminated.pid != active_pid {
                            return Err("ExitProcess child state PID mismatch".into());
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD PROCESS EXIT pid={} exit_code=0x{:08x} contexts_removed={} threads_signaled={} waiters_woken={} address_space_dropped=1 dll_detach_callbacks=0",
                                active_pid,
                                exit_code,
                                contexts_removed,
                                threads_signaled,
                                woken.len(),
                            ),
                        );
                        if contexts.is_empty() {
                            return Ok(());
                        }
                        if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                            active = next;
                            continue;
                        }
                        return Err("no runnable context after child ExitProcess".into());
                    }
                    if operation == child_loader::ProviderOp::CrtXcptFilter {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let xpointers = frame[2];
                        let record =
                            read_guest_words(&X86Memory(&child.address_space), xpointers, 1)?[0];
                        let context = read_guest_words(
                            &X86Memory(&child.address_space),
                            xpointers
                                .checked_add(4)
                                .ok_or("XcptFilter pointer overflow")?,
                            1,
                        )?[0];
                        let mut child_memory = X86Memory(&child.address_space);
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut child_memory,
                            )
                            .map_err(str::to_owned)?;
                        let PersonalityAction::Return(result) = action else {
                            return Err("_XcptFilter child provider did not return".into());
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT XCPTFILTER pid={} tid={} exception=0x{:08x} class={} disposition=default result={} xpointers=0x{:08x} record=0x{:08x} context=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                if frame[1] == wc3::seh::STATUS_ILLEGAL_INSTRUCTION {
                                    "SIGILL"
                                } else {
                                    "SIGSEGV"
                                },
                                result,
                                xpointers,
                                record,
                                context,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::CrtExceptHandler3 {
                        let words = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            5,
                        )?;
                        let return_address = words[0];
                        let record = words[1];
                        let frame = words[2];
                        let context = words[3];
                        let seh = child
                            .seh
                            .as_ref()
                            .ok_or("WC3 CHILD CRT EH3 FRONTIER reason=no-active-seh")?;
                        if return_address != thunk32::CHILD_SEH_RETURN_ADDRESS
                            || frame != seh.registration
                            || record != seh.exception_record_va
                            || context != seh.context_va
                        {
                            return Err(format!(
                                "WC3 CHILD CRT EH3 FRONTIER reason=unexpected-call-shape return=0x{return_address:08x} record=0x{record:08x} frame=0x{frame:08x} context=0x{context:08x}"
                            ));
                        }
                        let frame_words =
                            read_guest_words(&X86Memory(&child.address_space), frame, 5)?;
                        let scope = frame_words[2];
                        let current_level = i32::from_le_bytes(frame_words[3].to_le_bytes());
                        if current_level < -1 {
                            return Err(
                                "WC3 CHILD CRT EH3 FRONTIER reason=invalid-try-level".into()
                            );
                        }
                        if current_level >= 0 && scope == 0 {
                            return Err("WC3 CHILD CRT EH3 FRONTIER reason=null-scope-table".into());
                        }
                        if !child.seh3_diagnostic_logged {
                            child.seh3_diagnostic_logged = true;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CRT EH3 record=0x{:08x} frame=0x{:08x} context=0x{:08x} scope=0x{:08x} trylevel={}",
                                    record, frame, context, scope, current_level
                                ),
                            );
                            let mut level_now = current_level;
                            for _ in 0..65536 {
                                if level_now < 0 {
                                    break;
                                }
                                let (previous, filter, handler) =
                                    child_seh3_scope_entry(child, scope, level_now)?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD CRT EH3 SCOPE level={} prev={} filter=0x{:08x} handler=0x{:08x}",
                                        level_now, previous, filter, handler
                                    ),
                                );
                                if previous >= level_now {
                                    return Err(
                                        "WC3 CHILD CRT EH3 FRONTIER reason=scope-table-loop".into(),
                                    );
                                }
                                level_now = previous;
                            }
                            if level_now >= 0 {
                                return Err(
                                    "WC3 CHILD CRT EH3 FRONTIER reason=scope-table-too-deep".into(),
                                );
                            }
                        }
                        let provider_resume_eip = exit.registers.eip;
                        let provider_esp = exit.registers.esp;
                        let mut level_now = current_level;
                        loop {
                            if level_now < 0 {
                                let mut registers = exit.registers;
                                registers.eax = wc3::seh::DISPOSITION_CONTINUE_SEARCH;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD CRT EH3 SEARCH pid={} tid={} disposition=1",
                                        active_pid, active_tid
                                    ),
                                );
                                continue 'child_run;
                            }
                            let (previous, filter, handler) =
                                child_seh3_scope_entry(child, scope, level_now)?;
                            write_child_u32(child, frame + 0x0c, previous as u32)?;
                            if filter != 0 {
                                let call = ChildSeh3Call {
                                    provider_resume_eip,
                                    provider_esp,
                                    frame,
                                    scope,
                                    kind: ChildSeh3CallbackKind::Filter {
                                        level: level_now,
                                        previous,
                                        start_level: current_level,
                                    },
                                };
                                schedule_child_seh3_filter(
                                    child,
                                    &mut contexts[active],
                                    exit.registers,
                                    call,
                                    filter,
                                )?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD CRT EH3 FILTER pid={} tid={} level={} filter=0x{:08x} handler=0x{:08x}",
                                        active_pid, active_tid, level_now, filter, handler
                                    ),
                                );
                                continue 'child_run;
                            }
                            level_now = previous;
                        }
                    }
                    if operation == child_loader::ProviderOp::RtlUnwind {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            5,
                        )?;
                        let caller_ret = frame[0];
                        let target_frame = frame[1];
                        let target_ip = frame[2];
                        let exception_record = frame[3];
                        let return_value = frame[4];
                        let current_head =
                            read_seh_chain_head(&child.address_space, exit.registers.fs_base)?;
                        let target_relation =
                            classify_unwind_target(child, current_head, target_frame)?;
                        let target_location = (target_ip != 0)
                            .then(|| child_pc_owner(child, target_ip))
                            .flatten();
                        let (target_owner, target_rva) = if target_ip == 0 {
                            ("null", 0)
                        } else {
                            target_location.unwrap_or(("unknown", 0))
                        };
                        let exception_relation = if exception_record == 0 {
                            "null".to_owned()
                        } else if child
                            .seh
                            .as_ref()
                            .is_some_and(|seh| seh.exception_record_va == exception_record)
                        {
                            "active-seh".to_owned()
                        } else {
                            let mut header = [0; 20];
                            if child
                                .address_space
                                .read(exception_record, &mut header)
                                .map_err(|error| error.to_string())?
                                != header.len()
                            {
                                return Err("short RtlUnwind exception record header read".into());
                            }
                            format!(
                                "external(code=0x{:08x},flags=0x{:08x},next=0x{:08x},address=0x{:08x},parameters={})",
                                u32::from_le_bytes(header[0..4].try_into().unwrap()),
                                u32::from_le_bytes(header[4..8].try_into().unwrap()),
                                u32::from_le_bytes(header[8..12].try_into().unwrap()),
                                u32::from_le_bytes(header[12..16].try_into().unwrap()),
                                u32::from_le_bytes(header[16..20].try_into().unwrap()),
                            )
                        };
                        let (
                            active_registration,
                            active_next,
                            active_handler,
                            active_record,
                            active_context,
                            active_depth,
                        ) = child
                            .seh
                            .as_ref()
                            .map(|seh| {
                                (
                                    seh.registration,
                                    seh.next_registration,
                                    seh.handler,
                                    seh.exception_record_va,
                                    seh.context_va,
                                    seh.depth,
                                )
                            })
                            .unwrap_or((0, 0, 0, 0, 0, 0));
                        let active_target = child
                            .seh
                            .as_ref()
                            .is_some_and(|seh| seh.registration == target_frame);
                        let supported_current_head = target_frame != 0
                            && target_frame == current_head
                            && active_target
                            && target_ip != 0
                            && target_ip == caller_ret
                            && exception_record == 0
                            && target_location.is_some();
                        if supported_current_head {
                            let resumed = rtl_unwind_current_target_registers(
                                exit.registers,
                                exit.registers.esp,
                                target_ip,
                                return_value,
                            )
                            .map_err(str::to_owned)?;
                            contexts[active]
                                .context
                                .set_registers(resumed)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD RTLUNWIND CONTINUE pid={} tid={} target_frame=0x{:08x} target_ip=0x{:08x} target_owner={:?} target_rva=0x{:08x} return_value=0x{:08x} old_esp=0x{:08x} new_esp=0x{:08x} fs_head=0x{:08x} handlers_called=0 frames_popped=0",
                                    active_pid,
                                    active_tid,
                                    target_frame,
                                    target_ip,
                                    target_owner,
                                    target_rva,
                                    return_value,
                                    exit.registers.esp,
                                    resumed.esp,
                                    current_head,
                                ),
                            );
                            continue;
                        }
                        let reason = if target_frame == 0 {
                            "exit-unwind"
                        } else if matches!(
                            target_relation,
                            UnwindTargetRelation::LaterRegistration(_)
                        ) {
                            "multi-frame-unwind"
                        } else if matches!(target_relation, UnwindTargetRelation::NotInChain) {
                            "invalid-target-frame"
                        } else if !active_target {
                            "target-not-active-seh-registration"
                        } else if exception_record != 0 {
                            "supplied-exception-record"
                        } else if target_ip != caller_ret {
                            "nonlocal-target-ip"
                        } else if target_location.is_none() {
                            "unknown-target-ip"
                        } else {
                            "unsupported-current-head-shape"
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD RTLUNWIND FRONTIER pid={} tid={} reason={} caller_ret=0x{:08x} target_frame=0x{:08x} target_relation={} target_relation_detail={:?} target_ip=0x{:08x} target_ip_owner={:?} target_ip_rva=0x{:08x} exception_record=0x{:08x} exception_relation={} return_value=0x{:08x} fs_head=0x{:08x} active_registration=0x{:08x} active_next=0x{:08x} active_handler=0x{:08x} active_record=0x{:08x} active_context=0x{:08x} active_depth={}",
                                active_pid,
                                active_tid,
                                reason,
                                caller_ret,
                                target_frame,
                                target_relation.name(),
                                target_relation,
                                target_ip,
                                target_owner,
                                target_rva,
                                exception_record,
                                exception_relation,
                                return_value,
                                current_head,
                                active_registration,
                                active_next,
                                active_handler,
                                active_record,
                                active_context,
                                active_depth,
                            ),
                        );
                        return Ok(());
                    }
                    if operation == child_loader::ProviderOp::UnhandledExceptionFilter {
                        let exception_pointers = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?[1];
                        if exception_pointers == 0 {
                            return Err(
                                "WC3 CHILD UEF FRONTIER reason=null-exception-pointers".into()
                            );
                        }
                        let mut pointers = [0; 8];
                        if child
                            .address_space
                            .read(exception_pointers, &mut pointers)
                            .map_err(|error| error.to_string())?
                            != 8
                        {
                            return Err(
                                "WC3 CHILD UEF FRONTIER reason=unreadable-exception-pointers"
                                    .into(),
                            );
                        }
                        let filter = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .unhandled_exception_filter();
                        if filter == 0 {
                            let result = session
                                .process(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .complete_unhandled_exception_filter(None)
                                .map_err(str::to_owned)?;
                            let mut registers = exit.registers;
                            registers.eax = result;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD UEF RETURN pid={} tid={} source=default filter=0x00000000 result=0x{:08x} cleanup=4-by-thunk",
                                    active_pid, active_tid, result
                                ),
                            );
                            continue;
                        }
                        let Some((owner, rva)) = child_pc_owner(child, filter)
                            .map(|(owner, rva)| (owner.to_owned(), rva))
                        else {
                            return Err(format!(
                                "WC3 CHILD UEF FRONTIER reason=unknown-filter-address filter=0x{filter:08x}"
                            ));
                        };
                        let callback_esp = exit
                            .registers
                            .esp
                            .checked_sub(8)
                            .ok_or("UEF callback stack underflow")?;
                        let frame = [thunk32::CHILD_UEF_RETURN_ADDRESS, exception_pointers];
                        let mut bytes = [0; 8];
                        for (i, value) in frame.into_iter().enumerate() {
                            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                        }
                        if child
                            .address_space
                            .write(callback_esp, &bytes)
                            .map_err(|error| error.to_string())?
                            != 8
                        {
                            return Err("short UEF callback frame write".into());
                        }
                        child.unhandled_filter_call = Some(ChildUnhandledFilterCall {
                            return_esp: exit.registers.esp,
                            filter,
                            continuation: ChildUnhandledFilterContinuation::Provider {
                                resume_eip: exit.registers.eip,
                            },
                        });
                        let mut registers = exit.registers;
                        registers.eip = filter;
                        registers.esp = callback_esp;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD UEF FILTER CALL pid={} tid={} filter=0x{:08x} filter_owner={:?} filter_rva=0x{:08x} exception_pointers=0x{:08x} provider_esp=0x{:08x} callback_esp=0x{:08x}",
                                active_pid,
                                active_tid,
                                filter,
                                owner,
                                rva,
                                exception_pointers,
                                exit.registers.esp,
                                callback_esp
                            ),
                        );
                        continue;
                    }
                    if operation == child_loader::ProviderOp::LoadLibraryA {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?;
                        let name_ptr = frame[1];
                        if name_ptr == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY FRONTIER pid={} tid={} during=\"{}\" kind=null-name caller_ret=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                            return Ok(());
                        }
                        let requested = wc3::process::read_c_string(
                            &X86Memory(&child.address_space),
                            name_ptr,
                            260,
                        )
                        .map_err(|error| format!("LoadLibraryA module name: {error}"))?;
                        let existing = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .loaded_module_handle(&requested);
                        if let Some(handle) = existing {
                            let references = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .retain_loaded_module(handle)
                                .map_err(str::to_owned)?;
                            let mut registers = exit.registers;
                            registers.eax = handle;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY RETURN pid={} tid={} during=\"{}\" requested={:?} handle=0x{:08x} already_loaded=1 references={} cleanup=4-by-thunk",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    requested,
                                    handle,
                                    references,
                                ),
                            );
                            continue;
                        }
                        if wc3::process::is_system_provider_module(&requested) {
                            let (handle, references, already_loaded) = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .load_runtime_external_provider(&requested)
                                .map_err(str::to_owned)?;
                            let mut registers = exit.registers;
                            registers.eax = handle;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY PROVIDER pid={} tid={} during=\"{}\" requested={:?} handle=0x{:08x} already_loaded={} references={} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    requested,
                                    handle,
                                    already_loaded as u8,
                                    references,
                                    u32::from_le_bytes(caller_ret),
                                ),
                            );
                            continue;
                        }
                        let scratch = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .scratch_file_snapshot(&requested);
                        let local_image = if let Some(bytes) = scratch {
                            let stored = requested
                                .rsplit(['\\', '/'])
                                .next()
                                .unwrap_or(&requested)
                                .to_owned();
                            Some(("scratch", stored, bytes, None))
                        } else {
                            let listing = async_fs::list_dir(b"/common/Warcraft III")
                                .await
                                .map_err(|error| {
                                    format!(
                                        "list Warcraft III directory for LoadLibraryA: TRUEOSFS {error}"
                                    )
                                })?;
                            if listing.truncated {
                                return Err("Warcraft III directory listing truncated".into());
                            }
                            let stored = child_loader::resolve_file(&listing, &requested)
                                .map_err(str::to_owned)?;
                            if let Some(stored) = stored {
                                let path = format!("/common/Warcraft III/{stored}");
                                let bytes = async_fs::read_file(path.as_bytes()).await.map_err(
                                    |error| {
                                        format!(
                                            "read {path} for LoadLibraryA: TRUEOSFS error {error}"
                                        )
                                    },
                                )?;
                                Some(("trueosfs", stored, bytes, Some(listing)))
                            } else {
                                None
                            }
                        };
                        if let Some((source, stored, bytes, listing)) = local_image {
                            let local_sha256 = Sha256::digest(&bytes);
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY LOCAL IMAGE source={source} requested={requested:?} stored={stored:?} bytes={} sha256={}",
                                    bytes.len(),
                                    hex_digest(&local_sha256),
                                ),
                            );
                            if child.execution != ChildExecutionState::ImageEntryRunning {
                                return Err(format!(
                                    "WC3 CHILD LOADLIBRARY FRONTIER reason=outside-image-entry state={:?}",
                                    child.execution,
                                ));
                            }
                            let image = pe32::parse(&bytes).map_err(|error| {
                                format!("LoadLibraryA {source} PE {requested:?}: {error}")
                            })?;
                            let named_exports = image
                                .exports
                                .iter()
                                .filter(|export| export.name.is_some())
                                .count();
                            let forwarders = image
                                .exports
                                .iter()
                                .filter(|export| {
                                    matches!(export.target, pe32::ExportTarget::Forwarder(_))
                                })
                                .count();
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY LOCAL PE source={} pid={} tid={} requested={:?} stored={:?} bytes={} image_base=0x{:08x} entry_rva=0x{:08x} size_of_image=0x{:08x} sections={} imports={} exports={} named_exports={} forwarders={} relocations={}",
                                    source,
                                    active_pid,
                                    active_tid,
                                    requested,
                                    stored,
                                    bytes.len(),
                                    image.image_base,
                                    image.entry_rva,
                                    image.size_of_image,
                                    image.sections.len(),
                                    image.imports.len(),
                                    image.exports.len(),
                                    named_exports,
                                    forwarders,
                                    image.relocations.len(),
                                ),
                            );
                            for (index, import) in image.imports.iter().enumerate() {
                                logl::trace!("trace-init",
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD LOADLIBRARY LOCAL IMPORT source={} index={} module={:?} symbol={:?} iat_rva=0x{:08x}",
                                        source, index, import.module, import.symbol, import.iat_rva,
                                    ),
                                );
                            }
                            let module_handle = image.image_base;
                            child
                                .address_space
                                .map(
                                    module_handle,
                                    image.image.len(),
                                    Permissions::READ | Permissions::WRITE | Permissions::EXECUTE,
                                )
                                .map_err(|_| {
                                    format!(
                                        "WC3 CHILD LOADLIBRARY FRONTIER reason=preferred-base-unavailable module={requested:?} preferred=0x{module_handle:08x} relocations={}",
                                        image.relocations.len(),
                                    )
                                })?;
                            let written = child
                                .address_space
                                .write(module_handle, &image.image)
                                .map_err(|error| error.to_string())?;
                            if written != image.image.len() {
                                return Err("short local native image write".into());
                            }
                            if stored.eq_ignore_ascii_case("Storm.dll") {
                                install_storm_record_expand_trap(&child.address_space)?;
                            }
                            // Bind the requested image depth-first: a locally present
                            // imported DLL must be mapped and registered before its
                            // parent can receive real export addresses in its IAT.
                            let mut pending_images = vec![(requested.clone(), stored, image)];
                            let mut attach_indices = Vec::new();
                            while let Some((parent, stored, image)) = pending_images.pop() {
                                match bind_runtime_local_image_imports(
                                    child,
                                    &mut session
                                        .process_mut(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp,
                                    &parent,
                                    image.image_base,
                                    &image,
                                    listing.as_ref(),
                                )? {
                                    RuntimeImportBind::Complete => {
                                        session
                                            .process_mut(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .register_runtime_native_module(
                                                &parent,
                                                image.image_base,
                                            )
                                            .map_err(str::to_owned)?;
                                        let native_index = child.native_modules.len();
                                        child.native_modules.push(PendingNativeModule {
                                            requested: parent,
                                            stored,
                                            image,
                                            initialized: false,
                                        });
                                        attach_indices.push(native_index);
                                    }
                                    RuntimeImportBind::NeedNativeDependency {
                                        requested: dependency_requested,
                                        stored: dependency_stored,
                                    } => {
                                        let path =
                                            format!("/common/Warcraft III/{dependency_stored}");
                                        let bytes = async_fs::read_file(path.as_bytes())
                                            .await
                                            .map_err(|error| format!(
                                                "read {path} for runtime dependency: TRUEOSFS error {error}"
                                            ))?;
                                        let dependency_image = pe32::parse(&bytes).map_err(|error| {
                                            format!(
                                                "runtime dependency PE {dependency_requested:?}: {error}"
                                            )
                                        })?;
                                        child
                                            .address_space
                                            .map(
                                                dependency_image.image_base,
                                                dependency_image.image.len(),
                                                Permissions::READ
                                                    | Permissions::WRITE
                                                    | Permissions::EXECUTE,
                                            )
                                            .map_err(|_| format!(
                                                "WC3 CHILD RUNTIME NATIVE DEPENDENCY FRONTIER reason=preferred-base-unavailable module={dependency_requested:?} preferred=0x{:08x} relocations={}",
                                                dependency_image.image_base,
                                                dependency_image.relocations.len(),
                                            ))?;
                                        if child
                                            .address_space
                                            .write(
                                                dependency_image.image_base,
                                                &dependency_image.image,
                                            )
                                            .map_err(|error| error.to_string())?
                                            != dependency_image.image.len()
                                        {
                                            return Err(
                                                "short runtime native dependency image write"
                                                    .into(),
                                            );
                                        }
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD RUNTIME NATIVE DEPENDENCY MAP parent={parent:?} requested={dependency_requested:?} stored={dependency_stored:?} base=0x{:08x} imports={} entry_rva=0x{:08x}",
                                                dependency_image.image_base,
                                                dependency_image.imports.len(),
                                                dependency_image.entry_rva,
                                            ),
                                        );
                                        pending_images.push((parent, stored, image));
                                        pending_images.push((
                                            dependency_requested,
                                            dependency_stored,
                                            dependency_image,
                                        ));
                                    }
                                }
                            }
                            let native_index = *attach_indices
                                .first()
                                .ok_or("runtime native attach list empty")?;
                            let module_handle = child.native_modules[native_index].image.image_base;
                            let load_library_handle = child
                                .native_modules
                                .get(
                                    *attach_indices
                                        .last()
                                        .ok_or("runtime native attach tail missing")?,
                                )
                                .ok_or("runtime LoadLibrary requested module missing")?
                                .image
                                .image_base;
                            let entry = module_handle
                                .checked_add(child.native_modules[native_index].image.entry_rva)
                                .ok_or("local DLL entry overflow")?;
                            let provider_esp = exit.registers.esp;
                            let callback_esp = provider_esp
                                .checked_sub(16)
                                .ok_or("LoadLibrary DllMain stack underflow")?;
                            let frame =
                                [thunk32::CHILD_CALLBACK_RETURN_ADDRESS, module_handle, 1, 0];
                            let mut frame_bytes = [0u8; 16];
                            for (index, value) in frame.into_iter().enumerate() {
                                frame_bytes[index * 4..index * 4 + 4]
                                    .copy_from_slice(&value.to_le_bytes());
                            }
                            let written = child
                                .address_space
                                .write(callback_esp, &frame_bytes)
                                .map_err(|error| error.to_string())?;
                            if written != frame_bytes.len() {
                                return Err("short LoadLibrary DllMain frame write".into());
                            }
                            child.load_library_call = Some(ChildLoadLibraryCall {
                                provider_resume_eip: exit.registers.eip,
                                provider_esp,
                                native_index,
                                module_handle,
                                load_library_handle,
                                remaining_native_indices: attach_indices[1..].to_vec(),
                            });
                            let mut registers = exit.registers;
                            registers.eip = entry;
                            registers.esp = callback_esp;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD LOADLIBRARY MAP pid={} tid={} during=\"{}\" module={:?} mapped_base=0x{:08x} relocation_delta=0 imports={} entry=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    child.native_modules[native_index].requested,
                                    module_handle,
                                    child.native_modules[native_index].image.imports.len(),
                                    entry,
                                ),
                            );
                            continue;
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LOADLIBRARY FRONTIER pid={} tid={} during=\"{}\" requested={:?} kind=external stored=None caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                requested,
                                u32::from_le_bytes(caller_ret),
                            ),
                        );
                        return Ok(());
                    }
                    if operation == child_loader::ProviderOp::FreeLibrary {
                        let handle = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?[1];
                        let release = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .release_loaded_module(handle)
                            .map_err(str::to_owned)?;
                        match release {
                            wc3::process::ModuleRelease::Retained { remaining } => {
                                let mut registers = exit.registers;
                                registers.eax = 1;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD FREELIBRARY pid={} tid={} during={:?} handle=0x{:08x} remaining_references={} unloaded=0 result=1 cleanup=4-by-thunk",
                                        active_pid, active_tid, running_module_name, handle, remaining,
                                    ),
                                );
                                continue;
                            }
                            wc3::process::ModuleRelease::ExternalProviderUnloaded { module } => {
                                let mut registers = exit.registers;
                                registers.eax = 1;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD FREELIBRARY pid={} tid={} during={:?} module={:?} handle=0x{:08x} remaining_references=0 unloaded=1 kind=external-provider result=1 cleanup=4-by-thunk",
                                        active_pid, active_tid, running_module_name, module, handle,
                                    ),
                                );
                                continue;
                            }
                            wc3::process::ModuleRelease::NativeUnloadRequired { module } => {
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD FREELIBRARY FRONTIER reason=native-zero-reference-unload module={:?} handle=0x{:08x}",
                                        module, handle,
                                    ),
                                );
                                return Ok(());
                            }
                        }
                    }
                    if operation == child_loader::ProviderOp::GetProcAddress {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            3,
                        )?;
                        let hmodule = frame[1];
                        let selector = if frame[2] >> 16 == 0 {
                            ProcSelector::Ordinal(frame[2] as u16)
                        } else {
                            ProcSelector::Name(
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    frame[2],
                                    260,
                                )
                                .map_err(|error| format!("GetProcAddress selector: {error}"))?,
                            )
                        };
                        let provider_symbol = selector.provider_symbol();
                        let provider_module = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .external_provider_module_name(hmodule)
                            .map(str::to_owned);
                        if let Some(provider_module) = provider_module {
                            let import = child_loader::ProviderImport {
                                module: provider_module.clone(),
                                symbol: provider_symbol.clone(),
                                iat_rva: 0,
                            };
                            let data_export = child_loader::provider_data_export_address(&import);
                            let export_kind = child_loader::external_export_thunk_kind(&import);
                            if export_kind.is_none() && data_export.is_none() {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "child process missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_tid, ERROR_PROC_NOT_FOUND);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD GETPROCADDRESS MISS pid={} tid={} module={:?} selector={:?} reason=export-absent error={}",
                                        active_pid,
                                        active_tid,
                                        provider_module,
                                        selector,
                                        ERROR_PROC_NOT_FOUND,
                                    ),
                                );
                                log_get_proc_address_continuation(
                                    child,
                                    active_pid,
                                    active_tid,
                                    &provider_module,
                                    frame[0],
                                    &selector,
                                    0,
                                );
                                let mut registers = exit.registers;
                                registers.eax = 0;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                continue;
                            }
                            let existing = session
                                .process(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .provider_export_address(&provider_module, &provider_symbol);
                            let (address, source) = if let Some(address) = data_export {
                                (address, "provider-data")
                            } else if let Some(address) = existing {
                                (address, "provider-existing")
                            } else {
                                let addresses = {
                                    let process = &mut session
                                        .process_mut(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp;
                                    install_child_provider_imports(child, process, vec![import])?
                                };
                                (addresses[0], "provider-new")
                            };
                            let mut registers = exit.registers;
                            registers.eax = address;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS RETURN pid={} tid={} module={:?} selector={:?} address=0x{:08x} source={} cleanup=8-by-thunk",
                                    active_pid,
                                    active_tid,
                                    provider_module,
                                    selector,
                                    address,
                                    source,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                &provider_module,
                                frame[0],
                                &selector,
                                address,
                            );
                            continue;
                        }

                        let image_and_module = if hmodule == child.image.image_base {
                            Some((&child.image, "War3.exe".to_owned()))
                        } else {
                            child.native_modules.iter().find_map(|native| {
                                (native.image.image_base == hmodule)
                                    .then(|| (&native.image, native.stored.clone()))
                            })
                        };
                        let Some((image, module)) = image_and_module else {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS FRONTIER pid={} tid={} kind=unknown-hmodule handle=0x{:08x} selector={:?}",
                                    active_pid, active_tid, hmodule, selector,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child,
                                active_pid,
                                active_tid,
                                "<unknown-hmodule>",
                                frame[0],
                                &selector,
                                0,
                            );
                            return Ok(());
                        };
                        let export = image.exports.iter().find(|export| match &selector {
                            ProcSelector::Name(name) => export.name.as_deref() == Some(name),
                            ProcSelector::Ordinal(ordinal) => export.ordinal == u32::from(*ordinal),
                        });
                        let Some(export) = export else {
                            session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .set_last_error_for_thread(active_tid, ERROR_PROC_NOT_FOUND);
                            let mut registers = exit.registers;
                            registers.eax = 0;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS MISS pid={} tid={} module={:?} selector={:?} reason=export-not-found error={}",
                                    active_pid, active_tid, module, selector, ERROR_PROC_NOT_FOUND,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child, active_pid, active_tid, &module, frame[0], &selector, 0,
                            );
                            continue;
                        };
                        let pe32::ExportTarget::Rva(rva) = &export.target else {
                            let pe32::ExportTarget::Forwarder(forwarder) = &export.target else {
                                unreachable!()
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD GETPROCADDRESS FRONTIER pid={} tid={} kind=native-forwarder module={:?} selector={:?} forwarder={:?}",
                                    active_pid, active_tid, module, selector, forwarder,
                                ),
                            );
                            log_get_proc_address_continuation(
                                child, active_pid, active_tid, &module, frame[0], &selector, 0,
                            );
                            return Ok(());
                        };
                        if *rva >= image.size_of_image {
                            return Err("GetProcAddress export RVA outside image".into());
                        }
                        let address = image
                            .image_base
                            .checked_add(*rva)
                            .ok_or_else(|| "GetProcAddress export VA overflow".to_owned())?;
                        let mut registers = exit.registers;
                        registers.eax = address;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETPROCADDRESS RETURN pid={} tid={} module={:?} selector={:?} address=0x{:08x} source=native-export cleanup=8-by-thunk",
                                active_pid, active_tid, module, selector, address,
                            ),
                        );
                        log_get_proc_address_continuation(
                            child, active_pid, active_tid, &module, frame[0], &selector, address,
                        );
                        continue;
                    }
                    if operation == child_loader::ProviderOp::Direct3DCreate8 {
                        let [caller_ret, sdk_version] = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            2,
                        )?[..]
                        else {
                            unreachable!("Direct3DCreate8 frame has two words")
                        };
                        let result = if sdk_version == D3D_SDK_VERSION {
                            let process = &mut session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp;
                            install_child_d3d8_object(child, process)?;
                            process.retain_d3d8_object().map_err(str::to_owned)?;
                            thunk32::CHILD_D3D8_OBJECT_ADDRESS
                        } else {
                            0
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD D3D8 CREATE pid={} tid={} sdk_version={} object=0x{:08x} vtable=0x{:08x} methods={} result=0x{:08x} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                sdk_version,
                                thunk32::CHILD_D3D8_OBJECT_ADDRESS,
                                thunk32::CHILD_D3D8_VTABLE_ADDRESS,
                                D3D8_METHODS.len(),
                                result,
                                caller_ret,
                            ),
                        );
                        continue;
                    }
                    if operation == child_loader::ProviderOp::CreateWindowExA {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::CreateWindow(request)) = action
                        else {
                            return Err("CreateWindowExA produced unexpected action".into());
                        };
                        let hwnd = session.create_window(request).map_err(str::to_owned)?;
                        if let Some(presentation) = session.take_window_presentation() {
                            present_window(presentation, &mut frames, window_rgba, &session)?;
                        }
                        let window = session
                            .windows
                            .get(&hwnd)
                            .ok_or("created child window missing")?;
                        let ui4_frame = frames.contains_key(&hwnd);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CREATEWINDOWEXA RESULT pid={} tid={} hwnd=0x{:08x} class={:?} title={:?} wndproc=0x{:08x} icon=0x{:08x} cursor=0x{:08x} parent=0x{:08x} geometry={},{} {}x{} win32_visible={} ui4_frame={} ui4_policy=always-visible result=success cleanup=48-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                window.class,
                                window.title,
                                window.wndproc,
                                window.class_icon,
                                window.class_cursor,
                                window.parent,
                                window.x,
                                window.y,
                                window.width,
                                window.height,
                                window.visible as u8,
                                ui4_frame as u8,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = hwnd;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::SetWindowPos {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::SetWindowPos(request)) = action
                        else {
                            return Err("SetWindowPos produced unexpected action".into());
                        };
                        let hwnd = request.hwnd;
                        let insert_after = request.insert_after;
                        let result = session.set_window_pos(request).map_err(str::to_owned)?;
                        let position_changed = result.old_x != result.x || result.old_y != result.y;
                        let size_changed = result.old_width != result.width
                            || result.old_height != result.height;
                        if position_changed || size_changed {
                            let frame = frames
                                .get_mut(&hwnd)
                                .ok_or("SetWindowPos UI4 frame missing")?;
                            if position_changed {
                                frame
                                    .set_position(result.x, result.y)
                                    .map_err(|error| format!("move WC3 UI4 window: {error:?}"))?;
                            }
                            if size_changed {
                                frame
                                    .resize(result.width, result.height)
                                    .map_err(|error| format!("resize WC3 UI4 window: {error:?}"))?;
                            }
                        }
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETWINDOWPOS RESULT pid={} tid={} hwnd=0x{:08x} old={},{} {}x{} requested={} applied={},{} {}x{} move_changed={} size_changed={} insert_after={} ui4_zorder_action=none ui4_position_changed={} ui4_size_changed={} ui4_visible={} win32_visible={} result=TRUE cleanup=28-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                result.old_x,
                                result.old_y,
                                result.old_width,
                                result.old_height,
                                format_args!("{},{} {}x{}", result.x, result.y, result.width, result.height),
                                result.x,
                                result.y,
                                result.width,
                                result.height,
                                position_changed as u8,
                                size_changed as u8,
                                if insert_after == 0 { "HWND_TOP" } else { "unobserved" },
                                position_changed as u8,
                                size_changed as u8,
                                frames.contains_key(&hwnd) as u8,
                                result.win32_visible as u8,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = 1;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::ShowWindow {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::ShowWindow {
                            pid: _,
                            hwnd,
                            show,
                        }) = action
                        else {
                            return Err("ShowWindow produced unexpected action".into());
                        };
                        let result = session.show_window(hwnd, show).map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SHOWWINDOW pid={} tid={} hwnd=0x{:08x} requested_visible={} previous_visible={} ui4_visible={} presentation_action=ignored result={} cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                (show != 0) as u8,
                                result,
                                frames.contains_key(&hwnd) as u8,
                                result,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::SetFocus {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::SetFocus { pid: _, hwnd }) = action
                        else {
                            return Err("SetFocus produced unexpected action".into());
                        };
                        let previous = session.set_focus(hwnd).map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETFOCUS pid={} tid={} hwnd=0x{:08x} previous=0x{:08x} compositor_focus=unchanged result=0x{:08x} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                previous,
                                previous,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = previous;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::GetWindowRect {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::GetWindowRect {
                            pid: _,
                            hwnd,
                            output,
                        }) = action
                        else {
                            return Err("GetWindowRect produced unexpected action".into());
                        };
                        if output == 0 {
                            return Err("GetWindowRect null RECT frontier".into());
                        }
                        let rect = session.window_rect(hwnd).map_err(str::to_owned)?;
                        let mut bytes = [0u8; 16];
                        bytes[0..4].copy_from_slice(&rect[0].to_le_bytes());
                        bytes[4..8].copy_from_slice(&rect[1].to_le_bytes());
                        bytes[8..12].copy_from_slice(&rect[2].to_le_bytes());
                        bytes[12..16].copy_from_slice(&rect[3].to_le_bytes());
                        X86Memory(&child.address_space)
                            .write(output, &bytes)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETWINDOWRECT RESULT pid={} tid={} hwnd=0x{:08x} output=0x{:08x} rect=[{},{},{},{}] result=1 cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                output,
                                rect[0],
                                rect[1],
                                rect[2],
                                rect[3],
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = 1;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::SetWindowTextA {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::SetWindowText {
                            pid,
                            hwnd,
                            text,
                        }) = action
                        else {
                            return Err("SetWindowTextA produced unexpected action".into());
                        };
                        let new_title = text.clone();
                        let (previous, changed) = session
                            .set_window_text(pid, hwnd, text)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETWINDOWTEXTA RESULT pid={} tid={} hwnd=0x{:08x} previous={:?} new={:?} changed={} ui4_frame={} ui4_title_action=none result=TRUE cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                previous,
                                new_title,
                                changed as u8,
                                frames.contains_key(&hwnd) as u8,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = 1;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::GetDC {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::GetDC { pid, hwnd }) = action
                        else {
                            return Err("GetDC produced unexpected action".into());
                        };
                        session.validate_window_dc(pid, hwnd).map_err(str::to_owned)?;
                        let (hdc, reused) = session
                            .process_mut(pid)
                            .ok_or_else(|| "GetDC process missing".to_owned())?
                            .xp
                            .get_window_dc(hwnd)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETDC RESULT pid={} tid={} hwnd=0x{:08x} hdc=0x{:08x} persistent=1 reused={} target=window result=success cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                hdc,
                                reused as u8,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = hdc;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if matches!(
                        operation,
                        child_loader::ProviderOp::CreateEventA
                            | child_loader::ProviderOp::SetEvent
                            | child_loader::ProviderOp::ResetEvent
                            | child_loader::ProviderOp::CreateMutexA
                            | child_loader::ProviderOp::ReleaseMutex
                            | child_loader::ProviderOp::CloseHandle
                            | child_loader::ProviderOp::WaitForSingleObject
                            | child_loader::ProviderOp::WaitForMultipleObjects
                    ) {
                        let is_multiple_wait = operation
                            == child_loader::ProviderOp::WaitForMultipleObjects;
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let result = match action {
                            PersonalityAction::Return(result) => result,
                            PersonalityAction::Session(request) => service_sync_request(
                                &mut session,
                                active_pid,
                                active_tid,
                                request,
                                &mut contexts,
                                &mut wait_deadlines,
                            )?,
                            PersonalityAction::Block(request) => {
                                if is_multiple_wait {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD WAIT MULTIPLE CALL pid={} tid={} count={} handles_ptr=0x{:08x} handle0=0x{:08x} handle1=0x{:08x} wait_all={} timeout=0x{:08x} caller_ret=0x{:08x}",
                                            active_pid, active_tid, request.count,
                                            request.handles_pointer, request.handles[0],
                                            request.handles[1], request.wait_all, request.timeout,
                                            request.return_address,
                                        ),
                                    );
                                }
                                if let Some(result) =
                                    session.poll_wait(&request).map_err(str::to_owned)?
                                {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD WAIT {}RETURN pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} result=0x{:08x}",
                                            if is_multiple_wait { "MULTIPLE " } else { "" },
                                            active_pid, active_tid, request.handles[0], request.handles[1], result
                                        ),
                                    );
                                    result
                                } else {
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
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD WAIT {}BLOCK pid={} tid={} count={} handle0=0x{:08x} handle1=0x{:08x} wait_all={} timeout_ms={}",
                                            if is_multiple_wait { "MULTIPLE " } else { "" },
                                            active_pid,
                                            active_tid,
                                            request.count,
                                            request.handles[0],
                                            request.handles[1],
                                            request.wait_all,
                                            request.timeout
                                        ),
                                    );
                                    loop {
                                        if let Some(next) =
                                            pop_runnable_context(&mut session, &contexts)
                                        {
                                            active = next;
                                            break;
                                        }
                                        if let Some(deadline) =
                                            wait_deadlines.values().map(|wait| wait.deadline).min()
                                        {
                                            tokio::time::sleep_until(deadline).await;
                                            expire_runtime_waits(
                                                &mut session,
                                                &mut contexts,
                                                &mut wait_deadlines,
                                                &mut previous_wait_timeout,
                                            )?;
                                        } else {
                                            tokio::time::sleep(Duration::from_millis(8)).await;
                                        }
                                    }
                                    continue;
                                }
                            }
                            _ => return Err("unexpected child synchronization action".into()),
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SYNC RETURN pid={} tid={} {} eax=0x{:08x} cleanup={}-by-thunk",
                                active_pid,
                                active_tid,
                                symbol,
                                result,
                                operation.stack_cleanup_bytes()
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
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
                        let crt_atol = if operation == child_loader::ProviderOp::CrtAtol {
                            let string = read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                2,
                            )?[1];
                            Some(
                                wc3::process::crt_atol(
                                    &X86Memory(&child.address_space),
                                    string,
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
                        let gl_needs_frame = operation == child_loader::ProviderOp::GlDrawElements
                            || (operation == child_loader::ProviderOp::GlClear
                                && read_guest_words(&X86Memory(&child.address_space), exit.registers.esp, 2)?[1] & 0x0000_4000 != 0);
                        let gl_draw_frame = if gl_needs_frame {
                            let (_hglrc, hwnd, _mode) = session.process(active_pid)
                                .ok_or_else(|| "GL process missing".to_owned())?
                                .xp.gl_context_diagnostic(active_tid)
                                .ok_or_else(|| "GL call has no current context".to_owned())?;
                            let frame = frames.get_mut(&hwnd)
                                .ok_or_else(|| format!("GL call hwnd=0x{hwnd:08x} has no UI4 frame"))?;
                            frame.begin_gpu_frame().map_err(|error| format!("GL begin UI4 frame: {error:?}"))?;
                            let window_id = frame.window_id();
                            session.process_mut(active_pid)
                                .ok_or_else(|| "GL process missing".to_owned())?
                                .xp.bind_gl_ui4_window(hwnd, window_id);
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
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT TOUPPER pid={} tid={} character=0x{:08x} result=0x{:08x} cleanup=0-by-thunk",
                                            active_pid, active_tid, character, result,
                                        ),
                                    );
                                }
                                if let Some((_, overflow, input)) = crt_atol {
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD CRT ATOL pid={} tid={} input={:?} result={} eax=0x{:08x} overflow={} cleanup=0-by-thunk",
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
                                if crt_rand {
                                    let state = session
                                        .process(active_pid)
                                        .ok_or_else(|| "child process missing".to_owned())?
                                        .xp
                                        .crt_rng_seed();
                                    logl::log(
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
                                    child_loader::ProviderOp::ClipCursor => {
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
                                            _flags,
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
                                        let (resolved_language_id, language_kind) =
                                            wc3::process::format_message_language_resolution(
                                                *language_id,
                                            );
                                        let last_error = session
                                            .process(active_pid)
                                            .ok_or_else(|| "child process missing".to_owned())?
                                            .xp
                                            .last_error();
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD FORMATMESSAGEA LANGUAGE pid={} tid={} requested=0x{:04x} resolved=0x{:04x} kind={}",
                                                active_pid,
                                                active_tid,
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
                                                "WC3 CHILD FORMATMESSAGEA RESULT pid={} tid={} source=0x{:08x} module=\"Storm.dll\" message_id=0x{:08x} language_id=0x{:04x} resource_type=RT_MESSAGETABLE encoding={} chars={} text={:?} eax=0x{:08x} last_error={} cleanup=28-by-thunk",
                                                active_pid,
                                                active_tid,
                                                source,
                                                message_id,
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
                                        logl::log(
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
                                    child_loader::ProviderOp::TlsSetValue => {
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
                                    child_loader::ProviderOp::TlsGetValue => {
                                        let [_, slot] = read_guest_words(
                                            &X86Memory(&child.address_space),
                                            exit.registers.esp,
                                            2,
                                        )?[..] else {
                                            unreachable!("TlsGetValue frame has two words")
                                        };
                                        logl::log(
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
                                        symbol,
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
                                        symbol,
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
                            symbol,
                            exit.registers.eip,
                            exit.registers.esp,
                            u32::from_le_bytes(caller_ret)
                        ),
                    );
                    return Ok(());
                }
