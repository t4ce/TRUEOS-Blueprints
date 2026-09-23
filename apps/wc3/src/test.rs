// Centralized test definitions for the wc3 library and binary.

#[macro_export]
macro_rules! wc3_main_tests_1 {
    () => {
#[cfg(test)]
mod tests_main_1 {
    use super::*;

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
            }),
            load_library_call: None,
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
        assert_eq!(pid1.heaps.len(), 1);
        assert_eq!(pid2.heaps.len(), 1);
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
    static GUEST_MAP_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
#[cfg(test)] mod tests_seh_1 { use super::*;
 #[test] fn context_round_trips_visible_registers() { let r=Registers { eax:1,ebx:2,ecx:3,edx:4,esi:5,edi:6,ebp:7,eip:8,esp:9,eflags:10,fs_base:11,..Registers::default() }; assert_eq!(decode_x86_context(&encode_x86_context(r, None), 11).unwrap(),r); }
 #[test] fn page_fault_execute_is_access_violation() { let b=encode_page_fault_exception_record(0,0,0x10); assert_eq!(get(&b,0),STATUS_ACCESS_VIOLATION); assert_eq!(get(&b,12),0); assert_eq!(get(&b,16),2); assert_eq!(get(&b,20),8); }
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
        let import = ProviderImport { module: "KERNEL32.dll".into(), symbol: ProviderSymbol::Name("SetUnhandledExceptionFilter".into()), iat_rva: 0 };
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
        for (provider_id, symbol) in [(622, "HeapCreate"), (623, "HeapAlloc"), (624, "HeapFree")] {
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
}
    };
}
