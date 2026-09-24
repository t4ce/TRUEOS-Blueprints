impl XpProcess {
    fn dispatch_process_local_provider(
        &mut self,
        pid: u32,
        tid: u32,
        operation: ProviderOp,
        esp: u32,
        memory: &mut impl GuestMemory,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        match operation {
            ProviderOp::GetSystemInfo => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_system_info(esp, memory)?,
                ))
            }
            ProviderOp::GlobalMemoryStatus => {
                let result = self.global_memory_status(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::D3D8GetAdapterIdentifier => {
                let result = self.d3d8_get_adapter_identifier(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::D3D8Release => {
                let result = self.d3d8_release(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::InterlockedExchange => {
                let previous = self.interlocked_exchange(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(previous))
            }
            ProviderOp::InterlockedIncrement => {
                let incremented = self.interlocked_increment(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(incremented))
            }
            ProviderOp::InterlockedDecrement => {
                let decremented = self.interlocked_decrement(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(decremented))
            }
            ProviderOp::LoadStringA => {
                let result = self.load_string(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::FormatMessageA => {
                let result = self.format_message_a(esp, memory)?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::TlsAlloc => {
                let slot = self.tls_alloc()?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(slot))
            }
            ProviderOp::TlsSetValue => {
                let result = self.tls_set_value(tid, esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::TlsGetValue => {
                let result = self.tls_get_value(tid, esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtSetAppType => {
                let [_, app_type] = arguments::<2>(memory, esp)?;
                if !matches!(app_type, CRT_UNKNOWN_APP | CRT_CONSOLE_APP | CRT_GUI_APP) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__set_app_type",
                        detail: format!("unobserved app_type={app_type}"),
                    });
                }
                self.crt_app_type = app_type;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::CrtGetFmode => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CRT_FMODE_VA))
            }
            ProviderOp::CrtGetCommode => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CRT_COMMODE_VA))
            }
            ProviderOp::CrtXcptFilter => {
                let [_, exception_code, exception_pointers] = arguments::<3>(memory, esp)?;
                if exception_pointers == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!("null exception pointers code=0x{exception_code:08x}"),
                    });
                }
                let record = read_u32(memory, exception_pointers)?;
                let context = read_u32(
                    memory,
                    exception_pointers
                        .checked_add(4)
                        .ok_or("XcptFilter pointer overflow")?,
                )?;
                if record == 0 || context == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!(
                            "invalid exception pointers ptr=0x{exception_pointers:08x} record=0x{record:08x} context=0x{context:08x}"
                        ),
                    });
                }
                let record_code = read_u32(memory, record)?;
                if record_code != exception_code {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!(
                            "code mismatch argument=0x{exception_code:08x} record=0x{record_code:08x}"
                        ),
                    });
                }
                if !matches!(
                    exception_code,
                    crate::seh::STATUS_ACCESS_VIOLATION | crate::seh::STATUS_ILLEGAL_INSTRUCTION
                ) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "_XcptFilter",
                        detail: format!("unobserved exception code=0x{exception_code:08x}"),
                    });
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(EXCEPTION_FILTER_CONTINUE_SEARCH))
            }
            ProviderOp::CrtGetMainArgs => {
                let [_, argc_out, argv_out, env_out, wildcard, startup_info] =
                    arguments::<6>(memory, esp)?;
                if argc_out == 0 || argv_out == 0 || env_out == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__getmainargs",
                        detail: format!(
                            "null output argc=0x{argc_out:08x} argv=0x{argv_out:08x} env=0x{env_out:08x}"
                        ),
                    });
                }
                if wildcard != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "__getmainargs",
                        detail: format!("wildcard expansion={wildcard}"),
                    });
                }
                if startup_info != 0 {
                    let new_mode = read_u32(memory, startup_info)?;
                    if new_mode != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "__getmainargs",
                            detail: format!(
                                "startup_info=0x{startup_info:08x} new_mode={new_mode}"
                            ),
                        });
                    }
                }
                write_u32(memory, argc_out, CRT_ARGC)?;
                write_u32(memory, argv_out, CRT_ARGV_VA)?;
                write_u32(memory, env_out, CRT_ENVP_VA)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::CrtOnExit => {
                let [_, func] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if func == 0 || self.crt_onexit_callbacks.len() >= 65_536 {
                    return Ok(PersonalityAction::Return(0));
                }
                self.crt_onexit_callbacks.push(func);
                Ok(PersonalityAction::Return(func))
            }
            ProviderOp::CrtVsnprintf => {
                let result = crt_vsnprintf(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtMemmove => {
                let [_, destination, source, count] = arguments::<4>(memory, esp)?;
                let result = crt_memmove(memory, destination, source, count)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtIsDigit => {
                let [_, character] = arguments::<2>(memory, esp)?;
                // MSVCRT's _DIGIT classification bit is 0x04.  The CRT is
                // currently in its initial C locale, where only ASCII digits
                // have that classification.
                let result = if (b'0' as u32..=b'9' as u32).contains(&character) {
                    0x04
                } else {
                    0
                };
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtToUpper => {
                let [_, character] = arguments::<2>(memory, esp)?;
                let result = match character {
                    value @ 0x61..=0x7a => value - 0x20,
                    u32::MAX => u32::MAX,
                    0x00..=0xff => character,
                    other => {
                        return Err(ProviderDispatchError::Frontier {
                            api: "toupper",
                            detail: format!(
                                "character outside unsigned-char/EOF domain=0x{other:08x}"
                            ),
                        });
                    }
                };
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtAtol => {
                let [_, string] = arguments::<2>(memory, esp)?;
                let (result, _, _) = crt_atol(memory, string)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtRand => {
                // MSVCRT's process-global rand stream: holdrand =
                // holdrand * 214013 + 2531011, then expose its high 15 bits.
                self.crt_rng_seed = self
                    .crt_rng_seed
                    .wrapping_mul(214_013)
                    .wrapping_add(2_531_011);
                let result = (self.crt_rng_seed >> 16) & 0x7fff;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtSrand => {
                let [_, seed] = arguments::<2>(memory, esp)?;
                self.crt_rng_seed = seed;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::CrtStrncpy => {
                let [_, destination, source, count] = arguments::<4>(memory, esp)?;
                let result = crt_strncpy(memory, destination, source, count)?;
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrpbrk => {
                let [_, haystack, accept] = arguments::<3>(memory, esp)?;
                let haystack_bytes = read_c_bytes(memory, haystack)?;
                let accept_bytes = read_c_bytes(memory, accept)?;
                let result = staticstr::pbrk(&haystack_bytes, &accept_bytes)
                    .map(|offset| {
                        haystack
                            .checked_add(u32::try_from(offset).expect("bounded string offset"))
                            .expect("bounded string address")
                    })
                    .unwrap_or(0);
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrlwr | ProviderOp::CrtStrupr => {
                let [_, string] = arguments::<2>(memory, esp)?;
                let result = crt_strcase(memory, string, operation == ProviderOp::CrtStrupr)?;
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrncmp => {
                let [_, left, right, count] = arguments::<4>(memory, esp)?;
                let left = read_c_bytes(memory, left)?;
                let right = read_c_bytes(memory, right)?;
                let result = staticstr::compare(&left, &right, Some(count as usize), false) as u32;
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStricmp => {
                let [_, left, right] = arguments::<3>(memory, esp)?;
                let left = read_c_bytes(memory, left)?;
                let right = read_c_bytes(memory, right)?;
                let result = staticstr::compare(&left, &right, None, true) as u32;
                self.call_count = self.call_count.checked_add(1).ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrrchr => {
                let [_, string, character] = arguments::<3>(memory, esp)?;
                let result = crt_strrchr(memory, string, character)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrstr => {
                let [_, haystack, needle] = arguments::<3>(memory, esp)?;
                let result = crt_strstr(memory, haystack, needle)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtStrnicmp => {
                let [_, left, right, count] = arguments::<4>(memory, esp)?;
                let result = crt_strnicmp(memory, left, right, count)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::WsprintfA => {
                let result = wsprintf_a(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::CrtFullPath => {
                let [_, output, path, capacity] = arguments::<4>(memory, esp)?;
                let result = if output == 0 || capacity == 0 {
                    0
                } else {
                    let path = read_c_string(memory, path, 1024)?;
                    let Some(full_path) = crt_full_path(&path) else {
                        return Ok(PersonalityAction::Return(0));
                    };
                    let bytes = full_path.as_bytes();
                    if bytes.len().checked_add(1).ok_or("fullpath length")? > capacity as usize {
                        0
                    } else {
                        memory.write(output, bytes)?;
                        memory.write(
                            output
                                .checked_add(
                                    u32::try_from(bytes.len()).map_err(|_| "fullpath length")?,
                                )
                                .ok_or("fullpath output overflow")?,
                            &[0],
                        )?;
                        output
                    }
                };
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::FreeEnvironmentStringsW => {
                let pointer = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                if pointer != ENVIRONMENT_BLOCK_VA {
                    return Err(ProviderDispatchError::Unsupported);
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetStartupInfoA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_startup_info(esp, memory)?,
                ))
            }
            ProviderOp::GetStdHandle => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_std_handle(esp, memory)?))
            }
            ProviderOp::GetFileType => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_file_type(esp, memory)?))
            }
            ProviderOp::SetHandleCount => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.set_handle_count(esp, memory)?,
                ))
            }
            ProviderOp::GetACP => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_acp()))
            }
            ProviderOp::GetCPInfo => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.get_cp_info(esp, memory)?))
            }
            ProviderOp::GetStringTypeW => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_string_type(esp, memory)?,
                ))
            }
            ProviderOp::MultiByteToWideChar => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.multi_byte_to_wide(esp, memory)?,
                ))
            }
            ProviderOp::LCMapStringW => {
                let flags = read_u32(
                    memory,
                    esp.checked_add(8).ok_or("provider argument overflow")?,
                )?;
                if lc_map_mode(flags).is_none() {
                    return Err(ProviderDispatchError::Unsupported);
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.lc_map_string(esp, memory)?))
            }
            ProviderOp::GetModuleFileNameA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_module_filename(esp, memory)?,
                ))
            }
            ProviderOp::GetModuleHandleA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_module_handle_a(esp, memory)?,
                ))
            }
            ProviderOp::GetCurrentProcess => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CURRENT_PROCESS_PSEUDO_HANDLE))
            }
            ProviderOp::GetCurrentProcessId => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(pid))
            }
            ProviderOp::GetProcessHeap => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(PROCESS_HEAP_HANDLE))
            }
            ProviderOp::GetCurrentThread => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(CURRENT_THREAD_PSEUDO_HANDLE))
            }
            ProviderOp::GetCurrentThreadId => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(tid))
            }
            ProviderOp::OpenThreadToken => {
                let [_, thread, desired_access, open_as_self, _token_out] =
                    arguments::<5>(memory, esp)?;
                if thread != CURRENT_THREAD_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenThreadToken",
                        detail: format!("non-current-thread handle=0x{thread:08x}"),
                    });
                }
                if desired_access != TOKEN_QUERY || open_as_self != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenThreadToken",
                        detail: format!(
                            "unobserved shape access=0x{desired_access:08x} open_as_self={open_as_self}"
                        ),
                    });
                }

                // The current WC3 thread has no impersonation token. On this
                // failure path, Windows leaves the caller's output word alone.
                self.set_last_error(ERROR_NO_TOKEN);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::OpenProcessToken => {
                let [_, process, desired_access, token_out] = arguments::<4>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenProcessToken",
                        detail: format!("non-current-process handle=0x{process:08x}"),
                    });
                }
                if desired_access != TOKEN_QUERY {
                    return Err(ProviderDispatchError::Frontier {
                        api: "OpenProcessToken",
                        detail: format!("unobserved access=0x{desired_access:08x}"),
                    });
                }
                let handle = self.next_token_handle;
                self.next_token_handle = self
                    .next_token_handle
                    .checked_add(1)
                    .ok_or("token handle overflow")?;
                self.token_handles.insert(
                    handle,
                    TokenHandle {
                        pid,
                        access: desired_access,
                    },
                );
                memory.write(token_out, &handle.to_le_bytes())?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetTokenInformation => {
                let [
                    _,
                    token,
                    information_class,
                    information,
                    information_len,
                    return_len,
                ] = arguments::<6>(memory, esp)?;
                let Some(token_handle) = self.token_handles.get(&token).copied() else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    return Ok(PersonalityAction::Return(0));
                };
                if token_handle.access & TOKEN_QUERY == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: format!("token without TOKEN_QUERY handle=0x{token:08x}"),
                    });
                }
                if information_class != TOKEN_GROUPS_CLASS {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: format!("unobserved information class={information_class}"),
                    });
                }
                if return_len == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetTokenInformation",
                        detail: "null ReturnLength".into(),
                    });
                }
                memory.write(return_len, &XP_TOKEN_GROUPS_REQUIRED.to_le_bytes())?;
                if information == 0 || information_len < XP_TOKEN_GROUPS_REQUIRED {
                    self.set_last_error(ERROR_INSUFFICIENT_BUFFER);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }

                let sid = information
                    .checked_add(12)
                    .ok_or("GetTokenInformation SID address overflow")?;
                memory.write(information, &1u32.to_le_bytes())?;
                memory.write(information + 4, &sid.to_le_bytes())?;
                memory.write(information + 8, &XP_TOKEN_GROUP_ATTRIBUTES.to_le_bytes())?;
                memory.write(sid, &XP_EVERYONE_SID)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::AllocateAndInitializeSid => {
                let result = self.allocate_and_initialize_sid(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::EqualSid => {
                let result = self.equal_sid(esp, memory)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(result))
            }
            ProviderOp::ReadProcessMemory => {
                let [_, process, source, destination, size, bytes_read] =
                    arguments::<6>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "ReadProcessMemory",
                        detail: format!("non-self-process handle=0x{process:08x}"),
                    });
                }
                let size = usize::try_from(size)
                    .map_err(|_| ProviderDispatchError::Fault("ReadProcessMemory size"))?;
                let mut bytes = vec![0; size];
                if memory.read(source, &mut bytes).is_err()
                    || memory.write(destination, &bytes).is_err()
                {
                    self.set_last_error(299);
                    return Ok(PersonalityAction::Return(0));
                }
                if bytes_read != 0 {
                    memory.write(bytes_read, &(size as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::WriteProcessMemory => {
                let [_, process, destination, source, size, bytes_written] =
                    arguments::<6>(memory, esp)?;
                if process != CURRENT_PROCESS_PSEUDO_HANDLE {
                    return Err(ProviderDispatchError::Frontier {
                        api: "WriteProcessMemory",
                        detail: format!("non-self-process handle=0x{process:08x}"),
                    });
                }
                let size = usize::try_from(size)
                    .map_err(|_| ProviderDispatchError::Fault("WriteProcessMemory size"))?;
                let mut bytes = vec![0; size];
                if memory.read(source, &mut bytes).is_err()
                    || memory.write(destination, &bytes).is_err()
                {
                    self.set_last_error(299);
                    return Ok(PersonalityAction::Return(0));
                }
                if bytes_written != 0 {
                    memory.write(bytes_written, &(size as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetLastError => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(self.last_error_for_thread(tid)))
            }
            ProviderOp::GetTickCount => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(monotonic_counter_millis()))
            }
            ProviderOp::Sleep => {
                // Compatibility-only scheduling hint: the coordinator yields
                // once, but does not advance or tie guest time to host time.
                let [_, _milliseconds] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                // Sleep is void. The provider convention uses harmless EAX.
                Ok(PersonalityAction::Return(0))
            }
            ProviderOp::DisableThreadLibraryCalls => {
                let [_, module] = arguments::<2>(memory, esp)?;
                Ok(PersonalityAction::Return(
                    self.disable_thread_library_calls(module)?,
                ))
            }
            ProviderOp::CreateFileA => {
                let [
                    _,
                    filename,
                    desired_access,
                    share_mode,
                    security_attributes,
                    creation_disposition,
                    flags_and_attributes,
                    template_file,
                ] = arguments::<8>(memory, esp)?;
                if filename == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: "null filename".into(),
                    });
                }
                let path = read_c_string(memory, filename, 1024)?;
                if is_war3_scratch_path(&path) {
                    if security_attributes != 0 || template_file != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "scratch security=0x{security_attributes:08x} \\
                                 template=0x{template_file:08x}"
                            ),
                        });
                    }
                    let canonical = canonical_file_path(&path);
                    let existing = self.scratch_paths.get(&canonical).copied();
                    let file_id = match creation_disposition {
                        CREATE_NEW => {
                            if existing.is_some() {
                                self.set_last_error(ERROR_FILE_EXISTS);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            }
                            self.create_scratch_file(canonical, flags_and_attributes)?
                        }
                        CREATE_ALWAYS => {
                            if let Some(id) = existing {
                                self.scratch_files
                                    .get_mut(&id)
                                    .ok_or("scratch file disappeared")?
                                    .bytes
                                    .clear();
                                self.set_last_error(ERROR_ALREADY_EXISTS);
                                id
                            } else {
                                self.set_last_error(0);
                                self.create_scratch_file(canonical, flags_and_attributes)?
                            }
                        }
                        OPEN_EXISTING => {
                            let Some(id) = existing else {
                                self.set_last_error(ERROR_FILE_NOT_FOUND);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            };
                            id
                        }
                        OPEN_ALWAYS => {
                            if let Some(id) = existing {
                                self.set_last_error(ERROR_ALREADY_EXISTS);
                                id
                            } else {
                                self.set_last_error(0);
                                self.create_scratch_file(canonical, flags_and_attributes)?
                            }
                        }
                        TRUNCATE_EXISTING => {
                            let Some(id) = existing else {
                                self.set_last_error(ERROR_FILE_NOT_FOUND);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            };
                            if desired_access & GENERIC_WRITE == 0 {
                                self.set_last_error(ERROR_ACCESS_DENIED);
                                return Ok(PersonalityAction::Return(u32::MAX));
                            }
                            self.scratch_files
                                .get_mut(&id)
                                .ok_or("scratch file disappeared")?
                                .bytes
                                .clear();
                            id
                        }
                        other => {
                            return Err(ProviderDispatchError::Frontier {
                                api: "CreateFileA",
                                detail: format!(
                                    "scratch disposition={other} access=0x{desired_access:08x} \\
                                     share=0x{share_mode:08x} flags=0x{flags_and_attributes:08x}"
                                ),
                            });
                        }
                    };
                    let handle = self.next_file_handle;
                    self.next_file_handle = self
                        .next_file_handle
                        .checked_add(1)
                        .ok_or("file handle overflow")?;
                    self.file_handles.insert(
                        handle,
                        FileHandle {
                            backing: FileBacking::Scratch(file_id),
                            cursor: 0,
                            access: desired_access,
                            share: share_mode,
                        },
                    );
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(handle));
                }
                if let Some(relative) = warcraft_drive_relative_path(&path)
                    && !is_war3_mpq_path(&path)
                    && !is_self_image_path(&path)
                {
                    if relative.contains('\\') {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!("nested Warcraft path={path:?}"),
                        });
                    }
                    if creation_disposition != OPEN_EXISTING {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "Warcraft file disposition=0x{creation_disposition:08x}"
                            ),
                        });
                    }
                    if desired_access & FILE_WRITE_ACCESS_MASK != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "Warcraft file write access=0x{desired_access:08x}"
                            ),
                        });
                    }
                    if security_attributes != 0 || template_file != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "Warcraft file security=0x{security_attributes:08x} \
                                 template=0x{template_file:08x}"
                            ),
                        });
                    }
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::OpenFile(OpenFileRequest {
                        key: ThreadKey { pid, tid },
                        path: relative,
                        desired_access,
                        share_mode,
                        security_attributes,
                        creation_disposition,
                        flags_and_attributes,
                        template_file,
                    }));
                }
                if is_war3_mpq_path(&path) {
                    if creation_disposition != OPEN_EXISTING {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!("War3.mpq disposition=0x{creation_disposition:08x}"),
                        });
                    }
                    if desired_access & FILE_WRITE_ACCESS_MASK != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!("War3.mpq write access=0x{desired_access:08x}"),
                        });
                    }
                    if security_attributes != 0 || template_file != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: format!(
                                "War3.mpq security=0x{security_attributes:08x} template=0x{template_file:08x}"
                            ),
                        });
                    }
                    if self.war3_mpq_bytes.is_none() {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateFileA",
                            detail: "War3.mpq resident backing unavailable".into(),
                        });
                    }
                    let handle = self.next_file_handle;
                    self.next_file_handle = self
                        .next_file_handle
                        .checked_add(1)
                        .ok_or("file handle overflow")?;
                    self.file_handles.insert(
                        handle,
                        FileHandle {
                            backing: FileBacking::War3Mpq,
                            cursor: 0,
                            access: desired_access,
                            share: share_mode,
                        },
                    );
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(handle));
                }
                if !is_self_image_path(&path) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "unmodeled path={path:?} filename=0x{filename:08x} \\
                             access=0x{desired_access:08x} share=0x{share_mode:08x} \\
                             security=0x{security_attributes:08x} \\
                             disposition=0x{creation_disposition:08x} \\
                             flags=0x{flags_and_attributes:08x} \\
                             template=0x{template_file:08x}"
                        ),
                    });
                }
                if creation_disposition != 3 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!("self-image disposition=0x{creation_disposition:08x}"),
                    });
                }
                if desired_access & FILE_WRITE_ACCESS_MASK != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "self-image write access=0x{desired_access:08x} \\
                             share=0x{share_mode:08x} flags=0x{flags_and_attributes:08x}"
                        ),
                    });
                }
                if security_attributes != 0 || template_file != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "CreateFileA",
                        detail: format!(
                            "self-image security=0x{security_attributes:08x} \\
                             template=0x{template_file:08x}"
                        ),
                    });
                }

                let handle = self.next_file_handle;
                self.next_file_handle = self
                    .next_file_handle
                    .checked_add(1)
                    .ok_or("file handle overflow")?;
                self.file_handles.insert(
                    handle,
                    FileHandle {
                        backing: FileBacking::SelfImage,
                        cursor: 0,
                        access: desired_access,
                        share: share_mode,
                    },
                );
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(handle))
            }
            ProviderOp::GetFileSize => {
                let [_, handle, high] = arguments::<3>(memory, esp)?;
                let file = self.file_handle(handle)?;
                let length = self.file_length(file, self_image_bytes)?;
                if high != 0 {
                    memory.write(high, &((length >> 32) as u32).to_le_bytes())?;
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(length as u32))
            }
            ProviderOp::SetFilePointer => {
                let [_, handle, distance_low, distance_high, move_method] =
                    arguments::<5>(memory, esp)?;
                let file = self.file_handle(handle)?;
                let high = if distance_high == 0 {
                    0i64
                } else {
                    i64::from(i32::from_le_bytes(
                        read_u32(memory, distance_high)?.to_le_bytes(),
                    ))
                };
                let distance =
                    (high << 32) + i64::from(i32::from_le_bytes(distance_low.to_le_bytes()));
                let base = match move_method {
                    FILE_BEGIN => 0,
                    FILE_CURRENT => file.cursor,
                    FILE_END => self.file_length(file, self_image_bytes)?,
                    _ => {
                        return Err(ProviderDispatchError::Frontier {
                            api: "SetFilePointer",
                            detail: format!("unmodeled move method={move_method}"),
                        });
                    }
                };
                let cursor = if distance >= 0 {
                    base.checked_add(distance as u64)
                } else {
                    base.checked_sub(distance.unsigned_abs())
                }
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "SetFilePointer",
                    detail: format!("cursor outside self image base={base} distance={distance}"),
                })?;
                if distance_high != 0 {
                    memory.write(distance_high, &((cursor >> 32) as u32).to_le_bytes())?;
                }
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("self image handle disappeared")?
                    .cursor = cursor;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(cursor as u32))
            }
            ProviderOp::ReadFile => {
                let [_, handle, output, requested, bytes_read, overlapped] =
                    arguments::<6>(memory, esp)?;
                if overlapped != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "ReadFile",
                        detail: format!("overlapped=0x{overlapped:08x}"),
                    });
                }
                let file = self.file_handle(handle)?;
                if file.access & GENERIC_READ == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                }
                let start = usize::try_from(file.cursor)
                    .map_err(|_| ProviderDispatchError::Fault("file cursor"))?;
                let requested = usize::try_from(requested)
                    .map_err(|_| ProviderDispatchError::Fault("ReadFile size"))?;
                let transferred = match file.backing {
                    FileBacking::SelfImage => {
                        let backing = Self::self_image_bytes(self_image_bytes)?;
                        let source_start = start.min(backing.len());
                        let transferred = if start > backing.len() {
                            0
                        } else {
                            requested.min(backing.len() - start)
                        };
                        memory.write(output, &backing[source_start..source_start + transferred])?;
                        transferred
                    }
                    FileBacking::War3Mpq => {
                        let backing = self.war3_mpq_bytes.as_deref().ok_or_else(|| {
                            ProviderDispatchError::Frontier {
                                api: "War3.mpq",
                                detail: "resident backing unavailable".into(),
                            }
                        })?;
                        let source_start = start.min(backing.len());
                        let transferred = if start > backing.len() {
                            0
                        } else {
                            requested.min(backing.len() - start)
                        };
                        memory.write(output, &backing[source_start..source_start + transferred])?;
                        transferred
                    }
                    FileBacking::TrueosFs(id) => {
                        let resident = self
                            .resident_files
                            .get(&id)
                            .ok_or(ProviderDispatchError::Fault("resident file disappeared"))?;
                        let source_start = start.min(resident.bytes.len());
                        let transferred = if start > resident.bytes.len() {
                            0
                        } else {
                            requested.min(resident.bytes.len() - start)
                        };
                        memory.write(
                            output,
                            &resident.bytes[source_start..source_start + transferred],
                        )?;
                        transferred
                    }
                    FileBacking::Scratch(id) => {
                        let scratch = self
                            .scratch_files
                            .get(&id)
                            .ok_or("scratch file disappeared")?;
                        let source_start = start.min(scratch.bytes.len());
                        let transferred = if start > scratch.bytes.len() {
                            0
                        } else {
                            requested.min(scratch.bytes.len() - start)
                        };
                        memory.write(
                            output,
                            &scratch.bytes[source_start..source_start + transferred],
                        )?;
                        transferred
                    }
                };
                if bytes_read != 0 {
                    memory.write(bytes_read, &(transferred as u32).to_le_bytes())?;
                }
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("self image handle disappeared")?
                    .cursor =
                    file.cursor
                        .checked_add(u64::try_from(transferred).map_err(|_| {
                            ProviderDispatchError::Fault("ReadFile transferred length")
                        })?)
                        .ok_or("self image cursor overflow")?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::WriteFile => {
                let [_, handle, source, requested, bytes_written, overlapped] =
                    arguments::<6>(memory, esp)?;
                if overlapped != 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "WriteFile",
                        detail: format!("overlapped=0x{overlapped:08x}"),
                    });
                }
                let file = self.file_handle(handle)?;
                if file.access & GENERIC_WRITE == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                }
                let FileBacking::Scratch(file_id) = file.backing else {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    return Ok(PersonalityAction::Return(0));
                };
                let requested = usize::try_from(requested)
                    .map_err(|_| ProviderDispatchError::Fault("WriteFile size"))?;
                let mut input = vec![0; requested];
                memory.read(source, &mut input)?;
                let start = usize::try_from(file.cursor)
                    .map_err(|_| ProviderDispatchError::Fault("WriteFile cursor"))?;
                let end = start
                    .checked_add(requested)
                    .ok_or(ProviderDispatchError::Fault("WriteFile extent"))?;
                let scratch = self
                    .scratch_files
                    .get_mut(&file_id)
                    .ok_or("scratch file disappeared")?;
                if scratch.bytes.len() < end {
                    scratch.bytes.resize(end, 0);
                }
                scratch.bytes[start..end].copy_from_slice(&input);
                self.file_handles
                    .get_mut(&handle)
                    .ok_or("file handle disappeared")?
                    .cursor = end as u64;
                if bytes_written != 0 {
                    memory.write(bytes_written, &(requested as u32).to_le_bytes())?;
                }
                self.set_last_error(0);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::FlushFileBuffers => {
                let [_, handle] = arguments::<2>(memory, esp)?;
                let Some(file) = self.file_handles.get(&handle).copied() else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                };
                if file.access & GENERIC_WRITE == 0 {
                    self.set_last_error(ERROR_ACCESS_DENIED);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }
                match file.backing {
                    // Scratch writes update the process-local byte vector
                    // synchronously, so there is no lower buffered layer.
                    FileBacking::Scratch(_) => {}
                    FileBacking::SelfImage | FileBacking::War3Mpq | FileBacking::TrueosFs(_) => {
                        self.set_last_error(ERROR_ACCESS_DENIED);
                        self.call_count = self
                            .call_count
                            .checked_add(1)
                            .ok_or("call count overflow")?;
                        return Ok(PersonalityAction::Return(0));
                    }
                }
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetWindowsDirectoryA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_windows_directory_a(esp, memory)?,
                ))
            }
            ProviderOp::GetSystemDirectoryA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_system_directory_a(esp, memory)?,
                ))
            }
            ProviderOp::GetTempPathA => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_temp_path_a(esp, memory)?,
                ))
            }
            ProviderOp::GetDriveTypeA => {
                let [_, root_ptr] = arguments::<2>(memory, esp)?;
                let root = (root_ptr != 0)
                    .then(|| read_c_string(memory, root_ptr, 1024))
                    .transpose()?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(xp_drive_type(root.as_deref())))
            }
            ProviderOp::GetVolumeInformationA => {
                let [
                    _,
                    root_ptr,
                    volume_name_ptr,
                    volume_name_cap,
                    serial_ptr,
                    max_component_ptr,
                    fs_flags_ptr,
                    fs_name_ptr,
                    fs_name_cap,
                ] = arguments::<9>(memory, esp)?;
                let root = (root_ptr != 0)
                    .then(|| read_c_string(memory, root_ptr, 1024))
                    .transpose()?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if !xp_volume_exists(root.as_deref()) {
                    self.set_last_error(ERROR_PATH_NOT_FOUND);
                    return Ok(PersonalityAction::Return(0));
                }
                write_optional_ansi(
                    memory,
                    volume_name_ptr,
                    volume_name_cap,
                    XP_C_VOLUME_NAME,
                    "GetVolumeInformationA volume name",
                )?;
                if serial_ptr != 0 {
                    write_u32(memory, serial_ptr, XP_C_VOLUME_SERIAL)?;
                }
                if max_component_ptr != 0 {
                    write_u32(memory, max_component_ptr, XP_C_MAX_COMPONENT)?;
                }
                if fs_flags_ptr != 0 {
                    write_u32(memory, fs_flags_ptr, XP_C_FS_FLAGS)?;
                }
                write_optional_ansi(
                    memory,
                    fs_name_ptr,
                    fs_name_cap,
                    XP_C_FILE_SYSTEM_NAME,
                    "GetVolumeInformationA filesystem name",
                )?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetDiskFreeSpaceA => {
                let [
                    _,
                    root_ptr,
                    sectors_per_cluster_ptr,
                    bytes_per_sector_ptr,
                    free_clusters_ptr,
                    total_clusters_ptr,
                ] = arguments::<6>(memory, esp)?;
                let root = (root_ptr != 0)
                    .then(|| read_c_string(memory, root_ptr, 1024))
                    .transpose()?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                let Some(geometry) = xp_disk_geometry(root.as_deref()) else {
                    self.set_last_error(ERROR_PATH_NOT_FOUND);
                    return Ok(PersonalityAction::Return(0));
                };
                if sectors_per_cluster_ptr == 0
                    || bytes_per_sector_ptr == 0
                    || free_clusters_ptr == 0
                    || total_clusters_ptr == 0
                {
                    return Err(ProviderDispatchError::Frontier {
                        api: "GetDiskFreeSpaceA",
                        detail: format!(
                            "null output sectors=0x{sectors_per_cluster_ptr:08x} bytes=0x{bytes_per_sector_ptr:08x} free=0x{free_clusters_ptr:08x} total=0x{total_clusters_ptr:08x}"
                        ),
                    });
                }
                write_u32(memory, sectors_per_cluster_ptr, geometry.sectors_per_cluster)?;
                write_u32(memory, bytes_per_sector_ptr, geometry.bytes_per_sector)?;
                write_u32(memory, free_clusters_ptr, geometry.free_clusters)?;
                write_u32(memory, total_clusters_ptr, geometry.total_clusters)?;
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::SetCurrentDirectoryA => {
                let [_, directory] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if directory == 0 || read_c_string(memory, directory, 1024)?.is_empty() {
                    self.set_last_error(ERROR_PATH_NOT_FOUND);
                    return Ok(PersonalityAction::Return(0));
                }
                // WC3's modeled file operations use their absolute virtual paths,
                // so this establishes the API result without introducing a second
                // relative-path resolver.
                self.set_last_error(0);
                Ok(PersonalityAction::Return(1))
            }
            ProviderOp::GetFileAttributesA => {
                let [_, filename] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if filename == 0 {
                    self.set_last_error(ERROR_FILE_NOT_FOUND);
                    return Ok(PersonalityAction::Return(INVALID_FILE_ATTRIBUTES));
                }
                let path = read_c_string(memory, filename, 1024)?;
                let attributes = if is_self_image_path(&path)
                    || (is_war3_mpq_path(&path) && self.war3_mpq_bytes.is_some())
                {
                    Some(FILE_ATTRIBUTE_NORMAL)
                } else {
                    self.scratch_paths
                        .get(&canonical_file_path(&path))
                        .and_then(|id| self.scratch_files.get(id))
                        .map(|file| file.attributes)
                };
                match attributes {
                    Some(attributes) => {
                        self.set_last_error(0);
                        Ok(PersonalityAction::Return(attributes))
                    }
                    None => {
                        self.set_last_error(ERROR_FILE_NOT_FOUND);
                        Ok(PersonalityAction::Return(INVALID_FILE_ATTRIBUTES))
                    }
                }
            }
            ProviderOp::SetFileAttributesA => {
                let [_, filename, attributes] = arguments::<3>(memory, esp)?;
                if filename == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "SetFileAttributesA",
                        detail: format!("filename=NULL attributes=0x{attributes:08x}"),
                    });
                }
                let path = read_c_string(memory, filename, 1024)?;
                let _temporary = attributes & FILE_ATTRIBUTE_TEMPORARY != 0;
                if let Some(id) = self.scratch_paths.get(&canonical_file_path(&path)).copied() {
                    self.scratch_files
                        .get_mut(&id)
                        .ok_or("scratch file disappeared")?
                        .attributes = attributes;
                    self.set_last_error(0);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(1));
                }
                if !self.path_exists(&path) {
                    self.set_last_error(ERROR_FILE_NOT_FOUND);
                    self.call_count = self
                        .call_count
                        .checked_add(1)
                        .ok_or("call count overflow")?;
                    return Ok(PersonalityAction::Return(0));
                }
                Err(ProviderDispatchError::Frontier {
                    api: "SetFileAttributesA",
                    detail: format!("existing path={path:?} attributes=0x{attributes:08x}"),
                })
            }
            ProviderOp::FindFirstFileA => {
                let [_, pattern, find_data] = arguments::<3>(memory, esp)?;
                if pattern == 0 || find_data == 0 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "FindFirstFileA",
                        detail: format!("null pattern=0x{pattern:08x} find_data=0x{find_data:08x}"),
                    });
                }
                let pattern = read_c_string(memory, pattern, 1024)?;
                if !is_self_image_path(&pattern) {
                    return Err(ProviderDispatchError::Frontier {
                        api: "FindFirstFileA",
                        detail: format!(
                            "unmodeled pattern={pattern:?} find_data=0x{find_data:08x}"
                        ),
                    });
                }
                let image_size = self_image_bytes.map(|bytes| bytes.len() as u64).ok_or(
                    ProviderDispatchError::Fault("self image file backing unavailable"),
                )?;
                let mut data = [0u8; 320];
                data[0..4].copy_from_slice(&FILE_ATTRIBUTE_NORMAL.to_le_bytes());
                data[20..24].copy_from_slice(&((image_size >> 32) as u32).to_le_bytes());
                data[24..28].copy_from_slice(&(image_size as u32).to_le_bytes());
                data[44..53].copy_from_slice(b"War3.exe\0");
                memory.write(find_data, &data)?;
                let handle = self.next_find_handle;
                self.next_find_handle = self
                    .next_find_handle
                    .checked_add(1)
                    .ok_or("find handle overflow")?;
                self.find_handles.insert(handle);
                self.set_last_error(0);
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(handle))
            }
            ProviderOp::FindClose => {
                let [_, handle] = arguments::<2>(memory, esp)?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                if self.find_handles.remove(&handle) {
                    self.set_last_error(0);
                    Ok(PersonalityAction::Return(1))
                } else {
                    self.set_last_error(ERROR_INVALID_HANDLE);
                    Ok(PersonalityAction::Return(0))
                }
            }
            ProviderOp::QueryPerformanceFrequency => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.query_performance_frequency(esp, memory)?,
                ))
            }
            ProviderOp::QueryPerformanceCounter => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.query_performance_counter(esp, memory)?,
                ))
            }
            ProviderOp::GetLocalTime | ProviderOp::GetSystemTime => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.write_current_system_time(esp, memory)?,
                ))
            }
            ProviderOp::GetTimeZoneInformation => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.get_time_zone_information(esp, memory)?,
                ))
            }
            ProviderOp::TimeGetTime => {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(monotonic_counter_millis()))
            }
            _ => Err(ProviderDispatchError::Unsupported),
        }
    }

    pub fn dispatch_provider_for_process_typed(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        self.dispatch_provider_for_process_typed_with_self_image(
            pid,
            tid,
            provider_id,
            esp,
            memory,
            None,
        )
    }

    pub fn dispatch_provider_for_process_typed_with_self_image(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        self.with_active_last_error_tid(tid, |process| {
            process.dispatch_provider_for_process_typed_with_self_image_active(
                pid,
                tid,
                provider_id,
                esp,
                memory,
                self_image_bytes,
            )
        })
    }

    fn dispatch_provider_for_process_typed_with_self_image_active(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
        self_image_bytes: Option<&[u8]>,
    ) -> Result<PersonalityAction, ProviderDispatchError> {
        let provider = self
            .provider_import(provider_id)
            .cloned()
            .ok_or("unknown child provider import")?;
        let operation = provider_op(&provider);
        if operation.is_generic_process_local() {
            return self.dispatch_process_local_provider(
                pid,
                tid,
                operation,
                esp,
                memory,
                self_image_bytes,
            );
        }
        let action = match operation {
            ProviderOp::CreateEventA => Some(PersonalityAction::Session(
                SessionRequest::CreateEvent(self.create_event_request(esp, memory)?),
            )),
            ProviderOp::SetEvent => Some(PersonalityAction::Session(SessionRequest::SetEvent {
                pid,
                tid,
                handle: arguments::<2>(memory, esp)?[1],
            })),
            ProviderOp::ResetEvent => Some(PersonalityAction::Session(SessionRequest::ResetEvent {
                pid,
                tid,
                handle: arguments::<2>(memory, esp)?[1],
            })),
            ProviderOp::CreateMutexA => {
                let [_, attributes, initial_owner, name] = arguments::<4>(memory, esp)?;
                let inheritable = if attributes == 0 {
                    false
                } else {
                    let [length, descriptor, inherit] = arguments::<3>(memory, attributes)?;
                    if length != 12 {
                        self.set_last_error(87);
                        return Ok(PersonalityAction::Return(0));
                    }
                    if descriptor != 0 {
                        return Err(ProviderDispatchError::Frontier {
                            api: "CreateMutexA",
                            detail: "non-default security descriptor".into(),
                        });
                    }
                    inherit != 0
                };
                Some(PersonalityAction::Session(SessionRequest::CreateMutex {
                    key: ThreadKey { pid, tid },
                    request: CreateMutexRequest {
                        name: if name == 0 {
                            None
                        } else {
                            Some(read_c_string(memory, name, 260)?)
                        },
                        initial_owner: initial_owner != 0,
                        inheritable,
                    },
                }))
            }
            ProviderOp::ReleaseMutex => {
                Some(PersonalityAction::Session(SessionRequest::ReleaseMutex {
                    key: ThreadKey { pid, tid },
                    handle: arguments::<2>(memory, esp)?[1],
                }))
            }
            ProviderOp::CloseHandle => {
                let handle = arguments::<2>(memory, esp)?[1];
                if self.token_handles.remove(&handle).is_some()
                    || self.file_handles.remove(&handle).is_some()
                {
                    Some(PersonalityAction::Return(1))
                } else {
                    Some(PersonalityAction::Session(SessionRequest::CloseHandle {
                        pid,
                        handle,
                    }))
                }
            }
            ProviderOp::WaitForSingleObject => {
                let [return_address, handle, timeout] = arguments::<3>(memory, esp)?;
                Some(PersonalityAction::Block(WaitRequest {
                    key: ThreadKey { pid, tid },
                    return_address,
                    count: 1,
                    handles_pointer: 0,
                    handles: [handle, 0],
                    wait_all: 0,
                    timeout,
                }))
            }
            ProviderOp::WaitForMultipleObjects => {
                let frame = self.wait_for_multiple_objects(esp, memory)?;
                if frame.count > 2 {
                    return Err(ProviderDispatchError::Frontier {
                        api: "WaitForMultipleObjects",
                        detail: format!(
                            "count={} wait_all={} timeout=0x{:08x}",
                            frame.count, frame.wait_all, frame.timeout
                        ),
                    });
                }
                Some(PersonalityAction::Block(WaitRequest {
                    key: ThreadKey { pid, tid },
                    return_address: frame.return_address,
                    count: frame.count,
                    handles_pointer: frame.handles_pointer,
                    handles: frame.handles,
                    wait_all: frame.wait_all,
                    timeout: frame.timeout,
                }))
            }
            _ => None,
        };
        if let Some(action) = action {
            self.call_count = self
                .call_count
                .checked_add(1)
                .ok_or("call count overflow")?;
            return Ok(action);
        }
        match (&provider.module[..], &provider.symbol) {
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "GetEnvironmentStringsW" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(ENVIRONMENT_BLOCK_VA))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "GetCommandLineA" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(PROCESS_DATA_VA))
            }
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
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "WideCharToMultiByte" =>
            {
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.wide_to_multi_byte(esp, memory)?,
                ))
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
                if module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "ExitProcess" =>
            {
                let exit_code = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::ExitProcess(exit_code))
            }
            (module, ProviderSymbol::Name(symbol))
                if module.eq_ignore_ascii_case("KERNEL32.dll")
                    && symbol == "SetUnhandledExceptionFilter" =>
            {
                let filter = read_u32(
                    memory,
                    esp.checked_add(4).ok_or("provider argument overflow")?,
                )?;
                self.call_count = self
                    .call_count
                    .checked_add(1)
                    .ok_or("call count overflow")?;
                Ok(PersonalityAction::Return(
                    self.set_unhandled_exception_filter(filter),
                ))
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
            _ => Err(ProviderDispatchError::Unsupported),
        }
    }

    pub fn dispatch_provider_for_process(
        &mut self,
        pid: u32,
        tid: u32,
        provider_id: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<PersonalityAction, &'static str> {
        self.dispatch_provider_for_process_typed(pid, tid, provider_id, esp, memory)
            .map_err(|error| match error {
                ProviderDispatchError::Unsupported => "unsupported child provider import",
                ProviderDispatchError::Fault(error) => error,
                ProviderDispatchError::Frontier { .. } => "child provider frontier",
            })
    }

}
