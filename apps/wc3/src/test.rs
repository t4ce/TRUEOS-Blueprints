// Centralized test definitions for the wc3 library and binary.

#[macro_export]
macro_rules! wc3_main_tests_1 {
    () => {
#[cfg(test)]
mod tests_main_1 {
    use super::*;

    #[test]
    fn window_callback_return_policy_preserves_wndproc_lresult_for_dispatch() {
        assert_eq!(
            WindowCallbackReturn::Fixed(1).api_result(0x1234_5678),
            1
        );
        assert_eq!(
            WindowCallbackReturn::WndProc.api_result(0x1234_5678),
            0x1234_5678
        );
    }

    struct TestMemory {
        base: u32,
        bytes: Vec<u8>,
    }

    impl GuestMemory for TestMemory {
        fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
            let start = usize::try_from(
                address
                    .checked_sub(self.base)
                    .ok_or("test memory below base")?,
            )
            .map_err(|_| "test memory offset")?;
            output.copy_from_slice(
                self.bytes
                    .get(start..start + output.len())
                    .ok_or("test memory range")?,
            );
            Ok(())
        }

        fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
            let start = usize::try_from(
                address
                    .checked_sub(self.base)
                    .ok_or("test memory below base")?,
            )
            .map_err(|_| "test memory offset")?;
            self.bytes
                .get_mut(start..start + input.len())
                .ok_or("test memory range")?
                .copy_from_slice(input);
            Ok(())
        }
    }

    fn native_module(stored: &str, image_base: u32, entry_rva: u32) -> PendingNativeModule {
        PendingNativeModule {
            requested: stored.to_owned(),
            stored: stored.to_owned(),
            image: pe32::PeImage {
                image_base,
                entry_rva,
                size_of_image: 0,
                size_of_headers: 0,
                sections: Vec::new(),
                imports: Vec::new(),
                relocations: Vec::new(),
                exports: Vec::new(),
                image: Vec::new(),
            },
            initialized: false,
        }
    }

    #[test]
    fn rtl_unwind_current_head_restores_post_call_state() {
        let registers = Registers {
            eax: 0xffff_ffff,
            ebx: 0x1111_1111,
            ecx: 0x2222_2222,
            edx: 0x3333_3333,
            esi: 0x4444_4444,
            edi: 0x5555_5555,
            ebp: 0x6666_6666,
            esp: 0x043f_fc10,
            eip: 0x7fff_0000,
            eflags: 0x0001_0002,
            fs_base: 0x0020_3000,
            ..Registers::default()
        };

        let resumed = asupersync::rtl_unwind_current_target_registers(
            registers,
            registers.esp,
            0x0046_ae84,
            0x1234_5678,
        )
        .unwrap();

        assert_eq!(resumed.eip, 0x0046_ae84);
        assert_eq!(resumed.esp, 0x043f_fc24);
        assert_eq!(resumed.eax, 0x1234_5678);
        assert_eq!(resumed.ebx, registers.ebx);
        assert_eq!(resumed.ecx, registers.ecx);
        assert_eq!(resumed.edx, registers.edx);
        assert_eq!(resumed.esi, registers.esi);
        assert_eq!(resumed.edi, registers.edi);
        assert_eq!(resumed.ebp, registers.ebp);
        assert_eq!(resumed.eflags, registers.eflags);
        assert_eq!(resumed.fs_base, registers.fs_base);
    }

    #[test]
    fn rtl_unwind_current_head_rejects_stack_overflow() {
        assert_eq!(
            asupersync::rtl_unwind_current_target_registers(
                Registers::default(),
                u32::MAX - 19,
                0x0046_ae84,
                0,
            ),
            Err("RtlUnwind resume ESP overflow"),
        );
    }

    #[test]
    fn null_call_source_classifies_supported_indirect_call_forms() {
        let registers = Registers {
            eax: 0x0010_0000,
            ebx: 0,
            esi: 0x0020_0004,
            edi: 0x0030_0000,
            ..Registers::default()
        };

        assert_eq!(
            asupersync::classify_null_call_source(&[0x90, 0xff, 0xd6], registers, |_| None),
            asupersync::NullCallSource::Register {
                name: "esi",
                target: 0x0020_0004,
            },
        );
        assert_eq!(
            asupersync::classify_null_call_source(
                &[0xff, 0x15, 0x34, 0x12, 0x40, 0x00],
                registers,
                |slot| (slot == 0x0040_1234).then_some(0),
            ),
            asupersync::NullCallSource::AbsoluteMemory {
                slot: 0x0040_1234,
                target: Some(0),
            },
        );
        assert_eq!(
            asupersync::classify_null_call_source(
                &[0xff, 0x96, 0xfc, 0xff, 0xff, 0xff],
                registers,
                |slot| (slot == 0x0020_0000).then_some(0),
            ),
            asupersync::NullCallSource::RegisterMemory {
                name: "esi",
                displacement: -4,
                slot: 0x0020_0000,
                target: Some(0),
            },
        );
        assert_eq!(
            asupersync::classify_null_call_source(
                &[0xff, 0x57, 4],
                registers,
                |slot| (slot == 0x0030_0004).then_some(0),
            ),
            asupersync::NullCallSource::RegisterMemory {
                name: "edi",
                displacement: 4,
                slot: 0x0030_0004,
                target: Some(0),
            },
        );
        assert_eq!(
            asupersync::classify_null_call_source(
                &[0xff, 0x10],
                registers,
                |slot| (slot == 0x0010_0000).then_some(0),
            ),
            asupersync::NullCallSource::RegisterMemory {
                name: "eax",
                displacement: 0,
                slot: 0x0010_0000,
                target: Some(0),
            },
        );
    }

    #[test]
    fn get_proc_address_caller_recognizes_eax_absolute_stores() {
        assert_eq!(
            asupersync::eax_absolute_store(&[0xa3, 0x60, 0xa9, 0x49, 0x00]),
            Some(0x0049_a960),
        );
        assert_eq!(
            asupersync::eax_absolute_store(&[0x89, 0x05, 0x60, 0xa9, 0x49, 0x00]),
            Some(0x0049_a960),
        );
        assert_eq!(asupersync::eax_absolute_store(&[0x89, 0x45, 0xfc]), None);
    }

    #[test]
    fn process_data_va_is_private_between_launcher_and_child_address_spaces() {
        let launcher = AddressSpace::create().unwrap();
        let child = AddressSpace::create().unwrap();
        for address_space in [&launcher, &child] {
            address_space
                .map(
                    PROCESS_DATA_VA,
                    0x1000,
                    Permissions::READ | Permissions::WRITE,
                )
                .unwrap();
        }
        launcher
            .write(PROCESS_DATA_VA, wc3::process::COMMAND_LINE)
            .unwrap();
        child.write(PROCESS_DATA_VA, CHILD_COMMAND_LINE).unwrap();
        child.write(ENVIRONMENT_BLOCK_VA, &[0, 0, 0, 0]).unwrap();
        let mut launcher_bytes = [0; wc3::process::COMMAND_LINE.len()];
        let mut child_bytes = [0; CHILD_COMMAND_LINE.len()];
        let mut child_environment = [0; 4];
        launcher
            .read(PROCESS_DATA_VA, &mut launcher_bytes)
            .unwrap();
        child.read(PROCESS_DATA_VA, &mut child_bytes).unwrap();
        child
            .read(ENVIRONMENT_BLOCK_VA, &mut child_environment)
            .unwrap();
        assert_eq!(PROCESS_DATA_VA, 0x0021_1000);
        assert_eq!(launcher_bytes, wc3::process::COMMAND_LINE);
        assert_eq!(child_bytes, CHILD_COMMAND_LINE);
        assert_eq!(child_environment, [0, 0, 0, 0]);
        assert_ne!(launcher_bytes, CHILD_COMMAND_LINE);
    }

    #[test]
    fn child_dll_init_transitions_only_from_ready() {
        let modules = vec![
            native_module("Storm.dll", 0x1500_0000, 0x0003_2950),
            native_module("Mss32.dll", 0x2110_0000, 0x0002_f2e5),
        ];
        let mut state = ChildExecutionState::Loader;
        assert_eq!(
            begin_child_dll_init_state(&mut state, &modules),
            Err("child DLL init not ready")
        );

        state = ChildExecutionState::DllInitReady { native_index: 0 };
        assert_eq!(
            begin_child_dll_init_state(&mut state, &modules),
            Ok((0, 0x1500_0000, 0x1503_2950))
        );
        assert_eq!(
            state,
            ChildExecutionState::DllInitRunning { native_index: 0 }
        );
        assert!(!modules[0].initialized);
        assert!(!modules[1].initialized);
        assert_eq!(
            begin_child_dll_init_state(&mut state, &modules),
            Err("child DLL init not ready")
        );
    }

    #[test]
    fn rearm_existing_child_context_preserves_returned_thread_state_for_mss() {
        let address_space = AddressSpace::create().unwrap();
        address_space
            .map(
                STACK_BASE,
                STACK_BYTES,
                Permissions::READ | Permissions::WRITE,
            )
            .unwrap();
        let reserved = STACK_TOP - 0x20;
        address_space
            .write(reserved, &0x5743_3344u32.to_le_bytes())
            .unwrap();
        let returned = Registers {
            eax: 0x1111_1111,
            ebx: 0x2222_2222,
            ecx: 0x3333_3333,
            edx: 0x4444_4444,
            esi: 0x5555_5555,
            edi: 0x6666_6666,
            ebp: 0x7777_7777,
            eip: thunk32::CHILD_DLL_RETURN_AFTER_VMCALL,
            esp: STACK_TOP,
            eflags: 0x0000_0246,
            fs_base: 0x7ffde000,
        };
        let context = Context::create(&address_space, returned).unwrap();
        let mut guest = GuestContext {
            pid: 2,
            tid: 3,
            context,
            started: true,
            continuation: None,
            preemption_count: 0,
            last_preemption_page: 0,
            same_page_preemptions: 0,
        };
        let context_address = &guest.context as *const Context as usize;
        let mut child = PendingChild {
            pid: 2,
            tid: 3,
            active_thread_tid: 3,
            parked_threads: HashMap::new(),
            image: native_module("War3.exe", 0x0040_0000, 0).image,
            self_image_bytes: Arc::new(Vec::new()),
            native_modules: vec![
                native_module("Storm.dll", 0x1500_0000, 0x0003_2950),
                native_module("Mss32.dll", 0x2110_0000, 0x0002_f2e5),
            ],
            address_space,
            crt_heap_mapped_end: CHILD_CRT_HEAP_BASE,
            win_heap_mapped_end: CHILD_WIN_HEAP_BASE,
            provider_thunk_bytes: 0x1000,
            static_load_reserved: reserved,
            initterm: None,
            load_library_call: None,
            window_callback: None,
            cipow: None,
            cipow_diagnostic_logged: false,
            seh_handler_dumped: false,
            seh: None,
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
                prepared: true,
                native_requests: Vec::new(),
                next_native: 0,
            },
            execution: ChildExecutionState::DllInitReady { native_index: 1 },
        };

        let (name, entry, frame_esp) =
            arm_existing_child_dll_init(&mut child, &mut guest, 1, returned).unwrap();
        assert_eq!(name, "Mss32.dll");
        assert_eq!(entry, 0x2112_f2e5);
        assert_eq!(frame_esp, returned.esp - 16);
        assert_eq!(&guest.context as *const Context as usize, context_address);
        assert_eq!(guest.pid, 2);
        assert_eq!(guest.tid, 3);
        assert!(guest.started);
        assert_eq!(
            child.execution,
            ChildExecutionState::DllInitRunning { native_index: 1 }
        );

        let registers = guest.context.registers().unwrap();
        assert_eq!(registers.eip, 0x2112_f2e5);
        assert_eq!(registers.esp, returned.esp - 16);
        assert_eq!(registers.eflags, returned.eflags);
        assert_eq!(registers.fs_base, returned.fs_base);
        assert_eq!(registers.eax, returned.eax);
        assert_eq!(registers.ebx, returned.ebx);
        assert_eq!(registers.ecx, returned.ecx);
        assert_eq!(registers.edx, returned.edx);
        assert_eq!(registers.esi, returned.esi);
        assert_eq!(registers.edi, returned.edi);
        assert_eq!(registers.ebp, returned.ebp);

        let mut frame = [0; 16];
        child.address_space.read(frame_esp, &mut frame).unwrap();
        assert_eq!(u32::from_le_bytes(frame[0..4].try_into().unwrap()), thunk32::CHILD_DLL_RETURN_ADDRESS);
        assert_eq!(u32::from_le_bytes(frame[4..8].try_into().unwrap()), 0x2110_0000);
        assert_eq!(u32::from_le_bytes(frame[8..12].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(frame[12..16].try_into().unwrap()), reserved);
        assert_ne!(reserved, 0);
    }

    #[test]
    fn image_entry_has_readable_caller_headroom_below_stack_top() {
        let address_space = AddressSpace::create().unwrap();
        address_space
            .map(
                STACK_BASE,
                STACK_BYTES,
                Permissions::READ | Permissions::WRITE,
            )
            .unwrap();
        let reserved = STACK_TOP - 0x20;
        let marker = 0x5743_3344u32.to_le_bytes();
        address_space.write(reserved, &marker).unwrap();
        let returned = Registers {
            eip: thunk32::CHILD_DLL_RETURN_AFTER_VMCALL,
            esp: STACK_TOP,
            eflags: 0x0000_0246,
            fs_base: 0x0020_3000,
            ..Registers::default()
        };
        let context = Context::create(&address_space, returned).unwrap();
        let mut guest = GuestContext {
            pid: 2,
            tid: 3,
            context,
            started: true,
            continuation: None,
            preemption_count: 0,
            last_preemption_page: 0,
            same_page_preemptions: 0,
        };
        let mut child = PendingChild {
            pid: 2,
            tid: 3,
            active_thread_tid: 3,
            parked_threads: HashMap::new(),
            image: native_module("War3.exe", 0x0040_0000, 0).image,
            self_image_bytes: Arc::new(Vec::new()),
            native_modules: Vec::new(),
            address_space,
            crt_heap_mapped_end: CHILD_CRT_HEAP_BASE,
            win_heap_mapped_end: CHILD_WIN_HEAP_BASE,
            provider_thunk_bytes: 0x1000,
            static_load_reserved: reserved,
            initterm: None,
            load_library_call: None,
            window_callback: None,
            cipow: None,
            cipow_diagnostic_logged: false,
            seh_handler_dumped: false,
            seh: None,
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
                prepared: true,
                native_requests: Vec::new(),
                next_native: 0,
            },
            execution: ChildExecutionState::ImageEntryReady,
        };

        let (_, frame_esp) =
            arm_existing_child_image_entry(&mut child, &mut guest, returned).unwrap();

        assert_eq!(frame_esp, STACK_TOP - CHILD_IMAGE_ENTRY_HEADROOM);
        let mut caller = [0; CHILD_IMAGE_ENTRY_CALLER_BYTES];
        child.address_space.read(frame_esp, &mut caller).unwrap();
        assert_eq!(
            u32::from_le_bytes(caller[..4].try_into().unwrap()),
            thunk32::CHILD_IMAGE_RETURN_ADDRESS
        );
        assert!(caller[4..].iter().all(|byte| *byte == 0));
        let mut preserved_marker = [0; 4];
        child
            .address_space
            .read(child.static_load_reserved, &mut preserved_marker)
            .unwrap();
        assert_eq!(preserved_marker, marker);

        let entry_ebp = frame_esp - 4;
        let ebp_plus_8 = entry_ebp + 8;
        assert_eq!(ebp_plus_8, frame_esp + 4);
        assert!(ebp_plus_8 < STACK_TOP);
        assert!(frame_esp + CHILD_IMAGE_ENTRY_CALLER_BYTES as u32 <= STACK_TOP);
        let mut caller_slot = [0; 4];
        child.address_space.read(ebp_plus_8, &mut caller_slot).unwrap();
        assert_eq!(caller_slot, [0; 4]);
    }

    #[test]
    fn image_entry_caller_window_stays_below_stack_top() {
        let frame_esp = STACK_TOP - CHILD_IMAGE_ENTRY_HEADROOM;
        let prologue_ebp = frame_esp - 4;

        assert!(prologue_ebp + 8 < STACK_TOP);
        assert!(frame_esp + CHILD_IMAGE_ENTRY_CALLER_BYTES as u32 <= STACK_TOP);
    }

    #[test]
    fn reg_open_frame_is_diagnostic_only_and_preserves_all_guest_bytes() {
        let base = 0x0430_0000;
        let esp = 0x043f_ff00;
        let subkey = 0x043f_fe00;
        let mut memory = TestMemory {
            base,
            bytes: vec![0x5a; STACK_BYTES],
        };
        for (index, word) in [
            0x1502_e2f9u32,
            0x8000_0002,
            subkey,
            0,
            0x0002_0019,
            0x043f_fd00,
        ]
        .into_iter()
        .enumerate()
        {
            memory
                .write(esp + index as u32 * 4, &word.to_le_bytes())
                .unwrap();
        }
        memory.write(subkey, b"SOFTWARE\\Example\0").unwrap();
        let before = memory.bytes.clone();
        let frame = decode_reg_open_key_ex_a(&memory, esp).unwrap();
        assert_eq!(registry_root_name(frame.hkey), "HKEY_LOCAL_MACHINE");
        assert_eq!(frame.subkey.as_deref(), Some("SOFTWARE\\Example"));
        assert_eq!(frame.sam, 0x0002_0019);
        assert_eq!(memory.bytes, before);
    }

    #[test]
    fn child_exception_diagnostic_decodes_ud_without_guest_mutation() {
        let exception = decode_child_exception((1 << 31) | 6, 0);
        assert_eq!(exception.vector, Some(6));
        assert_eq!(exception.name, "#UD");
        assert!(exception.valid);
        assert_eq!(exception.error_valid, Some(false));
        assert_eq!(exception.error, None);
        assert_eq!(exception.fault_linear, None);
    }

    #[test]
    fn child_debug_exception_carries_qualification_as_debug_status() {
        let exception = decode_child_exception((1 << 31) | 1, 0x0000_4001);
        assert_eq!(exception.vector, Some(1));
        assert_eq!(exception.debug_status, Some(0x0000_4001));
    }

    #[test]
    fn table_fill_observation_recognizes_bs_single_steps_in_its_exact_range() {
        let registers = Registers {
            eip: 0x0046_14a5,
            ..Registers::default()
        };
        let bs = decode_child_exception((1 << 31) | 1, 0x0000_4000);
        let b0 = decode_child_exception((1 << 31) | 1, 0x0000_0001);

        assert!(asupersync::war3_dword_scan_single_step(bs, registers));
        assert!(!asupersync::war3_dword_scan_single_step(b0, registers));
        assert!(!asupersync::war3_dword_scan_single_step(
            bs,
            Registers {
                eip: 0x0046_1482,
                ..Registers::default()
            },
        ));
    }

    #[test]
    fn pure_bs_single_step_is_quiet_only_for_the_war3_seh_handler() {
        let registers = Registers {
            eip: 0x0046_14a5,
            ..Registers::default()
        };
        let bs = decode_child_exception((1 << 31) | 1, 0xffff_4ff0);
        let b0 = decode_child_exception((1 << 31) | 1, 0xffff_4001);

        assert!(asupersync::boring_war3_single_step(
            bs,
            registers,
            0x0045_a0c0,
        ));
        assert!(!asupersync::boring_war3_single_step(
            b0,
            registers,
            0x0045_a0c0,
        ));
        assert!(!asupersync::boring_war3_single_step(
            bs,
            Registers::default(),
            0x0045_a0c0,
        ));
        assert!(!asupersync::boring_war3_single_step(
            bs,
            registers,
            0x0045_a0c1,
        ));
    }

    #[test]
    fn child_exception_diagnostic_decodes_page_fault_and_rejects_invalid_info() {
        let exception = decode_child_exception((1 << 31) | (1 << 11) | 14, (1u64 << 32) | 2);
        assert_eq!(exception.vector, Some(14));
        assert_eq!(exception.name, "#PF");
        assert_eq!(exception.error, Some(2));
        assert_eq!(exception.fault_linear, Some(1));

        let invalid = decode_child_exception(0, 0);
        assert_eq!(invalid.vector, None);
        assert_eq!(invalid.name, "invalid-interruption-info");
        assert_eq!(invalid.interruption_type, None);
        assert_eq!(invalid.error_valid, None);
        assert_eq!(
            child_exception_fault_detail(exception),
            "linear=0x00000001 error=0x00000002 present=0 write=1 user=0 reserved=0 instruction_fetch=0"
        );
    }

    #[test]
    fn child_crt_mapping_grows_only_to_required_rw_pages() {
        assert_eq!(
            child_crt_mapping_end(CHILD_CRT_HEAP_BASE, CHILD_CRT_HEAP_BASE, 0x80).unwrap(),
            CHILD_CRT_HEAP_BASE + 0x1000,
        );
        assert_eq!(
            child_crt_mapping_end(
                CHILD_CRT_HEAP_BASE + 0x1000,
                CHILD_CRT_HEAP_BASE + 0xff8,
                0x10
            )
            .unwrap(),
            CHILD_CRT_HEAP_BASE + 0x2000,
        );
    }

    #[test]
    fn virtual_alloc_frame_decoder_reads_the_live_stdcall_frame() {
        let base = 0x0430_0000;
        let esp = 0x043f_ff00;
        let mut memory = TestMemory {
            base,
            bytes: vec![0; STACK_BYTES],
        };
        for (index, word) in [
            0x1502_02c1u32,
            0,
            0x0001_0000,
            0x0000_3000,
            0x0000_0004,
        ]
        .into_iter()
        .enumerate()
        {
            memory
                .write(esp + index as u32 * 4, &word.to_le_bytes())
                .unwrap();
        }
        assert_eq!(
            decode_virtual_alloc_frame(&memory, esp).unwrap(),
            VirtualAllocFrame {
                caller_ret: 0x1502_02c1,
                address: 0,
                size: 0x0001_0000,
                allocation_type: 0x0000_3000,
                protect: 0x0000_0004,
            }
        );
        assert_eq!(virtual_alloc_protect_name(0x04), "PAGE_READWRITE");
    }

    #[test]
    fn child_initterm_runs_non_null_callbacks_in_order_and_restores_provider() {
        let address_space = AddressSpace::create().unwrap();
        address_space
            .map(
                STACK_BASE,
                STACK_BYTES,
                Permissions::READ | Permissions::WRITE,
            )
            .unwrap();
        let begin = 0x0100_0000;
        address_space
            .map(begin, 0x1000, Permissions::READ | Permissions::WRITE)
            .unwrap();
        let entries = [0x1111_1111u32, 0, 0x2222_2222];
        for (index, target) in entries.into_iter().enumerate() {
            address_space
                .write(begin + index as u32 * 4, &target.to_le_bytes())
                .unwrap();
        }
        let provider_esp = 0x043f_ffa8;
        let provider_resume_eip = 0x0030_0fe0;
        let context = Context::create(
            &address_space,
            Registers {
                eip: provider_resume_eip,
                esp: provider_esp,
                eax: 338,
                ..Registers::default()
            },
        )
        .unwrap();
        let mut guest = GuestContext {
            pid: 2,
            tid: 3,
            context,
            started: true,
            continuation: None,
            preemption_count: 0,
            last_preemption_page: 0,
            same_page_preemptions: 0,
        };
        let child_image = native_module("War3.exe", 0x0040_0000, 0).image;
        let mut child = PendingChild {
            pid: 2,
            tid: 3,
            active_thread_tid: 3,
            parked_threads: HashMap::new(),
            image: child_image,
            self_image_bytes: Arc::new(Vec::new()),
            native_modules: vec![native_module("Storm.dll", 0x1500_0000, 0)],
            address_space,
            crt_heap_mapped_end: CHILD_CRT_HEAP_BASE,
            win_heap_mapped_end: CHILD_WIN_HEAP_BASE,
            provider_thunk_bytes: 0x1000,
            static_load_reserved: 0,
            initterm: Some(ChildInitterm {
                provider_resume_eip,
                provider_esp,
                begin,
                cursor: begin,
                end: begin + 12,
                callbacks_invoked: 0,
                rust_callbacks: 0,
            }),
            load_library_call: None,
            window_callback: None,
            cipow: None,
            cipow_diagnostic_logged: false,
            seh_handler_dumped: false,
            seh: None,
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
                prepared: true,
                native_requests: Vec::new(),
                next_native: 0,
            },
            execution: ChildExecutionState::DllInitRunning { native_index: 0 },
        };

        assert_eq!(
            advance_child_initterm(&mut child, &mut guest).unwrap(),
            InittermAdvance::CallbackScheduled
        );
        let first = guest.context.registers().unwrap();
        assert_eq!(first.eip, 0x1111_1111);
        assert_eq!(first.esp, provider_esp - 4);
        let mut callback_return = [0; 4];
        child
            .address_space
            .read(provider_esp - 4, &mut callback_return)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(callback_return),
            thunk32::CHILD_CALLBACK_RETURN_ADDRESS
        );

        let mut returned = first;
        returned.eip = thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL;
        returned.esp = provider_esp;
        returned.eax = 0xfeed_face;
        guest.context.set_registers(returned).unwrap();
        assert_eq!(
            advance_child_initterm(&mut child, &mut guest).unwrap(),
            InittermAdvance::CallbackScheduled
        );
        let second = guest.context.registers().unwrap();
        assert_eq!(second.eip, 0x2222_2222);
        assert_eq!(second.esp, provider_esp - 4);
        assert_eq!(child.initterm.as_ref().unwrap().callbacks_invoked, 2);

        let mut returned = second;
        returned.eip = thunk32::CHILD_CALLBACK_RETURN_AFTER_VMCALL;
        returned.esp = provider_esp;
        returned.eax = 0x1234_5678;
        guest.context.set_registers(returned).unwrap();
        assert_eq!(
            advance_child_initterm(&mut child, &mut guest).unwrap(),
            InittermAdvance::Complete
        );
        assert!(child.initterm.is_none());
        let complete = guest.context.registers().unwrap();
        assert_eq!(complete.eip, provider_resume_eip);
        assert_eq!(complete.esp, provider_esp);
        assert_eq!(complete.eax, 0);
    }
}
    };
}

#[macro_export]
macro_rules! wc3_process_tests_1 {
    () => {
#[cfg(test)]
mod tests_process_1 {
    use super::*;
    include!("test_process_state.rs");
    include!("test_process_providers.rs");
}
    };
}

#[macro_export]
macro_rules! wc3_imports_tests_1 {
    () => {
        #[cfg(test)]
        mod tests_imports_1 {
            use super::*;

            #[test]
            fn get_exit_code_process_is_a_kernel32_stdcall_eight_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "KERNEL32.dll".into(),
                    symbol: "GetExitCodeProcess".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::GetExitCodeProcess);
                assert_eq!(WinCall::GetExitCodeProcess.thunk_kind(), Kind::Stdcall(8));
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::GetExitCodeProcess.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 8, 0]);
            }

            #[test]
            fn set_event_is_a_kernel32_stdcall_four_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "KERNEL32.dll".into(),
                    symbol: "SetEvent".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::SetEvent);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::SetEvent.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn exit_thread_is_a_kernel32_stdcall_four_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "KERNEL32.dll".into(),
                    symbol: "ExitThread".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::ExitThread);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::ExitThread.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn set_last_error_is_a_kernel32_stdcall_four_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "KERNEL32.dll".into(),
                    symbol: "SetLastError".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::SetLastError);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::SetLastError.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn tls_get_value_is_a_kernel32_stdcall_four_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "KERNEL32.dll".into(),
                    symbol: "TlsGetValue".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::TlsGetValue);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::TlsGetValue.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn destroy_window_is_a_user32_stdcall_four_import() {
                let import = LauncherImport {
                    id: 0,
                    module: "USER32.dll".into(),
                    symbol: "DestroyWindow".into(),
                    iat_rva: 0,
                };
                assert_eq!(WinCall::from_import(&import), WinCall::DestroyWindow);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(0, WinCall::DestroyWindow.thunk_kind(), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }
        }
    };
}

#[macro_export]
macro_rules! wc3_assets_tests_1 {
    () => {
        #[cfg(test)]
        mod tests_assets_1 {
            use super::*;
            use std::cell::Cell;
            use trueos::async_fs::{DirEntry, NodeKind};

            // Any accidental guest mapping during a cache operation crosses this ABI.
            static GUEST_MAP_CALLS: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(0);

            #[unsafe(no_mangle)]
            extern "C" fn trueos_cabi_x86_address_space_map_v1(
                _handle: u64,
                _guest_va: u32,
                _len: u32,
                _permissions: u32,
            ) -> i32 {
                GUEST_MAP_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                -1
            }

            #[test]
            fn synthetic_file_lengths_do_not_truncate_to_guest_width() {
                assert_eq!(file_len(600 * 1024 * 1024), 629145600u64);
                let large = u32::MAX as usize + 600 * 1024 * 1024;
                assert_eq!(file_len(large), u64::from(u32::MAX) + 629145600);
            }

            fn listing() -> DirListing {
                DirListing {
                    entries: vec![DirEntry {
                        name: "War3.MPQ".into(),
                        kind: NodeKind::File,
                    }],
                    truncated: false,
                }
            }

            #[test]
            fn one_read_shared_allocation_independent_cursors() {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let reads = Cell::new(0);
                    let data = vec![1, 2, 3];
                    let original = data.as_ptr();
                    let mut cache = Wc3AssetCache::default();
                    assert!(
                        cache
                            .preload_with(&listing(), |path| {
                                assert_eq!(path, "/common/Warcraft III/War3.MPQ");
                                reads.set(reads.get() + 1);
                                std::future::ready(Ok(data))
                            })
                            .await
                            .unwrap()
                    );
                    assert!(
                        !cache
                            .preload_with(&listing(), |_| {
                                reads.set(reads.get() + 1);
                                std::future::ready(Err("must not read twice".into()))
                            })
                            .await
                            .unwrap()
                    );
                    let bytes = cache.lookup("war3.mpq").unwrap();
                    assert_eq!(bytes.as_ptr(), original);
                    for path in [
                        "War3.mpq",
                        "WAR3.MPQ",
                        ".\\war3.mpq",
                        "C:\\games\\Warcraft III\\war3.mpq",
                        "/common/Warcraft III/war3.mpq",
                    ] {
                        assert!(Arc::ptr_eq(&bytes, &cache.lookup(path).unwrap()));
                    }
                    assert_eq!(reads.get(), 1);
                    assert!(cache.lookup("war3.mpq.bak").is_none());
                    assert!(cache.lookup("war3.mpq/").is_none());
                    assert_eq!(
                        cache.war3_mpq().unwrap().stored_path(),
                        "/common/Warcraft III/War3.MPQ"
                    );
                    let mut a = ResidentFileHandle::new(Arc::clone(&bytes));
                    let b = ResidentFileHandle::new(Arc::clone(&bytes));
                    a.cursor = 600 * 1024 * 1024;
                    assert_eq!(a.cursor, 629145600u64);
                    a.cursor = u64::from(u32::MAX) + 600 * 1024 * 1024;
                    assert!(a.cursor > u64::from(u32::MAX));
                    assert_eq!(b.cursor, 0);
                    assert!(Arc::ptr_eq(&a.bytes, &b.bytes));
                    let _: u64 = a.len();
                    assert_eq!(a.len(), 3);
                    let mut fresh = Wc3AssetCache::default();
                    assert!(
                        fresh
                            .preload_with(&listing(), |_| {
                                reads.set(reads.get() + 1);
                                std::future::ready(Ok(vec![4]))
                            })
                            .await
                            .unwrap()
                    );
                    assert_eq!(reads.get(), 2);
                    assert_eq!(GUEST_MAP_CALLS.load(std::sync::atomic::Ordering::SeqCst), 0);
                });
            }

            #[test]
            fn failed_read_leaves_no_resident_backing() {
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap()
                    .block_on(async {
                        let mut cache = Wc3AssetCache::default();
                        assert_eq!(
                            cache
                                .preload_with(&listing(), |_| std::future::ready(Err(
                                    "out of memory".into()
                                )))
                                .await,
                            Err("out of memory".into())
                        );
                        assert!(cache.lookup("war3.mpq").is_none());
                    });
            }
        }
    };
}

#[macro_export]
macro_rules! wc3_seh_tests_1 {
    () => {
        #[cfg(test)]
        mod tests_seh_1 {
            use super::*;
            #[test]
            fn context_round_trips_visible_registers() {
                let r = Registers {
                    eax: 1,
                    ebx: 2,
                    ecx: 3,
                    edx: 4,
                    esi: 5,
                    edi: 6,
                    ebp: 7,
                    eip: 8,
                    esp: 9,
                    eflags: 10,
                    fs_base: 11,
                    ..Registers::default()
                };
                assert_eq!(
                    decode_x86_context(&encode_x86_context(r, None), 11).unwrap(),
                    r
                );
            }
            #[test]
            fn page_fault_execute_is_access_violation() {
                let b = encode_page_fault_exception_record(0, 0, 0x10);
                assert_eq!(get(&b, 0), STATUS_ACCESS_VIOLATION);
                assert_eq!(get(&b, 12), 0);
                assert_eq!(get(&b, 16), 2);
                assert_eq!(get(&b, 20), 8);
            }
        }
    };
}

#[macro_export]
macro_rules! wc3_child_loader_tests_1 {
    () => {
        #[cfg(test)]
        mod tests_child_loader_1 {
            use super::*;
            #[test]
            fn local_resolution_is_ascii_case_insensitive_and_ordinals_survive() {
                let listing = DirListing {
                    entries: vec![trueos::async_fs::DirEntry {
                        name: "Mss32.dll".into(),
                        kind: NodeKind::File,
                    }],
                    truncated: false,
                };
                let mut image = PeImage {
                    image_base: 0x400000,
                    entry_rva: 0,
                    size_of_image: 0x1000,
                    size_of_headers: 0,
                    sections: vec![],
                    imports: vec![
                        crate::pe32::ImportDescriptor {
                            module: "mss32.dll".into(),
                            symbol: ImportSymbol::Name("x".into()),
                            iat_rva: 0,
                        },
                        crate::pe32::ImportDescriptor {
                            module: "wsock32.dll".into(),
                            symbol: ImportSymbol::Ordinal(25),
                            iat_rva: 4,
                        },
                    ],
                    relocations: vec![],
                    exports: vec![],
                    image: vec![0; 8],
                };
                let surface = prepare(&mut image, &listing).unwrap();
                assert_eq!(surface.native[0].stored, "Mss32.dll");
                assert_eq!(surface.imports[0].symbol, ProviderSymbol::Ordinal(25));
                assert_eq!(u32::from_le_bytes(image.image[..4].try_into().unwrap()), 0);
                assert_eq!(
                    u32::from_le_bytes(image.image[4..8].try_into().unwrap()),
                    thunk32::THUNK_BASE
                );
            }

            #[test]
            fn msvcrt_acmdln_binds_as_data_without_shifting_provider_thunks() {
                let listing = DirListing {
                    entries: vec![],
                    truncated: false,
                };
                let mut image = PeImage {
                    image_base: 0x400000,
                    entry_rva: 0,
                    size_of_image: 0x1000,
                    size_of_headers: 0,
                    sections: vec![],
                    imports: vec![
                        crate::pe32::ImportDescriptor {
                            module: "MSVCRT.dll".into(),
                            symbol: ImportSymbol::Name("_acmdln".into()),
                            iat_rva: 0,
                        },
                        crate::pe32::ImportDescriptor {
                            module: "MSVCRT.dll".into(),
                            symbol: ImportSymbol::Name("malloc".into()),
                            iat_rva: 4,
                        },
                    ],
                    relocations: vec![],
                    exports: vec![],
                    image: vec![0; 8],
                };

                let surface = prepare(&mut image, &listing).unwrap();

                assert_eq!(surface.imports.len(), 2);
                assert_eq!(
                    u32::from_le_bytes(image.image[..4].try_into().unwrap()),
                    crate::process::CRT_ACMDLN_VA
                );
                assert_eq!(
                    u32::from_le_bytes(image.image[4..8].try_into().unwrap()),
                    thunk32::THUNK_BASE + thunk32::THUNK_BYTES as u32
                );
                assert_ne!(
                    &surface.thunks[..thunk32::THUNK_BYTES],
                    &[0x90; thunk32::THUNK_BYTES]
                );
            }

            #[test]
            fn initialize_critical_section_provider_uses_stdcall_cleanup() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(403, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);

                let unrelated = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("DefinitelyUnmodeled".into()),
                    iat_rva: 0,
                };
                thunk32::write(404, provider_thunk_kind(&unrelated), &mut bytes).unwrap();
                assert_eq!(bytes[8], 0xc3);
            }

            #[test]
            fn enter_critical_section_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("EnterCriticalSection".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(435, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn leave_critical_section_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("LeaveCriticalSection".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(437, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn set_unhandled_exception_filter_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("SetUnhandledExceptionFilter".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(409, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn virtual_alloc_provider_uses_stdcall_sixteen() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("VirtualAlloc".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(424, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x10, 0]);
            }

            #[test]
            fn virtual_free_provider_uses_stdcall_twelve_and_is_not_generic() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("VirtualFree".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::VirtualFree);
                assert!(operation.is_modeled());
                assert!(!operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 12);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(12));
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(425, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x0c, 0]);
            }

            #[test]
            fn rtl_unwind_is_runtime_stdcall_sixteen() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("RtlUnwind".into()),
                    iat_rva: 0,
                };

                let operation = provider_op(&import);

                assert_eq!(operation, ProviderOp::RtlUnwind);
                assert!(!operation.is_generic_process_local());
                assert!(operation.is_modeled());
                assert_eq!(operation.stack_cleanup_bytes(), 16);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(16));
            }

            #[test]
            fn exit_process_is_runtime_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("ExitProcess".into()),
                    iat_rva: 0,
                };

                let operation = provider_op(&import);

                assert_eq!(operation, ProviderOp::ExitProcess);
                assert!(operation.is_modeled());
                assert!(!operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn get_version_ex_a_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetVersionExA".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(581, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn free_environment_strings_w_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("FreeEnvironmentStringsW".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::FreeEnvironmentStringsW);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(613, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn get_startup_info_a_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetStartupInfoA".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetStartupInfoA);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(627, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn get_std_handle_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetStdHandle".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetStdHandle);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(625, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn get_file_type_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetFileType".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetFileType);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(626, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn set_handle_count_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("SetHandleCount".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::SetHandleCount);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(624, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn get_acp_is_pure_process_no_argument_provider() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetACP".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetACP);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 0);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(640, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(bytes[8], 0xc3);
            }

            #[test]
            fn get_cp_info_is_pure_process_stdcall_eight() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetCPInfo".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetCPInfo);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 8);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(8));
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(641, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x08, 0x00]);
            }

            #[test]
            fn get_string_type_w_is_pure_process_stdcall_sixteen() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetStringTypeW".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetStringTypeW);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 16);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(638, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x10, 0x00]);
            }

            #[test]
            fn multi_byte_to_wide_char_is_pure_process_stdcall_twenty_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("MultiByteToWideChar".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::MultiByteToWideChar);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 24);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(615, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x18, 0x00]);
            }

            #[test]
            fn lc_map_string_w_is_pure_process_stdcall_twenty_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("LCMapStringW".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::LCMapStringW);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 24);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(617, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x18, 0x00]);
            }

            #[test]
            fn get_module_file_name_a_is_pure_process_stdcall_twelve() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetModuleFileNameA".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetModuleFileNameA);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 12);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(642, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x0c, 0x00]);
            }

            #[test]
            fn get_module_handle_a_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetModuleHandleA);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(573, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
            }

            #[test]
            fn load_library_a_is_runtime_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("LoadLibraryA".into()),
                    iat_rva: 0,
                };

                let operation = provider_op(&import);

                assert_eq!(operation, ProviderOp::LoadLibraryA);
                assert!(!operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn get_proc_address_is_runtime_stdcall_eight() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetProcAddress".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetProcAddress);
                assert!(!operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 8);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(8));
            }

            #[test]
            fn get_current_process_is_pure_process_plain_return() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetCurrentProcess".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetCurrentProcess);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 0);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
                assert!(operation.is_modeled());
                assert!(!ProviderOp::Unknown.is_modeled());
            }

            #[test]
            fn get_windows_directory_a_is_pure_process_stdcall_eight() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetWindowsDirectoryA".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetWindowsDirectoryA);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 8);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(8));
            }

            #[test]
            fn get_system_directory_a_is_pure_process_stdcall_eight() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetSystemDirectoryA".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::GetSystemDirectoryA);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 8);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(8));
            }

            #[test]
            fn query_performance_frequency_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("QueryPerformanceFrequency".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::QueryPerformanceFrequency);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn query_performance_counter_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("QueryPerformanceCounter".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::QueryPerformanceCounter);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn get_system_time_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetSystemTime".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);

                assert_eq!(operation, ProviderOp::GetSystemTime);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn get_time_zone_information_is_pure_process_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("GetTimeZoneInformation".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);

                assert_eq!(operation, ProviderOp::GetTimeZoneInformation);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }

            #[test]
            fn time_get_time_is_pure_process_plain_return() {
                let import = ProviderImport {
                    module: "WINMM.dll".into(),
                    symbol: ProviderSymbol::Name("timeGetTime".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::TimeGetTime);
                assert!(operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 0);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
            }

            #[test]
            fn wide_char_to_multi_byte_provider_uses_stdcall_thirty_two() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("WideCharToMultiByte".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(612, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 0x20, 0x00]);
            }

            #[test]
            fn heap_providers_use_stdcall_twelve() {
                for (provider_id, symbol) in
                    [(622, "HeapCreate"), (623, "HeapAlloc"), (624, "HeapFree")]
                {
                    let import = ProviderImport {
                        module: "KERNEL32.dll".into(),
                        symbol: ProviderSymbol::Name(symbol.into()),
                        iat_rva: 0,
                    };
                    let mut bytes = [0; thunk32::THUNK_BYTES];
                    thunk32::write(provider_id, provider_thunk_kind(&import), &mut bytes).unwrap();
                    assert_eq!(&bytes[8..11], &[0xc2, 0x0c, 0], "{symbol}");
                }
            }

            #[test]
            fn set_last_error_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "KERNEL32.dll".into(),
                    symbol: ProviderSymbol::Name("SetLastError".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(408, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn malloc_provider_keeps_cdecl_return_cleanup() {
                let import = ProviderImport {
                    module: "MSVCRT.dll".into(),
                    symbol: ProviderSymbol::Name("malloc".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(337, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(bytes[8], 0xc3);
                assert_ne!(&bytes[8..11], &[0xc2, 4, 0]);
            }

            #[test]
            fn initterm_provider_keeps_cdecl_return_cleanup() {
                let import = ProviderImport {
                    module: "MSVCRT.dll".into(),
                    symbol: ProviderSymbol::Name("_initterm".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(338, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(bytes[8], 0xc3);
                assert_ne!(&bytes[8..11], &[0xc2, 8, 0]);
            }

            #[test]
            fn dllonexit_provider_keeps_cdecl_return_cleanup() {
                let import = ProviderImport {
                    module: "MSVCRT.dll".into(),
                    symbol: ProviderSymbol::Name("__dllonexit".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(342, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(bytes[8], 0xc3);
                assert_ne!(&bytes[8..11], &[0xc2, 12, 0]);
            }

            #[test]
            fn reg_open_key_ex_a_provider_uses_stdcall_twenty() {
                let import = ProviderImport {
                    module: "ADVAPI32.dll".into(),
                    symbol: ProviderSymbol::Name("RegOpenKeyExA".into()),
                    iat_rva: 0,
                };
                let mut bytes = [0; thunk32::THUNK_BYTES];
                thunk32::write(548, provider_thunk_kind(&import), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xc2, 20, 0]);
            }

            #[test]
            fn reg_close_key_provider_uses_stdcall_four() {
                let import = ProviderImport {
                    module: "ADVAPI32.dll".into(),
                    symbol: ProviderSymbol::Name("RegCloseKey".into()),
                    iat_rva: 0,
                };
                let operation = provider_op(&import);
                assert_eq!(operation, ProviderOp::RegCloseKey);
                assert!(operation.is_modeled());
                assert!(!operation.is_generic_process_local());
                assert_eq!(operation.stack_cleanup_bytes(), 4);
                assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
            }
        }
    };
}

#[macro_export]
macro_rules! wc3_thunk32_tests_1 {
    () => {
        #[cfg(test)]
        mod tests_thunk32_1 {
            use super::*;
            #[test]
            fn create_thread_has_real_stdcall_cleanup() {
                let mut bytes = [0; THUNK_BYTES];
                write(75, Kind::Stdcall(0x18), &mut bytes).unwrap();
                assert_eq!(&bytes[8..11], &[0xC2, 0x18, 0]);
            }

            #[test]
            fn thread_exit_trampoline_preserves_eax_for_blueprint_exit_state() {
                let mut page = [0x90; 0x1000];
                install_thread_exit(&mut page).unwrap();
                assert_eq!(
                    &page[THREAD_EXIT_OFFSET..THREAD_EXIT_OFFSET + 5],
                    &[0x0f, 0x01, 0xc1, 0x0f, 0x0b]
                );
            }

            #[test]
            fn guest_return_trampoline_is_distinct_from_thread_exit() {
                let mut page = [0x90; 0x1000];
                install_guest_return(&mut page).unwrap();
                assert_eq!(
                    &page[GUEST_RETURN_OFFSET..GUEST_RETURN_OFFSET + 5],
                    &[0x0f, 0x01, 0xc1, 0x0f, 0x0b]
                );
                assert_ne!(GUEST_RETURN_ADDRESS, THREAD_EXIT_ADDRESS);
            }

            #[test]
            fn ceil_uses_a_guest_native_cdecl_x87_thunk() {
                let mut page = [0x90; 0x1000];
                install_child_controls(&mut page).unwrap();
                assert_eq!(
                    &page[CHILD_CEIL_OFFSET..CHILD_CEIL_OFFSET + 40],
                    &[
                        0x83, 0xec, 0x04, 0xd9, 0x3c, 0x24, 0x66, 0x8b,
                        0x04, 0x24, 0x66, 0x25, 0xff, 0xf3, 0x66, 0x0d,
                        0x00, 0x08, 0x66, 0x89, 0x44, 0x24, 0x02, 0xd9,
                        0x6c, 0x24, 0x02, 0xdd, 0x44, 0x24, 0x08, 0xd9,
                        0xfc, 0xd9, 0x2c, 0x24, 0x83, 0xc4, 0x04, 0xc3,
                    ]
                );

                let mut thunk = [0; THUNK_BYTES];
                write(75, Kind::Ceil, &mut thunk).unwrap();
                assert_eq!(thunk[0], 0xe9);
                let displacement = i32::from_le_bytes(thunk[1..5].try_into().unwrap());
                assert_eq!(
                    address(75).unwrap().wrapping_add(5).wrapping_add_signed(displacement),
                    CHILD_CEIL_ADDRESS,
                );
            }

            #[test]
            fn floor_uses_a_guest_native_cdecl_x87_thunk() {
                let mut page = [0x90; 0x1000];
                install_child_controls(&mut page).unwrap();
                assert_eq!(
                    &page[CHILD_FLOOR_OFFSET..CHILD_FLOOR_OFFSET + 40],
                    &[
                        0x83, 0xec, 0x04, 0xd9, 0x3c, 0x24, 0x66, 0x8b,
                        0x04, 0x24, 0x66, 0x25, 0xff, 0xf3, 0x66, 0x0d,
                        0x00, 0x04, 0x66, 0x89, 0x44, 0x24, 0x02, 0xd9,
                        0x6c, 0x24, 0x02, 0xdd, 0x44, 0x24, 0x08, 0xd9,
                        0xfc, 0xd9, 0x2c, 0x24, 0x83, 0xc4, 0x04, 0xc3,
                    ]
                );

                let mut thunk = [0; THUNK_BYTES];
                write(75, Kind::Floor, &mut thunk).unwrap();
                assert_eq!(thunk[0], 0xe9);
                let displacement = i32::from_le_bytes(thunk[1..5].try_into().unwrap());
                assert_eq!(
                    address(75).unwrap().wrapping_add(5).wrapping_add_signed(displacement),
                    CHILD_FLOOR_ADDRESS,
                );
            }
        }
    };
}

#[macro_export]
macro_rules! wc3_session_tests_1 {
    () => {
#[cfg(test)]
mod tests_session_1 {
    use super::*;

    #[test]
    fn registry_image_parses_multiroot_case_insensitive_tree() {
        let fixture = b"Windows Registry Editor Version 5.00\n\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n\"x\"=dword:00000001\n[HKEY_LOCAL_MACHINE\\A]\n[HKEY_USERS\\B]\n[HKEY_CLASSES_ROOT\\C]\n[HKEY_CURRENT_CONFIG\\D]\n";
        let image = RegistryImage::parse(fixture).unwrap();
        let hkcu = image.root(0x8000_0001).unwrap();
        assert!(
            image
                .child_path(hkcu, "software\\BLIZZARD entertainment\\internal")
                .is_some()
        );
        assert!(
            image
                .root(0x8000_0002)
                .and_then(|root| image.child_path(root, "a"))
                .is_some()
        );
    }

    #[test]
    fn registry_state_starts_unloaded_and_utf16_rejects_odd_payloads() {
        let session = Wc3Session::new(XpProcess::new(Vec::new()));
        assert!(matches!(session.registry, RegistryState::Unloaded));
        assert!(matches!(
            RegistryImage::parse(&[0xff, 0xfe, b'R']),
            Err("registry UTF-16 odd trailing byte")
        ));
    }

    #[test]
    fn registry_index_leaves_unopened_values_lazy() {
        let fixture = b"[HKEY_CURRENT_USER\\A]\n\"huge\"=hex:aa,bb,cc,\\\n dd,ee\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n\"x\"=hex:01\n";
        let mut image = RegistryImage::parse(fixture).unwrap();
        let hkcu = image.root(0x8000_0001).unwrap();
        let a = image.child_path(hkcu, "a").unwrap();
        let internal = image
            .child_path(hkcu, "software\\blizzard entertainment\\internal")
            .unwrap();
        assert!(!image.values_loaded(a));
        assert!(!image.values_loaded(internal));
        image.ensure_values_loaded(internal).unwrap();
        assert!(image.values_loaded(internal));
        assert!(!image.values_loaded(a));
    }

    #[test]
    fn registry_lazy_values_decode_dword_as_little_endian_reg_dword() {
        let fixture = b"[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n\"Allow Local Files\"=dword:00000001\n";
        let mut image = RegistryImage::parse(fixture).unwrap();
        let hkcu = image.root(HKEY_CURRENT_USER).unwrap();
        let internal = image
            .child_path(hkcu, "software\\blizzard entertainment\\internal")
            .unwrap();

        assert!(!image.values_loaded(internal));
        image.ensure_values_loaded(internal).unwrap();
        let value = image.value(internal, "ALLOW LOCAL FILES").unwrap();
        assert_eq!(value.ty, 4);
        assert_eq!(value.bytes, [1, 0, 0, 0]);
    }

    #[test]
fn design_time_allow_local_files_is_present_and_explicitly_zero() {
        let value = crate::reg::lookup(
            crate::reg::HKEY_LOCAL_MACHINE,
            "software\\BLIZZARD entertainment\\warcraft iii",
            "ALLOW LOCAL FILES",
        )
        .unwrap();

        assert_eq!(value.ty, crate::reg::REG_DWORD);
        assert_eq!(value.bytes, [0, 0, 0, 0]);
        assert!(crate::reg::lookup(
            crate::reg::HKEY_CURRENT_USER,
            "Software\\Blizzard Entertainment\\Warcraft III",
            "Allow Local Files",
        )
        .is_none());
    }

    #[test]
    fn registry_root_helpers_classify_and_format_predefined_keys() {
        assert!(crate::reg::is_predefined_root(crate::reg::HKEY_CURRENT_USER));
        assert!(!crate::reg::is_predefined_root(0x5743_8201));
        assert_eq!(
            crate::reg::format_root_name(crate::reg::HKEY_CURRENT_USER),
            "HKEY_CURRENT_USER"
        );
    }

    #[test]
    fn xp_virtual_drive_topology_is_c_fixed_only() {
        assert_eq!(crate::process::xp_drive_type(None), crate::process::DRIVE_FIXED);
        assert_eq!(crate::process::xp_drive_type(Some("C:\\")), crate::process::DRIVE_FIXED);
        assert_eq!(crate::process::xp_drive_type(Some("c:/")), crate::process::DRIVE_FIXED);
        assert_eq!(
            crate::process::xp_drive_type(Some("D:\\")),
            crate::process::DRIVE_NO_ROOT_DIR
        );
        assert_eq!(
            crate::process::xp_drive_type(Some("A:\\")),
            crate::process::DRIVE_NO_ROOT_DIR
        );
        assert_eq!(
            crate::process::xp_drive_type(Some("")),
            crate::process::DRIVE_NO_ROOT_DIR
        );
    }

    #[test]
    fn xp_virtual_volume_exists_only_on_c() {
        assert!(crate::process::xp_volume_exists(None));
        assert!(crate::process::xp_volume_exists(Some("c:/")));
        assert!(!crate::process::xp_volume_exists(Some("D:\\")));
        assert!(!crate::process::xp_volume_exists(Some("")));
    }

    #[test]
    fn xp_virtual_disk_has_a_ten_gib_soft_cap() {
        let geometry = crate::process::xp_disk_geometry(Some("C:\\")).unwrap();
        assert_eq!(geometry, crate::process::XP_C_DISK_GEOMETRY);
        assert_eq!(
            u64::from(geometry.total_clusters)
                * u64::from(geometry.sectors_per_cluster)
                * u64::from(geometry.bytes_per_sector),
            crate::process::XP_C_DISK_BYTES
        );
        assert!(crate::process::xp_disk_geometry(Some("D:\\")).is_none());
    }

    #[test]
    fn registry_index_scans_utf16_crlf_giant_ignored_value_and_final_key() {
        let mut fixture = vec![0xff, 0xfe];
        fixture.extend(
            "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\A]\r\n\"blob\"=hex:"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        for _ in 0..1_100_000 {
            fixture.extend([b'a', 0, b'a', 0, b',', 0]);
        }
        fixture.extend(
            "\r\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        let mut progress = Vec::new();
        let image =
            RegistryImage::index(fixture, |scanned, keys| progress.push((scanned, keys))).unwrap();
        let hkcu = image.root(HKEY_CURRENT_USER).unwrap();
        assert_eq!(image.index_entry_count(), 2);
        assert!(
            image
                .child_path(hkcu, "software\\BLIZZARD entertainment\\internal")
                .is_some()
        );
        assert!(!progress.is_empty());
        assert_eq!(image.scanned_bytes(), image.backing.len());
        assert!(image.loaded_values.is_empty());
    }

    #[test]
    fn registry_root_and_relative_paths_are_case_insensitive() {
        let image = RegistryImage::parse(b"[HKEY_CURRENT_USER\\Software]\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment]\n[HKEY_CURRENT_USER\\Software\\Blizzard Entertainment\\Internal]\n").unwrap();
        let hkcu = image.root(HKEY_CURRENT_USER).unwrap();
        let software = image.child_path(hkcu, "software").unwrap();
        let blizzard = image
            .child_path(software, "BLIZZARD ENTERTAINMENT")
            .unwrap();
        assert!(image.child_path(blizzard, "internal").is_some());
        assert!(image.child_path(hkcu, "").is_some());
    }

    #[test]
    fn registry_hash_collision_candidate_requires_text_match() {
        let mut image = RegistryImage::parse(b"[HKEY_CURRENT_USER\\Actual]\n").unwrap();
        image.entries[0].path_hash = hash_query("different", image.encoding);
        assert!(image.find(HKEY_CURRENT_USER, "different").is_none());
    }

    fn request(handle: u32, timeout: u32) -> WaitRequest {
        WaitRequest {
            key: ThreadKey {
                pid: LAUNCHER_PID,
                tid: LAUNCHER_TID,
            },
            return_address: 0x0040_196f,
            count: 1,
            handles_pointer: 0,
            handles: [handle, 0],
            wait_all: 0,
            timeout,
        }
    }

    #[test]
    fn single_event_wait_consumes_auto_reset_and_preserves_manual_reset() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (auto, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(
            session.poll_single_wait(&request(auto, u32::MAX)).unwrap(),
            Some(0)
        );
        assert_eq!(
            session.event_state(LAUNCHER_PID, auto),
            Some((false, false))
        );
        assert_eq!(
            session.poll_single_wait(&request(auto, 0)).unwrap(),
            Some(0x102)
        );

        let (manual, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: true,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(
            session
                .poll_single_wait(&request(manual, u32::MAX))
                .unwrap(),
            Some(0)
        );
        assert_eq!(
            session.event_state(LAUNCHER_PID, manual),
            Some((true, true))
        );
    }

    fn mutex_request(name: &str, initial_owner: bool) -> CreateMutexRequest {
        CreateMutexRequest { name: Some(name.into()), initial_owner, inheritable: false }
    }

    #[test]
    fn named_mutex_initial_owner_recursion_and_cross_process_wait() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let owner = ThreadKey { pid: 1, tid: 1 };
        let child = session.create_child();
        let waiter = ThreadKey { pid: child.pid, tid: child.tid };
        let (first, existing) = session.create_mutex(owner, mutex_request("CMS32_MUTEX", true)).unwrap();
        assert!(!existing);
        let (second, existing) = session.create_mutex(waiter, mutex_request("CMS32_MUTEX", true)).unwrap();
        assert!(existing);
        assert_ne!(first, second);
        assert_eq!(session.poll_single_wait(&request(first, 0)).unwrap(), Some(0));
        let waiting = WaitRequest { key: waiter, ..request(second, u32::MAX) };
        assert_eq!(session.poll_single_wait(&waiting).unwrap(), None);
        assert_eq!(session.release_mutex(waiter, second), Err(288));
        assert_eq!(session.release_mutex(waiter, first), Err(6));
        session.block_wait(waiting.clone()).unwrap();
        assert_eq!(session.release_mutex(owner, first).unwrap(), Vec::new());
        assert!(session.blocked.contains_key(&waiter));
        assert_eq!(session.release_mutex(owner, first).unwrap(), vec![CompletedWait { request: waiting, result: 0 }]);
        assert!(!session.blocked.contains_key(&waiter));
        assert_eq!(session.poll_single_wait(&request(first, 0)).unwrap(), Some(0x102));
        assert_eq!(session.release_mutex(owner, first), Err(288));
        assert!(session.release_mutex(waiter, second).unwrap().is_empty());
        assert_eq!(session.poll_single_wait(&request(first, 0)).unwrap(), Some(0));
    }

    #[test]
    fn named_mutex_lifetime_case_sensitive_namespace_and_type_collisions() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let key = ThreadKey { pid: 1, tid: 1 };
        let (first, _) = session.create_mutex(key, mutex_request("CMS32_MUTEX", false)).unwrap();
        let object = session.process(1).unwrap().handles[&first].object;
        let (second, existing) = session.create_mutex(key, mutex_request("CMS32_MUTEX", true)).unwrap();
        assert!(existing);
        assert_eq!(session.release_mutex(key, second), Err(288));
        let (different, existing) = session.create_mutex(key, mutex_request("cms32_mutex", false)).unwrap();
        assert!(!existing);
        assert_ne!(session.process(1).unwrap().handles[&different].object, object);
        let (event_collision, _) = session.create_event(1, CreateEventRequest {
            name: Some("CMS32_MUTEX".into()), manual_reset: false, initial_state: false, inheritable: false,
        });
        assert_eq!(event_collision, 0);
        let (event, _) = session.create_event(1, CreateEventRequest {
            name: Some("EVENT".into()), manual_reset: false, initial_state: false, inheritable: false,
        });
        assert_eq!(session.create_mutex(key, mutex_request("EVENT", false)), Err(6));
        assert_eq!(session.release_mutex(key, event), Err(6));
        assert!(session.close_handle(1, first));
        assert_eq!(session.names.get("CMS32_MUTEX"), Some(&object));
        assert!(session.close_handle(1, second));
        assert!(!session.names.contains_key("CMS32_MUTEX"));
        assert!(!session.objects.contains_key(&object));
        assert!(!session.close_handle(1, second));
        let (recreated, existing) = session.create_mutex(key, mutex_request("CMS32_MUTEX", false)).unwrap();
        assert!(!existing);
        assert_ne!(session.process(1).unwrap().handles[&recreated].object, object);
    }

    #[test]
    fn mutex_owner_exit_reports_abandonment_and_transfers_ownership() {
        for process_exit in [false, true] {
            let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
            let child = session.create_child();
            let owner = ThreadKey { pid: child.pid, tid: child.tid };
            session.create_mutex(owner, mutex_request("CMS32_MUTEX", true)).unwrap();
            let waiter = ThreadKey { pid: 1, tid: 1 };
            let (handle, _) = session.create_mutex(waiter, mutex_request("CMS32_MUTEX", false)).unwrap();
            let waiting = request(handle, u32::MAX);
            session.block_wait(waiting.clone()).unwrap();
            let woken = if process_exit {
                session.terminate_process(child.pid, 0).unwrap()
            } else {
                session.signal_thread(owner, 0).unwrap()
            };
            assert_eq!(woken, vec![CompletedWait { request: waiting, result: 0x80 }]);
            assert!(session.release_mutex(waiter, handle).unwrap().is_empty());
            assert_eq!(session.poll_single_wait(&request(handle, 0)).unwrap(), Some(0));
        }
    }

    #[test]
    fn single_event_wait_blocks_only_for_positive_timeout_and_rejects_unknown_handles() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (handle, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: false,
                inheritable: false,
            },
        );
        assert_eq!(
            session.poll_single_wait(&request(handle, 100)).unwrap(),
            None
        );
        assert_eq!(
            session.poll_single_wait(&request(handle, 0)).unwrap(),
            Some(0x102)
        );
        assert_eq!(
            session
                .poll_single_wait(&request(0xdead_beef, 100))
                .unwrap(),
            Some(u32::MAX)
        );
    }

    #[test]
    fn process_exit_signals_persistent_process_and_thread_handles() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let child_key = ThreadKey {
            pid: child.pid,
            tid: child.tid,
        };

        assert_eq!(session.process_exit_code(child.pid), None);
        assert_eq!(session.thread_exit_code(child_key), None);
        assert_eq!(
            session
                .poll_single_wait(&request(child.process_handle, 0))
                .unwrap(),
            Some(0x0000_0102)
        );

        session.terminate_process(child.pid, 0xc000_0005).unwrap();

        assert!(session.process(child.pid).is_some());
        assert_eq!(session.process_exit_code(child.pid), Some(0xc000_0005));
        assert_eq!(session.thread_exit_code(child_key), Some(0xc000_0005));
        for timeout in [0, u32::MAX] {
            assert_eq!(
                session
                    .poll_single_wait(&request(child.process_handle, timeout))
                    .unwrap(),
                Some(0)
            );
            assert_eq!(
                session
                    .poll_single_wait(&request(child.thread_handle, timeout))
                    .unwrap(),
                Some(0)
            );
        }
    }

    #[test]
    fn get_exit_code_process_reads_process_object_lifecycle_state() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();

        assert_eq!(
            session.get_exit_code_process(LAUNCHER_PID, child.process_handle),
            Ok((child.pid, 259))
        );
        assert_eq!(
            session.get_exit_code_process(LAUNCHER_PID, child.thread_handle),
            Err("GetExitCodeProcess handle is not a process")
        );

        session.terminate_process(child.pid, 0xc000_0005).unwrap();
        assert_eq!(
            session.get_exit_code_process(LAUNCHER_PID, child.process_handle),
            Ok((child.pid, 0xc000_0005))
        );
    }

    #[test]
    fn signal_thread_wakes_waiters_on_the_persistent_thread_object() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let key = ThreadKey {
            pid: child.pid,
            tid: child.tid,
        };
        let wait = request(child.thread_handle, u32::MAX);
        session.block_wait(wait.clone()).unwrap();

        let woken = session.signal_thread(key, 0).unwrap();

        assert_eq!(session.thread_exit_code(key), Some(0));
        assert_eq!(
            woken,
            vec![CompletedWait {
                request: wait,
                result: 0,
            }]
        );
    }

    #[test]
    fn destroy_window_is_terminal_and_releases_focus_through_presentation() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let hwnd = session
            .create_window(CreateWindowRequest {
                owner: ThreadKey {
                    pid: LAUNCHER_PID,
                    tid: 2,
                },
                class: "splash".into(),
                wndproc: 0,
                class_icon: 0,
                class_cursor: 0,
                class_icon_sm: 0,
                title: "Warcraft III".into(),
                ex_style: 0,
                style: 0,
                x: 0,
                y: 0,
                width: 640,
                height: 480,
                parent: DESKTOP_HWND,
                menu: 0,
                instance: 0,
                param: 0,
            })
            .unwrap();
        session.set_focus(hwnd).unwrap();

        assert_eq!(
            session.destroy_window(LAUNCHER_PID, hwnd),
            Ok(DestroyWindowResult { was_focused: true })
        );
        assert!(!session.windows.contains_key(&hwnd));
        assert_eq!(session.focused_root(), None);
        assert_eq!(
            session.take_window_presentation(),
            Some(WindowPresentation::Destroy { hwnd })
        );
        assert_eq!(
            session.destroy_window(LAUNCHER_PID, hwnd),
            Err("DestroyWindow unknown window")
        );
    }

    #[test]
    fn get_window_long_a_reads_window_fields_and_zero_initialized_user_data() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let hwnd = session
            .create_window(CreateWindowRequest {
                owner: ThreadKey {
                    pid: LAUNCHER_PID,
                    tid: 2,
                },
                class: "window-long".into(),
                wndproc: 0x1234_5678,
                class_icon: 0,
                class_cursor: 0,
                class_icon_sm: 0,
                title: "window-long".into(),
                ex_style: 0x0000_0008,
                style: 0x80c0_0000,
                x: 0,
                y: 0,
                width: 640,
                height: 480,
                parent: 0,
                menu: 0x0000_0042,
                instance: 0x0040_0000,
                param: 0x0649_00c8,
            })
            .unwrap();

        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -4), Ok(0x1234_5678));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -6), Ok(0x0040_0000));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -8), Ok(0));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -12), Ok(0x42));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -16), Ok(0x80c0_0000));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -20), Ok(8));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, -21), Ok(0));
        assert_eq!(session.get_window_long_a(LAUNCHER_PID, hwnd, 0), Err("GetWindowLongA unobserved index"));
    }

    #[test]
    fn set_event_manual_reset_wakes_all_blocked_waiters_and_stays_signaled() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (event, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: true,
                initial_state: false,
                inheritable: false,
            },
        );
        let first = request(event, u32::MAX);
        let second = WaitRequest {
            key: ThreadKey {
                pid: LAUNCHER_PID,
                tid: 2,
            },
            ..first.clone()
        };
        session.block_wait(first.clone()).unwrap();
        session.block_wait(second.clone()).unwrap();

        let outcome = session.set_event(LAUNCHER_PID, event).unwrap();

        assert!(outcome.manual_reset);
        assert!(!outcome.was_signaled);
        assert_eq!(outcome.woken.len(), 2);
        assert!(outcome.woken.iter().all(|completed| completed.result == 0));
        assert_eq!(session.event_state(LAUNCHER_PID, event), Some((true, true)));
        assert!(!session.blocked.contains_key(&first.key));
        assert!(!session.blocked.contains_key(&second.key));
    }

    #[test]
    fn set_event_auto_reset_wakes_one_waiter_and_consumes_the_signal() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (event, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: false,
                inheritable: false,
            },
        );
        let first = request(event, u32::MAX);
        let second = WaitRequest {
            key: ThreadKey {
                pid: LAUNCHER_PID,
                tid: 2,
            },
            ..first.clone()
        };
        session.block_wait(first.clone()).unwrap();
        session.block_wait(second.clone()).unwrap();

        let outcome = session.set_event(LAUNCHER_PID, event).unwrap();

        assert!(!outcome.manual_reset);
        assert_eq!(outcome.woken.len(), 1);
        assert_eq!(session.event_state(LAUNCHER_PID, event), Some((false, false)));
        assert_eq!(session.blocked.len(), 1);
    }

    #[test]
    fn set_event_rejects_invalid_and_non_event_handles() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();

        assert_eq!(
            session.set_event(LAUNCHER_PID, 0xdead_beef),
            Err("SetEvent invalid handle")
        );
        assert_eq!(
            session.set_event(LAUNCHER_PID, child.process_handle),
            Err("SetEvent handle is not an event")
        );
    }

    #[test]
    fn reset_event_clears_signaled_state_and_rejects_non_events() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let (event, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: true,
                initial_state: true,
                inheritable: false,
            },
        );
        assert_eq!(session.event_state(LAUNCHER_PID, event), Some((true, true)));
        assert_eq!(
            session.reset_event(LAUNCHER_PID, event).unwrap(),
            ResetEventResult { manual_reset: true, was_signaled: true },
        );
        assert_eq!(session.event_state(LAUNCHER_PID, event), Some((true, false)));
        let child = session.create_child();
        assert_eq!(
            session.reset_event(LAUNCHER_PID, child.process_handle),
            Err("ResetEvent handle is not an event"),
        );
    }

    #[test]
    fn process_exit_wakes_a_blocked_single_waiter() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let wait = request(child.process_handle, u32::MAX);
        session.block_wait(wait.clone()).unwrap();

        let woken = session.terminate_process(child.pid, 7).unwrap();

        assert_eq!(
            woken,
            vec![CompletedWait {
                request: wait.clone(),
                result: 0,
            }]
        );
        assert!(!session.blocked.contains_key(&wait.key));
        assert_eq!(
            session
                .runnable
                .iter()
                .filter(|key| **key == wait.key)
                .count(),
            1
        );
    }

    fn multiple_request(handles: [u32; 2], wait_all: u32, timeout: u32) -> WaitRequest {
        WaitRequest {
            key: ThreadKey {
                pid: LAUNCHER_PID,
                tid: LAUNCHER_TID,
            },
            return_address: 0x0040_1362,
            count: 2,
            handles_pointer: 0x043f_ff00,
            handles,
            wait_all,
            timeout,
        }
    }

    #[test]
    fn process_exit_completes_two_object_wait_any_and_wait_all() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let wait_any = multiple_request(
            [child.process_handle, child.thread_handle],
            0,
            u32::MAX,
        );
        let wait_all = multiple_request(
            [child.process_handle, child.thread_handle],
            1,
            u32::MAX,
        );
        assert_eq!(session.poll_wait(&wait_any), Ok(None));
        assert_eq!(session.poll_wait(&wait_all), Ok(None));

        session.terminate_process(child.pid, 0xc000_0005).unwrap();

        assert_eq!(session.poll_wait(&wait_any), Ok(Some(0)));
        assert_eq!(
            session.poll_wait(&multiple_request(
                [child.thread_handle, child.process_handle],
                0,
                u32::MAX,
            )),
            Ok(Some(0))
        );
        assert_eq!(session.poll_wait(&wait_all), Ok(Some(0)));
    }

    #[test]
    fn wait_any_uses_lowest_signaled_index_and_wait_all_consumes_no_partial_event() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (auto, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: true,
                inheritable: false,
            },
        );
        let wait_all = multiple_request([auto, child.process_handle], 1, u32::MAX);
        assert_eq!(session.poll_wait(&wait_all), Ok(None));
        assert_eq!(session.event_state(LAUNCHER_PID, auto), Some((false, true)));

        session.terminate_process(child.pid, 7).unwrap();

        assert_eq!(session.poll_wait(&wait_all), Ok(Some(0)));
        assert_eq!(session.event_state(LAUNCHER_PID, auto), Some((false, false)));
        assert_eq!(
            session.poll_wait(&multiple_request([auto, child.process_handle], 0, 0)),
            Ok(Some(1))
        );
    }

    #[test]
    fn wait_any_consumes_only_the_selected_auto_reset_event() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (auto, _) = session.create_event(
            LAUNCHER_PID,
            CreateEventRequest {
                name: None,
                manual_reset: false,
                initial_state: true,
                inheritable: false,
            },
        );
        let wait = multiple_request([auto, child.process_handle], 0, 0);

        assert_eq!(session.poll_wait(&wait), Ok(Some(0)));
        assert_eq!(session.event_state(LAUNCHER_PID, auto), Some((false, false)));
        assert_eq!(session.poll_wait(&wait), Ok(Some(0x0000_0102)));
    }

    #[test]
    fn process_exit_wakes_a_blocked_two_object_wait() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let wait = multiple_request(
            [child.process_handle, child.thread_handle],
            0,
            u32::MAX,
        );
        session.block_wait(wait.clone()).unwrap();

        let completed = session.terminate_process(child.pid, 0xc000_0005).unwrap();

        assert_eq!(
            completed,
            vec![CompletedWait {
                request: wait.clone(),
                result: 0,
            }]
        );
        assert!(!session.blocked.contains_key(&wait.key));
        assert_eq!(
            session
                .runnable
                .iter()
                .filter(|key| **key == wait.key)
                .count(),
            1
        );
    }

    #[test]
    fn enqueue_deduplicates_a_runnable_child_thread() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let key = ThreadKey {
            pid: child.pid,
            tid: child.tid,
        };
        session.enqueue(key);
        session.enqueue(key);
        assert_eq!(
            session
                .runnable
                .iter()
                .filter(|queued| **queued == key)
                .count(),
            1
        );
    }

    #[test]
    fn thread_priority_uses_normal_class_bases_and_round_robins_equals() {
        let mut session = Wc3Session::new(XpProcess::new(Vec::new()));
        let child = session.create_child();
        let (first, first_handle) = session.create_child_thread(child.pid).unwrap();
        let (second, second_handle) = session.create_child_thread(child.pid).unwrap();

        assert_eq!(session.thread_base_priority(first), Some(8));
        assert_eq!(
            session.set_thread_priority(first, second_handle, 2),
            Ok((second, 0, 10))
        );
        assert_eq!(session.thread_base_priority(second), Some(10));
        assert_eq!(
            session.set_thread_priority(first, crate::process::CURRENT_THREAD_PSEUDO_HANDLE, -1),
            Ok((first, 0, 7))
        );
        assert_eq!(
            session.get_thread_priority(first, crate::process::CURRENT_THREAD_PSEUDO_HANDLE),
            Ok((first, -1, 7))
        );
        assert_eq!(session.get_thread_priority(first, second_handle), Ok((second, 2, 10)));
        assert_eq!(session.set_thread_priority(first, first_handle, 3), Err(87));
        assert_eq!(session.set_thread_priority(first, 0x1234_5678, 0), Err(6));
        assert_eq!(session.get_thread_priority(first, 0x1234_5678), Err(6));

        session.enqueue(first);
        session.enqueue(second);
        assert_eq!(
            session.take_highest_runnable(|key| key.pid == child.pid),
            Some(second)
        );

        assert_eq!(session.set_thread_priority(first, second_handle, -1), Ok((second, 2, 7)));
        session.enqueue(second);
        assert_eq!(
            session.take_highest_runnable(|key| key.pid == child.pid),
            Some(first)
        );
    }


}

#[test]
fn design_time_battle_net_gateways_exposes_the_rig_lan_realm() {
    let value = crate::reg::lookup(
        crate::reg::HKEY_CURRENT_USER,
        "software\\BLIZZARD entertainment\\warcraft iii",
        "BATTLE.NET GATEWAYS",
    )
    .unwrap();

    assert_eq!(value.ty, crate::reg::REG_MULTI_SZ);
    assert_eq!(
        value.bytes,
        b"1001\0\
          01\0\
          192.168.178.111\0\
          0\0\
          RoC 1.21b Realm\0\0"
    );
}
    };
}
