#!/usr/bin/env python3
"""Execute emitted IA32 ordered GL state thunks, including matrix capture."""
from pathlib import Path
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="xpapp-gl-state-batch-") as directory:
    work = Path(directory)
    (work / "emit.rs").write_text(f'''
#[path = "{APP / 'src/thunk32.rs'}"] mod thunk32;
fn main() {{
    let mut code = vec![0x90; 4096];
    thunk32::install_child_gl_batch_code(&mut code).unwrap();
    let trap = (thunk32::CHILD_LIGHT_BATCH_AFTER_VMCALL
        - thunk32::CHILD_LIGHT_BATCH_ADDRESS - 3) as usize;
    assert_eq!(&code[trap..trap + 3], &[0x0f, 0x01, 0xc1]);
    for address in [thunk32::CHILD_MATRIX_MODE_BATCH_ADDRESS,
        thunk32::CHILD_LOAD_MATRIX_BATCH_ADDRESS] {{
        assert_eq!(code[(address - thunk32::CHILD_LIGHT_BATCH_ADDRESS) as usize], 0x9c);
    }}
    for (address, tag) in [
        (thunk32::CHILD_GL_ENABLE_BATCH_ADDRESS, 4u8),
        (thunk32::CHILD_GL_DISABLE_BATCH_ADDRESS, 5),
        (thunk32::CHILD_GL_ENABLE_CLIENT_BATCH_ADDRESS, 6),
        (thunk32::CHILD_GL_DISABLE_CLIENT_BATCH_ADDRESS, 7),
        (thunk32::CHILD_GL_VERTEX_POINTER_BATCH_ADDRESS, 8),
        (thunk32::CHILD_GL_COLOR_POINTER_BATCH_ADDRESS, 9),
        (thunk32::CHILD_GL_NORMAL_POINTER_BATCH_ADDRESS, 10),
        (thunk32::CHILD_GL_TEXCOORD_POINTER_BATCH_ADDRESS, 11),
    ] {{
        let offset = (address - thunk32::CHILD_LIGHT_BATCH_ADDRESS) as usize;
        assert_eq!(&code[offset..offset + 5], &[0xba, tag, 0, 0, 0]);
    }}
    std::fs::write("glcode.bin", code).unwrap();
    let mut thunks = [0x90; thunk32::THUNK_BYTES * 11];
    for (n, kind) in [thunk32::Kind::GlLightBatch,
        thunk32::Kind::GlMatrixModeBatch, thunk32::Kind::GlLoadMatrixBatch,
        thunk32::Kind::GlEnableBatch, thunk32::Kind::GlDisableBatch,
        thunk32::Kind::GlEnableClientBatch, thunk32::Kind::GlDisableClientBatch,
        thunk32::Kind::GlVertexPointerBatch, thunk32::Kind::GlColorPointerBatch,
        thunk32::Kind::GlNormalPointerBatch, thunk32::Kind::GlTexCoordPointerBatch]
        .into_iter().enumerate() {{
        thunk32::write(7 + n as u32, kind,
            &mut thunks[n * thunk32::THUNK_BYTES..(n + 1) * thunk32::THUNK_BYTES]).unwrap();
    }}
    std::fs::write("thunks.bin", thunks).unwrap();
}}
''')
    subprocess.run(["rustc", "--edition=2024", "-Awarnings", "emit.rs", "-o", "emit"], cwd=work, check=True)
    subprocess.run([str(work / "emit")], cwd=work, check=True)
    code = bytearray((work / "glcode.bin").read_bytes())
    assert code[0xe8:0xeb] == bytes.fromhex("0f01c1")
    assert code[0x189:0x18c] == bytes.fromhex("0f01c1")
    assert code[0x194:0x197] == bytes.fromhex("0f01c1")
    # Linux cannot execute VMCALL. Leave the full trap fatal; make typed
    # fallback a no-op so its preserved EAX and stdcall cleanup can be tested.
    code[0xe8:0xeb] = bytes.fromhex("0f0b90")
    code[0x189:0x18c] = bytes.fromhex("909090")
    code[0x194:0x197] = bytes.fromhex("909090")
    (work / "glcode.bin").write_bytes(code)
    (work / "layout.ld").write_text('''ENTRY(_start)
SECTIONS {
 . = 0x002f1000; .batch : { *(.batch) }
 . = 0x002f2000; .glcode : { *(.glcode) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) }
 . = ALIGN(4096); .data : { *(.data) }
}''')
    source = r'''.intel_syntax noprefix
.section .batch,"aw"
batch_count: .long 0
records: .fill 4092,1,0xcc
.section .glcode,"ax"
.incbin "glcode.bin"
.section .thunks,"ax"
light_entry: .incbin "thunks.bin", 0, 12
mode_entry: .incbin "thunks.bin", 12, 12
load_entry: .incbin "thunks.bin", 24, 12
enable_entry: .incbin "thunks.bin", 36, 12
disable_entry: .incbin "thunks.bin", 48, 12
enable_client_entry: .incbin "thunks.bin", 60, 12
disable_client_entry: .incbin "thunks.bin", 72, 12
vertex_entry: .incbin "thunks.bin", 84, 12
color_entry: .incbin "thunks.bin", 96, 12
normal_entry: .incbin "thunks.bin", 108, 12
texcoord_entry: .incbin "thunks.bin", 120, 12
.section .data
saved_sp: .long 0
values: .long 0x3f800000,0x40000000,0x40400000,0x40800000
matrix: .long 0x80000000,0x7fc01234,2,3,4,5,6,7,8,9,10,11,12,13,14,0xdeadbeef
.section .text
.global _start
_start:
 mov [saved_sp], esp
 mov ebx, 0x12345678
 mov esi, 0x23456789
 mov edi, 0x3456789a
 mov ebp, 0x456789ab
 std
 push 0x1700
 mov eax, 8
 call mode_entry
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
 cmp dword ptr [records], 2
 jne fail
 cmp dword ptr [records+4], 0x1700
 jne fail
 push offset matrix
 mov eax, 9
 call load_entry
 cmp dword ptr [batch_count], 2
 jne fail
 cmp dword ptr [records+72], 3
 jne fail
 cmp dword ptr [records+72+8], 0x80000000
 jne fail
 cmp dword ptr [records+72+12], 0x7fc01234
 jne fail
 cmp dword ptr [records+72+8+60], 0xdeadbeef
 jne fail
 mov dword ptr [matrix], 0
 mov dword ptr [matrix+60], 0
 cmp dword ptr [records+72+8], 0x80000000
 jne fail
 cmp dword ptr [records+72+8+60], 0xdeadbeef
 jne fail
 push offset values
 push 0x1203
 push 0x4000
 mov eax, 7
 call light_entry
 cmp dword ptr [batch_count], 3
 jne fail
 cmp dword ptr [records+144], 1
 jne fail
 cmp dword ptr [records+144+4], 0x4000
 jne fail
 cmp dword ptr [records+144+8], 0x1203
 jne fail
 cmp dword ptr [records+144+12], 0x3f800000
 jne fail
 mov dword ptr [values], 0xdeadbeef
 push offset values
 push 0x1204
 push 0x4001
 mov eax, 7
 call light_entry
 cmp dword ptr [records+216+12], 0xdeadbeef
 jne fail
 cmp dword ptr [records+216+12+12], 0xcccccccc
 jne fail
 push offset values
 push 0x1205
 push 0x4002
 mov eax, 7
 call light_entry
 cmp dword ptr [records+288+12+4], 0xcccccccc
 jne fail
 # Typed fallbacks retain the original import id and stdcall cleanup.
 push 0
 mov eax, 9
 call load_entry
 cmp eax, 9
 jne fail
 push 0xffffffd0
 mov eax, 9
 call load_entry
 cmp eax, 9
 jne fail
 push 0x9999
 mov eax, 8
 call mode_entry
 cmp eax, 8
 jne fail
 push 0
 push 0x1203
 push 0x4000
 mov eax, 7
 call light_entry
 cmp eax, 7
 jne fail
 cmp esp, [saved_sp]
 jne fail
 cmp dword ptr [batch_count], 5
 jne fail
 # New state calls occupy ordered slots 5 through 12.
 std
 push 0x0b71
 mov eax, 10
 call enable_entry
 pushfd
 pop edx
 cld
 test edx, 0x400
 jz fail
 cmp esp, [saved_sp]
 jne fail
 cmp ebx, 0x12345678
 jne fail
 cmp esi, 0x23456789
 jne fail
 cmp edi, 0x3456789a
 jne fail
 cmp dword ptr [records+360], 4
 jne fail
 cmp dword ptr [records+364], 0x0b71
 jne fail
 push 0x0b71
 mov eax, 11
 call disable_entry
 cmp dword ptr [records+432], 5
 jne fail
 push 0x8074
 mov eax, 12
 call enable_client_entry
 cmp dword ptr [records+504], 6
 jne fail
 push 0x8074
 mov eax, 13
 call disable_client_entry
 cmp dword ptr [records+576], 7
 jne fail
 # 0xdeadbeef is deliberately unmapped: declarations must only copy its
 # address, leaving array reads to the draw that consumes it.
 push 0xdeadbeef
 push 16
 push 0x1406
 push 3
 mov eax, 14
 call vertex_entry
 cmp dword ptr [records+648], 8
 jne fail
 cmp dword ptr [records+652], 3
 jne fail
 cmp dword ptr [records+656], 0x1406
 jne fail
 cmp dword ptr [records+660], 16
 jne fail
 cmp dword ptr [records+664], 0xdeadbeef
 jne fail
 push 0xdeadbeef
 push 20
 push 0x1406
 push 4
 mov eax, 15
 call color_entry
 cmp dword ptr [records+720], 9
 jne fail
 cmp dword ptr [records+736], 0xdeadbeef
 jne fail
 push 0xdeadbeef
 push 12
 push 0x1406
 mov eax, 16
 call normal_entry
 cmp dword ptr [records+792], 10
 jne fail
 cmp dword ptr [records+796], 0x1406
 jne fail
 cmp dword ptr [records+800], 12
 jne fail
 cmp dword ptr [records+804], 0xdeadbeef
 jne fail
 push 0xdeadbeef
 push 8
 push 0x1406
 push 2
 mov eax, 17
 call texcoord_entry
 cmp dword ptr [records+864], 11
 jne fail
 cmp dword ptr [records+868], 2
 jne fail
 cmp dword ptr [records+880], 0xdeadbeef
 jne fail
 cmp esp, [saved_sp]
 jne fail
 cmp dword ptr [batch_count], 13
 jne fail
 # Complete the bounded queue, then require the full-capacity trap.
 mov ecx, 19
fill:
 push ecx
 push 0x1701
 mov eax, 8
 call mode_entry
 pop ecx
 loop fill
 cmp dword ptr [batch_count], 32
 jne fail
 push 0x1702
 mov eax, 8
 call mode_entry
 jmp fail
fail:
 mov eax, 1
 mov ebx, 2
 int 0x80
'''
    (work / "test.S").write_text(source)
    subprocess.run(["as", "--32", "test.S", "-o", "test.o"], cwd=work, check=True)
    subprocess.run(["ld", "-m", "elf_i386", "-T", "layout.ld", "test.o", "-o", "test"], cwd=work, check=True)
    result = subprocess.run([str(work / "test")], cwd=work, check=False, timeout=10)
    assert result.returncode == -4, f"IA32 ordered queue exit {result.returncode}, expected capacity UD2"
    print("PASS: ordered GL state capture, scalar and pointer declarations, ABI, fallback and capacity")
