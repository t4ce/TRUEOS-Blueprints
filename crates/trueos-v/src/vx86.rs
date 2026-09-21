//! Generic 32-bit x86 address-space and execution-context ABI.
//!
//! This module deliberately contains no Win32 or application semantics. It is
//! the narrow transport used by Blueprint-owned compatibility environments.

use crate::bp_abi;

pub const PERMISSION_READ: u32 = 1 << 0;
pub const PERMISSION_WRITE: u32 = 1 << 1;
pub const PERMISSION_EXECUTE: u32 = 1 << 2;

/// Maximum request/response body carried by one TRUEOS VM call.
///
/// Keep bulk-transfer chunking above the raw ABI so every x86 client gets the
/// same behavior without exposing transport details in application code.
pub const PAGE_BYTES: usize = 4096;
pub const TRANSFER_BYTES: usize = PAGE_BYTES - 56;

pub const EXIT_VMCALL: u32 = 1;
pub const EXIT_EXCEPTION: u32 = 2;
pub const EXIT_EPT_VIOLATION: u32 = 3;
pub const EXIT_HLT: u32 = 4;
pub const EXIT_CANCELLED: u32 = 5;
pub const EXIT_OTHER: u32 = 255;

pub use bp_abi::{
    TrueosX86DebugRegistersV1 as DebugRegisters,
    TrueosX86ExitV1 as Exit,
    TrueosX86RegistersV1 as Registers,
};

#[inline]
pub fn address_space_create() -> Result<u64, i32> {
    let mut handle = 0;
    let rc = unsafe { bp_abi::trueos_cabi_x86_address_space_create_v1(&mut handle) };
    if rc == 0 && handle != 0 {
        Ok(handle)
    } else {
        Err(if rc == 0 { -95 } else { rc })
    }
}

#[inline]
pub fn address_space_destroy(handle: u64) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_address_space_destroy_v1(handle) })
}

#[inline]
pub fn address_space_map(
    handle: u64,
    guest_va: u32,
    len: u32,
    permissions: u32,
) -> Result<(), i32> {
    status(unsafe {
        bp_abi::trueos_cabi_x86_address_space_map_v1(handle, guest_va, len, permissions)
    })
}

#[inline]
pub fn address_space_unmap(handle: u64, guest_va: u32, len: u32) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_address_space_unmap_v1(handle, guest_va, len) })
}

#[inline]
pub fn address_space_read(handle: u64, guest_va: u32, out: &mut [u8]) -> Result<usize, i32> {
    let rc = unsafe {
        bp_abi::trueos_cabi_x86_address_space_read_v1(handle, guest_va, out.as_mut_ptr(), out.len())
    };
    count(rc, out.len())
}

#[inline]
pub fn address_space_write(handle: u64, guest_va: u32, data: &[u8]) -> Result<usize, i32> {
    let rc = unsafe {
        bp_abi::trueos_cabi_x86_address_space_write_v1(handle, guest_va, data.as_ptr(), data.len())
    };
    count(rc, data.len())
}

#[inline]
pub fn context_create(address_space: u64, registers: &Registers) -> Result<u64, i32> {
    let mut handle = 0;
    let rc =
        unsafe { bp_abi::trueos_cabi_x86_context_create_v1(address_space, registers, &mut handle) };
    if rc == 0 && handle != 0 {
        Ok(handle)
    } else {
        Err(if rc == 0 { -95 } else { rc })
    }
}

#[inline]
pub fn context_destroy(handle: u64) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_context_destroy_v1(handle) })
}

#[inline]
pub fn context_registers(handle: u64) -> Result<Registers, i32> {
    let mut registers = Registers::default();
    status(unsafe { bp_abi::trueos_cabi_x86_context_registers_get_v1(handle, &mut registers) })?;
    Ok(registers)
}

#[inline]
pub fn context_set_registers(handle: u64, registers: &Registers) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_context_registers_set_v1(handle, registers) })
}

#[inline]
pub fn context_debug_registers(handle: u64) -> Result<DebugRegisters, i32> {
    let mut registers = DebugRegisters::default();
    status(unsafe {
        bp_abi::trueos_cabi_x86_context_debug_registers_get_v1(handle, &mut registers)
    })?;
    Ok(registers)
}

#[inline]
pub fn context_set_debug_registers(
    handle: u64,
    registers: &DebugRegisters,
) -> Result<(), i32> {
    status(unsafe {
        bp_abi::trueos_cabi_x86_context_debug_registers_set_v1(handle, registers)
    })
}

#[inline]
pub fn context_run(handle: u64) -> Result<Exit, i32> {
    context_execute(handle, false)
}

#[inline]
pub fn context_resume(handle: u64) -> Result<Exit, i32> {
    context_execute(handle, true)
}

#[inline]
pub fn context_park(handle: u64) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_context_park_v1(handle) })
}

#[inline]
pub fn context_cancel(handle: u64) -> Result<(), i32> {
    status(unsafe { bp_abi::trueos_cabi_x86_context_cancel_v1(handle) })
}

fn context_execute(handle: u64, resume: bool) -> Result<Exit, i32> {
    let mut exit = Exit::default();
    let rc = unsafe {
        if resume {
            bp_abi::trueos_cabi_x86_context_resume_v1(handle, &mut exit)
        } else {
            bp_abi::trueos_cabi_x86_context_run_v1(handle, &mut exit)
        }
    };
    status(rc)?;
    Ok(exit)
}

const fn status(rc: i32) -> Result<(), i32> {
    if rc == 0 { Ok(()) } else { Err(rc) }
}

fn count(rc: isize, capacity: usize) -> Result<usize, i32> {
    if rc < 0 {
        return Err(i32::try_from(rc).unwrap_or(-95));
    }
    let count = usize::try_from(rc).map_err(|_| -95)?;
    (count <= capacity).then_some(count).ok_or(-95)
}
