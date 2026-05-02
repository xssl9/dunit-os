BITS 32
ORG 0x100000

multiboot_header:
    dd 0xe85250d6
    dd 0
    dd multiboot_header_end - multiboot_header
    dd -(0xe85250d6 + 0 + (multiboot_header_end - multiboot_header))
    
    dw 0
    dw 0
    dd 8
multiboot_header_end:

start:
    cli
    mov esp, stack_top
    
    mov dword [0xb8000], 0x2f4b2f4f
    mov dword [0xb8004], 0x2f212f21
    
    call check_cpuid
    call check_long_mode
    call setup_page_tables
    call enable_paging
    
    lgdt [gdt64.pointer]
    jmp 0x08:long_mode_start

check_cpuid:
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 1 << 21
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    cmp eax, ecx
    je .no_cpuid
    ret
.no_cpuid:
    hlt

check_long_mode:
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .no_long_mode
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29
    jz .no_long_mode
    ret
.no_long_mode:
    hlt

setup_page_tables:
    mov eax, p3_table
    or eax, 0b11
    mov [p4_table], eax
    mov [p4_table + 511 * 8], eax
    
    mov eax, p2_table
    or eax, 0b11
    mov [p3_table], eax
    mov [p3_table + 510 * 8], eax
    
    mov ecx, 0
.map_p2_table:
    mov eax, 0x200000
    mul ecx
    or eax, 0b10000011
    mov [p2_table + ecx * 8], eax
    inc ecx
    cmp ecx, 512
    jne .map_p2_table
    ret

enable_paging:
    mov eax, p4_table
    mov cr3, eax
    
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax
    
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr
    
    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax
    ret

BITS 64
long_mode_start:
    mov ax, 0x10
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    mov rsp, stack_top
    
    mov rax, 0xb8000
    mov qword [rax], 0x2f592f582f572f56
    
    hlt
    jmp $

gdt64:
    dq 0
    dq 0x00209A0000000000
    dq 0x0000920000000000
.pointer:
    dw $ - gdt64 - 1
    dq gdt64

ALIGN 4096
p4_table:
    times 4096 db 0
p3_table:
    times 4096 db 0
p2_table:
    times 4096 db 0
stack_bottom:
    times 16384 db 0
stack_top:
