#!/usr/bin/env python3
"""Execute the audited Game.dll wndproc on Linux IA32 with small USER32 stubs.

Pass the local Game.dll path. No game binary is copied into the repository.
The CREATESTRUCT bytes come from the production Rust encoder.
"""
from pathlib import Path
import hashlib
import struct
import subprocess
import sys
import tempfile

APP = Path(__file__).resolve().parents[1]
game = Path(sys.argv[1]).read_bytes()
assert hashlib.sha256(game).hexdigest() == 'c8e21c031e52c06a91c0b110b1d5d6f978112b395da7d05f029120d7b8219b42', 'unaudited Game.dll'
pe = struct.unpack_from('<I', game, 0x3c)[0]
count, = struct.unpack_from('<H', game, pe + 6)
optional_size, = struct.unpack_from('<H', game, pe + 20)
base, = struct.unpack_from('<I', game, pe + 24 + 28)
sections = pe + 24 + optional_size

def read_va(va, size):
    rva = va - base
    for n in range(count):
        _, start, raw_size, raw = struct.unpack_from('<IIII', game, sections + n * 40 + 8)
        if start <= rva and rva + size <= start + raw_size:
            return game[raw + rva - start:raw + rva - start + size]
    raise AssertionError('unmapped code')

with tempfile.TemporaryDirectory(prefix='wc3-create-') as directory:
    work = Path(directory)
    (work / 'wndproc.bin').write_bytes(read_va(0x6f0cba40, 0x203))
    (work / 'render_gate.bin').write_bytes(read_va(0x6f0d5e90, 0x30))
    (work / 'area_threshold.bin').write_bytes(read_va(0x6f706bf4, 4))
    (work / 'emit.rs').write_text(f'''
#[path = "{APP / 'src/window_creation.rs'}"] mod creation;
fn main() {{
 let bytes = creation::payload(&[0,0,0x1000,0x2000,0x80000000,0,0,640,480,0,0,0x400000,0x64900c8]).unwrap();
 std::fs::write("payload.bin", bytes).unwrap();
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'emit.rs', '-o', 'emit'], cwd=work, check=True)
    subprocess.run([str(work / 'emit')], cwd=work, check=True)
    (work / 'test.s').write_text('''
.intel_syntax noprefix
.section .game,"ax"
wndproc: .incbin "wndproc.bin"
.section .render_gate,"ax"
render_gate: .incbin "render_gate.bin"
.section .area_threshold,"a"
.incbin "area_threshold.bin"
.section .object,"aw"
.space 4096
.section .forward_callback,"aw"
.long 0
.section .iat,"aw"
.long get_long, set_long, begin_paint, end_paint, default_proc
.section .data
payload: .incbin "payload.bin"
user_data: .long 0
sets: .long 0
paints: .long 0
saved_sp: .long 0
.section .text
.global _start
_start:
 mov ebx, 0x12345678
 mov esi, 0x23456789
 mov edi, 0x34567890
 mov ebp, 0x45678901
 mov [saved_sp], esp
 push offset payload
 push 0
 push 0x81
 push 0x57434003
 call wndproc
 cmp eax, 1
 jne fail
 cmp dword ptr [user_data], 0
 jne fail
 push offset payload + 48
 push 0
 push 0x83
 push 0x57434003
 call wndproc
 test eax, eax
 jne fail
 push offset payload
 push 0
 push 1
 push 0x57434003
 call wndproc
 test eax, eax
 jne fail
 cmp dword ptr [sets], 1
 jne fail
 cmp dword ptr [user_data], 0x64900c8
 jne fail
 push 0
 push 0
 push 15
 push 0x57434003
 call wndproc
 test eax, eax
 jne fail
 cmp dword ptr [paints], 1
 jne fail
 # The real gate rejects the zero rectangle before the size notification.
 fninit
 mov ecx, 0x64900c8
 call render_gate
 test eax, eax
 jne fail
 push 0x05a00a00
 push 0
 push 5
 push 0x57434003
 call wndproc
 test eax, eax
 jne fail
 cmp dword ptr [0x6490cac], 0
 jne fail
 cmp dword ptr [0x6490cb0], 0
 jne fail
 cmp dword ptr [0x6490cb4], 0x44b40000 # 1440.0
 jne fail
 cmp dword ptr [0x6490cb8], 0x45200000 # 2560.0
 jne fail
 mov ecx, 0x64900c8
 call render_gate
 cmp eax, 1
 jne fail
 cmp esp, [saved_sp]
 jne fail
 cmp ebx, 0x12345678
 jne fail
 cmp esi, 0x23456789
 jne fail
 cmp edi, 0x34567890
 jne fail
 cmp ebp, 0x45678901
 jne fail
 mov eax, 1
 xor ebx, ebx
 int 0x80
fail:
 mov eax, 1
 mov ebx, 1
 int 0x80
get_long:
 cmp dword ptr [esp+4], 0x57434003
 jne fail
 cmp dword ptr [esp+8], -21
 jne fail
 mov eax, [user_data]
 ret 8
set_long:
 cmp dword ptr [esp+4], 0x57434003
 jne fail
 cmp dword ptr [esp+8], -21
 jne fail
 mov eax, [user_data]
 mov edx, [esp+12]
 mov [user_data], edx
 inc dword ptr [sets]
 ret 12
begin_paint:
 inc dword ptr [paints]
 mov eax, 1
 ret 8
end_paint:
 mov eax, 1
 ret 8
default_proc:
 xor eax, eax
 cmp dword ptr [esp+8], 0x81
 sete al
 ret 16
''')
    (work / 'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x064900c8; .object : { *(.object) }
 . = 0x08048000; .text : { *(.text) }
 . = ALIGN(4096); .data : { *(.data) }
 . = 0x6f0cba40; .game : { *(.game) }
 . = 0x6f0d5e90; .render_gate : { *(.render_gate) }
 . = 0x6f706480; .iat : { *(.iat) }
 . = 0x6f706bf4; .area_threshold : { *(.area_threshold) }
 . = 0x6f862c00; .forward_callback : { *(.forward_callback) }
}''')
    subprocess.run(['as', '--32', 'test.s', '-o', 'test.o'], cwd=work, check=True)
    subprocess.run(['ld', '-m', 'elf_i386', '-T', 'layout.ld', 'test.o', '-o', 'test'], cwd=work, check=True)
    subprocess.run([str(work / 'test')], cwd=work, check=True)
print('PASS: actual Game.dll creation/paint/size branches, zero-area render gate opens after WM_SIZE, stdcall stack and nonvolatile registers')
