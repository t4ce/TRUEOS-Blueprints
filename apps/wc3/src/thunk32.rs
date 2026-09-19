pub const THUNK_BASE: u32 = 0x0030_0000;
pub const THUNK_BYTES: usize = 12;
pub const THREAD_EXIT_OFFSET: usize = 0x0ff0;
pub const THREAD_EXIT_ADDRESS: u32 = THUNK_BASE + THREAD_EXIT_OFFSET as u32;
pub const THREAD_EXIT_AFTER_VMCALL: u32 = THREAD_EXIT_ADDRESS + 3;

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
}
