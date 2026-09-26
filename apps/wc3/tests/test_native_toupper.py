#!/usr/bin/env python3
"""Run emitted guest toupper machine code on Linux IA32, including ABI checks."""
from pathlib import Path
import random
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='wc3-toupper-') as directory:
    work = Path(directory)
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::ToUpper, &mut thunk).unwrap();
    std::fs::write("thunk.bin", thunk).unwrap();
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'emit.rs', '-o', 'emit'], cwd=work, check=True)
    subprocess.run([str(work / 'emit')], cwd=work, check=True)
    (work / 'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 .rodata : { *(.rodata) }
 . = ALIGN(4096); .data : { *(.data) }
}''')
    data = ''
    calls = ''
    rows = [(value, value - 32 if 97 <= value <= 122 else value) for value in range(256)]
    rows.append((0xffffffff, 0xffffffff))
    # Replace only the fallback VMCALL with RET in this test image. Invalid
    # inputs must arrive there with EAX still containing provider id 7 and
    # the original cdecl stack; executing VMCALL itself requires the carrier.
    controls = bytearray((work / 'controls.bin').read_bytes())
    assert controls[0x553:0x556] == bytes.fromhex('0f01c1')
    controls[0x553:0x556] = bytes.fromhex('c39090')
    (work / 'controls.bin').write_bytes(controls)
    rows += [(value, 7) for value in (256, 257, 0x7fffffff, 0x80000000, 0xfffffffe)]
    for value, result in rows:
        for df in (0, 0x400):
            calls += f'''
 push {value}
 mov [saved_sp], esp
 mov esi, 0x12345678
 mov edi, 0x76543210
 mov ebx, 0x12345670
 mov ebp, 0x76543217
 {'std' if df else 'cld'}
 call entry
 pushfd
 pop edx
 cld
 cmp eax, {result}
 jne fail
 cmp esp, [saved_sp]
 jne fail
 cmp esi, 0x12345678
 jne fail
 cmp edi, 0x76543210
 jne fail
 cmp ebx, 0x12345670
 jne fail
 cmp ebp, 0x76543217
 jne fail
 and edx, 0x400
 cmp edx, {df}
 jne fail
 add esp, 4
'''
    source = '''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .data
saved_sp: .long 0
.balign 4096
.space 4095
edge: .byte 0
''' + data + '''.section .text
.global _start
_start:
''' + calls + '''
 mov eax, 1
 xor ebx, ebx
 int 0x80
fail:
 mov eax, 1
 mov ebx, 2
 int 0x80
'''
    (work / 'test.S').write_text(source)
    subprocess.run(['as', '--32', 'test.S', '-o', 'test.o'], cwd=work, check=True)
    subprocess.run(['ld', '-m', 'elf_i386', '-T', 'layout.ld', 'test.o', '-o', 'test'], cwd=work, check=True)
    subprocess.run([str(work / 'test')], cwd=work, check=True, timeout=10)
    print(f'PASS: {len(rows) * 2} native toupper cases; all bytes, EOF, invalid-input fallback state, cdecl, preserved registers and DF')
