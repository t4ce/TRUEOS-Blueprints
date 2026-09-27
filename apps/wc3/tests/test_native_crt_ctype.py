#!/usr/bin/env python3
"""Execute native MSVCRT C-locale classification thunks on Linux IA32."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]

with tempfile.TemporaryDirectory(prefix='wc3-ctype-') as directory:
    work = Path(directory)
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    for (id, name, kind) in [(7, "isdigit", thunk32::Kind::IsDigit),
                             (8, "ismbcspace", thunk32::Kind::IsMbcSpace)] {{
        let mut thunk = [0; thunk32::THUNK_BYTES];
        thunk32::write(id, kind, &mut thunk).unwrap();
        std::fs::write(format!("{{name}}.bin"), thunk).unwrap();
    }}
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'emit.rs', '-o', 'emit'], cwd=work, check=True)
    subprocess.run([str(work / 'emit')], cwd=work, check=True)
    (work / 'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 . = ALIGN(4096); .data : { *(.data) }
}''')
    rows = list(range(256)) + [0x100, 0x101, 0x7fffffff, 0x80000000, 0xffffffff]
    calls = ''
    for value in rows:
        digit = 4 if 0x30 <= value <= 0x39 else 0
        space = 8 if value == 0x20 or 9 <= value <= 13 else 0
        for name, expected in [('isdigit', digit), ('ismbcspace', space)]:
            for df in (0, 0x400):
                calls += f'''
 push {value}
 mov [saved_sp], esp
 mov esi, 0x12345678
 mov edi, 0x76543210
 mov ebx, 0x12345670
 mov ebp, 0x76543217
 {'std' if df else 'cld'}
 call {name}
 pushfd
 pop edx
 cld
 cmp eax, {expected}
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
isdigit: .incbin "isdigit.bin"
ismbcspace: .incbin "ismbcspace.bin"
.section .data
saved_sp: .long 0
.section .text
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
    print(f'PASS: {len(rows) * 4} native C-locale classification cases; C1 bits, cdecl, preserved registers and DF')
