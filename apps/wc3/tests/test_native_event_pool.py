#!/usr/bin/env python3
"""Compare the Rust event-pool table builder to the audited War3 i386 loop.
Uses a stub CreateEventA but executes the actual 167-byte guest loop on IA32.
"""
from pathlib import Path
import base64
import hashlib
import struct
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
BODY = base64.b64decode('VYvsg+w0U1ZXx0X8AAAAAL64cEUAx0X4uJBFAI1kJACLTfyLXfgzwIP5AQ+UwL8AACAAx0X0AAQAAIlF8I1JAItN8GoAagBRagD/FQCxRACJBotF9Ik7gccAACAAg8YEg8MESIlF9HXXi0X8xwSFrHBFAP8HAADHBIWccEUA/wMAAMcEhaRwRQAABAAAQIlF/ItF+AUAIAAAPbjQRQCJRfgPjHn///8=')
assert len(BODY) == 167
assert hashlib.sha256(BODY).hexdigest() == '61a62f92319326486be6585a7eb4210346becd33c0043979b9c3b8588bd5e5a1'
START_HANDLE = 0x5743200b
with tempfile.TemporaryDirectory(prefix='wc3-event-pool-') as directory:
    work = Path(directory)
    (work/'pool.bin').write_bytes(BODY)
    (work/'expected.rs').write_text(f'''
#[path = "{APP/'src/event_pool.rs'}"] mod event_pool;
fn main() {{
    let mut next = {START_HANDLE}u32;
    let tables = event_pool::build(|_| {{ let handle = next; next += 1; handle }}).unwrap();
    let mut out = vec![0u8; 0x6000];
    let mut write = |address: usize, bytes: &[u8]| {{
        let offset = address - 0x00457000;
        out[offset..offset+bytes.len()].copy_from_slice(bytes);
    }};
    write(event_pool::HANDLES_BASE as usize, &tables.handles);
    for bank in 0..2 {{
        write((event_pool::GENERATIONS_BASE + bank * 0x2000) as usize, &tables.generations[bank as usize]);
        write((0x004570ac + bank * 4) as usize, &0x7ffu32.to_le_bytes());
        write((0x0045709c + bank * 4) as usize, &0x3ffu32.to_le_bytes());
        write((0x004570a4 + bank * 4) as usize, &0x400u32.to_le_bytes());
    }}
    std::fs::write("expected.bin", out).unwrap();
}}
''')
    subprocess.run(['rustc', '--edition=2024', '-Awarnings', 'expected.rs', '-o', 'expected'], cwd=work, check=True)
    subprocess.run([str(work/'expected')], cwd=work, check=True)
    (work/'layout.ld').write_text('''ENTRY(_start)
SECTIONS {
 . = 0x004029a0; .pool : { *(.pool) }
 . = 0x0044b100; .iat : { *(.iat) }
 . = 0x00457000; .tables : { *(.tables) }
 . = 0x08048000; .text : { *(.text) }
}
''')
    (work/'test.s').write_text(f'''.intel_syntax noprefix
.section .pool,"ax"
pool: .incbin "pool.bin"
 jmp snapshot
.section .iat,"aw"
.long create_event
.section .tables,"aw"
tables: .space 0x6000
capture: .space 48
next_handle: .long {START_HANDLE}
.section .text,"ax"
.global _start
_start:
 mov eax, 0xaabbccdd
 mov ebx, 0x11223344
 mov ecx, 0x22334455
 mov edx, 0x33445566
 mov esi, 0x44556677
 mov edi, 0x55667788
 mov ebp, 0x66778899
 cld
 call pool
 mov ebx, 2
 jmp done
create_event:
 mov eax, [next_handle]
 inc DWORD PTR [next_handle]
 ret 16
snapshot:
 mov [capture], eax
 mov [capture+4], ebx
 mov [capture+8], ecx
 mov [capture+12], edx
 mov [capture+16], esi
 mov [capture+20], edi
 mov [capture+24], ebp
 mov [capture+28], esp
 pushfd
 pop DWORD PTR [capture+32]
 mov eax, [ebp-4]
 mov [capture+36], eax
 mov eax, [ebp-8]
 mov [capture+40], eax
 mov eax, [next_handle]
 mov [capture+44], eax
 mov eax, 4
 mov ebx, 1
 mov ecx, offset tables
 mov edx, 0x6000
 int 0x80
 cmp eax, 0x6000
 jne fail
 mov eax, 4
 mov ebx, 1
 mov ecx, offset capture
 mov edx, 48
 int 0x80
 cmp eax, 48
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
    assert actual[:0x6000] == (work/'expected.bin').read_bytes(), 'table differs from original x86'
    eax, ebx, ecx, edx, esi, edi, ebp, esp, eflags, bank, pointer, next_handle = struct.unpack('<12I', actual[0x6000:])
    assert (eax, ebx, ecx, esi, edi, bank, pointer, next_handle) == (
        0x0045d0b8, 0x0045c0b8, 1, 0x004590b8, 0x80200000, 2, 0x0045d0b8, START_HANDLE+2048)
    assert eflags & 0x8d5 == 0x44, hex(eflags)
    assert esp == (ebp - 0x40) & 0xffffffff
    print('2048-event Rust tables match the original i386 loop; final CPU state verified')
