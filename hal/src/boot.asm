section .multiboot
align 8
multiboot_header_start:
    dd 0xe85250d6
    dd 0
    dd multiboot_header_end - multiboot_header_start
    dd -(0xe85250d6 + 0 + (multiboot_header_end - multiboot_header_start))
    
    dw 0
    dw 0
    dd 8
multiboot_header_end:

section .text
bits 32
global _start
extern boot_main

_start:
    cli
    
    mov dword [0xb8000], 0x2f4b2f4f
    mov dword [0xb8004], 0x2f212f21
    
    mov esp, stack_top
    
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .no_long_mode
    
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29
    jz .no_long_mode
    
    mov edi, page_table_l4
    xor eax, eax
    mov ecx, 1024
    rep stosd
    
    mov edi, page_table_l3
    xor eax, eax
    mov ecx, 1024
    rep stosd
    
    mov edi, page_table_l2
    xor eax, eax
    mov ecx, 1024
    rep stosd
    
    mov dword [page_table_l4], page_table_l3 + 0x003
    mov dword [page_table_l3], page_table_l2 + 0x003
    
    mov edi, page_table_l2
    mov eax, 0x00000083
    mov ecx, 512
.set_page_table:
    mov [edi], eax
    add eax, 0x200000
    add edi, 8
    loop .set_page_table
    
    mov edi, page_table_l4
    mov cr3, edi
    
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
    
    lgdt [gdt64.pointer]
    jmp gdt64.code:long_mode_start

.no_long_mode:
    mov dword [0xb8000], 0x4f524f45
    mov dword [0xb8004], 0x4f3a4f52
    mov dword [0xb8008], 0x4f204f20
    mov dword [0xb800c], 0x4f6f4f4e
    mov dword [0xb8010], 0x4f4c4f20
    hlt

bits 64
long_mode_start:
    mov ax, gdt64.data
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    mov rsp, stack_top
    xor rbp, rbp
    
    call boot_main
    
    cli
    hlt
    jmp $

section .rodata
gdt64:
    dq 0
.code: equ $ - gdt64
    dq (1<<43) | (1<<44) | (1<<47) | (1<<53)
.data: equ $ - gdt64
    dq (1<<44) | (1<<47)
.pointer:
    dw $ - gdt64 - 1
    dq gdt64

section .bss
align 4096
page_table_l4:
    resb 4096
page_table_l3:
    resb 4096
page_table_l2:
    resb 4096

align 16
stack_bottom:
    resb 16384
stack_top:
