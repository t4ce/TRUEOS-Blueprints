#!/usr/bin/env python3
"""Run shipped i386 strtol helper; preserve tested provider fallback shapes."""
from pathlib import Path
import subprocess,tempfile
APP=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='xpapp-strtol-') as tmp:
 w=Path(tmp)
 (w/'emit.rs').write_text(f'''
#[path="{APP}/src/thunk32.rs"] mod thunk32;
fn main() {{
 let mut page=vec![0x90;4096];thunk32::install_child_controls(&mut page).unwrap();
 let start=thunk32::CHILD_STRTOL_ZERO_OFFSET;
 // Replace only the trap with mov ecx,eax; nop. This records the provider ID
 // without executing privileged VMCALL on Linux. The surrounding code is real.
 let trap=page[start..start+96].windows(3).position(|v|v==[15,1,193]).unwrap()+start;
 page[trap..trap+3].copy_from_slice(&[0x89,0xc1,0x90]);
 std::fs::write("controls.bin",page).unwrap();
 let mut thunk=[0;thunk32::THUNK_BYTES];thunk32::write(7,thunk32::Kind::StrtolZero,&mut thunk).unwrap();
 std::fs::write("thunk.bin",thunk).unwrap();
}}
''')
 subprocess.run(['rustc','--edition=2024','-Awarnings','emit.rs','-o','emit'],cwd=w,check=True)
 subprocess.run([str(w/'emit')],cwd=w,check=True)
 (w/'layout.ld').write_text('''ENTRY(_start)
SECTIONS { . = 0x002f0000; .controls : { *(.controls) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) } .rodata : { *(.rodata) }
 . = ALIGN(4096); .data : { *(.data) } }
''')
 # First case reuses the existing octal-zero provider regression. Others must
 # reach that provider intact, including its existing end-pointer/hex tests.
 cases=[('zero',0,8,0),('empty',0,8,7),('seven',0,8,7),('hex',0,8,7),
        ('zero','output',8,7),('zero',0,0,7),('zero',0,10,7),('zero',0,37,7),
        ('spaces',0,8,7),(0,0,8,7),('negative',0,8,7),('tail',0,8,7)]
 data='\n'.join('.long '+','.join(map(str,c)) for c in cases)
 (w/'test.s').write_text('''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .thunks,"ax"
entry: .incbin "thunk.bin"
.section .rodata
zero: .asciz "0"
empty: .asciz ""
seven: .asciz "07"
hex: .asciz "0x00000409tail"
spaces: .asciz " 0"
negative: .asciz "-0"
tail: .asciz "0tail"
.section .data
output: .long 0x12345678
saved_sp: .long 0
cases:
'''+data+'''
cases_end:
.section .text
.global _start
_start:
 mov ebp, offset cases
next:
 push DWORD PTR [ebp+8]
 push DWORD PTR [ebp+4]
 push DWORD PTR [ebp]
 mov [saved_sp], esp
 mov ebx,0x11223344
 mov esi,0x55667788
 mov edi,0x12344321
 mov ecx,0x76543210
 call entry
 cmp esp,[saved_sp]
 jne fail
 cmp eax,[ebp+12]
 jne fail
 test eax,eax
 jz fast
 cmp ecx,7
 jne fail
 jmp preserved
fast:
 cmp ecx,0x76543210
 jne fail
preserved:
 cmp ebx,0x11223344
 jne fail
 cmp esi,0x55667788
 jne fail
 cmp edi,0x12344321
 jne fail
 mov eax,[esp]
 cmp eax,[ebp]
 jne fail
 mov eax,[esp+4]
 cmp eax,[ebp+4]
 jne fail
 mov eax,[esp+8]
 cmp eax,[ebp+8]
 jne fail
 add esp,12
 add ebp,16
 cmp ebp,offset cases_end
 jb next
 cmp DWORD PTR [output],0x12345678
 jne fail
 xor ebx,ebx
 jmp finish
fail:
 mov ebx,1
finish:
 mov eax,1
 int 0x80
''')
 subprocess.run(['as','--32','test.s','-o','test.o'],cwd=w,check=True)
 subprocess.run(['ld','-m','elf_i386','-T','layout.ld','test.o','-o','test'],cwd=w,check=True)
 subprocess.run([str(w/'test')],check=True)
 print(f'{len(cases)} native strtol helper cases passed (value, fallback, ABI, arguments).')
