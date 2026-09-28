#!/usr/bin/env python3
"""Execute the emitted IA-32 iswspace thunk, including its provider fallback."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
PROVIDER_ID = 796

with tempfile.TemporaryDirectory(prefix="xpapp-iswspace-") as directory:
    work = Path(directory)
    (work / "emit.rs").write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write({PROVIDER_ID}, thunk32::Kind::IswSpace, &mut thunk).unwrap();
    std::fs::write("thunk.bin", thunk).unwrap();
}}
''')
    subprocess.run(["rustc", "--edition=2024", "-Awarnings", "emit.rs", "-o", "emit"], cwd=work, check=True)
    subprocess.run([str(work / "emit")], cwd=work, check=True)

    # A Linux user process cannot VMCALL. RET at that one instruction lets
    # the test observe whether the native guard restores the provider id.
    controls = bytearray((work / "controls.bin").read_bytes())
    fallback = 0xD80 + 30
    assert controls[fallback : fallback + 3] == bytes.fromhex("0f01c1")
    controls[fallback : fallback + 3] = bytes.fromhex("c39090")
    (work / "controls.bin").write_bytes(controls)
    (work / "layout.ld").write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x00302550; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 . = ALIGN(4096); .data : { *(.data) }
}''')

    rows = list(range(256)) + [0x100, 0xfffe, 0xffff, 0x10000, 0x10020,
                                0x10100, 0x1ffff, 0x7fffffff, 0xffffffff]
    calls = ""
    for value in rows:
        character = value & 0xffff
        if 0x100 <= character <= 0xfffe:
            expected = PROVIDER_ID
        else:
            expected = 8 if character == 0x20 or 9 <= character <= 13 else 0
        for direction in (0, 0x400):
            calls += f'''
 push {value}
 mov [saved_sp], esp
 mov esi, 0x12345678
 mov edi, 0x76543210
 mov ebx, 0x12345670
 mov ebp, 0x76543217
 {'std' if direction else 'cld'}
 call iswspace
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
 cmp edx, {direction}
 jne fail
 add esp, 4
'''
    source = '''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
iswspace: .incbin "thunk.bin"
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
    (work / "test.S").write_text(source)
    subprocess.run(["as", "--32", "test.S", "-o", "test.o"], cwd=work, check=True)
    subprocess.run(["ld", "-m", "elf_i386", "-T", "layout.ld", "test.o", "-o", "test"], cwd=work, check=True)
    subprocess.run([str(work / "test")], cwd=work, check=True, timeout=10)
    print(f"PASS: {len(rows) * 2} native iswspace and fallback cases; wchar truncation, ABI and DF")
