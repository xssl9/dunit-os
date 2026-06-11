section .text
bits 64

global gdt_flush
global gdt_load_tss

gdt_flush:
    lgdt [rdi]
    
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    pop rdi
    mov rax, 0x08
    push rax
    push rdi
    retfq

gdt_load_tss:
    ltr di
    ret
