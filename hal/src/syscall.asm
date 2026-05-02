section .text
bits 64

extern syscall_handler

global syscall_entry
syscall_entry:
    swapgs
    
    mov [gs:0x08], rsp
    mov rsp, [gs:0x00]
    
    push rcx
    push r11
    
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9
    
    mov rdi, rax
    mov rsi, rdi
    mov rdx, rsi
    mov rcx, rdx
    mov r8, r10
    mov r9, r8
    
    call syscall_handler
    
    pop r9
    pop r8
    pop r10
    pop rdx
    pop rsi
    pop rdi
    
    pop r11
    pop rcx
    
    mov rsp, [gs:0x08]
    
    swapgs
    
    sysretq

global syscall_init
syscall_init:
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1
    wrmsr
    
    mov ecx, 0xC0000081
    rdmsr
    mov edx, 0x00180008
    wrmsr
    
    mov ecx, 0xC0000082
    lea rax, [rel syscall_entry]
    mov rdx, rax
    shr rdx, 32
    wrmsr
    
    mov ecx, 0xC0000084
    mov eax, 0x00000200
    xor edx, edx
    wrmsr
    
    ret
