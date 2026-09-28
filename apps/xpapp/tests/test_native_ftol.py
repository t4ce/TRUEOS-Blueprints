#!/usr/bin/env python3
"""Execute the emitted IA-32 `_ftol` helper on Linux."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]

with tempfile.TemporaryDirectory(prefix="xpapp-ftol-") as directory:
    work = Path(directory)
    (work / "emit.rs").write_text(
        f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut page = vec![0x90; 4096];
    thunk32::install_child_controls(&mut page).unwrap();
    std::fs::write("controls.bin", page).unwrap();
    let mut thunk = [0; thunk32::THUNK_BYTES];
    thunk32::write(825, thunk32::Kind::Ftol, &mut thunk).unwrap();
    std::fs::write("thunk.bin", thunk).unwrap();
}}
'''
    )
    subprocess.run(
        ["rustc", "--edition=2024", "-Awarnings", "emit.rs", "-o", "emit"],
        cwd=work,
        check=True,
    )
    subprocess.run([str(work / "emit")], cwd=work, check=True)

    # The fallback is intentionally not executed as VMCALL on Linux. Replacing
    # it with RET makes the branch observable while preserving its restored
    # provider id and original cdecl frame.
    controls = bytearray((work / "controls.bin").read_bytes())
    ftol = 0xD00
    assert controls[ftol + 63 : ftol + 66] == bytes.fromhex("0f01c1")
    controls[ftol + 63 : ftol + 66] = bytes.fromhex("c39090")
    (work / "controls.bin").write_bytes(controls)

    (work / "layout.ld").write_text(
        """ENTRY(_start)
SECTIONS {
 . = 0x002f0000; .controls : { *(.controls) }
 . = 0x003026ac; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 .rodata : { *(.rodata) }
 . = ALIGN(4096); .data : { *(.data) }
}"""
    )

    # Input bits, expected EAX, expected EDX, and whether FISTP raises the
    # masked invalid flag. These include both signed-64 limits, the largest
    # representable f64 below +2^63, and overflow on either side of the range.
    rows = [
        ("positive", 0x3FFC000000000000, 1, 0, 0),
        ("negative", 0xBFFC000000000000, 0xFFFFFFFF, 0xFFFFFFFF, 0),
        ("negative_zero", 0x8000000000000000, 0, 0, 0),
        ("signed_min", 0xC3E0000000000000, 0, 0x80000000, 0),
        ("largest_below_positive_limit", 0x43DFFFFFFFFFFFFF, 0xFFFFFC00, 0x7FFFFFFF, 0),
        ("positive_overflow", 0x43E0000000000000, 0, 0x80000000, 1),
        ("negative_overflow", 0xC3E0000000000001, 0, 0x80000000, 1),
        ("infinity", 0x7FF0000000000000, 0, 0x80000000, 1),
        ("nan", 0x7FF8000000000000, 0, 0x80000000, 1),
    ]
    data = "\n".join(f"value_{name}: .quad 0x{bits:016x}" for name, bits, *_ in rows)
    calls = ""
    for index, (name, _, low, high, invalid) in enumerate(rows, start=1):
        calls += f'''
 mov byte ptr [progress], {index}
 fninit
 fldcw word ptr [fcw_masked]
 fld qword ptr [sentinel]
 fld qword ptr [value_{name}]
 mov [saved_sp], esp
 mov esi, 0x12345678
 mov edi, 0x76543210
 mov ebx, 0x12345670
 mov ebp, 0x76543217
 call entry
 cmp eax, 0x{low:08x}
 jne fail
 cmp edx, 0x{high:08x}
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
 fnstcw word ptr [fcw_observed]
 mov ax, [fcw_observed]
 cmp ax, 0x077f
 jne fail
 fnstsw ax
 movzx edx, ax
 and edx, 0x3800
 cmp edx, 0x3800
 jne fail
 and eax, 1
 cmp eax, {invalid}
 jne fail
 fst qword ptr [retained]
 cmp dword ptr [retained], 0
 jne fail
 cmp dword ptr [retained + 4], 0x3ff80000
 jne fail
'''

    # An unmasked-invalid control word must take the unmodified provider path:
    # no FISTP executes, EAX remains the import id, and the cdecl frame returns.
    fallback = '''
 mov byte ptr [progress], 100
 fninit
 fldcw word ptr [fcw_unmasked]
 fld qword ptr [sentinel]
 mov [saved_sp], esp
 mov esi, 0x12345678
 mov edi, 0x76543210
 mov ebx, 0x12345670
 mov ebp, 0x76543217
 call entry
 cmp eax, 825
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
 fnstcw word ptr [fcw_observed]
 mov ax, [fcw_observed]
 cmp ax, 0x077e
 jne fail
 fnstsw ax
 movzx edx, ax
 and edx, 0x3800
 cmp edx, 0x3800
 jne fail
 fst qword ptr [retained]
 cmp dword ptr [retained], 0
 jne fail
 cmp dword ptr [retained + 4], 0x3ff80000
 jne fail
'''

    source = f'''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .data
saved_sp: .long 0
progress: .byte 0
fcw_masked: .word 0x077f
fcw_unmasked: .word 0x077e
fcw_observed: .word 0
sentinel: .quad 0x3ff8000000000000
retained: .quad 0
{data}
.section .text
.global _start
_start:
{calls}
{fallback}
 mov eax, 1
 xor ebx, ebx
 int 0x80
fail:
 mov eax, 1
 movzx ebx, byte ptr [progress]
 add ebx, 1
 int 0x80
'''
    (work / "test.S").write_text(source)
    subprocess.run(["as", "--32", "test.S", "-o", "test.o"], cwd=work, check=True)
    subprocess.run(
        ["ld", "-m", "elf_i386", "-T", "layout.ld", "test.o", "-o", "test"],
        cwd=work,
        check=True,
    )
    result = subprocess.run([str(work / "test")], cwd=work, timeout=10)
    if result.returncode:
        raise AssertionError(f"native _ftol probe failed in row {result.returncode - 1}")
    print(f"PASS: {len(rows)} native masked _ftol values plus unmasked provider fallback; ABI, FCW, x87 stack and invalid status")
