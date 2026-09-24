#!/usr/bin/env python3
"""Execute Storm's audited expansion loop in freestanding i386 and compare Rust.
Requires rustc, GNU as/ld, and Linux IA32 execution support.
"""
from pathlib import Path
import base64
import hashlib
import random
import struct
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
BODY = base64.b64decode('i0X4ahBQV/8VPHIEFYtN+DPAiUbwiUbsiUb0iUb4iUb8iQaJRgSLRfSD6RCDxAyD7yyD7ixIiU34iUX0dcI=')
assert len(BODY) == 62
assert hashlib.sha256(BODY).hexdigest() == '88b51e9243bef74ff51915375b8c2b0b6cbe295751d694036df7a3104d2b3dac'
BASE = 0x06180010
with tempfile.TemporaryDirectory(prefix='wc3-record-expand-') as directory:
    work = Path(directory)
    (work/'loop.bin').write_bytes(BODY)
    (work/'expected.rs').write_text(f'''
#[path = "{APP/'src/record_expand.rs'}"] mod record_expand;
fn main() {{
    let input = std::fs::read("input.bin").unwrap();
    let count = input.len() / record_expand::INPUT_BYTES;
    let result = record_expand::build(&input, count).unwrap();
    std::fs::write("expected.bin", result).unwrap();
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'expected.rs', '-o', 'expected'], cwd=work, check=True)
    (work/'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x06180000; .buffer : { *(.buffer) }
 . = 0x08048000; .text : { *(.text) }
 . = 0x1501d5e0; .loop : { *(.loop) }
 . = 0x1504723c; .iat : { *(.iat) }
}
''')
    randomizer = random.Random(1501)
    for count in (2, 3, 17, 10759):
        input_bytes = bytes(randomizer.randrange(256) for _ in range(count*16))
        (work/'input.bin').write_bytes(input_bytes)
        subprocess.run([str(work/'expected')], cwd=work, check=True)
        (work/'test.s').write_text(f'''.intel_syntax noprefix
.section .buffer,"aw"
.space 16
source: .incbin "input.bin"
.space {0x74000 - 16 - len(input_bytes)}
.section .loop,"ax"
loop: .incbin "loop.bin"
 jmp snapshot
.section .iat,"aw"
.long move16
.section .text,"ax"
.global _start
_start:
 push ebp
 mov ebp, esp
 sub esp, 0x40
 mov DWORD PTR [ebp-4], {count}
 mov DWORD PTR [ebp-8], {BASE+(count-1)*16}
 mov DWORD PTR [ebp-12], {count-1}
 mov edi, {BASE+(count-1)*44}
 lea esi, [edi+0x24]
 mov ebx, 0x11223344
 cld
 jmp loop
move16:
 pushfd
 push esi
 push edi
 mov eax, [esp+16]
 mov esi, [esp+20]
 mov ecx, [esp+24]
 mov edi, eax
 cld
 rep movsb
 pop edi
 pop esi
 popfd
 ret
snapshot:
 cmp DWORD PTR [ebp-8], {BASE}
 jne fail
 cmp DWORD PTR [ebp-12], 0
 jne fail
 cmp edi, {BASE}
 jne fail
 cmp esi, {BASE+36}
 jne fail
 cmp eax, 0
 jne fail
 cmp ecx, {BASE}
 jne fail
 pushfd
 pop eax
 and eax, 0x8d5
 cmp eax, 0x44
 jne fail
 mov eax, 4
 mov ebx, 1
 mov ecx, {BASE+44}
 mov edx, {(count-1)*44}
 int 0x80
 cmp eax, {(count-1)*44}
 jne fail
 xor ebx, ebx
 jmp done
fail:
 mov ebx, 1
done:
 mov eax, 1
 int 0x80
''')
        subprocess.run(['as', '--32', 'test.s', '-o', 'test.o'], cwd=work, check=True)
        subprocess.run(['ld', '-m', 'elf_i386', '-T', 'layout.ld', 'test.o', '-o', 'test'], cwd=work, check=True)
        actual = subprocess.run([str(work/'test')], cwd=work, check=True, capture_output=True).stdout
        assert actual == (work/'expected.bin').read_bytes(), f'{count} record output mismatch'
    print('Storm i386/Rust expansion matches for 2, 3, 17, and 10759 records')
