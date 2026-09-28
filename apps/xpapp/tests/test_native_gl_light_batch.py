#!/usr/bin/env python3
"""Execute the emitted IA32 light thunk and verify queue, ABI and capacity."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="xpapp-light-batch-") as directory:
    work = Path(directory)
    (work / "emit.rs").write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(7, thunk32::Kind::GlLightBatch, &mut thunk).unwrap();
    std::fs::write("thunk.bin", thunk).unwrap();
}}
''')
    subprocess.run(["rustc", "--edition=2024", "-Awarnings", "emit.rs", "-o", "emit"], cwd=work, check=True)
    subprocess.run([str(work / "emit")], cwd=work, check=True)
    controls = bytearray((work / "controls.bin").read_bytes())
    assert controls[0xe72:0xe75] == bytes.fromhex("0f01c1")
    assert controls[0xeb0:0xeb3] == bytes.fromhex("0f01c1")
    # Native Linux cannot execute VMCALL. Full capacity must fault; typed
    # fallbacks return with their original provider ID and stdcall cleanup.
    controls[0xe72:0xe75] = bytes.fromhex("0f0b90")
    controls[0xeb0:0xeb3] = bytes.fromhex("909090")
    (work / "controls.bin").write_bytes(controls)
    (work / "layout.ld").write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x002f1000; .batch : { *(.batch) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 . = ALIGN(4096); .data : { *(.data) }
}''')
    source = r'''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .batch,"aw"
batch_count: .long 0
records: .fill 4092,1,0xcc
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .data
saved_sp: .long 0
values: .long 0x3f800000,0x40000000,0x40400000,0x40800000
.section .text
.global _start
_start:
 mov [saved_sp], esp
 mov ebx, 0x12345678
 mov esi, 0x23456789
 mov edi, 0x3456789a
 mov ebp, 0x456789ab
 std
 push offset values
 push 0x1203
 push 0x4000
 mov eax, 7
 call entry
 pushfd
 pop edx
 cld
 cmp esp, [saved_sp]
 jne fail
 cmp ebx, 0x12345678
 jne fail
 cmp esi, 0x23456789
 jne fail
 cmp edi, 0x3456789a
 jne fail
 cmp ebp, 0x456789ab
 jne fail
 test edx, 0x400
 jz fail
 cmp dword ptr [batch_count], 1
 jne fail
 mov dword ptr [values], 0xdeadbeef
 cmp dword ptr [records+8], 0x3f800000
 jne fail
 cld
 push offset values
 push 0x1204
 push 0x4001
 mov eax, 7
 call entry
 cmp dword ptr [batch_count], 2
 jne fail
 cmp dword ptr [records+24], 0x4001
 jne fail
 cmp dword ptr [records+24+8], 0xdeadbeef
 jne fail
 cmp dword ptr [records+24+8+12], 0xcccccccc
 jne fail
 push offset values
 push 0x1205
 push 0x4002
 mov eax, 7
 call entry
 cmp dword ptr [batch_count], 3
 jne fail
 cmp dword ptr [records+48+8+4], 0xcccccccc
 jne fail
 push 0
 push 0x1203
 push 0x4000
 mov eax, 7
 call entry
 cmp eax, 7
 jne fail
 cmp dword ptr [batch_count], 3
 jne fail
 push offset values
 push 0x9999
 push 0x4000
 mov eax, 7
 call entry
 cmp eax, 7
 jne fail
 cmp dword ptr [batch_count], 3
 jne fail
 mov ecx, 29
fill:
 push ecx
 push offset values
 push 0x1205
 push 0x4000
 mov eax, 7
 call entry
 pop ecx
 loop fill
 cmp dword ptr [batch_count], 32
 jne fail
 push offset values
 push 0x1205
 push 0x4000
 mov eax, 7
 call entry
 jmp fail                  # capacity VMCALL was not reached
fail:
 mov eax, 1
 mov ebx, 2
 int 0x80
'''
    (work / "test.S").write_text(source)
    subprocess.run(["as", "--32", "test.S", "-o", "test.o"], cwd=work, check=True)
    subprocess.run(["ld", "-m", "elf_i386", "-T", "layout.ld", "test.o", "-o", "test"], cwd=work, check=True)
    result = subprocess.run([str(work / "test")], cwd=work, check=False, timeout=10)
    assert result.returncode == -4, f"IA32 queue test exit {result.returncode}, expected capacity UD2"
    print("PASS: guest light queue copies at call time, 1/3/4 floats, fallback, ABI and capacity")
