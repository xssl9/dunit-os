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
extern kernel_main

_start:
    cli
    mov esp, stack_top
    
    mov dword [0xb8000], 0x2f4f2f42
    mov dword [0xb8004], 0x2f542f4f
    
    call kernel_main
    
    hlt
    jmp $

section .bss
align 16
stack_bottom:
    resb 16384
stack_top:
