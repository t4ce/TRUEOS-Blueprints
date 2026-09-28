; Ordered guest GL state queue. The host replays published records at every
; VM exit before provider dispatch or a thread switch. Count is published last.
BITS 32
ORG 0x002f2000

%define count 0x002f1000
%define records 0x002f1004
%define capacity 32

light:
    pushfd
    cld
    push eax                    ; original typed provider id for fallback
    push ebx
    push esi
    push edi
    mov ebx, [esp + 24]        ; light is checked before params dereference
    cmp ebx, 0x4000
    jb light_fallback
    cmp ebx, 0x4007
    ja light_fallback
    mov ecx, [esp + 28]        ; pname
    mov esi, [esp + 32]        ; params
    test esi, esi
    jz light_fallback
    cmp ecx, 0x1200
    jb light_fallback
    cmp ecx, 0x1203
    jbe light_four
    cmp ecx, 0x1204
    je light_three
    cmp ecx, 0x1205
    jb light_fallback
    cmp ecx, 0x1209
    ja light_fallback
    mov ecx, 1
    jmp light_ready
light_three:
    mov ecx, 3
    jmp light_ready
light_four:
    mov ecx, 4
light_ready:
    mov eax, ecx
    shl eax, 2
    neg eax
    cmp esi, eax             ; exact value span must not wrap
    ja light_fallback
    mov edx, 1
    jmp ready

mode:
    pushfd
    cld
    push eax
    push ebx
    push esi
    push edi
    mov ebx, [esp + 24]
    cmp ebx, 0x1700
    jb mode_fallback
    cmp ebx, 0x1702
    ja mode_fallback
    mov edx, 2
    xor ecx, ecx
    jmp ready

load:
    pushfd
    cld
    push eax
    push ebx
    push esi
    push edi
    mov esi, [esp + 24]
    test esi, esi
    jz load_fallback
    cmp esi, 0xffffffc0         ; last byte must not wrap
    ja load_fallback
    mov edx, 3
    mov ecx, 16
ready:
    cmp dword [count], capacity
    jb space
full_trap:
    vmcall                      ; drain/reset queue, then resume at space
space:
    mov eax, [count]
    lea edi, [eax + eax*8]
    shl edi, 3                  ; 72 bytes per record
    add edi, records + 8       ; payload starts after tag and arg
    cmp edx, 1
    jne copy
    add edi, 4                  ; light payload has pname before values
copy:
    test ecx, ecx
    jz copied
    ; Memmove direction handles guest pointers aliasing the queue slot.
    cmp edi, esi
    jbe forward
    lea eax, [esi + ecx*4]
    cmp edi, eax
    jae forward
    lea esi, [esi + ecx*4 - 4]
    lea edi, [edi + ecx*4 - 4]
    std
    rep movsd
    cld
    jmp copied
forward:
    rep movsd
copied:
    mov eax, [count]
    lea edi, [eax + eax*8]
    shl edi, 3
    add edi, records
    mov [edi], edx
    cmp edx, 3
    je load_arg
    mov ebx, [esp + 24]
    mov [edi + 4], ebx
    cmp edx, 1
    jne publish
    mov ebx, [esp + 28]
    mov [edi + 8], ebx
    jmp publish
load_arg:
    mov dword [edi + 4], 0
publish:
    inc dword [count]
    cmp edx, 1
    je light_return
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    ret 4
light_return:
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    ret 12
light_fallback:
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    vmcall
    ret 12
mode_fallback:
load_fallback:
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    vmcall
    ret 4
