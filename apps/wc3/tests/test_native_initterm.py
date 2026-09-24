#!/usr/bin/env python3
"""Compare the Rust initializer planner with actual i386 execution (Linux IA32).
Requires rustc and GNU as/ld. No game assets, kernel, or emulator required.
"""
from pathlib import Path
import struct
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
CODE, DATA = 0x09000000, 0x09100000
word = lambda n: struct.pack('<I', n)
with tempfile.TemporaryDirectory(prefix='wc3-initterm-') as directory:
    work = Path(directory)
    (work/'plan.rs').write_text('''
#[path = "%s"] mod initterm;
fn main() {
    let code = std::fs::read("code.bin").unwrap();
    let mut data = std::fs::read("data.bin").unwrap();
    let effect = initterm::plan(0x09000000, |address, out, access| {
        let (base, bytes) = match access {
            initterm::Access::Code => (0x09000000, &code),
            initterm::Access::Data => (0x09100000, &data),
        };
        let Some(offset) = address.checked_sub(base) else { return false; };
        let Some(input) = bytes.get(offset as usize..offset as usize + out.len()) else { return false; };
        out.copy_from_slice(input); true
    }).unwrap();
    if let Some((address, value)) = effect.store {
        let offset = (address - 0x09100000) as usize;
        data[offset..offset+4].copy_from_slice(&value.to_le_bytes());
    }
    let mut output = effect.eax.unwrap_or(0xaabbccdd).to_le_bytes().to_vec();
    output.extend_from_slice(&data);
    std::fs::write("expected.bin", output).unwrap();
}
''' % (APP/'src/initterm.rs'))
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'plan.rs', '-o', 'plan'], cwd=work, check=True)
    (work/'layout.ld').write_text('''ENTRY(_start)
SECTIONS { . = 0x08048000; .text : { *(.text) }
 . = 0x09000000; .callbacks : { *(.callbacks) }
 . = 0x09100000; .data : { *(.data) } }
''')
    cases = 0
    for jumps in range(5):
        for kind in ('copy', 'alias', 'store', 'ret'):
            for flags in (0x202, 0x603, 0xed7):
                dest = DATA if kind == 'alias' else DATA+4
                terminal = (b'\xa1'+word(DATA)+b'\xa3'+word(dest)+b'\xc3' if kind in ('copy', 'alias')
                            else b'\xc7\x05'+word(dest)+word(0xdeadbeef)+b'\xc3' if kind == 'store' else b'\xc3')
                (work/'code.bin').write_bytes((b'\xe9'+word(0))*jumps+terminal)
                (work/'data.bin').write_bytes(word(0x12345678 ^ cases)+word(0x87654321))
                subprocess.run([str(work/'plan')], cwd=work, check=True)
                (work/'test.s').write_text(f'''.intel_syntax noprefix
.section .callbacks,"ax"
callback: .incbin "code.bin"
.section .data
values: .incbin "data.bin"
saved_sp: .long 0
output: .space 12
.section .text
.global _start
_start:
 mov [saved_sp], esp
 mov eax, 0xaabbccdd
 mov ebx, 0x11223344
 mov ecx, 0x22334455
 mov edx, 0x33445566
 mov esi, 0x44556677
 mov edi, 0x55667788
 mov ebp, 0x66778899
 push {flags}
 popfd
 call callback
 mov [output], eax
 pushfd
 pop eax
 and eax, 0xcd5
 cmp eax, {flags & 0xcd5}
 jne fail
 cmp esp, [saved_sp]
 jne fail
 cmp ebx, 0x11223344
 jne fail
 cmp ecx, 0x22334455
 jne fail
 cmp edx, 0x33445566
 jne fail
 cmp esi, 0x44556677
 jne fail
 cmp edi, 0x55667788
 jne fail
 cmp ebp, 0x66778899
 jne fail
 mov eax, [values]
 mov [output+4], eax
 mov eax, [values+4]
 mov [output+8], eax
 mov eax, 4
 mov ebx, 1
 mov ecx, offset output
 mov edx, 12
 int 0x80
 xor ebx, ebx
 jmp exit
fail:
 mov ebx, 1
exit:
 mov eax, 1
 int 0x80
''')
                subprocess.run(['as', '--32', 'test.s', '-o', 'test.o'], cwd=work, check=True)
                subprocess.run(['ld', '-m', 'elf_i386', '-T', 'layout.ld', 'test.o', '-o', 'test'], cwd=work, check=True)
                actual = subprocess.run([str(work/'test')], cwd=work, check=True, capture_output=True).stdout
                assert actual == (work/'expected.bin').read_bytes(), (jumps, kind, flags)
                cases += 1
    print(f'{cases} native i386/Rust initializer comparisons passed')
