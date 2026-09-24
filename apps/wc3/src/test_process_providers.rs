    #[test]
    fn appended_provider_updates_the_existing_thunk_page_tail() {
        let mut xp = XpProcess::new(Vec::new());
        let existing = (0..337)
            .map(|index| ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(format!("existing_{index}")),
                iat_rva: 0,
            })
            .collect::<Vec<_>>();
        xp.install_provider_surface(existing, vec![0x90; 0x1000], Vec::new());
        let malloc = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("malloc".into()),
            iat_rva: 0,
        };
        let (addresses, old_bytes, new_bytes, updated_from, updated) =
            xp.append_provider_imports(vec![malloc]).unwrap();
        assert_eq!(addresses, vec![thunk32::address(337).unwrap()]);
        assert_eq!(old_bytes, 0x1000);
        assert_eq!(new_bytes, 0x1000);
        assert_eq!(updated_from, 337 * thunk32::THUNK_BYTES);
        assert_eq!(
            &updated[..9],
            &[0xb8, 0x51, 0x01, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]
        );
    }

    #[test]
    fn provider_export_lookup_hides_unmodeled_static_traps() {
        let modeled = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentProcess".into()),
            iat_rva: 0,
        };
        let unmodeled = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("DefinitelyUnmodeled".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(
            vec![modeled.clone(), unmodeled.clone()],
            vec![0x90; THUNK_PAGE_BYTES],
            Vec::new(),
        );
        let modeled_address = xp
            .provider_thunk_address("kernel32.dll", &modeled.symbol)
            .unwrap();
        assert_eq!(
            xp.provider_export_address("KERNEL32.dll", &modeled.symbol),
            Some(modeled_address)
        );
        assert_eq!(
            xp.provider_thunk_address("KERNEL32.dll", &unmodeled.symbol),
            Some(thunk32::address(1).unwrap())
        );
        assert_eq!(
            xp.provider_export_address("KERNEL32.dll", &unmodeled.symbol),
            None
        );
    }

    #[test]
    fn child_malloc_provider_returns_guest_pointer_without_memory_mutation() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("malloc".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1503_62e0).unwrap();
        write_u32(&mut memory, esp + 4, 0x80).unwrap();
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(CHILD_CRT_HEAP_BASE)
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_crt_set_app_type_is_cdecl_and_records_process_state() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("__set_app_type".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtSetAppType);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(crate::child_loader::provider_thunk_kind(&provider), thunk32::Kind::Return);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_1c67).unwrap();
        write_u32(&mut memory, esp + 4, CRT_GUI_APP).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.crt_app_type, CRT_GUI_APP);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_p_fmode_is_cdecl_and_returns_writable_process_slot() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("__p__fmode".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtGetFmode);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(crate::child_loader::provider_thunk_kind(&provider), thunk32::Kind::Return);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut stack = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut stack, esp, 0x0040_1c67).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut stack),
            Ok(PersonalityAction::Return(CRT_FMODE_VA))
        );

        let mut process_data = Memory {
            base: PROCESS_DATA_VA,
            bytes: vec![0; 0x1000],
        };
        write_u32(&mut process_data, CRT_FMODE_VA, 0x4000).unwrap();
        assert_eq!(read_u32(&process_data, CRT_FMODE_VA).unwrap(), 0x4000);
    }

    #[test]
    fn child_crt_p_commode_is_cdecl_and_returns_writable_process_slot() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("__p__commode".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtGetCommode);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut stack = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut stack, esp, 0x0040_1c8a).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut stack),
            Ok(PersonalityAction::Return(CRT_COMMODE_VA))
        );

        let mut process_data = Memory {
            base: PROCESS_DATA_VA,
            bytes: vec![0; 0x1000],
        };
        write_u32(&mut process_data, CRT_COMMODE_VA, 1).unwrap();
        assert_eq!(read_u32(&process_data, CRT_COMMODE_VA).unwrap(), 1);
    }

    #[test]
    fn child_crt_getmainargs_populates_fixed_process_outputs() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("__getmainargs".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtGetMainArgs);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let argc_out = esp + 0x20;
        let argv_out = esp + 0x24;
        let env_out = esp + 0x28;
        write_u32(&mut memory, esp, 0x0040_1cef).unwrap();
        write_u32(&mut memory, esp + 4, argc_out).unwrap();
        write_u32(&mut memory, esp + 8, argv_out).unwrap();
        write_u32(&mut memory, esp + 12, env_out).unwrap();
        write_u32(&mut memory, esp + 16, 0).unwrap();
        write_u32(&mut memory, esp + 20, 0).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(read_u32(&memory, argc_out).unwrap(), CRT_ARGC);
        assert_eq!(read_u32(&memory, argv_out).unwrap(), CRT_ARGV_VA);
        assert_eq!(read_u32(&memory, env_out).unwrap(), CRT_ENVP_VA);
    }

    #[test]
    fn child_crt_xcpt_filter_continues_search_for_access_violation() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_XcptFilter".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtXcptFilter);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let exception_pointers = esp + 0x20;
        let record = esp + 0x28;
        let context = esp + 0x30;
        write_u32(&mut memory, esp, 0x0040_1d0b).unwrap();
        write_u32(&mut memory, esp + 4, crate::seh::STATUS_ACCESS_VIOLATION).unwrap();
        write_u32(&mut memory, esp + 8, exception_pointers).unwrap();
        write_u32(&mut memory, exception_pointers, record).unwrap();
        write_u32(&mut memory, exception_pointers + 4, context).unwrap();
        write_u32(&mut memory, record, crate::seh::STATUS_ACCESS_VIOLATION).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(EXCEPTION_FILTER_CONTINUE_SEARCH))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_xcpt_filter_continues_search_for_illegal_instruction() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_XcptFilter".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let exception_pointers = esp + 0x20;
        let record = esp + 0x28;
        let context = esp + 0x30;
        write_u32(&mut memory, esp, 0x0040_1d72).unwrap();
        write_u32(
            &mut memory,
            esp + 4,
            crate::seh::STATUS_ILLEGAL_INSTRUCTION,
        )
        .unwrap();
        write_u32(&mut memory, esp + 8, exception_pointers).unwrap();
        write_u32(&mut memory, exception_pointers, record).unwrap();
        write_u32(&mut memory, exception_pointers + 4, context).unwrap();
        write_u32(&mut memory, record, crate::seh::STATUS_ILLEGAL_INSTRUCTION).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(EXCEPTION_FILTER_CONTINUE_SEARCH))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_onexit_registers_callback_in_order_and_is_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_onexit".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtOnExit);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let func = 0x0040_1200;
        write_u32(&mut memory, esp, 0x0040_1c09).unwrap();
        write_u32(&mut memory, esp + 4, func).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(func))
        );
        assert_eq!(xp.crt_onexit_callbacks(), &[func]);
        assert_eq!(xp.crt_onexit_count(), 1);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_strrchr_returns_the_last_match_and_is_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("strrchr".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtStrrchr);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let string = STACK_TOP - 0x100;
        memory.write(string, b"a/b/c\0").unwrap();
        write_u32(&mut memory, esp, 0x0040_1264).unwrap();
        write_u32(&mut memory, esp + 4, string).unwrap();
        write_u32(&mut memory, esp + 8, b'/' as u32).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(string + 3))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_strstr_returns_the_first_substring_match_and_is_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("strstr".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtStrstr);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let haystack = STACK_TOP - 0x180;
        let needle = STACK_TOP - 0x100;
        memory.write(haystack, b"abc needle needle\0").unwrap();
        memory.write(needle, b"needle\0").unwrap();
        write_u32(&mut memory, esp, 0x1501_c1f9).unwrap();
        write_u32(&mut memory, esp + 4, haystack).unwrap();
        write_u32(&mut memory, esp + 8, needle).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(haystack + 4))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_strnicmp_is_bounded_case_insensitive_and_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_strnicmp".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtStrnicmp);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let left = STACK_TOP - 0x180;
        let right = STACK_TOP - 0x100;
        memory.write(left, b"AbCz\0").unwrap();
        memory.write(right, b"aBcQ\0").unwrap();
        write_u32(&mut memory, esp, 0x1500_bbff).unwrap();
        write_u32(&mut memory, esp + 4, left).unwrap();
        write_u32(&mut memory, esp + 8, right).unwrap();
        write_u32(&mut memory, esp + 12, 3).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );

        write_u32(&mut memory, esp + 12, 4).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(9))
        );
        memory.write(left, b"A\0").unwrap();
        memory.write(right, b"b\0").unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(u32::MAX))
        );
        assert_eq!(xp.call_count, 3);
    }

    #[test]
    fn child_create_thread_is_stdcall_with_six_arguments() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("CreateThread".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CreateThread);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 24);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(24)
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(378, thunk32::Kind::Stdcall(24), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x18, 0]);
    }

    #[test]
    fn child_set_thread_priority_is_stdcall_with_two_arguments() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetThreadPriority".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::SetThreadPriority);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 8);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(8)
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(378, thunk32::Kind::Stdcall(8), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x08, 0]);
    }

    #[test]
    fn child_get_thread_priority_is_stdcall_with_one_argument() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetThreadPriority".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetThreadPriority);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(4)
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(343, thunk32::Kind::Stdcall(4), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x04, 0]);
    }

    #[test]
    fn child_wait_for_multiple_objects_blocks_with_two_handles_and_is_stdcall() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("WaitForMultipleObjects".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::WaitForMultipleObjects);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(16)
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(90, thunk32::Kind::Stdcall(16), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x10, 0]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        let handles = STACK_TOP - 0x100;
        for (index, value) in [0x0041_08d6, 2, handles, 0, u32::MAX].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        write_u32(&mut memory, handles, 0x5743_2812).unwrap();
        write_u32(&mut memory, handles + 4, 0x5743_3504).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 4, 0, esp, &mut memory),
            Ok(PersonalityAction::Block(WaitRequest {
                key: ThreadKey { pid: 2, tid: 4 },
                return_address: 0x0041_08d6,
                count: 2,
                handles_pointer: handles,
                handles: [0x5743_2812, 0x5743_3504],
                wait_all: 0,
                timeout: u32::MAX,
            }))
        );
        write_u32(&mut memory, esp + 4, 3).unwrap();
        assert!(matches!(
            xp.dispatch_provider_for_process_typed(2, 4, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier { api: "WaitForMultipleObjects", .. })
        ));
    }

    #[test]
    fn child_set_event_is_stdcall_and_routes_to_the_session() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetEvent".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::SetEvent);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(4)
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(25, thunk32::Kind::Stdcall(4), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x04, 0]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_3f19).unwrap();
        write_u32(&mut memory, esp + 4, 0x5743_2812).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::SetEvent {
                pid: 2,
                tid: 3,
                handle: 0x5743_2812,
            }))
        );
    }

    #[test]
    fn child_create_file_a_routes_warcraft_directory_files_to_async_open() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("CreateFileA".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        let path = STACK_TOP - 0x100;
        memory.write(path, b"C:\\Warcraft III\\War3X.mpq\0").unwrap();
        for (index, value) in [
            0x0041_08d6,
            path,
            GENERIC_READ,
            1,
            0,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::OpenFile(OpenFileRequest {
                key: ThreadKey { pid: 2, tid: 3 },
                path: "war3x.mpq".into(),
                desired_access: GENERIC_READ,
                share_mode: 1,
                security_attributes: 0,
                creation_disposition: OPEN_EXISTING,
                flags_and_attributes: FILE_ATTRIBUTE_NORMAL,
                template_file: 0,
            }))
        );
    }

    #[test]
    fn admitted_trueos_file_uses_resident_backing_for_size_and_read() {
        let providers = ["GetFileSize", "ReadFile"].map(|symbol| ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name(symbol.into()),
            iat_rva: 0,
        });
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let handle = xp
            .admit_trueos_file(
                r"C:\Warcraft III\War3Patch.mpq".into(),
                "/common/Warcraft III/War3Patch.MPQ".into(),
                std::sync::Arc::new(b"PATCH".to_vec()),
                GENERIC_READ,
                1,
            )
            .unwrap();
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x100;
        let read = STACK_TOP - 0x80;
        write_u32(&mut memory, esp, 0x0041_08d6).unwrap();
        write_u32(&mut memory, esp + 4, handle).unwrap();
        write_u32(&mut memory, esp + 8, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(5))
        );
        write_u32(&mut memory, esp + 8, output).unwrap();
        write_u32(&mut memory, esp + 12, 5).unwrap();
        write_u32(&mut memory, esp + 16, read).unwrap();
        write_u32(&mut memory, esp + 20, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(&memory.bytes[(output - STACK_BASE) as usize..][..5], b"PATCH");
        assert_eq!(read_u32(&memory, read).unwrap(), 5);
    }

    #[test]
    fn child_crt_memmove_preserves_both_overlap_directions_and_is_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("memmove".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtMemmove);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            if cfg!(feature = "host-memmove") { thunk32::Kind::Return } else { thunk32::Kind::Memmove }
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let buffer = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x1501_d5ed).unwrap();
        write_u32(&mut memory, esp + 12, 5).unwrap();

        memory.write(buffer, b"abcdef\0").unwrap();
        write_u32(&mut memory, esp + 4, buffer + 1).unwrap();
        write_u32(&mut memory, esp + 8, buffer).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(buffer + 1))
        );
        let mut right = [0; 6];
        memory.read(buffer, &mut right).unwrap();
        assert_eq!(&right, b"aabcde");

        memory.write(buffer, b"abcdef\0").unwrap();
        write_u32(&mut memory, esp + 4, buffer).unwrap();
        write_u32(&mut memory, esp + 8, buffer + 1).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(buffer))
        );
        let mut left = [0; 6];
        memory.read(buffer, &mut left).unwrap();
        assert_eq!(&left, b"bcdeff");
    }

    #[test]
    fn child_crt_toupper_uses_initial_c_locale_and_is_cdecl() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("toupper".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtToUpper);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1501_d409).unwrap();

        for (character, expected) in [
            (b'q' as u32, b'Q' as u32),
            (b'Q' as u32, b'Q' as u32),
            (b'/' as u32, b'/' as u32),
            (u32::MAX, u32::MAX),
        ] {
            write_u32(&mut memory, esp + 4, character).unwrap();
            assert_eq!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(expected))
            );
        }
        assert_eq!(xp.call_count, 4);
    }

    #[test]
    fn child_crt_atol_is_cdecl_and_clamps_signed_32_bit_values() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("atol".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtAtol);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(343, crate::child_loader::provider_thunk_kind(&provider), &mut thunk)
            .unwrap();
        assert_eq!(&thunk[..9], &[0xb8, 0x57, 0x01, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let input = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x6f00_2bef).unwrap();
        write_u32(&mut memory, esp + 4, input).unwrap();

        for (text, expected) in [
            (b" \t+2147483647tail\0".as_slice(), 0x7fff_ffff),
            (b"-2147483648\0", 0x8000_0000),
            (b"2147483648\0", 0x7fff_ffff),
            (b"-2147483649\0", 0x8000_0000),
            (b"not a number\0", 0),
        ] {
            memory.write(input, text).unwrap();
            assert_eq!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(expected))
            );
        }
        assert_eq!(xp.call_count, 5);
    }

    #[test]
    fn child_crt_ftol_is_cdecl_and_emits_a_plain_return_thunk() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_ftol".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtFtol);
        assert!(operation.is_modeled());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert!(!operation.is_generic_process_local());
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(825, crate::child_loader::provider_thunk_kind(&provider), &mut thunk)
            .unwrap();
        assert_eq!(&thunk[..9], &[0xb8, 0x39, 0x03, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]);
    }

    #[test]
    fn child_crt_srand_is_cdecl_and_sets_the_process_rng_seed() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("srand".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtSrand);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        assert_eq!(xp.crt_rng_seed(), 1);
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x6f29_bf82).unwrap();
        write_u32(&mut memory, esp + 4, 0x1234_5678).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.crt_rng_seed(), 0x1234_5678);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_crt_rand_consumes_the_srand_seed_with_msvcrt_lcg() {
        let srand = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("srand".into()),
            iat_rva: 0,
        };
        let rand = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("rand".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&rand);
        assert_eq!(operation, ProviderOp::CrtRand);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&rand),
            thunk32::Kind::Return
        );
        let mut thunk = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(813, crate::child_loader::provider_thunk_kind(&rand), &mut thunk)
            .unwrap();
        assert_eq!(&thunk[8..9], &[0xc3]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![srand, rand], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x6f29_bf82).unwrap();
        write_u32(&mut memory, esp + 4, 0x150b).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );

        write_u32(&mut memory, esp, 0x6f29_bf96).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(17_630))
        );
        assert_eq!(xp.crt_rng_seed(), 0x44de_4ba2);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(8_328))
        );
        assert_eq!(xp.crt_rng_seed(), 0x2088_c3dd);
        assert_eq!(xp.call_count, 3);
    }

    #[test]
    fn child_static_string_crt_exports_are_cdecl_and_mutate_or_compare_ansi_bytes() {
        for (symbol, operation) in [
            ("strncpy", ProviderOp::CrtStrncpy),
            ("strpbrk", ProviderOp::CrtStrpbrk),
            ("_strlwr", ProviderOp::CrtStrlwr),
            ("_strupr", ProviderOp::CrtStrupr),
            ("strncmp", ProviderOp::CrtStrncmp),
            ("_stricmp", ProviderOp::CrtStricmp),
        ] {
            let provider = ProviderImport {
                module: "MSVCRT.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            assert_eq!(provider_op(&provider), operation, "{symbol}");
            assert!(operation.is_modeled(), "{symbol}");
            assert!(operation.is_generic_process_local(), "{symbol}");
            assert_eq!(operation.stack_cleanup_bytes(), 0, "{symbol}");
            assert_eq!(crate::child_loader::provider_thunk_kind(&provider), thunk32::Kind::Return);
        }

        let providers = ["strncpy", "strpbrk", "_strlwr", "_strupr", "strncmp", "_stricmp"]
            .map(|symbol| ProviderImport {
                module: "MSVCRT.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            });
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        let left = STACK_TOP - 0x200;
        let right = STACK_TOP - 0x180;
        let output = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x6f00_0001).unwrap();

        memory.write(right, b"Ab\0").unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        write_u32(&mut memory, esp + 8, right).unwrap();
        write_u32(&mut memory, esp + 12, 5).unwrap();
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory), Ok(PersonalityAction::Return(output)));
        let mut copied = [0; 5];
        memory.read(output, &mut copied).unwrap();
        assert_eq!(&copied, b"Ab\0\0\0");

        memory.write(left, b"AbC!\0").unwrap();
        write_u32(&mut memory, esp + 4, left).unwrap();
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 2, esp, &mut memory), Ok(PersonalityAction::Return(left)));
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 3, esp, &mut memory), Ok(PersonalityAction::Return(left)));
        let mut transformed = [0; 5];
        memory.read(left, &mut transformed).unwrap();
        assert_eq!(&transformed, b"ABC!\0");

        memory.write(left, b"Alpha\0").unwrap();
        memory.write(right, b"ALPz\0").unwrap();
        write_u32(&mut memory, esp + 4, left).unwrap();
        write_u32(&mut memory, esp + 8, right).unwrap();
        write_u32(&mut memory, esp + 12, 3).unwrap();
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 4, esp, &mut memory), Ok(PersonalityAction::Return(32)));
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 5, esp, &mut memory), Ok(PersonalityAction::Return((-18i32) as u32)));

        memory.write(left, b"abcde\0").unwrap();
        memory.write(right, b"xzdc\0").unwrap();
        write_u32(&mut memory, esp + 4, left).unwrap();
        write_u32(&mut memory, esp + 8, right).unwrap();
        assert_eq!(xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory), Ok(PersonalityAction::Return(left + 2)));
    }

    #[test]
    fn child_wsprintf_a_formats_ansi_strings_and_hex_as_cdecl() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("wsprintfA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::WsprintfA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x80;
        let output = STACK_TOP - 0x300;
        let format = STACK_TOP - 0x200;
        let string = STACK_TOP - 0x180;
        memory.write(format, b"%s-%x\0").unwrap();
        memory.write(string, b"War3\0").unwrap();
        write_u32(&mut memory, esp, 0x1501_c23b).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        write_u32(&mut memory, esp + 8, format).unwrap();
        write_u32(&mut memory, esp + 12, string).unwrap();
        write_u32(&mut memory, esp + 16, 0x12ab).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(9))
        );
        assert_eq!(read_c_string(&memory, output, 32), Ok("War3-12ab".into()));
    }

    #[test]
    fn child_load_string_a_uses_stdcall_sixteen_and_process_dispatch() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("LoadStringA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::LoadStringA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(16));
    }

    #[test]
    fn child_crt_vsnprintf_formats_va_list_and_uses_legacy_truncation() {
        let provider = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_vsnprintf".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CrtVsnprintf);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Return);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x80;
        let output = STACK_TOP - 0x300;
        let format = STACK_TOP - 0x200;
        let string = STACK_TOP - 0x180;
        let va_list = STACK_TOP - 0x140;
        memory.write(format, b"%s-%08x\0").unwrap();
        memory.write(string, b"War3\0").unwrap();
        write_u32(&mut memory, va_list, string).unwrap();
        write_u32(&mut memory, va_list + 4, 0x12ab).unwrap();
        write_u32(&mut memory, esp, 0x1503_b37b).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        write_u32(&mut memory, esp + 8, 32).unwrap();
        write_u32(&mut memory, esp + 12, format).unwrap();
        write_u32(&mut memory, esp + 16, va_list).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(13))
        );
        assert_eq!(read_c_string(&memory, output, 32), Ok("War3-000012ab".into()));

        write_u32(&mut memory, esp + 8, 5).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(u32::MAX))
        );
        let mut truncated = [0; 5];
        memory.read(output, &mut truncated).unwrap();
        assert_eq!(&truncated, b"War3-");
    }

    #[test]
    fn child_set_current_directory_a_accepts_a_nonempty_ansi_path() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetCurrentDirectoryA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::SetCurrentDirectoryA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(4)
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let directory = STACK_TOP - 0x100;
        memory.write(directory, b"C:\\Warcraft III\0").unwrap();
        write_u32(&mut memory, esp, 0x0040_127f).unwrap();
        write_u32(&mut memory, esp + 4, directory).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(xp.last_error(), 0);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_get_file_attributes_a_reports_the_modeled_war3_image() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetFileAttributesA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetFileAttributesA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(4)
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let filename = STACK_TOP - 0x100;
        memory.write(filename, b"C:\\Warcraft III\\War3.exe\0").unwrap();
        write_u32(&mut memory, esp, 0x1501_c27b).unwrap();
        write_u32(&mut memory, esp + 4, filename).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(FILE_ATTRIBUTE_NORMAL))
        );
        assert_eq!(xp.last_error(), 0);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_interlocked_exchange_replaces_dword_and_returns_previous_value() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InterlockedExchange".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::InterlockedExchange);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 8);
        assert_eq!(
            provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(8)
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let target = STACK_TOP - 0x100;
        write_u32(&mut memory, target, 0x1122_3344).unwrap();
        write_u32(&mut memory, esp, 0x0040_2939).unwrap();
        write_u32(&mut memory, esp + 4, target).unwrap();
        write_u32(&mut memory, esp + 8, 0xaabb_ccdd).unwrap();
        xp.set_last_error(0x1234_5678);

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0x1122_3344))
        );
        assert_eq!(read_u32(&memory, target).unwrap(), 0xaabb_ccdd);
        assert_eq!(xp.last_error(), 0x1234_5678);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_interlocked_increment_updates_dword_and_returns_new_value() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InterlockedIncrement".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::InterlockedIncrement);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(4));
        let mut thunk = [0; thunk32::THUNK_BYTES];
        thunk32::write(123, provider_thunk_kind(&provider), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x04, 0x00]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let target = STACK_TOP - 0x100;
        write_u32(&mut memory, target, u32::MAX).unwrap();
        write_u32(&mut memory, esp, 0x1501_775e).unwrap();
        write_u32(&mut memory, esp + 4, target).unwrap();
        xp.set_last_error(0x1234_5678);

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(read_u32(&memory, target).unwrap(), 0);
        assert_eq!(xp.last_error(), 0x1234_5678);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_interlocked_decrement_updates_dword_and_returns_new_value() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InterlockedDecrement".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::InterlockedDecrement);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(4));
        let mut thunk = [0; thunk32::THUNK_BYTES];
        thunk32::write(123, provider_thunk_kind(&provider), &mut thunk).unwrap();
        assert_eq!(&thunk[8..11], &[0xc2, 0x04, 0x00]);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let target = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x1501_775e).unwrap();
        write_u32(&mut memory, esp + 4, target).unwrap();

        for (before, after) in [(5, 4), (1, 0), (0, u32::MAX)] {
            write_u32(&mut memory, target, before).unwrap();
            assert_eq!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(after))
            );
            assert_eq!(read_u32(&memory, target).unwrap(), after);
        }
        assert_eq!(xp.call_count, 3);
    }

    #[test]
    fn child_interlocked_add_rejects_null_and_unaligned_targets() {
        for (symbol, api) in [
            ("InterlockedIncrement", "InterlockedIncrement"),
            ("InterlockedDecrement", "InterlockedDecrement"),
        ] {
            let provider = ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            let mut xp = XpProcess::new_child();
            xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
            let mut memory = Memory {
                base: STACK_BASE,
                bytes: vec![0; STACK_BYTES],
            };
            let esp = STACK_TOP - 0x40;
            write_u32(&mut memory, esp, 0x1501_775e).unwrap();

            write_u32(&mut memory, esp + 4, 0).unwrap();
            assert!(matches!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Err(ProviderDispatchError::Frontier { api: actual, .. }) if actual == api
            ));

            write_u32(&mut memory, esp + 4, STACK_TOP - 0x102).unwrap();
            assert!(matches!(
                xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Err(ProviderDispatchError::Frontier { api: actual, .. }) if actual == api
            ));
        }
    }

    #[test]
    fn child_get_system_info_reports_one_cpu_xp_contract() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetSystemInfo".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetSystemInfo);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(
            provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(4)
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x0040_3000).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(read_u16(&memory, output).unwrap(), 0);
        assert_eq!(read_u32(&memory, output + 4).unwrap(), 0x1000);
        assert_eq!(read_u32(&memory, output + 8).unwrap(), 0x0001_0000);
        assert_eq!(read_u32(&memory, output + 12).unwrap(), 0x7ffe_ffff);
        assert_eq!(read_u32(&memory, output + 16).unwrap(), 1);
        assert_eq!(read_u32(&memory, output + 20).unwrap(), 1);
        assert_eq!(read_u32(&memory, output + 24).unwrap(), 586);
        assert_eq!(read_u32(&memory, output + 28).unwrap(), 0x10000);
        assert_eq!(read_u16(&memory, output + 32).unwrap(), 6);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_global_memory_status_reports_fixed_xp_memory_profile() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GlobalMemoryStatus".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GlobalMemoryStatus);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Stdcall(4));

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x0040_4000).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(read_u32(&memory, output).unwrap(), 32);
        assert_eq!(read_u32(&memory, output + 4).unwrap(), XP_MEMORY_LOAD);
        assert_eq!(read_u32(&memory, output + 8).unwrap(), XP_TOTAL_PHYS);
        assert_eq!(read_u32(&memory, output + 12).unwrap(), XP_AVAIL_PHYS);
        assert_eq!(read_u32(&memory, output + 16).unwrap(), XP_TOTAL_PAGEFILE);
        assert_eq!(read_u32(&memory, output + 20).unwrap(), XP_AVAIL_PAGEFILE);
        assert_eq!(read_u32(&memory, output + 24).unwrap(), XP_TOTAL_VIRTUAL);
        assert_eq!(read_u32(&memory, output + 28).unwrap(), XP_AVAIL_VIRTUAL);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn unsupported_child_provider_does_not_mutate_memory() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetComputerNameA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, STACK_TOP - 0x40, &mut memory),
            Err("unsupported child provider import")
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_get_version_returns_xp_version_without_touching_the_stack() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersion".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, STACK_TOP - 0x40, &mut memory)
                .unwrap(),
            PersonalityAction::Return(WINDOWS_XP_GET_VERSION)
        );
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_get_version_ex_a_uses_the_xp_personality() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersionExA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let info = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x2113_1a58).unwrap();
        write_u32(&mut memory, esp + 4, info).unwrap();
        write_u32(&mut memory, info, 0x94).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(
            [0, 4, 8, 12, 16]
                .map(|offset| read_u32(&memory, info + offset).unwrap()),
            [0x94, 5, 1, 2600, 2]
        );
    }

    #[test]
    fn child_get_version_ex_a_accepts_the_extended_xp_structure() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersionExA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let info = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x2113_1a58).unwrap();
        write_u32(&mut memory, esp + 4, info).unwrap();
        write_u32(&mut memory, info, OSVERSIONINFOEXA_SIZE).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(1)
        );
        assert_eq!(
            [0, 4, 8, 12, 16]
                .map(|offset| read_u32(&memory, info + offset).unwrap()),
            [OSVERSIONINFOEXA_SIZE, 5, 1, 2600, 2]
        );
        assert_eq!(read_u16(&memory, info + 0x94).unwrap(), 0);
        assert_eq!(read_u16(&memory, info + 0x96).unwrap(), 0);
        assert_eq!(read_u16(&memory, info + 0x98).unwrap(), 0);
        assert_eq!(memory.bytes[(info + 0x9a - STACK_BASE) as usize], VER_NT_WORKSTATION);
        assert_eq!(memory.bytes[(info + 0x9b - STACK_BASE) as usize], 0);
    }

    #[test]
    fn child_get_command_line_a_returns_the_process_data_va() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCommandLineA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(PROCESS_DATA_VA)
        );
    }

    #[test]
    fn child_get_environment_strings_w_returns_the_process_data_block() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetEnvironmentStringsW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, STACK_TOP - 0x40, &mut memory)
                .unwrap(),
            PersonalityAction::Return(ENVIRONMENT_BLOCK_VA)
        );
    }

    #[test]
    fn child_free_environment_strings_w_accepts_only_its_process_block() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("FreeEnvironmentStringsW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2113_2228).unwrap();
        write_u32(&mut memory, esp + 4, ENVIRONMENT_BLOCK_VA).unwrap();
        let before = memory.bytes.clone();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(pid2.call_count, 1);
        assert_eq!(memory.bytes, before);

        write_u32(&mut memory, esp + 4, ENVIRONMENT_BLOCK_VA + 4).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Unsupported)
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_read_process_memory_copies_only_from_the_current_process() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("ReadProcessMemory".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::ReadProcessMemory);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 20);

        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x100;
        let source = esp - 0x40;
        let destination = esp - 0x60;
        let bytes_read = esp - 0x70;
        let input = [0x12, 0x34, 0x56, 0x78];
        memory.write(source, &input).unwrap();
        for (index, value) in [
            0x2113_2228,
            CURRENT_PROCESS_PSEUDO_HANDLE,
            source,
            destination,
            input.len() as u32,
            bytes_read,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut copied = [0; 4];
        memory.read(destination, &mut copied).unwrap();
        assert_eq!(copied, input);
        assert_eq!(read_u32(&memory, bytes_read).unwrap(), input.len() as u32);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 8, STACK_TOP).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(pid2.last_error, 299);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 8, source).unwrap();
        write_u32(&mut memory, esp + 12, STACK_TOP).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(pid2.last_error, 299);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 4, 0x5743_5001).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "ReadProcessMemory",
                detail: "non-self-process handle=0x57435001".into(),
            })
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_write_process_memory_writes_only_to_the_current_process() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("WriteProcessMemory".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::WriteProcessMemory);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 20);

        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x100;
        let destination = esp - 0x40;
        let source = esp - 0x60;
        let bytes_written = esp - 0x70;
        let input = [0x12, 0x34, 0x56, 0x78];
        memory.write(source, &input).unwrap();
        for (index, value) in [
            0x2113_2228,
            CURRENT_PROCESS_PSEUDO_HANDLE,
            destination,
            source,
            input.len() as u32,
            bytes_written,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut written = [0; 4];
        memory.read(destination, &mut written).unwrap();
        assert_eq!(written, input);
        assert_eq!(read_u32(&memory, bytes_written).unwrap(), input.len() as u32);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 12, STACK_TOP).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(pid2.last_error, 299);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 8, STACK_TOP).unwrap();
        write_u32(&mut memory, esp + 12, source).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(pid2.last_error, 299);
        assert_eq!(pid2.call_count, 1);

        write_u32(&mut memory, esp + 4, 0x5743_5001).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "WriteProcessMemory",
                detail: "non-self-process handle=0x57435001".into(),
            })
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_startup_info_a_reuses_process_startup_info_semantics() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStartupInfoA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x200;
        let output = esp + 0x40;
        write_u32(&mut memory, esp, 0x2113_1cdf).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        let mut startup = [0u8; 0x44];
        memory.read(output, &mut startup).unwrap();
        assert_eq!(u32::from_le_bytes(startup[..4].try_into().unwrap()), 0x44);
        assert!(startup[4..].iter().all(|byte| *byte == 0));
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_std_handle_reuses_process_standard_handles() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStdHandle".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2113_1dde).unwrap();
        for (selector, expected) in [
            (-10i32, 0x5743_1001),
            (-11i32, 0x5743_1002),
            (-12i32, 0x5743_1003),
        ] {
            write_u32(&mut memory, esp + 4, selector as u32).unwrap();
            assert_eq!(
                pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(expected))
            );
        }
        assert_eq!(pid2.call_count, 3);

        write_u32(&mut memory, esp + 4, 1234).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(u32::MAX))
        );
        assert_eq!(pid2.call_count, 4);
    }

    #[test]
    fn child_get_file_type_reuses_process_file_type_semantics() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetFileType".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2113_1dec).unwrap();
        write_u32(&mut memory, esp + 4, 0x5743_1001).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(2))
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn launcher_get_file_type_reuses_process_file_type_semantics() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "GetFileType".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_1dec).unwrap();
        write_u32(&mut memory, esp + 4, 0x5743_1001).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(2)
        );
    }

    #[test]
    fn child_set_handle_count_reuses_process_compatibility_semantics() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetHandleCount".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let requested = 0x40;
        write_u32(&mut memory, esp, 0x2113_1e23).unwrap();
        write_u32(&mut memory, esp + 4, requested).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(requested))
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_acp_reuses_process_ansi_code_page() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetACP".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2113_6189).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(XP_ANSI_CODE_PAGE))
        );
        assert_eq!(XP_ANSI_CODE_PAGE, 1252);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn ascii_ctype1_distinguishes_the_minimal_crt_classes() {
        assert_eq!(ascii_ctype1(u16::from(b'A')), C1_ALPHA | C1_UPPER | C1_XDIGIT);
        assert_eq!(ascii_ctype1(u16::from(b'a')), C1_ALPHA | C1_LOWER | C1_XDIGIT);
        assert_eq!(ascii_ctype1(u16::from(b'F')), C1_ALPHA | C1_UPPER | C1_XDIGIT);
        assert_eq!(ascii_ctype1(u16::from(b'f')), C1_ALPHA | C1_LOWER | C1_XDIGIT);
        assert_eq!(ascii_ctype1(u16::from(b'0')), C1_DIGIT | C1_XDIGIT);
        assert_eq!(ascii_ctype1(u16::from(b' ')), C1_SPACE | C1_BLANK);
        assert_eq!(ascii_ctype1(u16::from(b'\t')), C1_SPACE | C1_BLANK | C1_CNTRL);
        assert_eq!(ascii_ctype1(u16::from(b'!')), C1_PUNCT);
        assert_eq!(ascii_ctype1(0), C1_CNTRL);
    }

    #[test]
    fn child_get_string_type_w_uses_ctype1_and_process_memory_only() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStringTypeW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let source = STACK_TOP - 0x200;
        let output = STACK_TOP - 0x300;
        for (index, value) in [b'A', b'a', b'F', b'f', b'0', b' ', b'\t', b'!', 0]
            .into_iter()
            .enumerate()
        {
            write_u16(&mut memory, source + index as u32 * 2, u16::from(value)).unwrap();
        }
        for (index, value) in [0x2113_61d2, CT_CTYPE1, source, 9, output]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let expected = [
            C1_ALPHA | C1_UPPER | C1_XDIGIT,
            C1_ALPHA | C1_LOWER | C1_XDIGIT,
            C1_ALPHA | C1_UPPER | C1_XDIGIT,
            C1_ALPHA | C1_LOWER | C1_XDIGIT,
            C1_DIGIT | C1_XDIGIT,
            C1_SPACE | C1_BLANK,
            C1_SPACE | C1_BLANK | C1_CNTRL,
            C1_PUNCT,
            C1_CNTRL,
        ];
        for (index, class) in expected.into_iter().enumerate() {
            assert_eq!(read_u16(&memory, output + index as u32 * 2).unwrap(), class);
        }
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_string_type_w_negative_count_includes_the_terminating_nul() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStringTypeW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let source = STACK_TOP - 0x200;
        let output = STACK_TOP - 0x300;
        for (index, value) in [b'A', b'a', 0].into_iter().enumerate() {
            write_u16(&mut memory, source + index as u32 * 2, u16::from(value)).unwrap();
        }
        write_u16(&mut memory, output + 6, 0x5a5a).unwrap();
        for (index, value) in [0x2113_61d2, CT_CTYPE1, source, u32::MAX, output]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(
            read_u16(&memory, output).unwrap(),
            C1_ALPHA | C1_UPPER | C1_XDIGIT
        );
        assert_eq!(
            read_u16(&memory, output + 2).unwrap(),
            C1_ALPHA | C1_LOWER | C1_XDIGIT
        );
        assert_eq!(read_u16(&memory, output + 4).unwrap(), C1_CNTRL);
        assert_eq!(read_u16(&memory, output + 6).unwrap(), 0x5a5a);
    }

    #[test]
    fn child_multi_byte_to_wide_char_reuses_cp_acp_conversion() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("MultiByteToWideChar".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let source = STACK_TOP - 0x200;
        let output = STACK_TOP - 0x300;
        memory.write(source, b"Az\0").unwrap();
        for (index, value) in [0x2113_4cfc, 0, 0, source, u32::MAX, output, 3]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(3))
        );
        assert_eq!(read_u16(&memory, output).unwrap(), u16::from(b'A'));
        assert_eq!(read_u16(&memory, output + 2).unwrap(), u16::from(b'z'));
        assert_eq!(read_u16(&memory, output + 4).unwrap(), 0);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn cp1252_scalar_mapping_round_trips_all_bytes() {
        for byte in 0u8..=u8::MAX {
            assert_eq!(encode_cp1252(decode_cp1252(byte)), Some(byte));
        }
        assert_eq!(decode_cp1252(0x8a), 0x0160);
        assert_eq!(decode_cp1252(0x8c), 0x0152);
        assert_eq!(decode_cp1252(0x9a), 0x0161);
        assert_eq!(decode_cp1252(0x9c), 0x0153);
        assert_eq!(decode_cp1252(0x9f), 0x0178);
    }

    #[test]
    fn lc_map_scalars_use_bounded_cp1252_case_pairs() {
        assert_eq!(lc_map_scalar(u16::from(b'A'), LcMapMode::Lower), u16::from(b'a'));
        assert_eq!(lc_map_scalar(u16::from(b'Z'), LcMapMode::Lower), u16::from(b'z'));
        assert_eq!(lc_map_scalar(u16::from(b'a'), LcMapMode::Upper), u16::from(b'A'));
        assert_eq!(lc_map_scalar(u16::from(b'z'), LcMapMode::Upper), u16::from(b'Z'));
        for (upper, lower) in [
            (0x00c0, 0x00e0),
            (0x00d6, 0x00f6),
            (0x00d8, 0x00f8),
            (0x00de, 0x00fe),
            (0x0160, 0x0161),
            (0x0152, 0x0153),
            (0x017d, 0x017e),
            (0x0178, 0x00ff),
        ] {
            assert_eq!(lc_map_scalar(upper, LcMapMode::Lower), lower);
            assert_eq!(lc_map_scalar(lower, LcMapMode::Upper), upper);
        }
        assert_eq!(lc_map_scalar(0x20ac, LcMapMode::Lower), 0x20ac);
        assert_eq!(lc_map_scalar(0x2122, LcMapMode::Upper), 0x2122);
    }

    #[test]
    fn lc_map_string_w_negative_count_size_query_includes_nul() {
        let xp = XpProcess::new(Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let source = STACK_TOP - 0x200;
        for (index, value) in [0x0041, 0x0062, 0x0160, 0x20ac, 0]
            .into_iter()
            .enumerate()
        {
            write_u16(&mut memory, source + index as u32 * 2, value).unwrap();
        }
        for (index, value) in [0x2113_4d10, 0, LCMAP_LOWERCASE, source, u32::MAX, 0, 0]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(xp.lc_map_string(esp, &mut memory), Ok(5));
    }

    #[test]
    fn child_lc_map_string_w_reuses_cp1252_case_mapping() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("LCMapStringW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let source = STACK_TOP - 0x200;
        let output = STACK_TOP - 0x300;
        let input = [0x0041, 0x00c0, 0x0160, 0x0152, 0x017d, 0x0178, 0x20ac, 0];
        let expected = [0x0061, 0x00e0, 0x0161, 0x0153, 0x017e, 0x00ff, 0x20ac, 0];
        for (index, value) in input.into_iter().enumerate() {
            write_u16(&mut memory, source + index as u32 * 2, value).unwrap();
        }
        for (index, value) in [
            0x2113_4d10,
            0,
            LCMAP_LOWERCASE | LCMAP_LINGUISTIC_CASING,
            source,
            u32::MAX,
            output,
            8,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(8))
        );
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(read_u16(&memory, output + index as u32 * 2).unwrap(), value);
        }
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_lc_map_string_w_rejects_unobserved_mapping_modes() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("LCMapStringW".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp + 8, 0x0000_0400).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Unsupported)
        );
        assert_eq!(pid2.call_count, 0);
    }

    #[test]
    fn child_get_module_file_name_a_reports_the_child_image_only() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleFileNameA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        for (index, value) in [
            0x2113_6200,
            0,
            output,
            CHILD_IMAGE_FILENAME.len() as u32,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(
                (CHILD_IMAGE_FILENAME.len() - 1) as u32
            ))
        );
        let mut filename = vec![0; CHILD_IMAGE_FILENAME.len()];
        memory.read(output, &mut filename).unwrap();
        assert_eq!(filename, CHILD_IMAGE_FILENAME);
        write_u32(&mut memory, esp + 4, pe32::IMAGE_BASE).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(
                (CHILD_IMAGE_FILENAME.len() - 1) as u32
            ))
        );
        memory.read(output, &mut filename).unwrap();
        assert_eq!(filename, CHILD_IMAGE_FILENAME);
        assert_eq!(pid2.call_count, 2);
    }

    #[test]
    fn child_runtime_system_provider_load_retains_d3d8_without_admitting_app_dlls() {
        assert!(is_system_provider_module("d3d8.dll"));
        assert!(is_system_provider_module("C:\\Windows\\System32\\D3D8.DLL"));
        assert!(!is_system_provider_module("War3Patch.dll"));

        let mut xp = XpProcess::new_child();
        let (handle, references, already_loaded) =
            xp.load_runtime_external_provider("d3d8.dll").unwrap();
        assert_eq!(references, 1);
        assert!(!already_loaded);
        assert_eq!(xp.external_provider_module_name(handle), Some("d3d8.dll"));

        let (retained, references, already_loaded) = xp
            .load_runtime_external_provider("C:\\Windows\\System32\\D3D8.DLL")
            .unwrap();
        assert_eq!(retained, handle);
        assert_eq!(references, 2);
        assert!(already_loaded);
    }

    #[test]
    fn child_runtime_external_provider_unloads_at_its_final_reference() {
        let mut xp = XpProcess::new_child();
        let (handle, references, already_loaded) =
            xp.load_runtime_external_provider("d3d8.dll").unwrap();
        assert_eq!(references, 1);
        assert!(!already_loaded);

        assert_eq!(
            xp.release_loaded_module(handle),
            Ok(ModuleRelease::ExternalProviderUnloaded {
                module: "d3d8.dll".into(),
            })
        );
        assert_eq!(xp.loaded_module_handle("d3d8.dll"), None);

        let (handle2, references2, already_loaded2) =
            xp.load_runtime_external_provider("d3d8.dll").unwrap();
        assert_eq!(references2, 1);
        assert!(!already_loaded2);
        assert_ne!(handle2, handle);
    }

    #[test]
    fn child_runtime_native_module_final_release_remains_registered() {
        let mut xp = XpProcess::new_child();
        xp.register_runtime_native_module("Storm.dll", 0x1500_0000)
            .unwrap();

        assert_eq!(
            xp.release_loaded_module(0x1500_0000),
            Ok(ModuleRelease::NativeUnloadRequired {
                module: "Storm.dll".into(),
            })
        );
        assert_eq!(xp.loaded_module_handle("Storm.dll"), Some(0x1500_0000));
    }

    #[test]
    fn child_d3d8_get_adapter_identifier_returns_the_trueos_adapter_snapshot() {
        let provider = ProviderImport {
            module: "d3d8.dll".into(),
            symbol: ProviderSymbol::Name("IDirect3D8::GetAdapterIdentifier".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; 0x2000],
        };
        let esp = STACK_BASE + 0x100;
        let output = STACK_BASE + 0x400;
        for (index, value) in [
            0x6f0d_0300,
            thunk32::CHILD_D3D8_OBJECT_ADDRESS,
            0,
            D3DENUM_NO_WHQL_LEVEL,
            output,
        ]
        .into_iter()
        .enumerate()
        {
            memory
                .write(esp + index as u32 * 4, &value.to_le_bytes())
                .unwrap();
        }

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(D3D_OK))
        );
        let mut identifier = [0u8; D3DADAPTER_IDENTIFIER8_BYTES];
        memory.read(output, &mut identifier).unwrap();
        assert_eq!(&identifier[..12], b"trueos-d3d8\0");
        assert_eq!(&identifier[0x200..0x21a], b"Intel(R) UHD Graphics 770\0");
        assert_eq!(
            u32::from_le_bytes(identifier[0x408..0x40c].try_into().unwrap()),
            TRUEOS_D3D8_VENDOR_ID
        );
        assert_eq!(
            u32::from_le_bytes(identifier[0x40c..0x410].try_into().unwrap()),
            TRUEOS_D3D8_DEVICE_ID
        );
        assert_eq!(
            u32::from_le_bytes(identifier[0x414..0x418].try_into().unwrap()),
            TRUEOS_D3D8_REVISION
        );
    }

    #[test]
    fn child_enum_display_devices_a_returns_trueos_adapter_and_monitor() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplayDevicesA".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; 0x2000],
        };
        let esp = STACK_BASE + 0x100;
        let output = STACK_BASE + 0x400;
        let device = STACK_BASE + 0x700;
        write_u32(&mut memory, output, 0x1a8).unwrap();

        let mut call = |memory: &mut Memory, device_ptr, index, flags| {
            for (word, value) in [0x6f0d_0300, device_ptr, index, output, flags]
                .into_iter()
                .enumerate()
            {
                write_u32(memory, esp + word as u32 * 4, value).unwrap();
            }
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, memory)
        };

        assert_eq!(call(&mut memory, 0, 0, 0), Ok(PersonalityAction::Return(1)));
        assert_eq!(read_c_string(&memory, output + 0x04, 32), Ok(r"\\.\DISPLAY1".into()));
        assert_eq!(
            read_c_string(&memory, output + 0x24, 128),
            Ok(TRUEOS_DISPLAY_ADAPTER_DESCRIPTION.into())
        );
        assert_eq!(read_u32(&memory, output + 0xa4), Ok(0x0000_0005));
        assert!(memory.bytes[(output - STACK_BASE) as usize + 0xa8..(output - STACK_BASE) as usize + 0x1a8]
            .iter()
            .all(|byte| *byte == 0));

        memory.write(device, b"\\\\.\\DISPLAY1\0").unwrap();
        write_u32(&mut memory, output, 0x1a8).unwrap();
        assert_eq!(call(&mut memory, device, 0, 0), Ok(PersonalityAction::Return(1)));
        assert_eq!(
            read_c_string(&memory, output + 0x04, 32),
            Ok(r"\\.\DISPLAY1\Monitor0".into())
        );
        assert_eq!(read_c_string(&memory, output + 0x24, 128), Ok("TRUEOS UI4 Display".into()));
        assert_eq!(read_u32(&memory, output + 0xa4), Ok(1));

        write_u32(&mut memory, output, 0x1a8).unwrap();
        assert_eq!(call(&mut memory, 0, 1, 0), Ok(PersonalityAction::Return(0)));
        write_u32(&mut memory, output, 0x1a8).unwrap();
        assert_eq!(call(&mut memory, device, 1, 0), Ok(PersonalityAction::Return(0)));
        memory.write(device, b"\\\\.\\UNKNOWN\0").unwrap();
        write_u32(&mut memory, output, 0x1a8).unwrap();
        assert_eq!(call(&mut memory, device, 0, 0), Ok(PersonalityAction::Return(0)));
    }

    #[test]
    fn child_enum_display_devices_a_rejects_unobserved_frames() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplayDevicesA".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; 0x2000] };
        let esp = STACK_BASE + 0x100;
        let output = STACK_BASE + 0x400;
        for (word, value) in [0x6f0d_0300, 0, 0, output, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + word as u32 * 4, value).unwrap();
        }
        write_u32(&mut memory, output, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "EnumDisplayDevicesA",
                detail: "unexpected cb=0".into(),
            })
        );
        write_u32(&mut memory, output, 0x1a8).unwrap();
        write_u32(&mut memory, esp + 16, 1).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "EnumDisplayDevicesA",
                detail: "unobserved flags=0x00000001".into(),
            })
        );
    }

    #[test]
    fn child_enum_display_settings_a_returns_the_inherited_trueos_mode() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplaySettingsA".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.set_desktop_size(2560, 1440);
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; 0x2000] };
        let esp = STACK_BASE + 0x100;
        let output = STACK_BASE + 0x400;
        let device = STACK_BASE + 0x700;
        memory.write(device, b"\\\\.\\DISPLAY1\0").unwrap();

        let mut call = |memory: &mut Memory, device_ptr, mode_num| {
            for (word, value) in [0x6f0d_0300, device_ptr, mode_num, output]
                .into_iter()
                .enumerate()
            {
                write_u32(memory, esp + word as u32 * 4, value).unwrap();
            }
            write_u16(memory, output + 0x24, 0x9c).unwrap();
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, memory)
        };

        assert_eq!(
            call(&mut memory, device, ENUM_CURRENT_SETTINGS),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(read_u16(&memory, output + 0x24), Ok(0x9c));
        assert_eq!(read_u16(&memory, output + 0x26), Ok(0));
        assert_eq!(read_u32(&memory, output + 0x28), Ok(TRUEOS_DISPLAY_FIELDS));
        assert_eq!(read_u32(&memory, output + 0x68), Ok(32));
        assert_eq!(read_u32(&memory, output + 0x6c), Ok(2560));
        assert_eq!(read_u32(&memory, output + 0x70), Ok(1440));
        assert_eq!(read_u32(&memory, output + 0x74), Ok(0));
        assert_eq!(read_u32(&memory, output + 0x78), Ok(60));

        assert_eq!(
            call(&mut memory, 0, ENUM_REGISTRY_SETTINGS),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(call(&mut memory, device, 0), Ok(PersonalityAction::Return(1)));
        assert_eq!(call(&mut memory, device, 1), Ok(PersonalityAction::Return(0)));
        memory.write(device, b"\\\\.\\UNKNOWN\0").unwrap();
        assert_eq!(
            call(&mut memory, device, ENUM_CURRENT_SETTINGS),
            Ok(PersonalityAction::Return(0))
        );
    }

    #[test]
    fn child_enum_display_settings_a_rejects_short_dev_mode() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplaySettingsA".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; 0x2000] };
        let esp = STACK_BASE + 0x100;
        let output = STACK_BASE + 0x400;
        for (word, value) in [0x6f0d_0300, 0, ENUM_CURRENT_SETTINGS, output]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + word as u32 * 4, value).unwrap();
        }
        write_u16(&mut memory, output + 0x24, 0x7b).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "EnumDisplaySettingsA",
                detail: "unexpected dmSize=123".into(),
            })
        );
    }

    #[test]
    fn child_inherits_the_launcher_desktop_size() {
        let mut launcher = XpProcess::new(Vec::new());
        launcher.set_desktop_size(2560, 1440);
        let mut session = crate::session::Wc3Session::new(launcher);
        let child = session.create_child();
        assert_eq!(session.process(child.pid).unwrap().xp.desktop_size(), (2560, 1440));
    }

    #[test]
    fn child_d3d8_release_returns_post_decrement_reference_count() {
        let provider = ProviderImport {
            module: "d3d8.dll".into(),
            symbol: ProviderSymbol::Name("IDirect3D8::Release".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        assert_eq!(xp.retain_d3d8_object(), Ok(1));

        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; 0x2000],
        };
        let esp = STACK_BASE + 0x100;
        write_u32(&mut memory, esp, 0x6f0d_02d2).unwrap();
        write_u32(&mut memory, esp + 4, thunk32::CHILD_D3D8_OBJECT_ADDRESS).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.d3d8_ref_count(), 0);
    }

    #[test]
    fn child_get_module_file_name_a_reports_native_module_paths() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleFileNameA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        pid2.register_native_module("MSS32.DLL", "Mss32.dll", 0x2110_0000)
            .unwrap();
        pid2.register_native_module("Storm.dll", "Storm.dll", 0x1500_0000)
            .unwrap();
        pid2.register_runtime_native_module("C:\\Windows\\SIntfNT.dll", 0x2000_0000)
            .unwrap();
        assert_eq!(pid2.loaded_module_handle("mss32.dll"), Some(0x2110_0000));
        assert_eq!(
            pid2.loaded_module_handle("C:\\Warcraft III\\Mss32.dll"),
            Some(0x2110_0000)
        );
        assert_eq!(pid2.loaded_module_handle("Unknown.dll"), None);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x2113_6200).unwrap();
        write_u32(&mut memory, esp + 8, output).unwrap();
        for (handle, expected) in [
            (0x2110_0000, b"C:\\Warcraft III\\Mss32.dll\0".as_slice()),
            (0x1500_0000, b"C:\\Warcraft III\\Storm.dll\0".as_slice()),
            (0x2000_0000, b"C:\\Windows\\SIntfNT.dll\0".as_slice()),
        ] {
            write_u32(&mut memory, esp + 4, handle).unwrap();
            write_u32(&mut memory, esp + 12, expected.len() as u32).unwrap();
            assert_eq!(
                pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return((expected.len() - 1) as u32))
            );
            let mut actual = vec![0; expected.len()];
            memory.read(output, &mut actual).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn child_get_module_file_name_a_reports_unmodeled_provider_filenames() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleFileNameA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(
            vec![provider],
            Vec::new(),
            vec![ChildProvider::External {
                module: "KERNEL32.dll".into(),
            }],
        );
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        for (index, value) in [
            0x2113_6200,
            PROVIDER_MODULE_HANDLE_BASE,
            STACK_TOP - 0x200,
            260,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Err(ProviderDispatchError::Frontier {
                api: "GetModuleFileNameA",
                detail: format!(
                    "module-filename-unmodeled handle=0x{PROVIDER_MODULE_HANDLE_BASE:08x} name=\"KERNEL32.dll\""
                ),
            })
        );
    }

    #[test]
    fn child_get_windows_directory_a_returns_xp_personality_directory() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetWindowsDirectoryA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        for (index, value) in [0x2110_1dca, output, 260].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(10))
        );
        let mut actual = vec![0; XP_WINDOWS_DIRECTORY.len()];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, XP_WINDOWS_DIRECTORY);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_windows_directory_a_reports_required_size_without_writing() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetWindowsDirectoryA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        memory.write(output, &[0x5a; 16]).unwrap();
        for (index, value) in [0x2110_1dca, output, 1].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(XP_WINDOWS_DIRECTORY.len() as u32))
        );
        let mut actual = [0; 16];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, [0x5a; 16]);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_system_directory_a_reuses_ansi_directory_contract() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetSystemDirectoryA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        for (index, value) in [0x2110_1dca, output, 260].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(19))
        );
        let mut actual = vec![0; XP_SYSTEM_DIRECTORY.len()];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, XP_SYSTEM_DIRECTORY);

        memory.write(output, &[0x5a; 24]).unwrap();
        write_u32(&mut memory, esp + 8, 1).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(20))
        );
        let mut sentinel = [0; 24];
        memory.read(output, &mut sentinel).unwrap();
        assert_eq!(sentinel, [0x5a; 24]);
        assert_eq!(pid2.call_count, 2);
    }

    #[test]
    fn child_get_temp_path_a_writes_windows_fallback() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetTempPathA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetTempPathA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 8);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(8)
        );
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = esp - 0x100;
        write_u32(&mut memory, esp, 0x0046_35e3).unwrap();
        write_u32(&mut memory, esp + 4, 0x400).unwrap();
        write_u32(&mut memory, esp + 8, output).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(11))
        );
        let mut actual = [0u8; 12];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(&actual, XP_TEMP_DIRECTORY);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn sintf_scratch_paths_accept_observed_variants() {
        assert_eq!(
            canonical_file_path(r"C:\\WINDOWS\\\SIntf16.dll"),
            r"c:\windows\sintf16.dll"
        );

        for path in [
            r"C:\\WINDOWS\\\SIntf16.dll",
            r"C:\\WINDOWS\\\SIntf32.dll",
            r"C:\\WINDOWS\\\SIntfNT.dll",
        ] {
            assert!(is_war3_scratch_path(path), "{path}");
        }
    }

    #[test]
    fn child_set_file_attributes_missing_temp_file_reports_not_found() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetFileAttributesA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::SetFileAttributesA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 8);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(8)
        );

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let filename = esp - 0x100;
        memory.write(filename, b"C:\\WINDOWS\\SIntf16.dll\0").unwrap();
        for (index, value) in [0x0046_363b, filename, FILE_ATTRIBUTE_TEMPORARY]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.last_error, ERROR_FILE_NOT_FOUND);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_query_performance_frequency_exposes_nanosecond_frequency() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("QueryPerformanceFrequency".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x2113_1b38).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut actual = [0; 8];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(u64::from_le_bytes(actual), XP_PERFORMANCE_COUNTER_FREQUENCY);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_query_performance_counter_exposes_monotonic_nanoseconds() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("QueryPerformanceCounter".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x2113_1b2c).unwrap();
        write_u32(&mut memory, esp + 4, output).unwrap();
        let before = monotonic_counter_nanos();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let after = monotonic_counter_nanos();
        let mut actual = [0; 8];
        memory.read(output, &mut actual).unwrap();
        let counter = u64::from_le_bytes(actual);
        assert!(before <= counter && counter <= after);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_time_get_time_exposes_wrapping_monotonic_milliseconds() {
        let provider = ProviderImport {
            module: "WINMM.dll".into(),
            symbol: ProviderSymbol::Name("timeGetTime".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2110_1fff).unwrap();
        let before = monotonic_counter_millis();
        let result = pid2
            .dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory)
            .unwrap();
        let after = monotonic_counter_millis();
        let PersonalityAction::Return(milliseconds) = result else {
            panic!("timeGetTime requested a runtime effect");
        };
        assert!(before <= milliseconds && milliseconds <= after);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_tick_count_exposes_wrapping_monotonic_milliseconds() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetTickCount".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetTickCount);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Return);

        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x6f00_5fb3).unwrap();
        let before = monotonic_counter_millis();
        let result = pid2
            .dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory)
            .unwrap();
        let after = monotonic_counter_millis();
        let PersonalityAction::Return(milliseconds) = result else {
            panic!("GetTickCount requested a runtime effect");
        };
        assert!(before <= milliseconds && milliseconds <= after);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_module_handle_a_null_reports_the_child_main_image() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x2113_094f).unwrap();
        write_u32(&mut memory, esp + 4, 0).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(pe32::IMAGE_BASE))
        );
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn child_get_current_process_returns_the_windows_pseudo_handle() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentProcess".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        assert_eq!(CURRENT_PROCESS_PSEUDO_HANDLE, 0xffff_ffff);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, STACK_TOP - 0x40, &mut memory),
            Ok(PersonalityAction::Return(CURRENT_PROCESS_PSEUDO_HANDLE))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn process_starts_with_default_heap() {
        let xp = XpProcess::new(Vec::new());
        assert!(xp.heaps.contains_key(&PROCESS_HEAP_HANDLE));
    }

    #[test]
    fn child_get_process_heap_returns_the_default_allocatable_heap() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetProcessHeap".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetProcessHeap);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&provider), thunk32::Kind::Return);

        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x1501_775e).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(PROCESS_HEAP_HANDLE))
        );

        write_u32(&mut memory, esp, 0x1501_7760).unwrap();
        write_u32(&mut memory, esp + 4, PROCESS_HEAP_HANDLE).unwrap();
        write_u32(&mut memory, esp + 8, 0).unwrap();
        write_u32(&mut memory, esp + 12, 16).unwrap();
        let allocation = xp.alloc_win_heap(esp, &mut memory).unwrap().unwrap();
        assert_eq!(allocation.heap, PROCESS_HEAP_HANDLE);
    }

    #[test]
    fn child_get_current_thread_returns_the_windows_pseudo_handle() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentThread".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };

        assert_eq!(CURRENT_THREAD_PSEUDO_HANDLE, 0xffff_fffe);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, STACK_TOP - 0x40, &mut memory),
            Ok(PersonalityAction::Return(CURRENT_THREAD_PSEUDO_HANDLE))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_self_image_file_supports_size_seek_read_and_close() {
        let providers = [
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("CreateFileA".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("GetFileSize".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("SetFilePointer".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("ReadFile".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("CloseHandle".into()),
                iat_rva: 0,
            },
        ];
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let filename = esp - 0x100;
        memory.write(filename, CHILD_IMAGE_FILENAME).unwrap();
        for (index, value) in [
            0x0049_d380,
            filename,
            0x8000_0000,
            1,
            0,
            3,
            0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(FILE_HANDLE_BASE))
        );
        assert_eq!(
            xp.file_handles.get(&FILE_HANDLE_BASE),
            Some(&FileHandle {
                backing: FileBacking::SelfImage,
                cursor: 0,
                access: GENERIC_READ,
                share: 1,
            })
        );
        assert!(is_self_image_path("c:/warcraft iii/war3.EXE"));

        let image = b"012345";
        let high = esp - 0x104;
        write_u32(&mut memory, esp, 0x0049_d390).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        write_u32(&mut memory, esp + 8, high).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed_with_self_image(
                2,
                3,
                1,
                esp,
                &mut memory,
                Some(image),
            ),
            Ok(PersonalityAction::Return(6))
        );
        assert_eq!(read_u32(&memory, high).unwrap(), 0);

        write_u32(&mut memory, esp, 0x0049_d3a0).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        write_u32(&mut memory, esp + 8, 2).unwrap();
        write_u32(&mut memory, esp + 12, 0).unwrap();
        write_u32(&mut memory, esp + 16, FILE_BEGIN).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed_with_self_image(
                2,
                3,
                2,
                esp,
                &mut memory,
                Some(image),
            ),
            Ok(PersonalityAction::Return(2))
        );

        let output = esp - 0x120;
        let bytes_read = esp - 0x108;
        write_u32(&mut memory, esp, 0x0049_d3b0).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        write_u32(&mut memory, esp + 8, output).unwrap();
        write_u32(&mut memory, esp + 12, 3).unwrap();
        write_u32(&mut memory, esp + 16, bytes_read).unwrap();
        write_u32(&mut memory, esp + 20, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed_with_self_image(
                2,
                3,
                3,
                esp,
                &mut memory,
                Some(image),
            ),
            Ok(PersonalityAction::Return(1))
        );
        let mut actual = [0; 3];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, *b"234");
        assert_eq!(read_u32(&memory, bytes_read).unwrap(), 3);

        write_u32(&mut memory, esp, 0x0049_d3c0).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 4, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert!(!xp.file_handles.contains_key(&FILE_HANDLE_BASE));
        assert_eq!(xp.call_count, 5);
    }

    #[test]
    fn child_war3_mpq_file_uses_resident_backing_for_size_and_read() {
        let providers = ["CreateFileA", "GetFileSize", "ReadFile"].map(|symbol| ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name(symbol.into()),
            iat_rva: 0,
        });
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        xp.install_war3_mpq(std::sync::Arc::new(b"MPQ!".to_vec()));
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let filename = esp - 0x100;
        memory.write(filename, b"C:\\Warcraft III\\War3.mpq\0").unwrap();
        for (index, value) in [0x0049_d380, filename, GENERIC_READ, 1, 0, OPEN_EXISTING, 0, 0]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(FILE_HANDLE_BASE))
        );
        assert_eq!(
            xp.file_handles.get(&FILE_HANDLE_BASE),
            Some(&FileHandle {
                backing: FileBacking::War3Mpq,
                cursor: 0,
                access: GENERIC_READ,
                share: 1,
            })
        );

        write_u32(&mut memory, esp, 0x0049_d390).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        write_u32(&mut memory, esp + 8, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(4))
        );

        let output = esp - 0x120;
        let bytes_read = esp - 0x108;
        for (index, value) in [0x0049_d3b0, FILE_HANDLE_BASE, output, 4, bytes_read, 0]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 2, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut actual = [0; 4];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, *b"MPQ!");
        assert_eq!(read_u32(&memory, bytes_read).unwrap(), 4);
    }

    #[test]
    fn child_sintf16_scratch_file_is_process_local_and_mutable() {
        let providers = [
            "CreateFileA",
            "WriteFile",
            "FlushFileBuffers",
            "SetFileAttributesA",
            "GetFileSize",
            "SetFilePointer",
            "ReadFile",
            "CloseHandle",
        ]
        .map(|symbol| ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name(symbol.into()),
            iat_rva: 0,
        });
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let filename = esp - 0x100;
        let input = esp - 0x120;
        let output = esp - 0x140;
        let bytes_transferred = esp - 0x160;
        memory.write(filename, b"C:\\WINDOWS\\SIntf16.dll\0").unwrap();
        memory.write(input, b"abc").unwrap();

        for (index, value) in [0x0046_363b, filename, FILE_ATTRIBUTE_TEMPORARY]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 3, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.last_error, ERROR_FILE_NOT_FOUND);

        for (index, value) in [
            0x0046_3641,
            filename,
            GENERIC_READ | GENERIC_WRITE,
            0,
            0,
            CREATE_ALWAYS,
            0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(FILE_HANDLE_BASE))
        );
        assert_eq!(xp.last_error, 0);

        for (index, value) in [
            0x0046_3650,
            FILE_HANDLE_BASE,
            input,
            3,
            bytes_transferred,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(read_u32(&memory, bytes_transferred).unwrap(), 3);

        write_u32(&mut memory, esp, 0x0046_372b).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 2, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );

        for (index, value) in [0x0046_3660, filename, FILE_ATTRIBUTE_TEMPORARY]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 3, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );

        write_u32(&mut memory, esp, 0x0046_3670).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        write_u32(&mut memory, esp + 8, 0).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 4, esp, &mut memory),
            Ok(PersonalityAction::Return(3))
        );

        for (index, value) in [0x0046_3680, FILE_HANDLE_BASE, 0, 0, FILE_BEGIN]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 5, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );

        for (index, value) in [
            0x0046_3690,
            FILE_HANDLE_BASE,
            output,
            3,
            bytes_transferred,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 6, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut actual = [0; 3];
        memory.read(output, &mut actual).unwrap();
        assert_eq!(actual, *b"abc");

        write_u32(&mut memory, esp, 0x0046_36a0).unwrap();
        write_u32(&mut memory, esp + 4, FILE_HANDLE_BASE).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 7, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert!(!xp.file_handles.contains_key(&FILE_HANDLE_BASE));
        let scratch_id = xp
            .scratch_paths
            .get(r"c:\windows\sintf16.dll")
            .copied()
            .unwrap();
        let scratch = xp.scratch_files.get(&scratch_id).unwrap();
        assert_eq!(scratch.path, r"c:\windows\sintf16.dll");
        assert_eq!(scratch.bytes, b"abc");
        assert_eq!(scratch.attributes, FILE_ATTRIBUTE_TEMPORARY);
        assert_eq!(
            xp.scratch_file_snapshot(r"C:\\WINDOWS\\\SIntf16.dll"),
            Some(b"abc".to_vec())
        );
    }

    #[test]
    fn child_open_thread_token_reports_no_impersonation_token() {
        let provider = ProviderImport {
            module: "ADVAPI32.dll".into(),
            symbol: ProviderSymbol::Name("OpenThreadToken".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let token_out = esp - 4;
        write_u32(&mut memory, token_out, 0xaaaa_5555).unwrap();
        for (index, value) in [
            0x0045_ef60,
            CURRENT_THREAD_PSEUDO_HANDLE,
            0x0000_0008,
            0,
            token_out,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(xp.last_error, 1008);
        assert_eq!(read_u32(&memory, token_out).unwrap(), 0xaaaa_5555);
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_open_process_token_tracks_and_closes_the_current_process_token() {
        let providers = [
            ProviderImport {
                module: "ADVAPI32.dll".into(),
                symbol: ProviderSymbol::Name("OpenProcessToken".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name("CloseHandle".into()),
                iat_rva: 0,
            },
        ];
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        xp.set_last_error(ERROR_NO_TOKEN);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let token_out = esp - 4;
        for (index, value) in [
            0x0045_ef82,
            CURRENT_PROCESS_PSEUDO_HANDLE,
            TOKEN_QUERY,
            token_out,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let handle = read_u32(&memory, token_out).unwrap();
        assert_eq!(handle, TOKEN_HANDLE_BASE);
        assert_eq!(xp.last_error, ERROR_NO_TOKEN);
        assert_eq!(xp.token_handles.get(&handle).unwrap().pid, 2);
        assert_eq!(xp.token_handles.get(&handle).unwrap().access, TOKEN_QUERY);

        write_u32(&mut memory, esp, 0x0045_ef95).unwrap();
        write_u32(&mut memory, esp + 4, handle).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert!(!xp.token_handles.contains_key(&handle));
        assert_eq!(xp.call_count, 2);
    }

    #[test]
    fn child_get_token_information_reports_and_writes_the_token_groups_shape() {
        let providers = [
            ProviderImport {
                module: "ADVAPI32.dll".into(),
                symbol: ProviderSymbol::Name("OpenProcessToken".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "ADVAPI32.dll".into(),
                symbol: ProviderSymbol::Name("GetTokenInformation".into()),
                iat_rva: 0,
            },
        ];
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let token_out = esp - 4;
        for (index, value) in [
            0x0045_ef82,
            CURRENT_PROCESS_PSEUDO_HANDLE,
            TOKEN_QUERY,
            token_out,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let token = read_u32(&memory, token_out).unwrap();

        let return_len = esp - 8;
        for (index, value) in [0x0045_efb0, token, TOKEN_GROUPS_CLASS, 0, 0, return_len]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(read_u32(&memory, return_len).unwrap(), XP_TOKEN_GROUPS_REQUIRED);
        assert_eq!(xp.last_error, ERROR_INSUFFICIENT_BUFFER);

        let information = esp - 0x100;
        for (index, value) in [
            0x0045_efc3,
            token,
            TOKEN_GROUPS_CLASS,
            information,
            XP_TOKEN_GROUPS_REQUIRED,
            return_len,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        assert_eq!(read_u32(&memory, return_len).unwrap(), XP_TOKEN_GROUPS_REQUIRED);
        assert_eq!(read_u32(&memory, information).unwrap(), 1);
        assert_eq!(read_u32(&memory, information + 4).unwrap(), information + 12);
        assert_eq!(read_u32(&memory, information + 8).unwrap(), 7);
        let mut sid = [0; XP_EVERYONE_SID.len()];
        memory.read(information + 12, &mut sid).unwrap();
        assert_eq!(sid, XP_EVERYONE_SID);
        assert_eq!(xp.last_error, ERROR_INSUFFICIENT_BUFFER);
        assert_eq!(xp.call_count, 3);
    }

    #[test]
    fn child_allocate_and_initialize_sid_and_equal_sid_use_canonical_guest_sids() {
        let providers = [
            ProviderImport {
                module: "ADVAPI32.dll".into(),
                symbol: ProviderSymbol::Name("AllocateAndInitializeSid".into()),
                iat_rva: 0,
            },
            ProviderImport {
                module: "ADVAPI32.dll".into(),
                symbol: ProviderSymbol::Name("EqualSid".into()),
                iat_rva: 0,
            },
        ];
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(providers.to_vec(), Vec::new(), Vec::new());
        let mut memory = ProcessMemory {
            stack: vec![0; STACK_BYTES],
            process_data: vec![0; 0x1000],
        };
        let esp = STACK_TOP - 0x100;
        let authority = esp - 0x10;
        let sid_out = esp - 0x14;
        memory.write(authority, &[0, 0, 0, 0, 0, 5]).unwrap();
        for (index, value) in [
            0x0045_ef45,
            authority,
            2,
            32,
            544,
            0,
            0,
            0,
            0,
            0,
            0,
            sid_out,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let sid = read_u32(&memory, sid_out).unwrap();
        assert_eq!(sid, PROCESS_SID_ARENA_BASE);
        let mut bytes = [0; 16];
        memory.read(sid, &mut bytes).unwrap();
        assert_eq!(
            bytes,
            [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0]
        );

        for (index, value) in [0x0045_ef60, sid, sid].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, value).unwrap();
        }
        assert_eq!(canonical_sid(&memory, sid).unwrap(), bytes);
        assert_eq!(xp.equal_sid(esp, &memory), Ok(1));
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(1)),
            "EqualSid identical dispatch"
        );

        let different = sid + 0x40;
        memory.write(different, &bytes).unwrap();
        memory.write(different + 8, &33u32.to_le_bytes()).unwrap();
        write_u32(&mut memory, esp + 8, different).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Return(0)),
            "EqualSid distinct dispatch"
        );
        assert_eq!(xp.call_count, 3);
    }

    #[test]
    fn child_get_current_process_id_returns_calling_pid_without_side_effects() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentProcessId".into()),
            iat_rva: 0,
        };
        for pid in [2, 41] {
            let mut xp = XpProcess::new_child();
            xp.install_provider_surface(vec![provider.clone()], Vec::new(), Vec::new());
            xp.set_last_error(0x1234_5678);
            let mut memory = Memory {
                base: STACK_TOP - 4,
                bytes: 0x0046_12c3u32.to_le_bytes().to_vec(),
            };
            let before = memory.bytes.clone();
            for tid in [3, 91] {
                assert_eq!(
                    xp.dispatch_provider_for_process_typed(pid, tid, 0, STACK_TOP - 4, &mut memory),
                    Ok(PersonalityAction::Return(pid))
                );
                assert_eq!(xp.last_error, 0x1234_5678);
                assert_eq!(memory.bytes, before);
            }
            assert_eq!(xp.call_count, 2);
        }
    }

    #[test]
    fn child_get_current_process_id_dynamic_export_has_zero_argument_return() {
        let provider = ProviderImport {
            module: "kernel32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentProcessId".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetCurrentProcessId);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);

        let mut xp = XpProcess::new_child();
        assert_eq!(xp.provider_export_address("KERNEL32.dll", &provider.symbol), None);
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("KERNEL32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[..9], &[0xb8, 0, 0, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]);
    }

    #[test]
    fn child_get_current_thread_dynamic_export_has_zero_argument_return() {
        let provider = ProviderImport {
            module: "kernel32.dll".into(),
            symbol: ProviderSymbol::Name("GetCurrentThread".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetCurrentThread);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Return
        );

        let mut xp = XpProcess::new_child();
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("KERNEL32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[..9], &[0xb8, 0, 0, 0, 0, 0x0f, 0x01, 0xc1, 0xc3]);
    }

    #[test]
    fn child_open_thread_token_dynamic_export_uses_stdcall_sixteen() {
        let provider = ProviderImport {
            module: "advapi32.dll".into(),
            symbol: ProviderSymbol::Name("OpenThreadToken".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::OpenThreadToken);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(16)
        );

        let mut xp = XpProcess::new_child();
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("ADVAPI32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[8..11], &[0xc2, 0x10, 0]);
    }

    #[test]
    fn child_open_process_token_dynamic_export_uses_stdcall_twelve() {
        let provider = ProviderImport {
            module: "advapi32.dll".into(),
            symbol: ProviderSymbol::Name("OpenProcessToken".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::OpenProcessToken);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 12);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(12)
        );

        let mut xp = XpProcess::new_child();
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("ADVAPI32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[8..11], &[0xc2, 0x0c, 0]);
    }

    #[test]
    fn child_get_token_information_dynamic_export_uses_stdcall_twenty() {
        let provider = ProviderImport {
            module: "advapi32.dll".into(),
            symbol: ProviderSymbol::Name("GetTokenInformation".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GetTokenInformation);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 20);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(20)
        );

        let mut xp = XpProcess::new_child();
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("ADVAPI32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[8..11], &[0xc2, 0x14, 0]);
    }

    #[test]
    fn child_create_file_a_dynamic_export_uses_stdcall_twenty_eight() {
        let provider = ProviderImport {
            module: "kernel32.dll".into(),
            symbol: ProviderSymbol::Name("CreateFileA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CreateFileA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 28);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(28)
        );

        let mut xp = XpProcess::new_child();
        let (addresses, _, _, updated_from, bytes) =
            xp.append_provider_imports(vec![provider.clone()]).unwrap();
        assert_eq!(updated_from, 0);
        assert_eq!(addresses, vec![thunk32::address(0).unwrap()]);
        assert_eq!(
            xp.provider_export_address("KERNEL32.dll", &provider.symbol),
            Some(addresses[0])
        );
        assert_eq!(&bytes[8..11], &[0xc2, 0x1c, 0]);
    }

    #[test]
    fn child_file_dynamic_exports_use_win32_stdcall_cleanup() {
        for (symbol, operation, cleanup) in [
            ("GetFileSize", ProviderOp::GetFileSize, 8),
            ("SetFilePointer", ProviderOp::SetFilePointer, 16),
            ("ReadFile", ProviderOp::ReadFile, 20),
            ("WriteFile", ProviderOp::WriteFile, 20),
            ("FlushFileBuffers", ProviderOp::FlushFileBuffers, 4),
        ] {
            let provider = ProviderImport {
                module: "kernel32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            let actual = provider_op(&provider);
            assert_eq!(actual, operation, "{symbol}");
            assert!(actual.is_modeled(), "{symbol}");
            assert!(actual.is_generic_process_local(), "{symbol}");
            assert_eq!(actual.stack_cleanup_bytes(), cleanup, "{symbol}");
            assert_eq!(
                crate::child_loader::provider_thunk_kind(&provider),
                thunk32::Kind::Stdcall(cleanup),
                "{symbol}"
            );
        }
    }

    #[test]
    fn child_sid_dynamic_exports_use_their_win32_stdcall_cleanup() {
        for (symbol, operation, cleanup) in [
            (
                "AllocateAndInitializeSid",
                ProviderOp::AllocateAndInitializeSid,
                44,
            ),
            ("EqualSid", ProviderOp::EqualSid, 8),
        ] {
            let provider = ProviderImport {
                module: "advapi32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            assert_eq!(provider_op(&provider), operation);
            assert!(operation.is_modeled());
            assert!(operation.is_generic_process_local());
            assert_eq!(operation.stack_cleanup_bytes(), cleanup);
            assert_eq!(
                crate::child_loader::provider_thunk_kind(&provider),
                thunk32::Kind::Stdcall(cleanup)
            );
        }
    }

    #[test]
    fn child_exit_process_is_non_returning_lifecycle_action() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("ExitProcess".into()),
            iat_rva: 0,
        };
        let mut xp = XpProcess::new_child();
        xp.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0046_7796).unwrap();
        write_u32(&mut memory, esp + 4, 0xc000_0005).unwrap();

        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::ExitProcess(0xc000_0005))
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_create_event_a_provider_decodes_anonymous_auto_reset_frame() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("CreateEventA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::CreateEventA);
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        let mut xp = XpProcess::new_child();
        let (_, _, _, _, thunk_bytes) = xp.append_provider_imports(vec![provider]).unwrap();
        assert_eq!(&thunk_bytes[8..11], &[0xc2, 0x10, 0]);

        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        for (offset, value) in [0x0046_151c, 0, 0, 0, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + offset as u32 * 4, value).unwrap();
        }
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::CreateEvent(
                CreateEventRequest {
                    name: None,
                    manual_reset: false,
                    initial_state: false,
                    inheritable: false,
                },
            ))),
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_reset_event_provider_is_stdcall_and_dispatches_event_handle() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("ResetEvent".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::ResetEvent);
        assert!(operation.is_modeled());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut xp = XpProcess::new_child();
        let (_, _, _, _, thunk_bytes) = xp.append_provider_imports(vec![provider]).unwrap();
        assert_eq!(&thunk_bytes[8..11], &[0xc2, 4, 0]);

        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_3f19).unwrap();
        write_u32(&mut memory, esp + 4, 0x5743_2812).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::ResetEvent {
                pid: 2,
                tid: 3,
                handle: 0x5743_2812,
            })),
        );
        assert_eq!(xp.call_count, 1);
    }

    #[test]
    fn child_global_alloc_uses_the_win32_heap_arena_for_fixed_blocks() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GlobalAlloc".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&provider);
        assert_eq!(operation, ProviderOp::GlobalAlloc);
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 8);
        let mut xp = XpProcess::new_child();
        let (_, _, _, _, thunk_bytes) = xp.append_provider_imports(vec![provider]).unwrap();
        assert_eq!(&thunk_bytes[8..11], &[0xc2, 8, 0]);

        let zeroed = xp.alloc_global_fixed(0x40, 9).unwrap().unwrap();
        assert_eq!(zeroed.flags, 0x40);
        assert_eq!(zeroed.requested, 9);
        assert_eq!(zeroed.pointer, CHILD_WIN_HEAP_BASE);
        assert_eq!(zeroed.end, CHILD_WIN_HEAP_BASE + 16);
        let fixed = xp.alloc_global_fixed(0, 8).unwrap().unwrap();
        assert_eq!(fixed.pointer, zeroed.end);
        assert_eq!(fixed.end, zeroed.end + 8);
        assert_eq!(xp.call_count, 2);
        assert_eq!(
            xp.alloc_global_fixed(0x2, 8),
            Err(ProviderDispatchError::Frontier {
                api: "GlobalAlloc",
                detail: "movable-memory flags=0x00000002 bytes=8".into(),
            }),
        );
    }

    #[test]
    fn child_message_box_a_has_win32_stdcall_shape() {
        let provider = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("MessageBoxA".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&provider);

        assert_eq!(operation, ProviderOp::MessageBoxA);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(
            crate::child_loader::provider_thunk_kind(&provider),
            thunk32::Kind::Stdcall(16)
        );
    }

    #[test]
    fn child_mutex_providers_decode_real_stdcall_frames_and_last_error() {
        let providers = ["CreateMutexA", "ReleaseMutex", "CloseHandle", "WaitForSingleObject", "GetLastError"]
            .map(|symbol| ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            });
        let mut xp = XpProcess::new_child();
        let (addresses, _, _, _, bytes) = xp.append_provider_imports(providers.to_vec()).unwrap();
        for (id, cleanup) in [12, 4, 4, 8, 0].into_iter().enumerate() {
            assert_eq!(xp.provider_export_address("kernel32.dll", &providers[id].symbol), Some(addresses[id]));
            let tail = &bytes[id * thunk32::THUNK_BYTES + 8..];
            if cleanup == 0 {
                assert_eq!(tail[0], 0xc3);
            } else {
                assert_eq!(&tail[..3], &[0xc2, cleanup, 0]);
            }
        }
        let mut memory = Memory { base: STACK_BASE, bytes: vec![0; STACK_BYTES] };
        let esp = STACK_TOP - 0x40;
        let name = STACK_TOP - 0x100;
        let attributes = STACK_TOP - 0x200;
        memory.write(name, b"CMS32_MUTEX\0").unwrap();
        for (offset, value) in [0x0046_134e, attributes, 1, name].into_iter().enumerate() {
            write_u32(&mut memory, esp + offset as u32 * 4, value).unwrap();
        }
        for (offset, value) in [12, 0, 1].into_iter().enumerate() {
            write_u32(&mut memory, attributes + offset as u32 * 4, value).unwrap();
        }
        let key = ThreadKey { pid: 2, tid: 3 };
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::CreateMutex {
                key,
                request: CreateMutexRequest { name: Some("CMS32_MUTEX".into()), initial_owner: true, inheritable: true },
            }))
        );
        write_u32(&mut memory, esp + 4, 0x5743_2001).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 1, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::ReleaseMutex { key, handle: 0x5743_2001 }))
        );
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 2, esp, &mut memory),
            Ok(PersonalityAction::Session(SessionRequest::CloseHandle { pid: 2, handle: 0x5743_2001 }))
        );
        write_u32(&mut memory, esp, 0x0046_1457).unwrap();
        write_u32(&mut memory, esp + 8, u32::MAX).unwrap();
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 3, esp, &mut memory),
            Ok(PersonalityAction::Block(WaitRequest {
                key, return_address: 0x0046_1457, count: 1, handles_pointer: 0,
                handles: [0x5743_2001, 0], wait_all: 0, timeout: u32::MAX,
            }))
        );
        xp.set_last_error(183);
        assert_eq!(
            xp.dispatch_provider_for_process_typed(2, 3, 4, esp, &mut memory),
            Ok(PersonalityAction::Return(183))
        );
        assert_eq!(xp.last_error, 183);
        assert_eq!(xp.call_count, 5);
    }

    #[test]
    fn child_get_module_handle_a_resolves_native_module_aliases() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        pid2.register_native_module("MSS32.DLL", "Mss32.dll", 0x2110_0000)
            .unwrap();
        pid2.register_native_module("Storm.dll", "Storm.dll", 0x1500_0000)
            .unwrap();
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let name = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x2113_094f).unwrap();
        write_u32(&mut memory, esp + 4, name).unwrap();
        for spelling in [b"Mss32.dll\0".as_slice(), b"mss32.dll\0", b"MSS32.DLL\0"] {
            memory.write(name, spelling).unwrap();
            assert_eq!(
                pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
                Ok(PersonalityAction::Return(0x2110_0000))
            );
        }
        memory.write(name, b"Storm.dll\0").unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0x1500_0000))
        );
        memory.write(name, b"Unknown.dll\0").unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(0))
        );
        assert_eq!(pid2.last_error, ERROR_MOD_NOT_FOUND);
        assert_eq!(pid2.call_count, 5);
    }

    #[test]
    fn child_get_module_handle_a_resolves_stable_external_provider_handles() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new_child();
        pid2.install_provider_surface(
            vec![provider],
            Vec::new(),
            vec![
                ChildProvider::External {
                    module: "KERNEL32.dll".into(),
                },
                ChildProvider::External {
                    module: "MSVCRT.dll".into(),
                },
            ],
        );
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let name = STACK_TOP - 0x200;
        write_u32(&mut memory, esp, 0x2113_094f).unwrap();
        write_u32(&mut memory, esp + 4, name).unwrap();
        memory.write(name, b"kernel32.dll\0").unwrap();
        let kernel = match pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory) {
            Ok(PersonalityAction::Return(handle)) => handle,
            other => panic!("unexpected KERNEL32 result: {other:?}"),
        };
        memory.write(name, b"KERNEL32.DLL\0").unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(kernel))
        );
        memory.write(name, b"MSVCRT.dll\0").unwrap();
        let msvcrt = match pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory) {
            Ok(PersonalityAction::Return(handle)) => handle,
            other => panic!("unexpected MSVCRT result: {other:?}"),
        };
        assert_ne!(kernel, 0);
        assert_ne!(kernel, msvcrt);
        assert_eq!(pid2.loaded_module_handle("KERNEL32.dll"), Some(kernel));
    }

    #[test]
    fn loaded_module_registration_rejects_conflicting_names_and_handles() {
        let mut pid2 = XpProcess::new_child();
        pid2.register_native_module("MSS32.DLL", "Mss32.dll", 0x2110_0000)
            .unwrap();
        pid2.register_native_module("mss32.dll", "MSS32.DLL", 0x2110_0000)
            .unwrap();
        assert_eq!(
            pid2.register_native_module("mss32.dll", "Mss32.dll", 0x2111_0000),
            Err("loaded module name collision")
        );
        assert_eq!(
            pid2.register_native_module("Other.dll", "Other.dll", 0x2110_0000),
            Err("loaded module handle collision")
        );
    }

    #[test]
    fn launcher_and_child_have_distinct_main_image_filenames() {
        let launcher = XpProcess::new(Vec::new());
        let child = XpProcess::new_child();
        assert_eq!(launcher.image.filename(), LAUNCHER_IMAGE_FILENAME);
        assert_eq!(child.image.filename(), CHILD_IMAGE_FILENAME);
        assert_ne!(launcher.image.filename(), child.image.filename());
    }

    #[test]
    fn launcher_get_acp_reuses_process_ansi_code_page() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "GetACP".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        write_u32(&mut memory, esp, 0x0040_6189).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(XP_ANSI_CODE_PAGE)
        );
    }

    #[test]
    fn child_get_cp_info_reuses_process_ansi_code_page() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetCPInfo".into()),
            iat_rva: 0,
        };
        let mut pid2 = XpProcess::new(Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let output = STACK_TOP - 0x100;
        write_u32(&mut memory, esp, 0x2113_61b0).unwrap();
        write_u32(&mut memory, esp + 4, XP_ANSI_CODE_PAGE).unwrap();
        write_u32(&mut memory, esp + 8, output).unwrap();
        assert_eq!(
            pid2.dispatch_provider_for_process_typed(2, 3, 0, esp, &mut memory),
            Ok(PersonalityAction::Return(1))
        );
        let mut info = [0; 0x14];
        memory.read(output, &mut info).unwrap();
        assert_eq!(u32::from_le_bytes(info[..4].try_into().unwrap()), 1);
        assert_eq!(info[4], b'?');
        assert_eq!(info[5..], [0; 0x0f]);
        assert_eq!(pid2.call_count, 1);
    }

    #[test]
    fn launcher_set_handle_count_reuses_process_compatibility_semantics() {
        let imports = vec![LauncherImport {
            id: 0,
            module: "KERNEL32.dll".into(),
            symbol: "SetHandleCount".into(),
            iat_rva: 0,
        }];
        let mut xp = XpProcess::new(imports);
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        let requested = 0x40;
        write_u32(&mut memory, esp, 0x0040_1e23).unwrap();
        write_u32(&mut memory, esp + 4, requested).unwrap();
        assert_eq!(
            xp.dispatch(1, 0, esp, &mut memory).unwrap(),
            PersonalityAction::Return(requested)
        );
    }

    #[test]
    fn child_heap_create_is_process_private_and_lazy() {
        let provider = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("HeapCreate".into()),
            iat_rva: 0,
        };
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        pid1.install_provider_surface(vec![provider.clone()], Vec::new(), Vec::new());
        pid2.install_provider_surface(vec![provider], Vec::new(), Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0x5a; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        for (index, word) in [0x2113_1b92, 0x0004_0000, 0x0000_1000, 0x0000_0000]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid2.dispatch_provider_for_process(2, 3, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0x5743_0001)
        );
        assert_eq!(
            pid2.heaps.get(&0x5743_0001),
            Some(&WinHeap {
                options: 0x0004_0000,
                initial_size: 0x0000_1000,
                maximum_size: 0,
            })
        );
        assert!(pid2.allocations.is_empty());
        for (index, word) in [0x0040_0000, 0, 0, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid1.dispatch_provider_for_process(1, 1, 0, esp, &mut memory)
                .unwrap(),
            PersonalityAction::Return(0x5743_0001)
        );
        // Each process already owns its default heap in addition to HeapCreate's heap.
        assert!(pid1.heaps.contains_key(&PROCESS_HEAP_HANDLE));
        assert!(pid2.heaps.contains_key(&PROCESS_HEAP_HANDLE));
        assert_eq!(pid1.heaps.len(), 2);
        assert_eq!(pid2.heaps.len(), 2);
        assert_ne!(pid1.heaps.get(&0x5743_0001), pid2.heaps.get(&0x5743_0001));
    }

    #[test]
    fn child_win_heap_allocations_are_private_aligned_and_bounded() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        for (index, word) in [0x2113_1b92, 1, 0x1000, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let pid1_heap = pid1.create_win_heap(esp, &memory).unwrap().handle;
        let pid2_heap = pid2.create_win_heap(esp, &memory).unwrap().handle;
        assert_eq!(pid2_heap, 0x5743_0001);

        for (index, word) in [0x2113_24c2, pid2_heap, 0x8, 1]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let first = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        assert_eq!(first.pointer, CHILD_WIN_HEAP_BASE);
        assert_eq!(first.end, CHILD_WIN_HEAP_BASE + 8);
        assert_eq!(first.flags & 0x8, 0x8);
        assert_eq!(pid2.win_heap_allocations.get(&first.pointer), Some(&first));

        for (index, word) in [0x2113_24c2, pid2_heap, 0, 9].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let second = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        assert_eq!(second.pointer, CHILD_WIN_HEAP_BASE + 8);
        assert_eq!(second.pointer % 8, 0);

        for (index, word) in [0x2113_24c2, pid2_heap, HEAP_GENERATE_EXCEPTIONS, 12]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let exceptional = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        assert_eq!(exceptional.flags, HEAP_GENERATE_EXCEPTIONS);
        assert_eq!(exceptional.requested, 12);

        for (index, word) in [
            0x2113_24c2,
            pid2_heap,
            HEAP_GENERATE_EXCEPTIONS | HEAP_ZERO_MEMORY,
            12,
        ]
        .into_iter()
        .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert!(pid2.alloc_win_heap(esp, &memory).unwrap().is_some());

        for (index, word) in [0x2113_24c2, pid2_heap, 0x10, 12].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid2.alloc_win_heap(esp, &memory),
            Err("HeapAlloc flags frontier")
        );

        for (index, word) in [0x2113_24c2, pid1_heap, 0, 1].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid1.alloc_win_heap(esp, &memory).unwrap().unwrap().pointer,
            CHILD_WIN_HEAP_BASE
        );

        write_u32(&mut memory, esp + 4, 0xdead_beef).unwrap();
        assert_eq!(pid2.alloc_win_heap(esp, &memory).unwrap(), None);
        assert_eq!(pid2.last_error, 6);

        pid2.win_heap_next = CHILD_WIN_HEAP_LIMIT - 8;
        for (index, word) in [0x2113_24c2, pid2_heap, 0, 9].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(pid2.alloc_win_heap(esp, &memory).unwrap(), None);
    }

    #[test]
    fn child_win_heap_free_releases_an_allocation_only_once() {
        let mut pid2 = XpProcess::new_child();
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;
        for (index, word) in [0x2113_1b92, 0, 0x1000, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let heap = pid2.create_win_heap(esp, &memory).unwrap().handle;
        for (index, word) in [0x2113_24c2, heap, 0, 16].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let allocation = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        for (index, word) in [0x2113_0706, heap, 0, allocation.pointer]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(
            pid2.free_win_heap(esp, &memory).unwrap(),
            Some(allocation)
        );
        assert!(!pid2.win_heap_allocations.contains_key(&allocation.pointer));
        assert_eq!(pid2.free_win_heap(esp, &memory).unwrap(), None);
        assert_eq!(pid2.last_error, 6);
    }

    #[test]
    fn child_win_heap_accepts_multiple_heaps_in_one_process() {
        let mut pid2 = XpProcess::new_child();
        let mut memory = Memory {
            base: STACK_BASE,
            bytes: vec![0; STACK_BYTES],
        };
        let esp = STACK_TOP - 0x40;

        for (index, word) in [0x2113_1b92, 1, 0x1000, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let heap1 = pid2.create_win_heap(esp, &memory).unwrap().handle;
        for (index, word) in [0x0046_847b, 1, 0x1000, 0].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let heap2 = pid2.create_win_heap(esp, &memory).unwrap().handle;
        assert_eq!(heap1, 0x5743_0001);
        assert_eq!(heap2, 0x5743_0002);

        for (index, word) in [0x2113_24c2, heap1, 0, 16].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let first = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        assert_eq!(first.heap, heap1);

        for (index, word) in [0x0046_765e, heap2, 0, 256].into_iter().enumerate() {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        let second = pid2.alloc_win_heap(esp, &memory).unwrap().unwrap();
        assert_eq!(second.heap, heap2);
        assert_ne!(first.pointer, second.pointer);
        assert!(second.pointer >= first.end);

        for (index, word) in [0x2113_0706, heap1, 0, second.pointer]
            .into_iter()
            .enumerate()
        {
            write_u32(&mut memory, esp + index as u32 * 4, word).unwrap();
        }
        assert_eq!(pid2.free_win_heap(esp, &memory).unwrap(), None);
        assert_eq!(pid2.last_error, 6);
        assert!(pid2.win_heap_allocations.contains_key(&second.pointer));

        write_u32(&mut memory, esp + 4, heap2).unwrap();
        assert_eq!(pid2.free_win_heap(esp, &memory).unwrap(), Some(second));
        assert!(!pid2.win_heap_allocations.contains_key(&second.pointer));
    }

    #[test]
    fn registry_handles_are_process_private() {
        let mut pid1 = XpProcess::new(Vec::new());
        let mut pid2 = XpProcess::new(Vec::new());
        let pid1_handle = pid1.open_registry_key(7, 0x0002_0019).unwrap();
        let pid2_handle = pid2.open_registry_key(7, 0x0002_0019).unwrap();
        assert_eq!(pid1_handle, 0x5743_8001);
        assert_eq!(pid2_handle, 0x5743_8001);
        assert_eq!(pid1.registry_handles.len(), 1);
        assert_eq!(pid2.registry_handles.len(), 1);
    }

    #[test]
    fn registry_close_removes_only_the_opened_handle() {
        let mut xp = XpProcess::new(Vec::new());
        let handle = xp.open_registry_key(7, 0x0002_0019).unwrap();

        assert!(xp.close_registry_handle(handle));
        assert_eq!(xp.registry_handle_node(handle), None);
        assert!(!xp.close_registry_handle(handle));
    }
