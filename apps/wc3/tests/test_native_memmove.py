#!/usr/bin/env python3
"""Execute the actual emitted thunk/helper in a freestanding Linux i386 ELF.
Requires rustc, GNU as/ld, and Linux IA32 execution support; no emulator model.
"""
from pathlib import Path
import random
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
SIZE = 131137
randomizer = random.Random(3016)
initial = bytes(randomizer.randrange(256) for _ in range(SIZE))
cases = [(0, 0, 0), (10, 10, 100), (1, 0, 32), (0, 1, 32),
         (4093, 0, 65539), (0, 4093, 65539), (70001, 3, 60001)]
# Includes all small overlap distances and both incoming direction flags.
for count in (1, 2, 3, 4, 7, 16, 31, 64):
    for distance in (-17, -1, 0, 1, 17):
        cases.append((128 + distance, 128, count))
for _ in range(30):
    count = randomizer.randrange(SIZE)
    cases.append((randomizer.randrange(SIZE-count+1), randomizer.randrange(SIZE-count+1), count))

with tempfile.TemporaryDirectory(prefix='wc3-native-memmove-') as directory:
    work = Path(directory)
    source = APP / 'src/thunk32.rs'
    (work/'emit.rs').write_text(f'''
#[path = "{source}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::Memmove, &mut thunk).unwrap();
    std::fs::write("thunk.bin", thunk).unwrap();
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'emit.rs', '-o', 'emit'], cwd=work, check=True)
    subprocess.run([str(work/'emit')], cwd=work, check=True)
    (work/'initial.bin').write_bytes(initial)
    (work/'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 .rodata : { *(.rodata) }
 . = ALIGN(4096); .data : { *(.data) }
}''')
    header = '''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .rodata
initial: .incbin "initial.bin"
.section .data
saved_sp: .long 0
saved_bp: .long 0
buffer: .space SIZE
cases:
'''.replace('SIZE', str(SIZE))
    code = '''cases_end:
.section .text
.global _start
_start:
 mov ebp, offset cases
loop_case:
 cld
 mov esi, offset initial
 mov edi, offset buffer
 mov ecx, SIZE
 rep movsb
 push DWORD PTR [ebp+8]
 push DWORD PTR [ebp+4]
 push DWORD PTR [ebp]
 mov [saved_sp], esp
 mov [saved_bp], ebp
 mov esi, 0x12345678
 mov edi, 0x87654321
 mov ebx, 0x13579bdf
 cmp DWORD PTR [ebp+12], 0
 je forward_flags
 std
forward_flags:
 call entry
 pushfd
 pop edx
 cld
 cmp esp, [saved_sp]
 jne fail
 cmp ebp, [saved_bp]
 jne fail
 cmp esi, 0x12345678
 jne fail
 cmp edi, 0x87654321
 jne fail
 cmp ebx, 0x13579bdf
 jne fail
 cmp eax, [ebp]
 jne fail
 and edx, 0x400
 cmp edx, [ebp+12]
 jne fail
 add esp, 12
 mov eax, 4
 mov ebx, 1
 mov ecx, offset buffer
 mov edx, SIZE
 int 0x80
 cmp eax, SIZE
 jne fail
 add ebp, 16
 cmp ebp, offset cases_end
 jb loop_case
 mov eax, 1
 xor ebx, ebx
 int 0x80
fail:
 mov eax, 1
 mov ebx, 2
 int 0x80
'''.replace('SIZE', str(SIZE))
    def execute(rows):
        (work/'test.S').write_text(header + rows + code)
        subprocess.run(['as', '--32', 'test.S', '-o', 'test.o'], cwd=work, check=True)
        subprocess.run(['ld', '-m', 'elf_i386', '-T', 'layout.ld', 'test.o', '-o', 'test'], cwd=work, check=True)
        return subprocess.run([str(work/'test')], cwd=work, capture_output=True, timeout=15)
    rows = ''
    expected = bytearray()
    for dst, src, count in cases:
        result = bytearray(initial)
        result[dst:dst+count] = initial[src:src+count]
        for df in (0, 0x400):
            rows += f'.long buffer+{dst}, buffer+{src}, {count}, {df}\n'
            expected.extend(result)
    result = execute(rows)
    assert result.returncode == 0, (result.returncode, result.stderr)
    assert result.stdout == expected, 'native helper differs from snapshot-copy memmove'
    # The old provider treats identical pointers and zero size as no-ops, even
    # for inaccessible addresses. Neither case may dereference those addresses.
    result = execute('.long 0xffffffff, 0, 0, 0\n.long 0xffffffff, 0xffffffff, 16, 1024\n')
    assert result.returncode == 0 and result.stdout == initial * 2
    for row in ('.long buffer, 0xfffffffc, 16, 0\n', '.long 0xfffffffc, buffer, 16, 0\n'):
        assert execute(row).returncode == -4, 'wrapping ranges must fault with UD2'
    print(f'PASS: {len(cases)*2} native copy cases, cdecl/register/DF preservation, no-op and overflow cases')
