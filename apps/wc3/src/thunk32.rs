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
pub const CHILD_UEF_RETURN_ADDRESS: u32 = CHILD_CONTROL_BASE + 0x50;
pub const CHILD_UEF_RETURN_AFTER_VMCALL: u32 = CHILD_UEF_RETURN_ADDRESS + 3;
pub const CHILD_CIPOW_SPILL_OFFSET: usize = 0x100;
pub const CHILD_CIPOW_SPILL_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_SPILL_OFFSET as u32;
pub const CHILD_CIPOW_EXPONENT_OFFSET: usize = 0x200;
pub const CHILD_CIPOW_EXPONENT_ADDRESS: u32 =
    CHILD_CONTROL_BASE + CHILD_CIPOW_EXPONENT_OFFSET as u32;
pub const CHILD_CIPOW_BASE_OFFSET: usize = 0x208;
pub const CHILD_CIPOW_BASE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_BASE_OFFSET as u32;
pub const CHILD_CIPOW_RESULT_OFFSET: usize = 0x210;
pub const CHILD_CIPOW_RESULT_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_RESULT_OFFSET as u32;
pub const CHILD_CIPOW_RESTORE_OFFSET: usize = 0x120;
pub const CHILD_CIPOW_RESTORE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_CIPOW_RESTORE_OFFSET as u32;
pub const CHILD_CIPOW_SPILL_AFTER_VMCALL: u32 = CHILD_CIPOW_SPILL_ADDRESS + 15;
pub const CHILD_MEMMOVE_OFFSET: usize = 0x300;
pub const CHILD_MEMMOVE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_MEMMOVE_OFFSET as u32;
pub const CHILD_D3D8_VTABLE_OFFSET: usize = 0x400;
pub const CHILD_D3D8_VTABLE_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_D3D8_VTABLE_OFFSET as u32;
pub const CHILD_D3D8_OBJECT_OFFSET: usize = 0x440;
pub const CHILD_D3D8_OBJECT_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_D3D8_OBJECT_OFFSET as u32;
pub const CHILD_STRNCMP_OFFSET: usize = 0x500;
pub const CHILD_STRNCMP_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_STRNCMP_OFFSET as u32;
pub const CHILD_TOUPPER_OFFSET: usize = 0x540;
pub const CHILD_TOUPPER_ADDRESS: u32 = CHILD_CONTROL_BASE + CHILD_TOUPPER_OFFSET as u32;

// Initial C locale: ASCII a-z only. EAX carries the provider id until the
// domain guard passes. Invalid unsigned-char/EOF inputs VMCALL into the
// existing Rust frontier with the original stack and provider id intact.
const CHILD_TOUPPER_CODE: &[u8] = &[
    0x9c, 0x8b, 0x54, 0x24, 0x08, 0x81, 0xfa, 0xff, 0x00, 0x00, 0x00, 0x76,
    0x0a, 0x83, 0xfa, 0xff, 0x74, 0x05, 0x9d, 0x0f, 0x01, 0xc1, 0xc3, 0x89,
    0xd0, 0x8d, 0x4a, 0x9f, 0x83, 0xf9, 0x19, 0x77, 0x03, 0x83, 0xe8, 0x20,
    0x9d, 0xc3,
];

// Cdecl strncmp: byte reads only, unsigned-byte difference, stop at count,
// first mismatch or NUL. Preserve EFLAGS/ESI/EDI and do not touch memory for
// count=0. Reject pointer wrap before the next read. Guest faults remain faults.
const CHILD_STRNCMP_CODE: &[u8] = &[
    0x9c, 0x56, 0x57, 0x8b, 0x74, 0x24, 0x10, 0x8b, 0x7c, 0x24, 0x14, 0x8b,
    0x4c, 0x24, 0x18, 0x31, 0xc0, 0x85, 0xc9, 0x74, 0x19, 0x0f, 0xb6, 0x06,
    0x0f, 0xb6, 0x17, 0x29, 0xd0, 0x75, 0x0f, 0x85, 0xd2, 0x74, 0x0b, 0x49,
    0x74, 0x08, 0x46, 0x74, 0x09, 0x47, 0x74, 0x06, 0xeb, 0xe7, 0x5f, 0x5e,
    0x9d, 0xc3, 0x0f, 0x0b,
];


// Cdecl memmove: save EFLAGS/ESI/EDI; read dst/src/len at ESP+16/+20/+24;
// no-op for zero length or identical pointers; reject 32-bit range wrap;
// REP MOVSB forward unless dst lies inside the source range, then backward.
// Restore the incoming flags (including DF), nonvolatile registers, and stack;
// return the destination in EAX. Unmapped/protected memory faults normally.
const CHILD_MEMMOVE_CODE: &[u8] = &[
    0x9c, 0x56, 0x57, 0x8b, 0x44, 0x24, 0x10, 0x8b, 0x74, 0x24, 0x14, 0x8b,
    0x4c, 0x24, 0x18, 0x89, 0xc7, 0x85, 0xc9, 0x74, 0x2a, 0x39, 0xf7, 0x74,
    0x26, 0x8d, 0x51, 0xff, 0x01, 0xf2, 0x72, 0x23, 0x8d, 0x51, 0xff, 0x01,
    0xfa, 0x72, 0x1c, 0xfc, 0x39, 0xf7, 0x76, 0x11, 0x89, 0xfa, 0x29, 0xf2,
    0x39, 0xca, 0x73, 0x09, 0x8d, 0x74, 0x0e, 0xff, 0x8d, 0x7c, 0x0f, 0xff,
    0xfd, 0xf3, 0xa4, 0x5f, 0x5e, 0x9d, 0xc3, 0x0f, 0x0b,
];

pub fn install_child_controls(output: &mut [u8]) -> Result<(), &'static str> {
    output.get_mut(CHILD_TOUPPER_OFFSET..CHILD_TOUPPER_OFFSET + CHILD_TOUPPER_CODE.len())
        .ok_or("toupper helper range")?.copy_from_slice(CHILD_TOUPPER_CODE);
    output.get_mut(CHILD_STRNCMP_OFFSET..CHILD_STRNCMP_OFFSET + CHILD_STRNCMP_CODE.len())
        .ok_or("strncmp helper range")?.copy_from_slice(CHILD_STRNCMP_CODE);
    output.get_mut(CHILD_MEMMOVE_OFFSET..CHILD_MEMMOVE_OFFSET + CHILD_MEMMOVE_CODE.len())
        .ok_or("memmove helper range")?.copy_from_slice(CHILD_MEMMOVE_CODE);
    for offset in [0usize, 0x10, 0x20, 0x30, 0x40, 0x50] {
        let trap = output
            .get_mut(offset..offset + 5)
            .ok_or("child control range")?;
        trap.copy_from_slice(&[0x0f, 0x01, 0xc1, 0x0f, 0x0b]);
    }
    let spill = output
        .get_mut(CHILD_CIPOW_SPILL_OFFSET..CHILD_CIPOW_SPILL_OFFSET + 20)
        .ok_or("cipow spill range")?;
    spill[..15].copy_from_slice(&[
        0xdd,
        0x1d,
        CHILD_CIPOW_EXPONENT_ADDRESS as u8,
        (CHILD_CIPOW_EXPONENT_ADDRESS >> 8) as u8,
        (CHILD_CIPOW_EXPONENT_ADDRESS >> 16) as u8,
        (CHILD_CIPOW_EXPONENT_ADDRESS >> 24) as u8,
        0xdd,
        0x1d,
        CHILD_CIPOW_BASE_ADDRESS as u8,
        (CHILD_CIPOW_BASE_ADDRESS >> 8) as u8,
        (CHILD_CIPOW_BASE_ADDRESS >> 16) as u8,
        (CHILD_CIPOW_BASE_ADDRESS >> 24) as u8,
        0x0f,
        0x01,
        0xc1,
    ]);
    spill[15..17].copy_from_slice(&[0x0f, 0x0b]);
    let restore = output
        .get_mut(CHILD_CIPOW_RESTORE_OFFSET..CHILD_CIPOW_RESTORE_OFFSET + 7)
        .ok_or("cipow restore range")?;
    restore.copy_from_slice(&[
        0xdd,
        0x05,
        CHILD_CIPOW_RESULT_ADDRESS as u8,
        (CHILD_CIPOW_RESULT_ADDRESS >> 8) as u8,
        (CHILD_CIPOW_RESULT_ADDRESS >> 16) as u8,
        (CHILD_CIPOW_RESULT_ADDRESS >> 24) as u8,
        0xc3,
    ]);
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Kind {
    Stop,
    Return,
    Stdcall(u8),
    /// Cdecl guest-native memory move; no provider trap or temporary buffer.
    Memmove,
    /// Cdecl guest-native unsigned byte comparison.
    Strncmp,
    /// Initial C locale conversion, with provider fallback for invalid inputs.
    ToUpper,
}

pub fn write(import_id: u32, kind: Kind, output: &mut [u8]) -> Result<(), &'static str> {
    if output.len() < THUNK_BYTES {
        return Err("wc3 thunk buffer too small");
    }
    output[..THUNK_BYTES].fill(0x90);
    if kind == Kind::ToUpper {
        let next = address(import_id).and_then(|address| address.checked_add(10))
            .ok_or("toupper thunk address overflow")?;
        output[0] = 0xb8;
        output[1..5].copy_from_slice(&import_id.to_le_bytes());
        output[5] = 0xe9;
        output[6..10].copy_from_slice(&CHILD_TOUPPER_ADDRESS.wrapping_sub(next).to_le_bytes());
        return Ok(());
    }
    if matches!(kind, Kind::Memmove | Kind::Strncmp) {
        let target = if kind == Kind::Memmove { CHILD_MEMMOVE_ADDRESS } else { CHILD_STRNCMP_ADDRESS };
        let next = address(import_id).and_then(|address| address.checked_add(5))
            .ok_or("memmove thunk address overflow")?;
        output[0] = 0xe9;
        output[1..5].copy_from_slice(&target.wrapping_sub(next).to_le_bytes());
        return Ok(());
    }
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
        Kind::Memmove | Kind::Strncmp | Kind::ToUpper => unreachable!(),
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
crate::wc3_thunk32_tests_1!();
