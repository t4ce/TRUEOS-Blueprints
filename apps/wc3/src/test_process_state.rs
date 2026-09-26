
    fn write_u16(memory: &mut impl GuestMemory, address: u32, value: u16) -> Result<(), &'static str> {
        memory.write(address, &value.to_le_bytes())
    }
    struct Memory {
        base: u32,
        bytes: Vec<u8>,
    }
    impl GuestMemory for Memory {
        fn read(&self, a: u32, o: &mut [u8]) -> Result<(), &'static str> {
            let n = (a - self.base) as usize;
            o.copy_from_slice(self.bytes.get(n..n + o.len()).ok_or("read")?);
            Ok(())
        }
        fn write(&mut self, a: u32, i: &[u8]) -> Result<(), &'static str> {
            let n = (a - self.base) as usize;
            self.bytes
                .get_mut(n..n + i.len())
                .ok_or("write")?
                .copy_from_slice(i);
            Ok(())
        }
    }

    struct ProcessMemory {
        stack: Vec<u8>,
        process_data: Vec<u8>,
    }

    impl GuestMemory for ProcessMemory {
        fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
            let end = address.checked_add(output.len() as u32).ok_or("read")?;
            let (base, bytes) = if address >= STACK_BASE && end <= STACK_TOP {
                (STACK_BASE, &self.stack)
            } else if address >= PROCESS_DATA_VA && end <= PROCESS_DATA_VA + 0x1000 {
                (PROCESS_DATA_VA, &self.process_data)
            } else {
                return Err("read");
            };
            let offset = usize::try_from(address - base).map_err(|_| "read")?;
            output.copy_from_slice(bytes.get(offset..offset + output.len()).ok_or("read")?);
            Ok(())
        }

        fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
            let end = address.checked_add(input.len() as u32).ok_or("write")?;
            let (base, bytes) = if address >= STACK_BASE && end <= STACK_TOP {
                (STACK_BASE, &mut self.stack)
            } else if address >= PROCESS_DATA_VA && end <= PROCESS_DATA_VA + 0x1000 {
                (PROCESS_DATA_VA, &mut self.process_data)
            } else {
                return Err("write");
            };
            let offset = usize::try_from(address - base).map_err(|_| "write")?;
            bytes
                .get_mut(offset..offset + input.len())
                .ok_or("write")?
                .copy_from_slice(input);
            Ok(())
        }
    }
    #[test]
    fn system_font_metric_matches_draw_text_measurement_string() {
        let text = b"Copyright \xa9 2002 Blizzard Entertainment. All Rights Reserved.";
        assert_eq!(text.len(), 61);
        let width = text
            .iter()
            .map(|byte| system_font_advance_cp1252(*byte).unwrap())
            .sum::<u32>();
        assert_eq!(width, 406);
    }

    #[test]
    fn child_format_message_a_resolves_user_default_and_keeps_explicit_language_exact() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("FormatMessageA".into()),
            iat_rva: 0,
        };
        assert_eq!(provider_op(&provider), ProviderOp::FormatMessageA);
        assert!(provider_op(&provider).is_generic_process_local());
        assert_eq!(provider_op(&provider).stack_cleanup_bytes(), 28);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(28));

        let base = 0x0010_0000;
        let module = base;
        let mut memory = Memory { base, bytes: vec![0; 0x10_000] };
        // PE optional header's resource data directory.
        write_u32(&mut memory, module + 0x3c, 0x80).unwrap();
        write_u32(&mut memory, module + 0x80 + 24 + 96 + 16, 0x1000).unwrap();
        write_u32(&mut memory, module + 0x80 + 24 + 96 + 20, 0x1000).unwrap();
        let root = module + 0x1000;
        write_u16(&mut memory, root + 14, 1).unwrap();
        write_u32(&mut memory, root + 16, RT_MESSAGETABLE).unwrap();
        write_u32(&mut memory, root + 20, 0x8000_0020).unwrap();
        let kind = root + 0x20;
        write_u16(&mut memory, kind + 14, 1).unwrap();
        write_u32(&mut memory, kind + 16, 7).unwrap();
        write_u32(&mut memory, kind + 20, 0x8000_0040).unwrap();
        let name = root + 0x40;
        write_u16(&mut memory, name + 14, 1).unwrap();
        // Storm contains a real en-US resource.  The caller requests the
        // symbolic LANG_USER_DEFAULT, which XP resolves to en-US.
        write_u32(&mut memory, name + 16, 0x0409).unwrap();
        write_u32(&mut memory, name + 20, 0x90).unwrap();
        write_u32(&mut memory, root + 0x90, 0x2000).unwrap();
        write_u32(&mut memory, root + 0x94, 26).unwrap();
        let data = module + 0x2000;
        write_u32(&mut memory, data, 1).unwrap();
        write_u32(&mut memory, data + 4, 0x8510_0084).unwrap();
        write_u32(&mut memory, data + 8, 0x8510_0084).unwrap();
        write_u32(&mut memory, data + 12, 16).unwrap();
        write_u16(&mut memory, data + 16, 10).unwrap();
        write_u16(&mut memory, data + 18, 0).unwrap();
        memory.write(data + 20, b"OK%n\0").unwrap();

        let esp = base + 0x8000;
        let buffer = base + 0x8100;
        for (index, value) in [
            0x0040_1c67,
            FORMAT_MESSAGE_FROM_HMODULE,
            module,
            0x8510_0084,
            0x0400,
            buffer,
            32,
            0,
        ].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(4))
        );
        let mut output = [0; 5];
        memory.read(buffer, &mut output).unwrap();
        assert_eq!(output, *b"OK\r\n\0");
        assert_eq!(xp.last_format_message_encoding(), Some(MessageResourceEncoding::Ansi));

        // A genuine explicit language must not silently use en-US.
        write_u32(&mut memory, esp + 4 * 4, 0x0407).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.last_error(), ERROR_RESOURCE_LANG_NOT_FOUND);
    }

    #[test]
    fn child_format_message_a_from_system_uses_xp_text_and_preserves_last_error() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("FormatMessageA".into()),
            iat_rva: 0,
        };
        let base = 0x0010_0000;
        let mut memory = Memory { base, bytes: vec![0; 0x1000] };
        let esp = base + 0x400;
        let buffer = base + 0x500;
        for (index, value) in [
            0x0040_1c67,
            FORMAT_MESSAGE_FROM_SYSTEM,
            0,
            ERROR_FILE_NOT_FOUND,
            0,
            buffer,
            128,
            0,
        ].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        xp.set_last_error_for_thread(3, ERROR_FILE_NOT_FOUND);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(44))
        );
        let mut output = [0; 45];
        memory.read(buffer, &mut output).unwrap();
        assert_eq!(output, *b"The system cannot find the file specified.\r\n\0");
        assert_eq!(xp.last_error_for_thread(3), ERROR_FILE_NOT_FOUND);
        assert_eq!(xp.last_format_message_encoding(), Some(MessageResourceEncoding::Ansi));
    }

    #[test]
    fn child_peek_message_a_is_user32_process_local_and_empty_queue_returns_false() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("PeekMessageA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::PeekMessageA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 20);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(20));

        let base = 0x0010_0000;
        let mut memory = Memory { base, bytes: vec![0; 0x1000] };
        let esp = base + 0x400;
        for (index, value) in [
            0x0040_1c67,
            base + 0x500,
            0,
            0,
            0,
            1,
        ].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
    }

    #[test]
    fn child_peek_message_a_observes_and_removes_a_queued_window_paint() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("PeekMessageA".into()),
            iat_rva: 0,
        };
        let base = 0x0010_0000;
        let output = base + 0x500;
        let esp = base + 0x400;
        let mut memory = Memory { base, bytes: vec![0; 0x1000] };
        for (index, value) in [0x0040_1c67, output, 0, 0, 0, 0]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        xp.queue_window_paint(0x5743_4003);

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(read_u32(&memory, output).unwrap(), 0x5743_4003);
        assert_eq!(read_u32(&memory, output + 4).unwrap(), 0x000f);

        write_u32(&mut memory, esp + 5 * 4, 1).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
    }

    #[test]
    fn child_get_message_a_retrieves_but_does_not_remove_window_paint() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("GetMessageA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetMessageA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(16));

        let base = 0x0010_0000;
        let output = base + 0x500;
        let esp = base + 0x400;
        let mut memory = Memory { base, bytes: vec![0; 0x1000] };
        for (index, value) in [0x0040_1c67, output, 0, 0, 0]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        xp.queue_window_paint(0x5743_4003);

        for _ in 0..2 {
            assert_eq!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(1))
            );
            assert_eq!(read_u32(&memory, output).unwrap(), 0x5743_4003);
            assert_eq!(read_u32(&memory, output + 4).unwrap(), 0x000f);
        }
    }

    #[test]
    fn proven_create_thread_is_logical_and_suspended() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "CreateThread".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [
            0x0040_2039,
            0,
            0x2000,
            0x0040_2072,
            0x0021_0560,
            4,
            0x0021_0560,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(THREAD_HANDLE_BASE)
        );
        assert_eq!(read_u32(&memory, 0x0021_0560).unwrap(), 2);
        assert_eq!(xp.threads[0].suspend_count, 1);
    }

    #[test]
    fn get_exit_code_process_delegates_handle_lifecycle_to_the_session() {
        let mut xp = XpProcess::new(vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "GetExitCodeProcess".into(),
            iat_rva: 0,
        }]);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_2200, 0x5743_6001, 0x0021_0560]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Session(SessionRequest::GetExitCodeProcess(
                GetExitCodeProcessRequest {
                    pid: 1,
                    tid: 1,
                    handle: 0x5743_6001,
                    exit_code_pointer: 0x0021_0560,
                },
            )),
        );
    }

    #[test]
    fn exit_thread_is_a_non_returning_launcher_action() {
        let mut xp = XpProcess::new(vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "ExitThread".into(),
            iat_rva: 0,
        }]);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_2300).unwrap();
        write_u32(&mut memory, esp + 4, 0).unwrap();

        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::ExitThread(0)
        );
    }

    #[test]
    fn tls_get_value_isolated_by_thread_for_the_same_slot() {
        let mut xp = XpProcess::new(Vec::new());
        let slot = xp.tls_alloc().unwrap();
        xp.tls_values.insert((1, slot), 0x1111_1111);
        xp.tls_values.insert((2, slot), 0x2222_2222);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_3f45).unwrap();
        write_u32(&mut memory, esp + 4, slot).unwrap();

        assert_eq!(xp.tls_get_value(1, esp, &memory), Ok(0x1111_1111));
        assert_eq!(xp.tls_get_value(2, esp, &memory), Ok(0x2222_2222));
        assert_eq!(xp.last_error, 0);
    }

    #[test]
    fn tls_get_value_returns_zero_and_clears_last_error_for_an_untouched_slot() {
        let mut xp = XpProcess::new(Vec::new());
        let slot = xp.tls_alloc().unwrap();
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_3f45).unwrap();
        write_u32(&mut memory, esp + 4, slot).unwrap();
        xp.last_error = 0x1234_5678;

        assert_eq!(xp.tls_get_value(2, esp, &memory), Ok(0));
        assert_eq!(xp.last_error, 0);
    }

    #[test]
    fn tls_get_value_rejects_an_out_of_range_slot() {
        let mut xp = XpProcess::new(Vec::new());
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_3f45).unwrap();
        write_u32(&mut memory, esp + 4, 64).unwrap();

        assert_eq!(xp.tls_get_value(2, esp, &memory), Ok(0));
        assert_eq!(xp.last_error, 87);
    }

    #[test]
    fn launcher_set_last_error_updates_the_existing_process_value() {
        let mut xp = XpProcess::new(vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "SetLastError".into(),
            iat_rva: 0,
        }]);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_3f91).unwrap();
        write_u32(&mut memory, esp + 4, 0).unwrap();
        xp.set_last_error(0xdead_beef);

        assert_eq!(xp.dispatch(1, 0, esp, &mut memory), Ok(PersonalityAction::Return(0)));
        assert_eq!(xp.last_error, 0);
    }

    #[test]
    fn load_image_resolves_numeric_bitmap_resource_without_fixed_rva() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "USER32.dll".into(),
            symbol: "LoadImageA".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let base = pe32::IMAGE_BASE;
        let mut memory = Memory {
            base,
            bytes: vec![0; 0x6000],
        };
        write_u32(&mut memory, base + 0x3c, 0x80).unwrap();
        write_u32(&mut memory, base + 0x80 + 24 + 96 + 16, 0x1000).unwrap();
        write_u32(&mut memory, base + 0x80 + 24 + 96 + 20, 0x4000).unwrap();
        // RT_BITMAP (2) -> resource ID 106 -> language 1033 -> data entry.
        write_u16(&mut memory, base + 0x1000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x1000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x1000 + 16, 2).unwrap();
        write_u32(&mut memory, base + 0x1000 + 20, 0x8000_1000).unwrap();
        write_u16(&mut memory, base + 0x2000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x2000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x2000 + 16, 106).unwrap();
        write_u32(&mut memory, base + 0x2000 + 20, 0x8000_2000).unwrap();
        write_u16(&mut memory, base + 0x3000 + 12, 0).unwrap();
        write_u16(&mut memory, base + 0x3000 + 14, 1).unwrap();
        write_u32(&mut memory, base + 0x3000 + 16, 1033).unwrap();
        write_u32(&mut memory, base + 0x3000 + 20, 0x3000).unwrap();
        write_u32(&mut memory, base + 0x4000, 0x4500).unwrap();
        write_u32(&mut memory, base + 0x4004, 44).unwrap();
        write_u32(&mut memory, base + 0x4500, 40).unwrap();
        write_u32(&mut memory, base + 0x4504, 1).unwrap();
        write_u32(&mut memory, base + 0x4508, 1).unwrap();
        write_u16(&mut memory, base + 0x450c, 1).unwrap();
        write_u16(&mut memory, base + 0x450e, 24).unwrap();
        write_u32(&mut memory, base + 0x4514, 4).unwrap();
        memory.bytes[0x4500 + 40..0x4500 + 44].copy_from_slice(&[0, 0, 255, 0]);
        let esp = base + 0x5000;
        for (index, value) in [0x0040_1501, base, 106, 0, 0, 0, 0x2000]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let PersonalityAction::Session(SessionRequest::LoadImage(request)) =
            xp.dispatch(2, 0, esp, &mut memory).unwrap()
        else {
            panic!("LoadImageA did not produce a decode request");
        };
        assert_eq!(request.resource_id, 106);
        assert_eq!(request.width, 1);
        assert_eq!(request.height, 1);
        assert_eq!(request.bit_count, 24);
        let bmp = bmp_file_from_dib(&request.dib).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(&bmp[14..], request.dib.as_slice());
        let layout = dib_layout(&request.dib).unwrap();
        let handle = xp
            .admit_bitmap(request, vec![255, 0, 0, 255], 0x0500_0000, layout)
            .unwrap();
        let info = xp.bitmap_info(handle).unwrap();
        assert_eq!(handle, GDI_HANDLE_BASE);
        assert_eq!(info.resource_id, 106);
        assert_eq!(info.dib_bytes, 44);
        assert_eq!(info.bits_len, 4);
        assert_eq!(info.rgba, vec![255, 0, 0, 255]);
    }

    #[test]
    fn select_object_returns_previous_bitmap_and_rejects_duplicate_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SelectObject".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        xp.gdi_objects.insert(
            GDI_HANDLE_BASE,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 500,
                height: 400,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 200_000,
                clr_used: 256,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: Vec::new(),
                bits_va: 0x0500_0000,
                bits_len: 200_000,
                row_stride: 500,
                pixel_offset: 1064,
                stock: false,
            }),
        );
        let hdc0 = GDI_HANDLE_BASE + 1;
        let hdc1 = GDI_HANDLE_BASE + 2;
        xp.gdi_objects.insert(
            hdc0,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            hdc1,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_156f, hdc0, GDI_HANDLE_BASE].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_MONO_BITMAP)
        );
        assert_eq!(
            xp.compatible_dc_info(hdc0),
            Some((GDI_HANDLE_BASE, 1, 1, 1))
        );
        for (index, value) in [0x0040_156f, hdc1, GDI_HANDLE_BASE].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        assert_eq!(
            xp.compatible_dc_info(hdc1),
            Some((STOCK_MONO_BITMAP, 1, 1, 1))
        );
    }

    #[test]
    fn create_compatible_dc_resolves_memory_and_window_paint_sources() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "CreateCompatibleDC".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let paint = GDI_HANDLE_BASE + 3;
        xp.gdi_objects.insert(
            paint,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd: 0x5743_4002 },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0).unwrap();
        write_u32(&mut memory, esp + 4, paint).unwrap();
        let PersonalityAction::Return(result) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreateCompatibleDC did not return");
        };
        assert_ne!(result, paint);
        assert_eq!(xp.dc_target(result), Some(None));
        assert_eq!(xp.selected_bitmap(result), Some(STOCK_MONO_BITMAP));
        assert!(xp.gdi_live(paint));

        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        assert!(!xp.gdi_live(0x5743_7003));
    }

    #[test]
    fn get_dib_color_table_reads_live_rgbquad_ranges() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "GetDIBColorTable".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let hdc = GDI_HANDLE_BASE + 1;
        let palette = vec![[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]];
        xp.gdi_objects.insert(
            bitmap,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 1,
                height: 1,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 4,
                clr_used: 3,
                dib: Vec::new(),
                decoded_rgba: vec![3, 2, 1, 255],
                palette,
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 52,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_1587, hdc, 1, 4, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(2)
        );
        let mut output = [0; 8];
        memory.read(0x0021_0600, &mut output).unwrap();
        assert_eq!(&output, &[5, 6, 7, 8, 9, 10, 11, 12]);
        for (index, value) in [0x0040_1587, hdc, 9, 1, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        for (index, value) in [0x0040_1587, hdc, 0, 0, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        for (index, value) in [0x0040_1587, STOCK_MONO_BITMAP, 0, 1, 0x0021_0600]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }
    #[test]
    fn select_palette_swaps_per_dc_state_and_protects_selected_palette() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SelectPalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let paint = GDI_HANDLE_BASE;
        let memory_dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
        for (handle, target) in [
            (paint, DcTarget::WindowPaint { hwnd: 0x5743_4002 }),
            (
                memory_dc,
                DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
            ),
        ] {
            xp.gdi_objects.insert(
                handle,
                GdiObject::DeviceContext(DeviceContext {
                    compatible_with: DcCompatibility::Display,
                    target,
                    selected_palette: STOCK_DEFAULT_PALETTE,
                    palette_force_background: false,
                    realized_palette: None,
                    text_color: 0,
                    bk_color: 0x00ff_ffff,
                    bk_mode: OPAQUE,
                }),
            );
        }
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 10,
                    green: 20,
                    blue: 30,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [0x0040_16e8, paint, palette, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_DEFAULT_PALETTE)
        );
        assert_eq!(xp.selected_palette(paint), Some(palette));
        assert_eq!(xp.selected_palette(memory_dc), Some(STOCK_DEFAULT_PALETTE));
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(palette)
        );
        write_u32(&mut memory, esp + 4, memory_dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(STOCK_DEFAULT_PALETTE)
        );
        assert_eq!(xp.selected_palette(memory_dc), Some(palette));
        write_u32(&mut memory, esp + 4, paint).unwrap();
        write_u32(&mut memory, esp + 8, STOCK_DEFAULT_PALETTE).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(palette)
        );
        write_u32(&mut memory, esp + 4, STOCK_DEFAULT_PALETTE).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn realize_palette_is_first_use_only_and_is_invalidated_by_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "RealizePalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let palette = GDI_HANDLE_BASE + 1;
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd: 0x5743_4002 },
                selected_palette: palette,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_16f0).unwrap();
        write_u32(&mut memory, esp + 4, hdc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(xp.realized_palette(hdc), Some(Some(palette)));
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.realized_palette(hdc), Some(Some(palette)));
    }

    #[test]
    fn create_palette_preserves_guest_palette_entries_and_bounds_reads() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "CreatePalette".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let pointer = 0x0021_05e0;
        xp.allocations.insert(pointer, 12);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        write_u16(&mut memory, pointer, 0x0300).unwrap();
        write_u16(&mut memory, pointer + 2, 2).unwrap();
        memory
            .write(pointer + 4, &[10, 20, 30, 0, 40, 50, 60, 1])
            .unwrap();
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_15cb).unwrap();
        write_u32(&mut memory, esp + 4, pointer).unwrap();
        let PersonalityAction::Return(handle) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreatePalette did not return a handle");
        };
        assert_ne!(handle, 0);
        assert_eq!(xp.palette_info(handle), Some((0x0300, 2)));

        write_u16(&mut memory, pointer + 2, 0).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        write_u16(&mut memory, pointer + 2, 3).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
        write_u16(&mut memory, pointer + 2, 257).unwrap();
        assert!(xp.dispatch(1, 0, esp, &mut memory).is_err());
    }

    #[test]
    fn image_mapping_precedes_overlapping_stack() {
        let image = Mapping {
            address: 0x0040_0000,
            bytes: {
                let mut b = vec![0; 0x3000];
                b[0x2072..0x2076].copy_from_slice(&[0x55, 0x8b, 0xec, 0x6a]);
                b
            },
            executable: true,
        };
        let stack = Mapping {
            address: 0x0040_0000,
            bytes: vec![0; 0x10000],
            executable: false,
        };
        assert_eq!(
            &diagnostic_read(0x0040_2072, &image, &stack).unwrap()[..4],
            &[0x55, 0x8b, 0xec, 0x6a]
        );
    }

    #[test]
    fn delete_dc_only_removes_device_context_and_releases_selection() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "DeleteDC".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
        xp.gdi_objects.insert(
            bitmap,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 1,
                height: 1,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 4,
                clr_used: 1,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: vec![[0, 0, 0, 0]],
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 44,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            dc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_15ec).unwrap();
        write_u32(&mut memory, esp + 4, dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(dc));
        assert!(xp.gdi_live(bitmap));
        assert!(xp.gdi_live(palette));

        let dc2 = GDI_HANDLE_BASE + 3;
        xp.gdi_objects.insert(
            dc2,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: STOCK_MONO_BITMAP,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        write_u32(&mut memory, esp, 0x0040_156f).unwrap();
        write_u32(&mut memory, esp + 4, dc2).unwrap();
        write_u32(&mut memory, esp + 8, bitmap).unwrap();
        assert_eq!(xp.select_object(esp, &memory).unwrap(), STOCK_MONO_BITMAP);

        write_u32(&mut memory, esp + 4, bitmap).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        write_u32(&mut memory, esp + 4, palette).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn delete_object_obeys_gdi_lifetime_and_selection_rules() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "DeleteObject".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let bitmap = GDI_HANDLE_BASE;
        let dc = GDI_HANDLE_BASE + 1;
        let palette = GDI_HANDLE_BASE + 2;
        xp.gdi_objects.insert(
            bitmap,
            GdiObject::Bitmap(BitmapObject {
                resource_id: 106,
                width: 1,
                height: 1,
                planes: 1,
                bit_count: 8,
                compression: 0,
                size_image: 4,
                clr_used: 1,
                dib: Vec::new(),
                decoded_rgba: vec![0, 0, 0, 255],
                palette: vec![[0, 0, 0, 0]],
                bits_va: 0x0500_0000,
                bits_len: 4,
                row_stride: 4,
                pixel_offset: 44,
                stock: false,
            }),
        );
        xp.gdi_objects.insert(
            dc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::Memory {
                    selected_bitmap: bitmap,
                },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        xp.gdi_objects.insert(
            palette,
            GdiObject::Palette(PaletteObject {
                version: 0x0300,
                entries: vec![PaletteEntry {
                    red: 1,
                    green: 2,
                    blue: 3,
                    flags: 0,
                }],
                stock: false,
            }),
        );
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        write_u32(&mut memory, esp, 0x0040_188a).unwrap();
        write_u32(&mut memory, esp + 4, bitmap).unwrap();

        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(bitmap));

        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        xp.gdi_objects.remove(&dc);
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(bitmap));

        write_u32(&mut memory, esp + 4, palette).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert!(!xp.gdi_live(palette));

        write_u32(&mut memory, esp + 4, dc).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(STOCK_MONO_BITMAP));

        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn historical_launcher_layout_and_create_process_frontier_match() {
        assert_eq!(STACK_BASE, 0x0430_0000);
        assert_eq!(STACK_TOP, 0x0440_0000);
        assert_eq!(COMMAND_LINE, b"\"Warcraft III.exe\"\0");
        assert_eq!(
            CHILD_COMMAND_LINE,
            b"\"war3.exe\" -opengl -nosound -swtnl\0"
        );
        assert_eq!(crate::session::WINDOW_HANDLE_BASE, 0x5743_4001);

        // This is the hardware-observed #89 frame.  Its positions are a
        // consequence of the restored historical stack, not special cases in
        // the dispatch implementation.
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f8b0;
        let command_line = 0x043f_fb20;
        let startup_info = 0x043f_fc24;
        let process_information = 0x043f_f900;
        memory.write(command_line, b"\"war3.exe\" \0").unwrap();
        for (index, value) in [
            0x0040_12e0,
            0,
            command_line,
            0,
            0,
            1,
            0,
            0,
            0,
            startup_info,
            process_information,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        let frame = XpProcess::new(Vec::new())
            .create_process_a(esp, &memory)
            .unwrap();
        assert_eq!(
            frame,
            CreateProcessAFrame {
                return_address: 0x0040_12e0,
                application_name: 0,
                command_line,
                process_attributes: 0,
                thread_attributes: 0,
                inherit_handles: 1,
                creation_flags: 0,
                environment: 0,
                current_directory: 0,
                startup_info,
                process_information,
            }
        );
    }

    #[test]
    fn session_allocates_first_and_second_windows_and_presents_only_visible_one() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "USER32.dll".into(),
                symbol: "RegisterClassA".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "USER32.dll".into(),
                symbol: "CreateWindowExA".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: pe32::IMAGE_BASE,
            bytes: vec![0; 0x10_000],
        };
        let class = pe32::IMAGE_BASE + 0x80a8;
        let title = pe32::IMAGE_BASE + 0x8080;
        let wndclass = pe32::IMAGE_BASE + 0x8100;
        memory.write(class, b"Warcraft III\0").unwrap();
        memory.write(title, b"Launching Warcraft III\0").unwrap();
        for (offset, value) in [0, 0x0040_1630, 0, 0, pe32::IMAGE_BASE, 0, 0, 0, 0, class]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, wndclass + offset as u32 * 4, value).unwrap();
        }
        let register_esp = pe32::IMAGE_BASE + 0x9000;
        write_u32(&mut memory, register_esp, 0).unwrap();
        write_u32(&mut memory, register_esp + 4, wndclass).unwrap();
        assert!(matches!(
            xp.dispatch(1, 0, register_esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        ));
        let esp = pe32::IMAGE_BASE + 0x9000;
        for (index, value) in [
            0x0040_1946,
            0,
            class,
            title,
            0,
            1030,
            520,
            500,
            400,
            DESKTOP_HWND,
            0,
            pe32::IMAGE_BASE,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let PersonalityAction::Session(SessionRequest::CreateWindow(request)) =
            xp.dispatch(1, 1, esp, &mut memory).unwrap()
        else {
            panic!("missing create window request")
        };
        let mut session = crate::session::Wc3Session::new(XpProcess::new(Vec::new()));
        let first = session
            .create_window(crate::session::CreateWindowRequest {
                owner: ThreadKey { pid: 1, tid: 1 },
                style: 0x8000_0000,
                ex_style: 0,
                ..request.clone()
            })
            .unwrap();
        assert_eq!(first, 0x5743_4001);
        let second = session
            .create_window(crate::session::CreateWindowRequest {
                owner: ThreadKey { pid: 1, tid: 2 },
                ..request
            })
            .unwrap();
        assert_eq!(second, 0x5743_4002);
        assert!(!session.windows[&second].visible);
        assert_eq!(session.show_window(second, 1).unwrap(), 0);
        assert_eq!(
            session.take_window_presentation(),
            Some(crate::session::WindowPresentation::Show {
                hwnd: second,
                x: 1030,
                y: 520,
                width: 500,
                height: 400
            })
        );
    }

    #[test]
    fn wait_for_multiple_objects_frontier_captures_both_handles() {
        let xp = XpProcess::new(Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        let handles_pointer = 0x043f_f900;
        let handles = [0x5743_2001, 0x5743_5001];
        for (index, value) in [0x0040_1362, 2, handles_pointer, 0, u32::MAX]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        write_u32(&mut memory, handles_pointer, handles[0]).unwrap();
        write_u32(&mut memory, handles_pointer + 4, handles[1]).unwrap();

        assert_eq!(
            xp.wait_for_multiple_objects(esp, &memory).unwrap(),
            WaitForMultipleObjectsFrame {
                return_address: 0x0040_1362,
                count: 2,
                handles_pointer,
                handles,
                wait_all: 0,
                timeout: u32::MAX,
            }
        );
    }

    #[test]
    fn set_text_color_swaps_color_without_changing_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetTextColor".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let hwnd = 0x5743_4002;
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_17aa, hdc, 0x0000_c8f0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(xp.text_color(hdc), Some(0));
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0000_c8f0));
        assert_eq!(xp.dc_target(hdc), Some(Some(hwnd)));
        assert_eq!(xp.selected_palette(hdc), Some(STOCK_DEFAULT_PALETTE));
        assert_eq!(xp.realized_palette(hdc), Some(None));

        write_u32(&mut memory, esp + 8, 0x0012_3456).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0x0000_c8f0)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0012_3456));

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(u32::MAX)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0012_3456));
    }

    #[test]
    fn set_bk_color_swaps_default_white_without_changing_other_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetBkColor".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let hdc = GDI_HANDLE_BASE;
        let hwnd = 0x5743_4002;
        xp.gdi_objects.insert(
            hdc,
            GdiObject::DeviceContext(DeviceContext {
                compatible_with: DcCompatibility::Display,
                target: DcTarget::WindowPaint { hwnd },
                selected_palette: STOCK_DEFAULT_PALETTE,
                palette_force_background: false,
                realized_palette: None,
                text_color: 0x0000_c8f0,
                bk_color: 0x00ff_ffff,
                bk_mode: OPAQUE,
            }),
        );
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_17b3, hdc, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0x00ff_ffff)
        );
        let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&hdc) else {
            panic!("device context disappeared")
        };
        assert_eq!(dc.bk_color, 0);
        assert_eq!(dc.text_color, 0x0000_c8f0);
        assert_eq!(dc.selected_palette, STOCK_DEFAULT_PALETTE);
        assert!(matches!(dc.target, DcTarget::WindowPaint { hwnd: value } if value == hwnd));

        write_u32(&mut memory, esp + 8, 0x0012_3456).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            xp.gdi_objects.get(&hdc).and_then(|value| match value {
                GdiObject::DeviceContext(dc) => Some(dc.bk_color),
                _ => None,
            }),
            Some(0x0012_3456)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(u32::MAX)
        );
        assert_eq!(xp.text_color(hdc), Some(0x0000_c8f0));
    }

    #[test]
    fn set_bk_mode_uses_opaque_defaults_and_preserves_dc_state() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "GDI32.dll".into(),
            symbol: "SetBkMode".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        for (index, value) in [0x0040_1561, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let memory_hdc = xp.create_compatible_dc(esp, &memory).unwrap();
        let paint_struct = 0x043f_f500;
        let paint_hdc = xp
            .begin_paint(0x5743_4002, paint_struct, 500, 400, &mut memory)
            .unwrap();

        for hdc in [memory_hdc, paint_hdc] {
            let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&hdc) else {
                panic!("missing device context")
            };
            assert_eq!(dc.bk_mode, OPAQUE);
            assert_eq!(dc.text_color, 0);
            assert_eq!(dc.bk_color, 0x00ff_ffff);
        }

        write_u32(&mut memory, esp, 0x0040_17bc).unwrap();
        write_u32(&mut memory, esp + 4, paint_hdc).unwrap();
        write_u32(&mut memory, esp + 8, TRANSPARENT).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(OPAQUE)
        );
        let Some(GdiObject::DeviceContext(dc)) = xp.gdi_objects.get(&paint_hdc) else {
            panic!("missing paint device context")
        };
        assert_eq!(dc.bk_mode, TRANSPARENT);
        assert_eq!(dc.text_color, 0);
        assert_eq!(dc.bk_color, 0x00ff_ffff);
        assert_eq!(dc.selected_palette, STOCK_DEFAULT_PALETTE);
        assert!(matches!(dc.target, DcTarget::WindowPaint { hwnd } if hwnd == 0x5743_4002));

        write_u32(&mut memory, esp + 8, OPAQUE).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(TRANSPARENT)
        );
        write_u32(&mut memory, esp + 8, 3).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            xp.gdi_objects
                .get(&paint_hdc)
                .and_then(|value| match value {
                    GdiObject::DeviceContext(dc) => Some(dc.bk_mode),
                    _ => None,
                }),
            Some(OPAQUE)
        );

        write_u32(&mut memory, esp + 4, STOCK_MONO_BITMAP).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
    }

    #[test]
    fn end_paint_retires_only_matching_active_window_paint() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "USER32.dll".into(),
                symbol: "EndPaint".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "GDI32.dll".into(),
                symbol: "DeleteDC".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = 0x043f_f700;
        let ps = 0x043f_f500;
        let hwnd = 0x5743_4002;
        let hdc = xp.begin_paint(hwnd, ps, 500, 400, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, ps).unwrap(), hdc);
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        for (index, value) in [0x0040_1833, hwnd + 1, ps].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        write_u32(&mut memory, esp + 4, hwnd).unwrap();
        write_u32(&mut memory, ps, 0).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(xp.active_paint_count(), 1);
        assert!(xp.gdi_live(hdc));

        write_u32(&mut memory, ps, hdc).unwrap();
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(xp.active_paint_count(), 0);
        assert!(!xp.gdi_live(hdc));
        assert_eq!(
            xp.dispatch(2, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );

        let ps2 = ps + 0x100;
        let hdc2 = xp.begin_paint(hwnd, ps2, 500, 400, &mut memory).unwrap();
        write_u32(&mut memory, esp, 0x0040_15ec).unwrap();
        write_u32(&mut memory, esp + 4, hdc2).unwrap();
        assert_eq!(
            xp.dispatch(2, 1, esp, &mut memory).unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(xp.gdi_live(hdc2));
        assert_eq!(xp.active_paint_count(), 1);
    }

    #[test]
    fn resume_thread_transfers_created_thread_to_blueprint_lifecycle() {
        let imports = vec![
            LauncherImport {
                id: 0,
                module: "KERNEL32.dll".into(),
                symbol: "CreateThread".into(),
                iat_rva: 0,
            },
            LauncherImport {
                id: 1,
                module: "KERNEL32.dll".into(),
                symbol: "ResumeThread".into(),
                iat_rva: 4,
            },
        ];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: 0x0021_0000,
            bytes: vec![0; 0x1000],
        };
        let esp = 0x0021_0800;
        for (index, value) in [
            0x0040_2039,
            0,
            0x2000,
            0x0040_2072,
            0x0021_0560,
            4,
            0x0021_0560,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        let PersonalityAction::Return(handle) = xp.dispatch(1, 0, esp, &mut memory).unwrap() else {
            panic!("CreateThread unexpectedly reached a frontier");
        };
        write_u32(&mut memory, esp, 0x0040_0000).unwrap();
        write_u32(&mut memory, esp + 4, handle).unwrap();
        assert_eq!(
            xp.dispatch(1, 1, esp, &mut memory).unwrap(),
            PersonalityAction::Return(1)
        );
        let runnable = xp.take_runnable_thread().unwrap();
        assert_eq!(runnable.tid, 2);
        assert_eq!(runnable.suspend_count, 0);
        xp.exit_thread(runnable.tid, 0x1234).unwrap();
        assert_eq!(xp.threads[0].exit_code, Some(0x1234));
    }

    #[test]
    fn child_initialize_critical_section_is_process_private() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
            iat_rva: 0,
        };
        let pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let critical_section = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x1501_fbd3).unwrap();
        write_u32(&mut memory, esp + 4, critical_section).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert!(pid2.critical_sections.contains_key(&critical_section));
        assert!(!pid1.critical_sections.contains_key(&critical_section));
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), u32::MAX);
    }

    #[test]
    fn child_enter_critical_section_dispatch_is_recursive_and_process_private() {
        let initialize = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
            iat_rva: 0,
        };
        let enter = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("EnterCriticalSection".into()),
            iat_rva: 0,
        };
        let leave = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("LeaveCriticalSection".into()),
            iat_rva: 0,
        };
        let pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![initialize, enter, leave], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let critical_section = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x1502_2867).unwrap();
        write_u32(&mut memory, esp + 4, critical_section).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 1, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 1, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 2);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        let before_wrong_owner = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 4, 2, esp, &mut memory),
            Err("critical section owner")
        );
        assert_eq!(memory.bytes, before_wrong_owner);
        pid2.dispatch_provider_for_process(2, 3, 2, esp, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 1);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 3);
        pid2.dispatch_provider_for_process(2, 3, 2, esp, &mut memory).unwrap();
        assert_eq!(read_u32(&memory, critical_section + 4).unwrap(), u32::MAX);
        assert_eq!(read_u32(&memory, critical_section + 8).unwrap(), 0);
        assert_eq!(read_u32(&memory, critical_section + 12).unwrap(), 0);
        assert!(!pid1.has_critical_section(critical_section));
        assert!(pid2.has_critical_section(critical_section));
    }

    #[test]
    fn child_set_last_error_returns_void_without_guest_memory_mutation() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetLastError".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1502_e393).unwrap();
        write_u32(&mut memory, esp + 4, 0x1234_5678).unwrap();
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0)
        );
        assert_eq!(pid2.last_error, 0x1234_5678);
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_set_last_error_is_process_private() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetLastError".into()),
            iat_rva: 0,
        };
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid1.set_last_error(0xfeed_face);
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1502_e393).unwrap();
        write_u32(&mut memory, esp + 4, 0x1234_5678).unwrap();
        pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
            .unwrap();
        assert_eq!(pid1.last_error, 0xfeed_face);
        assert_eq!(pid2.last_error, 0x1234_5678);
    }

    #[test]
    fn child_last_error_and_tls_get_value_are_isolated_by_guest_thread() {
        let imports = ["SetLastError", "GetLastError", "TlsGetValue"]
            .into_iter()
            .map(|symbol| ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            })
            .collect();
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(imports, Vec::new(), Vec::new());
        let slot = xp.tls_alloc().unwrap();
        xp.tls_values.insert((4, slot), 0x4444_0000);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_1234).unwrap();

        // Each SetLastError provider call updates only its calling guest TID.
        write_u32(&mut memory, esp + 4, 0x3333_0000).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        write_u32(&mut memory, esp + 4, 0x4444_0000).unwrap();
        xp.dispatch_provider_for_process_typed(2, 4, 0, esp, &mut memory)
            .unwrap();

        // TID 4's successful TlsGetValue clears only TID 4's LastError.
        write_u32(&mut memory, esp + 4, slot).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 4, 2, esp, &mut memory),
            Ok(PersonalityAction::Return(0x4444_0000))
        );

        assert_eq!(xp.last_error_for_thread(3), 0x3333_0000);
        assert_eq!(xp.last_error_for_thread(4), 0);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(0x3333_0000))
        );
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 4, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
    }

    #[test]
    fn child_crt_malloc_is_aligned_and_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let first = pid2.crt_malloc(0x80).unwrap().unwrap();
        assert_eq!(first.pointer, CHILD_CRT_HEAP_BASE);
        assert_eq!(first.end, CHILD_CRT_HEAP_BASE + 0x80);
        let second = pid2.crt_malloc(1).unwrap().unwrap();
        assert_eq!(second.pointer, CHILD_CRT_HEAP_BASE + 0x80);
        assert_eq!(second.end, CHILD_CRT_HEAP_BASE + 0x88);
        assert_eq!(
            pid1.crt_malloc(0x80).unwrap().unwrap().pointer,
            CHILD_CRT_HEAP_BASE
        );
    }

    #[test]
    fn guest_word_frames_use_one_read_and_reject_overflow_before_access() {
        struct FrameMemory(std::cell::Cell<usize>);
        impl GuestMemory for FrameMemory {
            fn read(&self, address: u32, out: &mut [u8]) -> Result<(), &'static str> {
                self.0.set(self.0.get() + 1);
                if address != 0x1000 || out.len() > 16 { return Err("unmapped frame"); }
                for (i, byte) in out.iter_mut().enumerate() { *byte = i as u8; }
                Ok(())
            }
            fn write(&mut self, _: u32, _: &[u8]) -> Result<(), &'static str> { unreachable!() }
        }
        let memory = FrameMemory(std::cell::Cell::new(0));
        assert_eq!(read_guest_words(&memory, 0x1000, 4).unwrap(), vec![0x03020100, 0x07060504, 0x0b0a0908, 0x0f0e0d0c]);
        assert_eq!(memory.0.get(), 1);
        assert!(read_guest_words(&memory, u32::MAX - 2, 1).is_err());
        assert!(read_guest_words(&memory, 0, usize::MAX).is_err());
        assert!(read_guest_words(&memory, u32::MAX, 0).unwrap().is_empty());
        assert_eq!(memory.0.get(), 1);
        assert!(read_guest_words(&memory, 0x1000, 5).is_err());
        assert_eq!(memory.0.get(), 2);
        assert_eq!(arguments::<4>(&memory, 0x1000).unwrap(), [0x03020100, 0x07060504, 0x0b0a0908, 0x0f0e0d0c]);
        assert_eq!(memory.0.get(), 3);
        assert!(arguments::<1>(&memory, u32::MAX - 2).is_err());
        assert_eq!(arguments::<0>(&memory, u32::MAX).unwrap(), []);
        assert_eq!(memory.0.get(), 3);
    }

    #[test]
    fn callback_table_growth_is_linear_and_preserves_old_ownership() {
        let mut process = XpProcess::new(Vec::new());
        let mut pointer = process.crt_malloc(4).unwrap().unwrap().pointer;
        let mut copied = 0;
        let mut moves = 0;
        for entries in 0..5000u32 {
            let used = entries * 4;
            if used + 4 > process.crt_allocation_capacity(pointer).unwrap() {
                let old_capacity = process.crt_allocation_capacity(pointer);
                let growth = process.crt_grow_callback_table(pointer, used, used + 4).unwrap().unwrap();
                assert_eq!(growth.used_bytes, used);
                assert!(growth.required_bytes >= used + 4);
                assert_eq!(process.crt_allocation_capacity(pointer), old_capacity);
                assert!(process.retire_crt_allocation(pointer));
                pointer = growth.pointer;
                copied += used;
                moves += 1;
            }
        }
        assert!(copied < 5000 * 4 * 2, "copied {copied} bytes");
        assert!(moves <= 10, "moved {moves} times");
    }

    #[test]
    fn callback_table_growth_falls_back_to_exact_size_under_pressure() {
        let mut process = XpProcess::new(Vec::new());
        let old = process.crt_malloc(8).unwrap().unwrap();
        let remaining = CHILD_CRT_HEAP_LIMIT - CHILD_CRT_HEAP_BASE - 8 - 16;
        process.crt_malloc(remaining).unwrap().unwrap();
        let growth = process.crt_grow_callback_table(old.pointer, 8, 12).unwrap().unwrap();
        assert_eq!(growth.required_bytes, 12);
        assert_eq!(growth.used_bytes, 8);
        assert_eq!(process.crt_allocation_capacity(old.pointer), Some(8));
        assert_eq!(process.crt_allocation_capacity(growth.pointer), Some(16));
        assert!(process.crt_malloc(1).unwrap().is_none());
    }

    #[test]
    fn child_crt_resize_preserves_old_ownership_until_commit() {
        let mut process = XpProcess::new(Vec::new());
        let old = process.crt_malloc(5).unwrap().unwrap();
        assert_eq!(process.crt_allocation_capacity(old.pointer), Some(8));
        let resize = process.crt_resize(old.pointer, 8, 12).unwrap().unwrap();
        assert_ne!(resize.pointer, old.pointer);
        assert_eq!(resize.used_bytes, 8);
        assert_eq!(resize.required_bytes, 12);
        assert!(resize.moved);
        assert_eq!(process.crt_allocation_capacity(old.pointer), Some(8));
        assert_eq!(process.crt_allocation_capacity(resize.pointer), Some(16));
        assert!(process.retire_crt_allocation(old.pointer));
        assert_eq!(process.crt_allocation_capacity(old.pointer), None);
        assert_eq!(process.crt_allocation_capacity(resize.pointer), Some(16));
    }

    #[test]
    fn child_virtual_reservations_are_granular_and_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let first = pid2.virtual_reserve_null(0x10000).unwrap().unwrap();
        assert_eq!(first.base, CHILD_VIRTUAL_ALLOC_BASE);
        assert_eq!(first.size, XP_ALLOCATION_GRANULARITY);
        assert_eq!(
            pid2.virtual_reservation_state(),
            (1, CHILD_VIRTUAL_ALLOC_BASE + XP_ALLOCATION_GRANULARITY)
        );
        let one_byte = pid2.virtual_reserve_null(1).unwrap().unwrap();
        assert_eq!(one_byte.size, XP_ALLOCATION_GRANULARITY);
        let rounded = pid2.virtual_reserve_null(0x10001).unwrap().unwrap();
        assert_eq!(rounded.size, 0x20000);
        assert_eq!(
            pid2.virtual_reservation_containing(rounded.base + 0x10000, 0x10000),
            Some(&rounded)
        );
        assert_eq!(
            pid1.virtual_reserve_null(1).unwrap().unwrap().base,
            CHILD_VIRTUAL_ALLOC_BASE
        );
        assert!(pid1.virtual_reservation_at(one_byte.base).is_none());
        assert_eq!(pid2.virtual_reserve_null(u32::MAX), Err("VirtualAlloc size overflow"));
    }

    #[test]
    fn child_virtual_commit_is_page_granular_and_rejects_overlap() {
        let mut process = XpProcess::new(Vec::new());
        let reservation = process.virtual_reserve_null(0x10000).unwrap().unwrap();
        let request = process
            .virtual_prepare_commit(reservation.base, 0x1000)
            .unwrap()
            .unwrap();
        assert_eq!(request.reservation_base, reservation.base);
        assert_eq!(request.reservation_size, 0x10000);
        assert_eq!(request.size, XP_PAGE_SIZE);
        assert_eq!(process.virtual_reservation_at(reservation.base).unwrap().committed, []);
        process.virtual_finish_commit(request).unwrap();
        assert_eq!(process.virtual_commit_state(), (1, XP_PAGE_SIZE));
        assert_eq!(
            process.virtual_prepare_commit(reservation.base, XP_PAGE_SIZE),
            Err("VirtualAlloc overlapping commit")
        );
        assert_eq!(
            process.virtual_prepare_commit(reservation.base + 0xf000, 0x1001),
            Ok(None)
        );
    }

    #[test]
    fn child_virtual_release_retires_only_an_exact_zero_size_reservation() {
        let mut process = XpProcess::new(Vec::new());
        let reservation = process.virtual_reserve_null(0x10000).unwrap().unwrap();
        let first = process
            .virtual_prepare_commit(reservation.base, XP_PAGE_SIZE)
            .unwrap()
            .unwrap();
        process.virtual_finish_commit(first).unwrap();
        let second = process
            .virtual_prepare_commit(reservation.base + XP_PAGE_SIZE, XP_PAGE_SIZE)
            .unwrap()
            .unwrap();
        process.virtual_finish_commit(second).unwrap();
        let reserve_next = process.virtual_reservation_state().1;

        assert!(process
            .virtual_prepare_release(reservation.base, XP_PAGE_SIZE)
            .is_none());
        let prepared = process.virtual_prepare_release(reservation.base, 0).unwrap();
        assert_eq!(prepared.committed.len(), 2);
        assert_eq!(prepared.committed.iter().map(|commit| commit.size).sum::<u32>(), 0x2000);
        process
            .virtual_finish_release(prepared.base, prepared.size)
            .unwrap();

        assert!(process.virtual_reservation_at(reservation.base).is_none());
        assert_eq!(process.virtual_commit_state(), (0, 0));
        assert_eq!(process.virtual_reservation_state(), (0, reserve_next));
    }

    #[test]
    fn child_virtual_null_commit_page_rounds_but_advances_cursor_to_64k() {
        let mut process = XpProcess::new(Vec::new());
        process.virtual_reserve_null(0x80000).unwrap().unwrap();
        assert_eq!(process.virtual_reservation_state().1, 0x0608_0000);

        let request = process.virtual_prepare_null_commit(0x80010).unwrap().unwrap();
        assert_eq!(request.base, 0x0608_0000);
        assert_eq!(request.size, 0x0008_1000);
        assert_eq!(request.next_reserve, 0x0611_0000);
        process.virtual_finish_null_commit(request).unwrap();

        assert_eq!(process.virtual_reservation_at(request.base).unwrap().size, request.size);
        assert_eq!(process.virtual_commit_state(), (1, request.size));
        assert_eq!(process.virtual_reservation_state().1, request.next_reserve);
    }
