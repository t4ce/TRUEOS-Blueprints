#!/usr/bin/env python3
"""Run the emitted guest _stricmp machine code on Linux IA32."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]

def reference(left, right):
    for a, b in zip(left + b'\0', right + b'\0'):
        a = a + 0x20 if 0x41 <= a <= 0x5a else a
        b = b + 0x20 if 0x41 <= b <= 0x5a else b
        if a != b or a == 0:
            return a - b
    raise AssertionError('NUL termination')

cases = [
    (b'', b''),
    (b'AbCd', b'aBcD'),
    (b'abc', b'abd'),
    (b'\xc0', b'\xe0'),  # Host personality is deliberately ASCII-only.
    (b'a\0ignored', b'A\0other'),
]

with tempfile.TemporaryDirectory(prefix='wc3-stricmp-') as directory:
    work = Path(directory)
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::Stricmp, &mut thunk).unwrap();
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
    data, calls = '.section .rodata\n', ''
    for index, (left, right) in enumerate(cases):
        data += f'left{index}: .byte ' + ','.join(map(str, left + b'\0')) + '\n'
        data += f'right{index}: .byte ' + ','.join(map(str, right + b'\0')) + '\n'
        for df in (0, 0x400):
            calls += f'''
 push offset right{index}
 push offset left{index}
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
 cmp eax, {reference(left, right)}
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
 add esp, 8
'''
    source = '''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .data
saved_sp: .long 0
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
    print(f'PASS: {len(cases) * 2} native _stricmp cases; ASCII fold, NUL, cdecl, preserved registers and DF')
