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
                        let caller_ret = read_guest_words(
                            &X86Memory(&child.address_space),
                            pending.provider_esp,
                            1,
                        )?[0];
                        let sequence = GuestThreadContext::last_execution_diagnostic().sequence;
                        if !base.is_finite() || !exponent.is_finite() {
                            return Err(format!(
                                "WC3 CHILD CRT CIPOW FRONTIER seq={sequence} pid={active_pid} tid={active_tid} \
                                 caller_ret=0x{caller_ret:08x} base={base:?} base_bits=0x{:016x} \
                                 exponent={exponent:?} exponent_bits=0x{:016x} \
                                 reason=nonfinite-operands-unmodeled",
                                base.to_bits(),
                                exponent.to_bits(),
                            ));
                        }
                        let result = base.powf(exponent);
                        if !result.is_finite() {
                            return Err(format!(
                                "WC3 CHILD CRT CIPOW FRONTIER seq={sequence} pid={active_pid} tid={active_tid} \
                                 caller_ret=0x{caller_ret:08x} base={base:?} base_bits=0x{:016x} \
                                 exponent={exponent:?} exponent_bits=0x{:016x} \
                                 result={result:?} result_bits=0x{:016x} \
                                 reason=exceptional-math-result-unmodeled",
                                base.to_bits(),
                                exponent.to_bits(),
                                result.to_bits(),
                            ));
                        }
                        let written = child
                            .address_space
                            .write(
                                thunk32::CHILD_CIPOW_RESULT_ADDRESS,
                                &result.to_bits().to_le_bytes(),
                            )
                            .map_err(|error| error.to_string())?;
                        if written != 8 {
                            return Err("short CIPOW result write".into());
                        }
                        let mut registers = exit.registers;
                        registers.eip = thunk32::CHILD_CIPOW_RESTORE_ADDRESS;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if exit.registers.eip == thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL {
                        if let Some(mut pending) = child.window_callback.take() {
                            let expected_esp = pending.creation.map_or(pending.provider_esp, |(scratch, _)| scratch);
                            if exit.registers.esp != expected_esp {
                                return Err(format!(
                                    "{} callback ESP mismatch expected=0x{:08x} actual=0x{:08x}",
                                    pending.reason,
                                    expected_esp, exit.registers.esp,
                                ));
                            }
                            let wndproc_eax = exit.registers.eax;
                            let api_eax = if let Some((scratch, phase)) = pending.creation {
                                logl::log(level::IMPORTANT, format_args!(
                                    "WC3 CHILD CREATION CALLBACK RETURN pid={} tid={} hwnd=0x{:08x} message=0x{:08x} wndproc_eax=0x{:08x}",
                                    active_pid, active_tid, pending.hwnd, phase.message(), wndproc_eax));
                                match phase.advance(wndproc_eax) {
                                    wc3::window_creation::Advance::Call(next) => {
                                        pending.creation = Some((scratch, next));
                                        pending.message = next.message();
                                        let registers = pending.creation_registers(child, exit.registers)?;
                                        logl::log(level::IMPORTANT, format_args!(
                                            "WC3 CHILD CALL_GUEST pid={} tid={} reason=CreateWindowExA hwnd=0x{:08x} message=0x{:08x} lparam=0x{:08x}",
                                            active_pid, active_tid, pending.hwnd, next.message(), next.lparam(scratch)));
                                        child.window_callback = Some(pending);
                                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                                        continue;
                                    }
                                    wc3::window_creation::Advance::Complete(success) => {
                                        let user_data = session.get_window_long_a(active_pid, pending.hwnd, -21).map_err(str::to_owned)?;
                                        if !success {
                                            session.destroy_window(active_pid, pending.hwnd).map_err(str::to_owned)?;
                                            if let Some(presentation) = session.take_window_presentation() {
                                                present_window(presentation, &mut frames, window_rgba, &session)?;
                                            }
                                        }
                                        let result = if success { pending.hwnd } else { 0 };
                                        logl::log(level::IMPORTANT, format_args!(
                                            "WC3 CHILD CREATEWINDOWEXA RESULT pid={} tid={} hwnd=0x{:08x} creation_callbacks=delivered user_data=0x{:08x} result=0x{:08x} cleanup=48-by-thunk",
                                            active_pid, active_tid, pending.hwnd, user_data, result));
                                        result
                                    }
                                }
                            } else {
                                pending.return_policy.api_result(wndproc_eax)
                            };
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD CALL_GUEST RETURN pid={} tid={} reason={} hwnd=0x{:08x} wndproc=0x{:08x} message=0x{:08x} wndproc_eax=0x{:08x} api_eax=0x{:08x}",
                                    active_pid,
                                    active_tid,
                                    pending.reason,
                                    pending.hwnd,
                                    pending.wndproc,
                                    pending.message,
                                    wndproc_eax,
                                    api_eax,
                                ),
                            );
                            let mut registers = exit.registers;
                            registers.eip = pending.provider_resume_eip;
                            registers.esp = pending.provider_esp;
                            registers.eax = api_eax;
                            contexts[active]
                                .context
                                .set_registers(registers)
                                .map_err(|error| error.to_string())?;
                            continue;
                        }
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
                    let provider_symbol = match &provider.symbol {
                        child_loader::ProviderSymbol::Name(name) => name.clone(),
                        child_loader::ProviderSymbol::Ordinal(ordinal) => format!("#{ordinal}"),
                    };
                    let last_execution = GuestThreadContext::last_execution_diagnostic();
                    logl::trace!(
                        "trace-api",
                        level::IMPORTANT,
                        format_args!(
                            "WC3 PROVIDER ENTRY seq={} pid={} tid={} provider_id={} module=\"{}\" symbol=\"{}\" eip=0x{:08x} esp=0x{:08x} caller_ret=0x{:08x}",
                            last_execution.sequence,
                            active_pid,
                            active_tid,
                            provider_id,
                            provider.module,
                            provider_symbol,
                            exit.registers.eip,
                            exit.registers.esp,
                            u32::from_le_bytes(caller_ret),
                        ),
                    );
                    contexts[active].context.set_execution_provenance(
                        format!("{}!{}", provider.module, provider_symbol),
                        u32::from_le_bytes(caller_ret),
                    );
                    // Render names only for diagnostics; the common modeled
                    // provider path does not need an allocated String.
                    let symbol = || match &provider.symbol {
                        child_loader::ProviderSymbol::Name(name) => format!("symbol=\"{}\"", name),
                        child_loader::ProviderSymbol::Ordinal(ordinal) => {
                            format!("ordinal={}", ordinal)
                        }
                    };
                    include!("asupersync_child_vmcall_special_cases.rs");
                    let operation = child_loader::provider_op(&provider);
                    include!("asupersync_child_vmcall_operations.rs");
                    include!("asupersync_child_vmcall_fallback.rs");
                }
