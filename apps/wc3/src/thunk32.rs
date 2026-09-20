pub const THUNK_BASE: u32 = 0x0030_0000;
pub const THUNK_BYTES: usize = 12;
pub const THREAD_EXIT_OFFSET: usize = 0x0ff0;
pub const THREAD_EXIT_ADDRESS: u32 = THUNK_BASE + THREAD_EXIT_OFFSET as u32;
pub const THREAD_EXIT_AFTER_VMCALL: u32 = THREAD_EXIT_ADDRESS + 3;
pub const GUEST_RETURN_OFFSET: usize = 0x0fe0;
pub const GUEST_RETURN_ADDRESS: u32 = THUNK_BASE + GUEST_RETURN_OFFSET as u32;
pub const GUEST_RETURN_AFTER_VMCALL: u32 = GUEST_RETURN_ADDRESS + 3;
pub const CHILD_CONTROL_BASE: u32 = 0x002f_0000;
pub const CHILD_DLL_RETURN_ADDRESS: u32 = CHILD_CONTROL_BASE;
pub const CHILD_DLL_RETURN_AFTER_VMCALL: u32 = CHILD_DLL_RETURN_ADDRESS + 3;
pub const CHILD_THREAD_EXIT_ADDRESS: u32 = CHILD_CONTROL_BASE + 0x10;
pub const CHILD_THREAD_EXIT_AFTER_VMCALL: u32 = CHILD_THREAD_EXIT_ADDRESS + 3;
pub const CHILD_CALLBACK_RETURN_ADDRESS: u32 = CHILD_CONTROL_BASE + 0x20;
pub const CHILD_CALLBACK_RETURN_AFTER_VMCALL: u32 = CHILD_CALLBACK_RETURN_ADDRESS + 3;
pub const CHILD_IMAGE_RETURN_ADDRESS: u32 = CHILD_CONTROL_BASE + 0x30;
pub const CHILD_IMAGE_RETURN_AFTER_VMCALL: u32 = CHILD_IMAGE_RETURN_ADDRESS + 3;
pub const CHILD_SEH_RETURN_ADDRESS: u32 = CHILD_CONTROL_BASE + 0x40;
pub const CHILD_SEH_RETURN_AFTER_VMCALL: u32 = CHILD_SEH_RETURN_ADDRESS + 3;
pub const CHILD_CIPOW_SPILL_OFFSET: usize = 0x100;
pub const CHILD_CIPOW_SPILL_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_SPILL_OFFSET as u32;
pub const CHILD_CIPOW_EXPONENT_OFFSET: usize = 0x200;
pub const CHILD_CIPOW_EXPONENT_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_EXPONENT_OFFSET as u32;
pub const CHILD_CIPOW_BASE_OFFSET: usize = 0x208;
pub const CHILD_CIPOW_BASE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_BASE_OFFSET as u32;
pub const CHILD_CIPOW_RESULT_OFFSET: usize = 0x210;
pub const CHILD_CIPOW_RESULT_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_RESULT_OFFSET as u32;
pub const CHILD_CIPOW_RESTORE_OFFSET: usize = 0x120;
pub const CHILD_CIPOW_RESTORE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_RESTORE_OFFSET as u32;
pub const CHILD_CIPOW_SPILL_AFTER_VMCALL: u32 = CHILD_CIPOW_SPILL_ADDRESS + 15;

pub fn install_child_controls(output: &mut [u8]) -> Result<(), &'static str> {
    for offset in [0usize, 0x10, 0x20, 0x30, 0x40] {
        let trap = output
            .get_mut(offset..offset + 5)
            .ok_or("child control range")?;
        trap.copy_from_slice(&[0x0f, 0x01, 0xc1, 0x0f, 0x0b]);
    }
    let spill = output.get_mut(CHILD_CIPOW_SPILL_OFFSET..CHILD_CIPOW_SPILL_OFFSET + 20).ok_or("cipow spill range")?;
    spill[..15].copy_from_slice(&[
        0xdd, 0x1d, CHILD_CIPOW_EXPONENT_ADDRESS as u8, (CHILD_CIPOW_EXPONENT_ADDRESS >> 8) as u8, (CHILD_CIPOW_EXPONENT_ADDRESS >> 16) as u8, (CHILD_CIPOW_EXPONENT_ADDRESS >> 24) as u8,
        0xdd, 0x1d, CHILD_CIPOW_BASE_ADDRESS as u8, (CHILD_CIPOW_BASE_ADDRESS >> 8) as u8, (CHILD_CIPOW_BASE_ADDRESS >> 16) as u8, (CHILD_CIPOW_BASE_ADDRESS >> 24) as u8,
        0x0f, 0x01, 0xc1,
    ]);
    spill[15..17].copy_from_slice(&[0x0f, 0x0b]);
    let restore = output.get_mut(CHILD_CIPOW_RESTORE_OFFSET..CHILD_CIPOW_RESTORE_OFFSET + 7).ok_or("cipow restore range")?;
    restore.copy_from_slice(&[0xdd, 0x05, CHILD_CIPOW_RESULT_ADDRESS as u8, (CHILD_CIPOW_RESULT_ADDRESS >> 8) as u8, (CHILD_CIPOW_RESULT_ADDRESS >> 16) as u8, (CHILD_CIPOW_RESULT_ADDRESS >> 24) as u8, 0xc3]);
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Kind {
    Stop,
    Return,
    Stdcall(u8),
}

pub fn write(import_id: u32, kind: Kind, output: &mut [u8]) -> Result<(), &'static str> {
    if output.len() < THUNK_BYTES {
        return Err("wc3 thunk buffer too small");
    }
    output[..THUNK_BYTES].fill(0x90);
    output[..8].copy_from_slice(&[
        0xB8,
        import_id as u8,
        (import_id >> 8) as u8,
        (import_id >> 16) as u8,
        (import_id >> 24) as u8,
        0x0F,
        0x01,
        0xC1,
    ]);
    match kind {
        Kind::Return => output[8] = 0xC3,
        Kind::Stdcall(bytes) => {
            output[8] = 0xC2;
            output[9] = bytes;
            output[10] = 0;
        }
        Kind::Stop => {
            output[8] = 0x0F;
            output[9] = 0x0B;
        }
    }
    Ok(())
}

pub fn install_thread_exit(output: &mut [u8]) -> Result<(), &'static str> {
    let trampoline = output
        .get_mut(THREAD_EXIT_OFFSET..THREAD_EXIT_OFFSET + 5)
        .ok_or("wc3 thread-exit thunk range")?;
    // Preserve ThreadProc's EAX return value across the trap. The Blueprint
    // identifies this boundary by EIP rather than by an import id.
    trampoline.copy_from_slice(&[0x0f, 0x01, 0xc1, 0x0f, 0x0b]);
    Ok(())
}

pub fn install_guest_return(output: &mut [u8]) -> Result<(), &'static str> {
    let trampoline = output
        .get_mut(GUEST_RETURN_OFFSET..GUEST_RETURN_OFFSET + 5)
        .ok_or("wc3 guest-return thunk range")?;
    trampoline.copy_from_slice(&[0x0f, 0x01, 0xc1, 0x0f, 0x0b]);
    Ok(())
}

pub const fn address(import_id: u32) -> Option<u32> {
    match import_id.checked_mul(THUNK_BYTES as u32) {
        Some(offset) => THUNK_BASE.checked_add(offset),
        None => None,
    }
}

#[cfg(test)]
mod tests {
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
