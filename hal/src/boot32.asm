section .text
bits 64

global _start
extern boot_main

_start:
    cli
    mov rsp, stack_top
    xor rbp, rbp
    
    call boot_main
    
    cli
    hlt
    jmp $

section .bss
align 16
stack_bottom:
    resb 16384
stack_top:
