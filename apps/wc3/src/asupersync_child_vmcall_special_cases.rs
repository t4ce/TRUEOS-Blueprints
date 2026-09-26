                    {
                    if provider.module.eq_ignore_ascii_case("OPENGL32.dll") {
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD OPENGL CALL pid={} tid={} during=\"{}\" \
                                 provider_id={} symbol={:?} caller_ret=0x{:08x} \
                                 cleanup={}-by-thunk",
                                active_pid,
                                active_tid,
                                running_module_name,
                                provider_id,
                                provider.symbol,
                                u32::from_le_bytes(caller_ret),
                                child_loader::provider_op(&provider).stack_cleanup_bytes(),
                            ),
                        );
                    }
                    if cfg!(feature = "trace-api") && matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("MSVCRT.dll")
                                && name == "strtol"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let input = if frame[1] == 0 {
                            None
                        } else {
                            Some(wc3::process::read_c_string(
                                &X86Memory(&child.address_space),
                                frame[1],
                                256,
                            )?)
                        };
                        logl::trace!(
                            "trace-api",
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD CRT STRTOL CALL pid={} tid={} \\
                                 input_ptr=0x{:08x} input={:?} end_ptr=0x{:08x} \\
                                 base={} caller_ret=0x{:08x} cleanup=0-by-thunk",
                                active_pid,
                                active_tid,
                                frame[1],
                                input,
                                frame[2],
                                frame[3] as i32,
                                frame[0],
                            ),
                        );
                    }
                    if matches!(
                        &provider.symbol,
                        child_loader::ProviderSymbol::Name(name)
                            if provider.module.eq_ignore_ascii_case("KERNEL32.dll")
                                && name == "OpenEventA"
                    ) {
                        let frame = read_guest_words(
                            &X86Memory(&child.address_space),
                            exit.registers.esp,
                            4,
                        )?;
                        let name = if frame[3] == 0 {
                            None
                        } else {
                            Some(
                                wc3::process::read_c_string(
                                    &X86Memory(&child.address_space),
                                    frame[3],
                                    256,
                                )
                                .map_err(|error| format!("OpenEventA name: {error}"))?,
                            )
                        };
                        logl::log(
                            level::IMPORTANT,
                            format_args!(
                                "WC3 CHILD OPENEVENTA CALL pid={} tid={} desired_access=0x{:08x} inherit_handle=0x{:08x} inheritable={} name_ptr=0x{:08x} name={:?} caller_ret=0x{:08x}",
                                active_pid,
                                active_tid,
                                frame[1],
                                frame[2],
                                (frame[2] != 0) as u8,
                                frame[3],
                                name,
                                frame[0],
                            ),
                        );
                    }
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                        let diagnostic_caller_ret = if cfg!(feature = "trace-api") {
                            Some(read_guest_words(
                                &X86Memory(&child.address_space),
                                exit.registers.esp,
                                1,
                            )?[0])
                        } else {
                            None
                        };
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
                        if let Some(caller_ret) = diagnostic_caller_ret {
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
                        }
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                            logl::trace!(
                                "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                        logl::trace!(
                            "trace-api",
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
                    }
