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

_start:
    cli
    
    mov dword [0xb8000], 0x2f542f45
    mov dword [0xb8004], 0x2f542f53
    mov dword [0xb8008], 0x2f4b2f20
    mov dword [0xb800c], 0x2f522f45
    mov dword [0xb8010], 0x2f4c2f4e
    mov dword [0xb8014], 0x2f212f21
    
    hlt
    jmp $
