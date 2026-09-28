#!/usr/bin/env python3
"""Execute emitted i386 rand/srand: sequence, reseeding, ABI and shared updates."""
from pathlib import Path
import random,struct,subprocess,tempfile
APP=Path(__file__).resolve().parents[1]
r=random.Random(813)
seeds=[1,0x150b,0,0x12345678,0xffffffff,0x80000000]+[r.getrandbits(32) for _ in range(24)]
def advance(s): return (s*214013+2531011)&0xffffffff
expected=bytearray()
for seed in seeds:
 expected+=struct.pack('<I',seed)
 for _ in range(1024):
  seed=advance(seed);expected+=struct.pack('<II',(seed>>16)&0x7fff,seed)
assert (advance(0x150b)>>16)&0x7fff==17630
assert (advance(advance(0x150b))>>16)&0x7fff==8328
final=1
for _ in range(40000):final=advance(final)
with tempfile.TemporaryDirectory(prefix='xpapp-rng-') as tmp:
 w=Path(tmp)
 (w/'emit.rs').write_text(f'''
#[path="{APP}/src/thunk32.rs"] mod thunk32;
fn main() {{
 let mut page=vec![0x90;4096];thunk32::install_child_controls(&mut page).unwrap();
 std::fs::write("controls.bin",page).unwrap();
 let mut thunks=[0;24];
 thunk32::write(7,thunk32::Kind::Rand,&mut thunks[..12]).unwrap();
 thunk32::write(8,thunk32::Kind::Srand,&mut thunks[12..]).unwrap();
 std::fs::write("thunks.bin",thunks).unwrap();
 std::fs::write("seed.bin",thunk32::CHILD_RNG_INITIAL_SEED.to_le_bytes()).unwrap();
}}
''')
 subprocess.run(['rustc','--edition=2024','-Awarnings','emit.rs','-o','emit'],cwd=w,check=True)
 subprocess.run([str(w/'emit')],cwd=w,check=True)
 (w/'expected.bin').write_bytes(expected)
 (w/'layout.ld').write_text('''ENTRY(_start)
SECTIONS { . = 0x002f0000; .controls : { *(.controls) }
 . = 0x002f1000; .rng : { *(.rng) }
 . = 0x00300054; .thunks : { *(.thunks) }
 . = 0x08048000; .text : { *(.text) } .rodata : { *(.rodata) }
 . = ALIGN(4096); .data : { *(.data) } }
''')
 (w/'test.s').write_text('''.intel_syntax noprefix
.section .controls,"ax"
.incbin "controls.bin"
.section .rng,"aw"
.space 4080
seed: .incbin "seed.bin"
.section .thunks,"ax"
rand: .incbin "thunks.bin",0,12
srand: .incbin "thunks.bin",12,12
.section .rodata
expected: .incbin "expected.bin"
expected_end:
.section .data
saved_sp: .long 0
child_pid: .long 0
child_status: .long 0
.section .text
.global _start
_start:
 # Default seed is usable before any srand call.
 call rand
 cmp eax,41
 jne fail
 mov esi,offset expected
next_seed:
 mov ebx,0x11223344
 mov ebp,0x55667788
 mov edi,1024
 push DWORD PTR [esi]
 mov [saved_sp],esp
 call srand
 test eax,eax
 jne fail
 cmp esp,[saved_sp]
 jne fail
 mov eax,[esi]
 cmp [seed],eax
 jne fail
 add esp,4
 add esi,4
next_value:
 mov [saved_sp],esp
 call rand
 cmp esp,[saved_sp]
 jne fail
 cmp eax,[esi]
 jne fail
 cmp ebx,0x11223344
 jne fail
 cmp ebp,0x55667788
 jne fail
 mov eax,[seed]
 cmp eax,[esi+4]
 jne fail
 add esi,8
 dec edi
 jnz next_value
 cmp esi,offset expected_end
 jb next_seed
 # Exercise the actual locked update with two concurrent Linux processes.
 # mmap2(MAP_SHARED|MAP_FIXED|MAP_ANONYMOUS) gives them the same seed page.
 mov eax,192
 mov ebx,0x002f1000
 mov ecx,4096
 mov edx,3
 mov esi,0x31
 mov edi,-1
 xor ebp,ebp
 int 0x80
 cmp eax,0x002f1000
 jne fail
 push 1
 call srand
 add esp,4
 mov eax,2
 int 0x80
 test eax,eax
 js fail
 jz child
 mov [child_pid],eax
 call many
 mov eax,7
 mov ebx,[child_pid]
 mov ecx,offset child_status
 xor edx,edx
 int 0x80
 cmp eax,[child_pid]
 jne fail
 cmp DWORD PTR [child_status],0
 jne fail
 cmp DWORD PTR [seed],FINAL_SEED
 jne fail
 xor ebx,ebx
 jmp finish
child:
 call many
 xor ebx,ebx
 jmp finish
many:
 mov edi,20000
again:
 call rand
 dec edi
 jnz again
 ret
fail:
 mov ebx,1
finish:
 mov eax,1
 int 0x80
'''.replace('FINAL_SEED',hex(final)))
 subprocess.run(['as','--32','test.s','-o','test.o'],cwd=w,check=True)
 subprocess.run(['ld','-m','elf_i386','-T','layout.ld','test.o','-o','test'],cwd=w,check=True)
 subprocess.run([str(w/'test')],check=True,timeout=10)
 print(f'Passed: {len(seeds)*1024} sequence/state checks; default seed, reseeding, cdecl ABI; 40000 concurrent updates.')
