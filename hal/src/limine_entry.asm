section .text
bits 64

global _start
extern boot_main

_start:
    cli
    
    lea rsp, [rel stack_top]
    xor rbp, rbp
    
    call boot_main
    
    cli
.hang:
    hlt
    jmp .hang

section .bss
align 16
stack_bottom:
    resb 65536
stack_top:
