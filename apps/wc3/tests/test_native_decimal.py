#!/usr/bin/env python3
"""Run emitted guest decimal machine code on Linux IA32, including ABI checks."""
from pathlib import Path
import random
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='wc3-decimal-') as directory:
    work = Path(directory)
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::Decimal, &mut thunk).unwrap();
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
    data = '.section .rodata\n'
    calls = ''
    cases = [(str(i).encode(), i) for i in range(300)]
    cases += [(b'10;K1', 10), (b'999999999', 999999999), (b'00000123;K-1', 123),
              (b'1' + b'x' * 254, 1)]
    # These must reach the original Rust parser with provider id 7 intact.
    cases += [(value, 7) for value in (b'', b'-1', b'+1', b' 1', b'\t1',
              b'1000000000', b'2147483648', b'1\xff', b'1' + b'x' * 255)]
    rows = []
    for i, (value, expected) in enumerate(cases):
        data += f's{i}: .byte ' + ','.join(map(str, value + b'\0')) + '\n'
        rows.append((f'offset s{i}', expected))
    rows += [('0', 7), ('0xffffffff', 7)]
    # RET substitutes for the privileged fallback instruction so the test
    # checks its provider id, restored stack, registers and flags on Linux.
    controls = bytearray((work / 'controls.bin').read_bytes())
    assert controls[0x65e:0x661] == bytes.fromhex('0f01c1')
    controls[0x65e:0x661] = bytes.fromhex('c39090')
    (work / 'controls.bin').write_bytes(controls)
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
    print(f'PASS: {len(rows) * 2} native decimal cases; decimal prefixes, bounds and parser fallback state, cdecl, preserved registers and DF')
