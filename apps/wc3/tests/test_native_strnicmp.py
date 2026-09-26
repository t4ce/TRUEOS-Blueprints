#!/usr/bin/env python3
"""Run emitted guest strnicmp machine code on Linux IA32, including ABI checks."""
from pathlib import Path
import random
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
rng = random.Random(3026)
cases = [(b'', b'', 0), (b'', b'', 1), (b'a', b'b', 1),
         (b'abc', b'abd', 2), (b'abc', b'abd', 3),
         (b'\xff', b'\x01', 1), (b'a\0x', b'a\0y', 3)]
for _ in range(250):
    a = bytes(rng.randrange(256) for _ in range(rng.randrange(1, 128)))
    b = a if rng.randrange(2) else bytes(rng.randrange(256) for _ in range(len(a)))
    cases.append((a, b, rng.randrange(len(a) + 2)))

def fold(byte):
    try:
        return bytes([byte]).decode('cp1252').lower().encode('cp1252')[0]
    except UnicodeError:
        return byte

# Every CP1252 byte, including accented pairs and undefined byte codes.
for byte in range(256):
    cases.append((bytes([byte]), bytes([fold(byte)]), 0x7fffffff))
    cases.append((bytes([byte]), bytes([(byte + 1) % 256]), 1))
cases += [(b'AbCz', b'aBcQ', 3), (b'AbCz', b'aBcQ', 4)]

def reference(a, b, n):
    for x, y in zip((a + b'\0')[:n], (b + b'\0')[:n]):
        x, y = fold(x), fold(y)
        if x != y or x == 0:
            return x - y
    return 0

with tempfile.TemporaryDirectory(prefix='wc3-strnicmp-') as directory:
    work = Path(directory)
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::Strnicmp, &mut thunk).unwrap();
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
    rows = []
    for i, (a, b, count) in enumerate(cases):
        for side, value in [('a', a), ('b', b)]:
            data += f'{side}{i}: .byte ' + ','.join(map(str, value + b'\0')) + '\n'
        rows.append((f'offset a{i}', f'offset b{i}', count, reference(a, b, count)))
    # Zero count must not dereference invalid pointers. The final byte of the
    # mapped ELF data page tests that NUL/count termination does not read ahead.
    rows += [('0', '0xffffffff', 0, 0), ('offset edge', 'offset edge', 1, 0),
             ('offset edge', 'offset edge', 100, 0)]
    for left, right, count, result in rows:
        for df in (0, 0x400):
            calls += f'''
 push {count}
 push {right}
 push {left}
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
 add esp, 12
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
    print(f'PASS: {len(rows) * 2} native strnicmp cases; CP1252 bytes, NUL/count/page boundaries, cdecl, preserved registers and DF')
