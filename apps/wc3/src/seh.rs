use trueos::x86::Registers;

pub const STATUS_ACCESS_VIOLATION: u32 = 0xc000_0005;
pub const STATUS_SINGLE_STEP: u32 = 0x8000_0004;
pub const STATUS_INTEGER_DIVIDE_BY_ZERO: u32 = 0xc000_0094;
pub const DISPOSITION_CONTINUE_EXECUTION: u32 = 0;
pub const DISPOSITION_CONTINUE_SEARCH: u32 = 1;
pub const DISPOSITION_NESTED_EXCEPTION: u32 = 2;
pub const DISPOSITION_COLLIDED_UNWIND: u32 = 3;
pub const X86_CONTEXT_BYTES: usize = 716;
pub const EXCEPTION_RECORD_BYTES: usize = 80;
pub const X86_CONTEXT_FULL: u32 = 0x0001_0007;
pub const X86_CONTEXT_DEBUG_REGISTERS: u32 = 0x0001_0010;
pub const X86_EFLAGS_TF: u32 = 1 << 8;

const DR6: usize = 20;
const DR7: usize = 24;
const X86_DEFAULT_DR7: u32 = 0x0000_0400;
const EDI: usize = 156;
const ESI: usize = 160;
const EBX: usize = 164;
const EDX: usize = 168;
const ECX: usize = 172;
pub const ECX_OFFSET: usize = ECX;
const EAX: usize = 176;
const EBP: usize = 180;
const EIP: usize = 184;
const EFLAGS: usize = 192;
const ESP: usize = 196;

fn put(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn get(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

pub fn encode_x86_context(registers: Registers, dr6: Option<u32>) -> [u8; X86_CONTEXT_BYTES] {
    let mut bytes = [0; X86_CONTEXT_BYTES];
    let mut flags = X86_CONTEXT_FULL;
    if dr6.is_some() {
        flags |= X86_CONTEXT_DEBUG_REGISTERS;
    }
    put(&mut bytes, 0, flags);
    if let Some(dr6) = dr6 {
        put(&mut bytes, DR6, dr6);
        put(&mut bytes, DR7, X86_DEFAULT_DR7);
    }
    put(&mut bytes, EDI, registers.edi); put(&mut bytes, ESI, registers.esi);
    put(&mut bytes, EBX, registers.ebx); put(&mut bytes, EDX, registers.edx);
    put(&mut bytes, ECX, registers.ecx); put(&mut bytes, EAX, registers.eax);
    put(&mut bytes, EBP, registers.ebp); put(&mut bytes, EIP, registers.eip);
    put(&mut bytes, EFLAGS, registers.eflags); put(&mut bytes, ESP, registers.esp);
    bytes
}

pub fn decode_x86_context(bytes: &[u8; X86_CONTEXT_BYTES], fs_base: u32) -> Result<Registers, &'static str> {
    if get(bytes, 0) & X86_CONTEXT_FULL != X86_CONTEXT_FULL { return Err("SEH context flags"); }
    Ok(Registers { edi: get(bytes, EDI), esi: get(bytes, ESI), ebx: get(bytes, EBX), edx: get(bytes, EDX), ecx: get(bytes, ECX), eax: get(bytes, EAX), ebp: get(bytes, EBP), eip: get(bytes, EIP), eflags: get(bytes, EFLAGS), esp: get(bytes, ESP), fs_base, ..Registers::default() })
}

/// The saved CONTEXT describes the interrupted instruction, while live handler
/// execution follows x86 exception-delivery rules and clears Trap Flag.
pub fn exception_handler_registers(
    interrupted: Registers,
    handler: u32,
    stack_pointer: u32,
) -> Registers {
    let mut handler_registers = interrupted;
    handler_registers.eip = handler;
    handler_registers.esp = stack_pointer;
    handler_registers.eflags &= !X86_EFLAGS_TF;
    handler_registers
}

pub fn encode_page_fault_exception_record(eip: u32, linear: u32, error: u32) -> [u8; EXCEPTION_RECORD_BYTES] {
    let mut bytes = [0; EXCEPTION_RECORD_BYTES];
    put(&mut bytes, 0, STATUS_ACCESS_VIOLATION);
    put(&mut bytes, 12, eip); put(&mut bytes, 16, 2);
    put(&mut bytes, 20, if error & 0x10 != 0 { 8 } else if error & 2 != 0 { 1 } else { 0 });
    put(&mut bytes, 24, linear); bytes
}

pub fn encode_single_step_exception_record(eip: u32) -> [u8; EXCEPTION_RECORD_BYTES] {
    let mut bytes = [0; EXCEPTION_RECORD_BYTES];
    put(&mut bytes, 0, STATUS_SINGLE_STEP);
    put(&mut bytes, 12, eip);
    bytes
}

pub fn encode_integer_divide_by_zero_exception_record(
    eip: u32,
) -> [u8; EXCEPTION_RECORD_BYTES] {
    let mut bytes = [0; EXCEPTION_RECORD_BYTES];
    put(&mut bytes, 0, STATUS_INTEGER_DIVIDE_BY_ZERO);
    put(&mut bytes, 12, eip);
    put(&mut bytes, 16, 0); // NumberParameters
    bytes
}

#[cfg(test)]
crate::wc3_seh_tests_1!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_step_record_has_no_parameters_or_unwind_flags() {
        let record = encode_single_step_exception_record(0x0046_1449);
        assert_eq!(u32::from_le_bytes(record[0..4].try_into().unwrap()), STATUS_SINGLE_STEP);
        assert_eq!(u32::from_le_bytes(record[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[8..12].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[12..16].try_into().unwrap()), 0x0046_1449);
        assert_eq!(u32::from_le_bytes(record[16..20].try_into().unwrap()), 0);
    }

    #[test]
    fn integer_divide_by_zero_record_has_no_parameters_or_unwind_flags() {
        let record = encode_integer_divide_by_zero_exception_record(0x0045_a0c0);
        assert_eq!(
            u32::from_le_bytes(record[0..4].try_into().unwrap()),
            STATUS_INTEGER_DIVIDE_BY_ZERO,
        );
        assert_eq!(u32::from_le_bytes(record[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[8..12].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(record[12..16].try_into().unwrap()), 0x0045_a0c0);
        assert_eq!(u32::from_le_bytes(record[16..20].try_into().unwrap()), 0);
    }

    #[test]
    fn exception_delivery_preserves_saved_tf_but_clears_live_handler_tf() {
        let interrupted = Registers {
            eflags: 0x0000_0106,
            fs_base: 0x0020_3000,
            ..Registers::default()
        };
        let saved_context = encode_x86_context(interrupted, None);
        let saved_registers = decode_x86_context(&saved_context, interrupted.fs_base).unwrap();
        let handler_registers =
            exception_handler_registers(interrupted, 0x0045_a0c0, 0x043f_fc00);

        assert_eq!(saved_registers.eflags & X86_EFLAGS_TF, X86_EFLAGS_TF);
        assert_eq!(handler_registers.eflags & X86_EFLAGS_TF, 0);
        assert_eq!(handler_registers.eflags, 0x0000_0006);
    }

    #[test]
    fn debug_context_carries_guest_debug_state() {
        let context = encode_x86_context(Registers::default(), Some(0x4000));
        assert_eq!(get(&context, 0) & X86_CONTEXT_DEBUG_REGISTERS, X86_CONTEXT_DEBUG_REGISTERS);
        assert_eq!(get(&context, DR6), 0x4000);
        assert_eq!(get(&context, DR7), X86_DEFAULT_DR7);
    }
}
