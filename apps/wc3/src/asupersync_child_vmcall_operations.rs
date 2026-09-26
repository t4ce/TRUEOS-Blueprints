                    {
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
                                logl::trace!("trace-api",
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
                        let resolved = wc3::process::load_library_module_name(&requested);
                        let normalization = if resolved == requested {
                            "unchanged"
                        } else {
                            "default-dll-extension"
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD LOADLIBRARY NORMALIZE pid={} tid={} requested={:?} resolved={:?} reason={}",
                                active_pid,
                                active_tid,
                                requested,
                                resolved,
                                normalization,
                            ),
                        );
                        let existing = session
                            .process(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .loaded_module_handle(&resolved);
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
                                    "WC3 CHILD LOADLIBRARY RETURN pid={} tid={} during=\"{}\" requested={:?} resolved={:?} handle=0x{:08x} already_loaded=1 references={} cleanup=4-by-thunk",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    requested,
                                    resolved,
                                    handle,
                                    references,
                                ),
                            );
                            continue;
                        }
                        if wc3::process::is_system_provider_module(&resolved) {
                            let (handle, references, already_loaded) = session
                                .process_mut(active_pid)
                                .ok_or_else(|| "child process missing".to_owned())?
                                .xp
                                .load_runtime_external_provider(&resolved)
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
                                    "WC3 CHILD LOADLIBRARY PROVIDER pid={} tid={} during=\"{}\" requested={:?} resolved={:?} handle=0x{:08x} already_loaded={} references={} caller_ret=0x{:08x} cleanup=4-by-thunk",
                                    active_pid,
                                    active_tid,
                                    running_module_name,
                                    requested,
                                    resolved,
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
                            .scratch_file_snapshot(&resolved);
                        let local_image = if let Some(bytes) = scratch {
                            let stored = resolved
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
                            let stored = child_loader::resolve_file(&listing, &resolved)
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
                                    "WC3 CHILD LOADLIBRARY LOCAL IMAGE source={source} requested={requested:?} resolved={resolved:?} stored={stored:?} bytes={} sha256={}",
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
                                format!("LoadLibraryA {source} PE {resolved:?}: {error}")
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
                                    "WC3 CHILD LOADLIBRARY LOCAL PE source={} pid={} tid={} requested={:?} resolved={:?} stored={:?} bytes={} image_base=0x{:08x} entry_rva=0x{:08x} size_of_image=0x{:08x} sections={} imports={} exports={} named_exports={} forwarders={} relocations={}",
                                    source,
                                    active_pid,
                                    active_tid,
                                    requested,
                                    resolved,
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
                                        "WC3 CHILD LOADLIBRARY FRONTIER reason=preferred-base-unavailable module={resolved:?} preferred=0x{module_handle:08x} relocations={}",
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
                            let mut pending_images = vec![(resolved.clone(), stored, image)];
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
                                "WC3 CHILD LOADLIBRARY FRONTIER pid={} tid={} during=\"{}\" requested={:?} resolved={:?} kind=external stored=None caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                running_module_name,
                                requested,
                                resolved,
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
                        if child.window_callback.is_some() {
                            return Err("nested CreateWindowExA callback frontier".into());
                        }
                        let args: [u32; 13] = read_guest_words(
                            &X86Memory(&child.address_space), exit.registers.esp, 13)?
                            .try_into().map_err(|_| "creation argument count")?;
                        let payload = wc3::window_creation::payload(&args).map_err(str::to_owned)?;
                        let scratch = exit.registers.esp.checked_sub(payload.len() as u32)
                            .ok_or("creation payload stack underflow")?;
                        if child.address_space.write(scratch, &payload).map_err(|error| error.to_string())? != payload.len() {
                            return Err("short creation payload write".into());
                        }
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
                                "WC3 CHILD CREATEWINDOWEXA BEGIN pid={} tid={} hwnd=0x{:08x} class={:?} title={:?} wndproc=0x{:08x} icon=0x{:08x} cursor=0x{:08x} parent=0x{:08x} param=0x{:08x} geometry={},{} {}x{} win32_visible={} ui4_frame={} ui4_policy=always-visible creation_callbacks=synchronous result=pending cleanup=48-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                window.class,
                                window.title,
                                window.wndproc,
                                window.class_icon,
                                window.class_cursor,
                                window.parent,
                                window.param,
                                window.x,
                                window.y,
                                window.width,
                                window.height,
                                window.visible as u8,
                                ui4_frame as u8,
                            ),
                        );
                        let phase = wc3::window_creation::Phase::NcCreate;
                        let pending = ChildWindowCallback {
                            provider_resume_eip: exit.registers.eip,
                            provider_esp: exit.registers.esp,
                            reason: "CreateWindowExA",
                            return_policy: WindowCallbackReturn::Fixed(hwnd),
                            creation: Some((scratch, phase)),
                            hwnd, wndproc: window.wndproc, message: phase.message(),
                        };
                        let registers = pending.creation_registers(child, exit.registers)?;
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD CALL_GUEST pid={} tid={} reason=CreateWindowExA hwnd=0x{:08x} message=0x{:08x} lparam=0x{:08x}",
                            active_pid, active_tid, hwnd, phase.message(), phase.lparam(scratch)));
                        child.window_callback = Some(pending);
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
                    if operation == child_loader::ProviderOp::SetForegroundWindow {
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
                        let PersonalityAction::Session(
                            SessionRequest::SetForegroundWindow { caller, hwnd },
                        ) = action
                        else {
                            return Err("SetForegroundWindow produced unexpected action".into());
                        };
                        let (previous, owner) = session
                            .set_foreground_window(caller, hwnd)
                            .map_err(str::to_owned)?;
                        let base = session.thread_base_priority(owner).unwrap_or(8);
                        let effective = session.thread_effective_priority(owner).unwrap_or(base);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETFOREGROUNDWINDOW pid={} tid={} hwnd=0x{:08x} previous=0x{:08x} owner=pid{}/tid{} base_priority={} foreground_boost={} effective_priority={} focused=1 result=1 cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                previous.unwrap_or(0),
                                owner.pid,
                                owner.tid,
                                base,
                                (effective != base) as u8,
                                effective,
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
                    if operation == child_loader::ProviderOp::SetActiveWindow {
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
                        let PersonalityAction::Session(SessionRequest::SetActiveWindow {
                            caller,
                            hwnd,
                        }) = action
                        else {
                            return Err("SetActiveWindow produced unexpected action".into());
                        };
                        let previous = session
                            .set_active_window(caller, hwnd)
                            .map_err(str::to_owned)?;
                        let owner = session
                            .windows
                            .get(&hwnd)
                            .ok_or("SetActiveWindow window disappeared")?
                            .owner;
                        let active_window = session.active_windows.get(&caller).copied().unwrap_or(0);
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SETACTIVEWINDOW pid={} tid={} hwnd=0x{:08x} owner=pid{}/tid{} previous=0x{:08x} active=0x{:08x} foreground=0x{:08x} focused=0x{:08x} scheduler_change=0 result=0x{:08x} cleanup=4-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                owner.pid,
                                owner.tid,
                                previous,
                                active_window,
                                session.foreground_window.unwrap_or(0),
                                session.focused_window.unwrap_or(0),
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
                    if operation == child_loader::ProviderOp::SetWindowLongA {
                        let action = session.process_mut(active_pid).ok_or("child process missing")?.xp
                            .dispatch_provider_for_process_typed(active_pid, active_tid, provider_id,
                                exit.registers.esp, &mut X86Memory(&child.address_space))
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::SetWindowLongA { pid, hwnd, index, value }) = action
                            else { return Err("SetWindowLongA produced unexpected action".into()); };
                        let previous = session.set_window_long_a(pid, hwnd, index, value).map_err(str::to_owned)?;
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD SETWINDOWLONGA pid={} tid={} hwnd=0x{:08x} index={} previous=0x{:08x} value=0x{:08x} source=guest cleanup=12-by-thunk",
                            active_pid, active_tid, hwnd, index, previous, value));
                        let mut registers = exit.registers;
                        registers.eax = previous;
                        contexts[active].context.set_registers(registers).map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::GetWindowLongA {
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
                        let PersonalityAction::Session(SessionRequest::GetWindowLongA {
                            pid,
                            hwnd,
                            index,
                        }) = action
                        else {
                            return Err("GetWindowLongA produced unexpected action".into());
                        };
                        let result = session
                            .get_window_long_a(pid, hwnd, index)
                            .map_err(str::to_owned)?;
                        let index_name = match index {
                            -4 => "GWL_WNDPROC",
                            -6 => "GWL_HINSTANCE",
                            -8 => "GWL_HWNDPARENT",
                            -12 => "GWL_ID",
                            -16 => "GWL_STYLE",
                            -20 => "GWL_EXSTYLE",
                            -21 => "GWL_USERDATA",
                            value if value >= 0 => "WINDOW_EXTRA",
                            _ => "UNKNOWN",
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD GETWINDOWLONGA pid={} tid={} hwnd=0x{:08x} index={} index_name={} result=0x{:08x} cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                index,
                                index_name,
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
                    if operation == child_loader::ProviderOp::DestroyWindow {
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
                        let PersonalityAction::Session(SessionRequest::DestroyWindow {
                            pid,
                            hwnd,
                        }) = action
                        else {
                            return Err("DestroyWindow produced unexpected action".into());
                        };
                        let frame_present = frames.contains_key(&hwnd);
                        let result = match session.destroy_window(pid, hwnd) {
                            Ok(destroyed) => {
                                if let Some(presentation) = session.take_window_presentation() {
                                    present_window(
                                        presentation,
                                        &mut frames,
                                        window_rgba,
                                        &session,
                                    )?;
                                }
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD DESTROYWINDOW RESULT pid={} tid={} hwnd=0x{:08x} frame_present={} ui4_frame=closed focused={} result=TRUE cleanup=4-by-thunk",
                                        active_pid,
                                        active_tid,
                                        hwnd,
                                        frame_present as u8,
                                        destroyed.was_focused as u8,
                                    ),
                                );
                                1
                            }
                            Err(_) => {
                                session
                                    .process_mut(pid)
                                    .ok_or("DestroyWindow caller missing")?
                                    .xp
                                    .set_last_error_for_thread(active_tid, 1400);
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD DESTROYWINDOW RESULT pid={} tid={} hwnd=0x{:08x} frame_present={} ui4_frame=unchanged focused=- result=FALSE error=1400 cleanup=4-by-thunk",
                                        active_pid,
                                        active_tid,
                                        hwnd,
                                        frame_present as u8,
                                    ),
                                );
                                0
                            }
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::UpdateWindow {
                        let provider_resume_eip = exit.registers.eip;
                        let provider_esp = exit.registers.esp;
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "child process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                provider_esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::Session(SessionRequest::UpdateWindow {
                            pid: _,
                            hwnd,
                        }) = action
                        else {
                            return Err("UpdateWindow produced unexpected action".into());
                        };
                        let pending = session.update_window(hwnd).map_err(str::to_owned)?;
                        if pending == 0 {
                            logl::log(
                                level::IMPORTANT,
                                format_args!(
                                    "WC3 CHILD UPDATEWINDOW RESULT pid={} tid={} hwnd=0x{:08x} paint_pending=0 callback=none result=0 cleanup=4-by-thunk",
                                    active_pid, active_tid, hwnd,
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

                        let wndproc = session
                            .windows
                            .get(&hwnd)
                            .ok_or("UpdateWindow window disappeared")?
                            .wndproc;
                        let callback_esp = provider_esp
                            .checked_sub(20)
                            .ok_or("UpdateWindow callback stack underflow")?;
                        let frame = [
                            thunk32::CHILD_CALLBACK_RETURN_ADDRESS,
                            hwnd,
                            0x000f,
                            0,
                            0,
                        ];
                        let mut frame_bytes = [0u8; 20];
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
                            return Err("short UpdateWindow callback frame write".into());
                        }
                        if child.window_callback.is_some() {
                            return Err("UpdateWindow callback already pending".into());
                        }
                        child.window_callback = Some(ChildWindowCallback {
                            provider_resume_eip,
                            provider_esp,
                            reason: "UpdateWindow/WM_PAINT",
                            return_policy: WindowCallbackReturn::Fixed(1),
                            creation: None,
                            hwnd,
                            wndproc,
                            message: 0x000f,
                        });
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CALL_GUEST pid={} tid={} reason=UpdateWindow/WM_PAINT hwnd=0x{:08x} wndproc=0x{:08x} message=0x0000000f wparam=0x00000000 lparam=0x00000000",
                                active_pid, active_tid, hwnd, wndproc,
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eip = wndproc;
                        registers.esp = callback_esp;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::DispatchMessageA {
                        let provider_resume_eip = exit.registers.eip;
                        let provider_esp = exit.registers.esp;
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            provider_esp,
                            2,
                        )?;
                        let message_ptr = frame[1];
                        if message_ptr == 0 {
                            return Err("DispatchMessageA null MSG pointer".into());
                        }
                        let message_words = read_guest_words(
                            &X86Memory(&child.address_space),
                            message_ptr,
                            4,
                        )?;
                        let hwnd = message_words[0];
                        let message = message_words[1];
                        let wparam = message_words[2];
                        let lparam = message_words[3];
                        let window = session
                            .windows
                            .get(&hwnd)
                            .ok_or("DispatchMessageA unknown window")?;
                        if window.owner.pid != active_pid {
                            return Err("DispatchMessageA window owner mismatch".into());
                        }
                        let wndproc = window.wndproc;
                        let callback_esp = provider_esp
                            .checked_sub(20)
                            .ok_or("DispatchMessageA callback stack underflow")?;
                        let callback_frame = [
                            thunk32::CHILD_CALLBACK_RETURN_ADDRESS,
                            hwnd,
                            message,
                            wparam,
                            lparam,
                        ];
                        let mut callback_bytes = [0u8; 20];
                        for (index, value) in callback_frame.into_iter().enumerate() {
                            callback_bytes[index * 4..index * 4 + 4]
                                .copy_from_slice(&value.to_le_bytes());
                        }
                        if child
                            .address_space
                            .write(callback_esp, &callback_bytes)
                            .map_err(|error| error.to_string())?
                            != callback_bytes.len()
                        {
                            return Err("short DispatchMessageA callback frame write".into());
                        }
                        if child.window_callback.is_some() {
                            return Err("DispatchMessageA callback already pending".into());
                        }
                        child.window_callback = Some(ChildWindowCallback {
                            provider_resume_eip,
                            provider_esp,
                            reason: "DispatchMessageA",
                            return_policy: WindowCallbackReturn::WndProc,
                            creation: None,
                            hwnd,
                            wndproc,
                            message,
                        });
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CALL_GUEST pid={} tid={} reason=DispatchMessageA hwnd=0x{:08x} wndproc=0x{:08x} message=0x{:08x} wparam=0x{:08x} lparam=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                hwnd,
                                wndproc,
                                message,
                                wparam,
                                lparam,
                                frame[0],
                            ),
                        );
                        let mut registers = exit.registers;
                        registers.eip = wndproc;
                        registers.esp = callback_esp;
                        contexts[active]
                            .context
                            .set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::BeginPaint {
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
                        let PersonalityAction::Session(SessionRequest::BeginPaint {
                            pid,
                            hwnd,
                            paint_struct,
                        }) = action
                        else {
                            return Err("BeginPaint produced unexpected action".into());
                        };
                        let (width, height) = session
                            .begin_paint_window(pid, hwnd)
                            .map_err(str::to_owned)?;
                        let hdc = session
                            .process_mut(pid)
                            .ok_or("BeginPaint process missing")?
                            .xp
                            .begin_paint(
                                hwnd,
                                paint_struct,
                                width,
                                height,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD BEGINPAINT pid={} tid={} hwnd=0x{:08x} ps=0x{:08x} hdc=0x{:08x} target=WINDOW_PAINT client={}x{} rcPaint=[0,0,{},{}] erase=0 result=0x{:08x} cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                paint_struct,
                                hdc,
                                width,
                                height,
                                width,
                                height,
                                hdc,
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
                    if operation == child_loader::ProviderOp::ImmAssociateContext {
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
                        let PersonalityAction::Session(SessionRequest::ImmAssociateContext {
                            pid,
                            hwnd,
                            himc,
                        }) = action
                        else {
                            return Err("ImmAssociateContext produced unexpected action".into());
                        };
                        let previous = session
                            .imm_associate_context(pid, hwnd, himc)
                            .map_err(str::to_owned)?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD IMMASSOCIATECONTEXT RESULT pid={} tid={} hwnd=0x{:08x} himc=0x{:08x} previous=0x{:08x} ime_policy=disabled keyboard_layouts=de+en ui4_input_action=none cleanup=8-by-thunk",
                                active_pid,
                                active_tid,
                                hwnd,
                                himc,
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
                    if operation == child_loader::ProviderOp::DuplicateHandle {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            8,
                        )?;
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DUPLICATEHANDLE CALL pid={} tid={} source_process=0x{:08x} source_handle=0x{:08x} target_process=0x{:08x} target_out=0x{:08x} desired_access=0x{:08x} inherit={} options=0x{:08x} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                frame[3],
                                frame[4],
                                frame[5],
                                frame[6],
                                frame[7],
                                frame[0],
                            ),
                        );
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
                        let PersonalityAction::Session(SessionRequest::DuplicateHandle(request)) = action
                        else {
                            return Err("DuplicateHandle produced unexpected action".into());
                        };
                        if request.target_process != 0xffff_ffff {
                            return Err(format!(
                                "DuplicateHandle target process frontier=0x{:08x}",
                                request.target_process,
                            ));
                        }
                        let target_out = request.target_out;
                        let duplicated = match session.duplicate_handle(request) {
                            Ok(duplicated) => duplicated,
                            Err(error) => {
                                session
                                    .process_mut(active_pid)
                                    .ok_or_else(|| "DuplicateHandle caller missing".to_owned())?
                                    .xp
                                    .set_last_error_for_thread(active_tid, error);
                                let mut registers = exit.registers;
                                registers.eax = 0;
                                contexts[active]
                                    .context
                                    .set_registers(registers)
                                    .map_err(|error| error.to_string())?;
                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD DUPLICATEHANDLE RESULT pid={} tid={} result=0 error={} cleanup=28-by-thunk",
                                        active_pid, active_tid, error,
                                    ),
                                );
                                continue;
                            }
                        };
                        if duplicated.target_pid != active_pid {
                            return Err(format!(
                                "DuplicateHandle target pid={} is not active pid={active_pid}",
                                duplicated.target_pid,
                            ));
                        }
                        X86Memory(&child.address_space)
                            .write(target_out, &duplicated.handle.to_le_bytes())
                            .map_err(|error| error.to_string())?;
                        let (object_kind, object_owner) = match session.objects.get(&duplicated.object) {
                            Some(SessionObject::Thread(thread)) => (
                                "thread",
                                format!("pid{}/tid{}", thread.key.pid, thread.key.tid),
                            ),
                            Some(SessionObject::Process(process)) => {
                                ("process", format!("pid{}", process.pid))
                            }
                            Some(SessionObject::Event(_)) => ("event", "-".into()),
                            Some(SessionObject::Mutex(_)) => ("mutex", "-".into()),
                            Some(SessionObject::IoCompletionPort(_)) => {
                                ("io-completion-port", "-".into())
                            }
                            None => return Err("DuplicateHandle object disappeared".into()),
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD DUPLICATEHANDLE RESULT pid={} tid={} source_process=pid{} source_handle=0x{:08x} target_process=pid{} target_handle=0x{:08x} inherit={} same_access=1 same_object=1 object_kind={} object_owner={} result=1 cleanup=28-by-thunk",
                                active_pid,
                                active_tid,
                                duplicated.source_pid,
                                frame[2],
                                duplicated.target_pid,
                                duplicated.handle,
                                frame[6] as u8,
                                object_kind,
                                object_owner,
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
                    if matches!(
                        operation,
                        child_loader::ProviderOp::CreateEventA
                            | child_loader::ProviderOp::OpenEventA
                            | child_loader::ProviderOp::SetEvent
                            | child_loader::ProviderOp::ResetEvent
                            | child_loader::ProviderOp::CreateIoCompletionPort
                            | child_loader::ProviderOp::GetQueuedCompletionStatus
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
                        let (result, zero_wait_handle) = match action {
                            PersonalityAction::Return(result) => (result, None),
                            PersonalityAction::Session(request) => (
                                service_sync_request(
                                    &mut session,
                                    active_pid,
                                    active_tid,
                                    request,
                                    &mut contexts,
                                    &mut wait_deadlines,
                                )?,
                                None,
                            ),
                            PersonalityAction::IoCompletionWait(request) => {
                                const ERROR_INVALID_HANDLE: u32 = 6;
                                const WAIT_TIMEOUT_ERROR: u32 = 258;

                                logl::log(
                                    level::IMPORTANT,
                                    format_args!(
                                        "WC3 CHILD GETQUEUEDCOMPLETIONSTATUS CALL \\
                                         pid={} tid={} port=0x{:08x} \\
                                         bytes_out=0x{:08x} key_out=0x{:08x} \\
                                         overlapped_out=0x{:08x} timeout=0x{:08x}",
                                        request.key.pid,
                                        request.key.tid,
                                        request.port,
                                        request.bytes_out,
                                        request.completion_key_out,
                                        request.overlapped_out,
                                        request.timeout,
                                    ),
                                );

                                let result = match session.take_io_completion_packet(request.key, request.port) {
                                    Err(_) => {
                                        session
                                            .process_mut(request.key.pid)
                                            .ok_or_else(|| "GQCS process missing".to_owned())?
                                            .xp
                                            .set_last_error_for_thread(
                                                request.key.tid,
                                                ERROR_INVALID_HANDLE,
                                            );
                                        write_child_u32(child, request.overlapped_out, 0)?;
                                        0
                                    }
                                    Ok(Some(packet)) => {
                                        write_child_u32(
                                            child,
                                            request.bytes_out,
                                            packet.bytes_transferred,
                                        )?;
                                        write_child_u32(
                                            child,
                                            request.completion_key_out,
                                            packet.completion_key,
                                        )?;
                                        write_child_u32(
                                            child,
                                            request.overlapped_out,
                                            packet.overlapped,
                                        )?;
                                        1
                                    }
                                    Ok(None) if request.timeout == 0 => {
                                        write_child_u32(child, request.overlapped_out, 0)?;
                                        session
                                            .process_mut(request.key.pid)
                                            .ok_or_else(|| "GQCS process missing".to_owned())?
                                            .xp
                                            .set_last_error_for_thread(
                                                request.key.tid,
                                                WAIT_TIMEOUT_ERROR,
                                            );
                                        0
                                    }
                                    Ok(None) if request.timeout == INFINITE => {
                                        session
                                            .block_io_completion(request.clone())
                                            .map_err(str::to_owned)?;
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD IOCP BLOCK pid={} tid={} \\
                                                 port=0x{:08x} timeout=INFINITE queue_depth=0 \\
                                                 transport=offline",
                                                request.key.pid,
                                                request.key.tid,
                                                request.port,
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
                                    Ok(None) => {
                                        return Err(format!(
                                            "GetQueuedCompletionStatus finite-timeout frontier \\
                                             pid={} tid={} port=0x{:08x} timeout={}",
                                            request.key.pid,
                                            request.key.tid,
                                            request.port,
                                            request.timeout,
                                        ));
                                    }
                                };
                                (result, None)
                            }
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
                                let (result, zero_wait_handle) = if let Some(result) =
                                    session.poll_wait(&request).map_err(str::to_owned)?
                                {
                                    // Keep every signal/error. Empty zero-timeout polls are
                                    // accounted per call site, so one hot site cannot hide
                                    // another behind a process-global sampling counter.
                                    let zero_wait_handle =
                                        (result == WAIT_TIMEOUT && request.timeout == 0)
                                            .then_some(request.handles[0]);
                                    if let Some(handle) = zero_wait_handle {
                                        let site = IdlePollSite {
                                            key: request.key,
                                            caller_return: request.return_address,
                                            handles: request.handles,
                                            wait_all: request.wait_all,
                                            timeout: request.timeout,
                                        };
                                        let now = tokio::time::Instant::now();
                                        let stats = idle_poll_sites.entry(site).or_insert(
                                            IdlePollStats {
                                                polls: 0,
                                                last_reported_polls: 0,
                                                last_report: now,
                                            },
                                        );
                                        stats.polls += 1;
                                        let report = stats.polls <= 4
                                            || now.duration_since(stats.last_report)
                                                >= Duration::from_secs(1);
                                        if report {
                                            let delta = stats.polls - stats.last_reported_polls;
                                            stats.last_reported_polls = stats.polls;
                                            stats.last_report = now;
                                            let stack_words = read_guest_words(
                                                &X86Memory(&child.address_space),
                                                exit.registers.esp,
                                                8,
                                            )
                                            .unwrap_or_default();
                                            let object = session.describe_handle(request.key.pid, handle);
                                            let event_state = session.event_state(request.key.pid, handle);
                                            logl::log(
                                                level::IMPORTANT,
                                                format_args!(
                                                    "WC3 CHILD IDLE POLL PROVENANCE pid={} tid={} handle=0x{:08x} object={} manual_reset={:?} signaled={:?} provider_return=WAIT_TIMEOUT caller_ret=0x{:08x} wait_all={} poll_count={} delta={} guest_stack_candidates={:08x?}",
                                                    request.key.pid,
                                                    request.key.tid,
                                                    handle,
                                                    object,
                                                    event_state.map(|state| state.0),
                                                    event_state.map(|state| state.1),
                                                    request.return_address,
                                                    request.wait_all,
                                                    stats.polls,
                                                    delta,
                                                    stack_words,
                                                ),
                                            );
                                        }
                                    } else {
                                        logl::log(
                                            level::IMPORTANT,
                                            format_args!(
                                                "WC3 CHILD WAIT {}RETURN pid={} tid={} handle0=0x{:08x} handle1=0x{:08x} result=0x{:08x} timeout_ms={} caller_ret=0x{:08x}",
                                                if is_multiple_wait { "MULTIPLE " } else { "" },
                                                active_pid, active_tid, request.handles[0], request.handles[1], result,
                                                request.timeout, request.return_address,
                                            ),
                                        );
                                    }
                                    (result, zero_wait_handle)
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
                                };
                                (result, zero_wait_handle)
                            }
                            _ => return Err("unexpected child synchronization action".into()),
                        };
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD SYNC RETURN pid={} tid={} {} eax=0x{:08x} cleanup={}-by-thunk",
                                active_pid,
                                active_tid,
                                symbol(),
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
                        if let Some(handle) = zero_wait_handle {
                            // The caller has already received its immediate
                            // WAIT_TIMEOUT. Treat a tight polling loop as a
                            // scheduler safepoint so it cannot monopolize the
                            // serialized x86 executor.
                            tokio::task::yield_now().await;
                            expire_runtime_waits(
                                &mut session,
                                &mut contexts,
                                &mut wait_deadlines,
                                &mut previous_wait_timeout,
                            )?;
                            session.enqueue(active_key);
                            if let Some(next) = pop_runnable_context(&mut session, &contexts) {
                                if next != active {
                                    let next_key = contexts[next].key();
                                    logl::log(
                                        level::IMPORTANT,
                                        format_args!(
                                            "WC3 CHILD ZERO-WAIT SCHEDULE from=pid{}/tid{} handle=0x{:08x} result=WAIT_TIMEOUT to=pid{}/tid{} reason=zero-timeout-safepoint",
                                            active_pid,
                                            active_tid,
                                            handle,
                                            next_key.pid,
                                            next_key.tid,
                                        ),
                                    );
                                }
                                active = next;
                            }
                        }
                        continue;
                    }
                    if operation == child_loader::ProviderOp::FillRect {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "FillRect process missing".to_owned())?
                            .xp
                            .dispatch_provider_for_process_typed(
                                active_pid,
                                active_tid,
                                provider_id,
                                exit.registers.esp,
                                &mut X86Memory(&child.address_space),
                            )
                            .map_err(|error| error.to_string())?;
                        let PersonalityAction::WindowFillRect(request) = action else {
                            return Err("FillRect produced unexpected action".into());
                        };
                        paint_window_fill_rect(&request, &mut frames, window_rgba)?;
                        logl::log(level::IMPORTANT, format_args!(
                            "WC3 CHILD FILLRECT pid={} tid={} hwnd=0x{:08x} hdc=0x{:08x} rect={:?} result=1 cleanup=12-by-thunk",
                            active_pid, active_tid, request.hwnd, request.hdc, request.rect,
                        ));
                        let mut registers = exit.registers;
                        registers.eax = 1;
                        contexts[active].context.set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    if operation == child_loader::ProviderOp::SetDeviceGammaRamp {
                        let action = session
                            .process_mut(active_pid)
                            .ok_or_else(|| "SetDeviceGammaRamp process missing".to_owned())?
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
                            PersonalityAction::WindowGammaRamp(request) => {
                                if let Some(frame) = frames.get(&request.hwnd) {
                                    // Acknowledge the guest request without changing the shared
                                    // display LUT or our last-applied gamma state. Retain the
                                    // complete requested table in bounded log records for diagnosis.
                                    logl::log(level::IMPORTANT, format_args!(
                                        "WC3 CHILD SETDEVICEGAMMARAMP REQUEST pid={} tid={} hwnd=0x{:08x} hdc=0x{:08x} ui4_window={} entries=256xRGB16 policy=log-only programmed=0 result=1 cleanup=8-by-thunk",
                                        active_pid, active_tid, request.hwnd, request.hdc, frame.window_id(),
                                    ));
                                    for (channel, values) in ["red", "green", "blue"].into_iter()
                                        .zip(request.ramp.chunks_exact(256)) {
                                        logl::log(level::IMPORTANT, format_args!(
                                            "WC3 GAMMA REQUEST channel={} min={} max={} first={} middle={} last={}",
                                            channel, values.iter().min().unwrap(), values.iter().max().unwrap(),
                                            values[0], values[128], values[255],
                                        ));
                                        for (chunk, values) in values.chunks_exact(16).enumerate() {
                                            logl::log(level::IMPORTANT, format_args!(
                                                "WC3 GAMMA REQUEST channel={} start={} values={:?}",
                                                channel, chunk * 16, values,
                                            ));
                                        }
                                    }
                                    1
                                } else {
                                    0
                                }
                            },
                            PersonalityAction::Return(result) => result,
                            _ => return Err("SetDeviceGammaRamp produced unexpected action".into()),
                        };
                        let mut registers = exit.registers;
                        registers.eax = result;
                        contexts[active].context.set_registers(registers)
                            .map_err(|error| error.to_string())?;
                        continue;
                    }
                    }
