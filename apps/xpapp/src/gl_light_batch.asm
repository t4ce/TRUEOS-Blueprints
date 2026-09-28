; Guest glLightfv thunk target. The host drains this single-producer queue at
; every VM exit, before dispatching the exit or selecting another thread.
BITS 32
ORG 0x002f0e00

%define count 0x002f1000
%define records 0x002f1004
%define capacity 32

start:
    pushfd
    push eax                    ; provider id for the original fallback
    push ebx
    push esi
    push edi
    mov ecx, [esp + 28]        ; pname
    mov edx, [esp + 32]        ; params
    test edx, edx
    jz fallback
    cmp ecx, 0x1200            ; AMBIENT..POSITION: four floats
    jb fallback
    cmp ecx, 0x1203
    jbe four
    cmp ecx, 0x1204            ; SPOT_DIRECTION: three floats
    je three
    cmp ecx, 0x1205            ; EXPONENT..QUADRATIC_ATTENUATION: one
    jb fallback
    cmp ecx, 0x1209
    ja fallback
    mov ecx, 1
    jmp ready
three:
    mov ecx, 3
    jmp ready
four:
    mov ecx, 4
ready:
    cmp dword [count], capacity
    jb space
full_trap:
    vmcall                      ; host drains, then resumes at space
space:
    mov ebx, [count]
    lea edi, [ebx + ebx*2]
    shl edi, 3                 ; 24 bytes per record
    add edi, records
    mov ebx, [esp + 24]        ; light
    mov [edi], ebx
    mov ebx, [esp + 28]        ; pname
    mov [edi + 4], ebx
    add edi, 8
    mov esi, [esp + 32]
    rep movsd                  ; exact pname-dependent byte count
    inc dword [count]          ; publish only after the record is complete
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    ret 12
fallback:
    pop edi
    pop esi
    pop ebx
    pop eax
    popfd
    vmcall                     ; original typed provider, including errors
    ret 12
