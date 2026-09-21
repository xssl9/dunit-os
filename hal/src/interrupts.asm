section .text
bits 64

%macro ISR_NOERRCODE 1
global isr%1
isr%1:
    push 0
    push %1
    jmp isr_common_stub
%endmacro

%macro ISR_ERRCODE 1
global isr%1
isr%1:
    push %1
    jmp isr_common_stub
%endmacro

ISR_NOERRCODE 0
ISR_NOERRCODE 1
ISR_NOERRCODE 2
ISR_NOERRCODE 3
ISR_NOERRCODE 4
ISR_NOERRCODE 5
ISR_NOERRCODE 6
ISR_NOERRCODE 7
ISR_ERRCODE 8
ISR_NOERRCODE 9
ISR_ERRCODE 10
ISR_ERRCODE 11
ISR_ERRCODE 12
ISR_ERRCODE 13
ISR_ERRCODE 14
ISR_NOERRCODE 15
ISR_NOERRCODE 16
ISR_ERRCODE 17
ISR_NOERRCODE 18
ISR_NOERRCODE 19
ISR_NOERRCODE 20
ISR_NOERRCODE 21
ISR_NOERRCODE 22
ISR_NOERRCODE 23
ISR_NOERRCODE 24
ISR_NOERRCODE 25
ISR_NOERRCODE 26
ISR_NOERRCODE 27
ISR_NOERRCODE 28
ISR_NOERRCODE 29
ISR_ERRCODE 30
ISR_NOERRCODE 31

%assign i 32
%rep 224
ISR_NOERRCODE i
%assign i i+1
%endrep

extern interrupt_handler
extern syscall_escape_user_fault
extern user_fpu_state

isr_common_stub:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    ; Save SIMD before Rust/C interrupt handling can touch XMM registers.
    ; The saved CS is at +144 after the 15 general-purpose pushes.
    test byte [rsp + 144], 3
    jz .no_user_fpu_save
    mov rax, [rel user_fpu_state]
    test rax, rax
    jz .no_user_fpu_save
    fxsave [rax]
.no_user_fpu_save:
    
    mov rdi, rsp
    mov rax, rsp
    and rsp, -16
    ; Keep the interrupted kernel's SIMD state too. The user copy above is
    ; durable across a scheduler escape; this stack copy protects normal IRQ
    ; returns even if the handler itself uses XMM registers.
    sub rsp, 528
    mov [rsp + 512], rax
    fxsave [rsp]
    call interrupt_handler
    cmp rax, 1
    je .escape_user_fault
    cmp rax, 2
    je .escape_user_fault
    fxrstor [rsp]
    mov rsp, [rsp + 512]
    
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    
    add rsp, 16
    
    iretq

.escape_user_fault:
    jmp syscall_escape_user_fault
