section .text
bits 64

extern syscall_handler

%define USER_CODE_SELECTOR 0x1B
%define USER_DATA_SELECTOR 0x23

global syscall_entry
syscall_entry:
    ; SYSCALL leaves the user return RIP in RCX and RFLAGS in R11.
    ; It does not switch stacks, so use a private kernel syscall stack until
    ; the process layer grows per-process kernel stacks.
    mov [rel syscall_user_rsp], rsp
    lea rsp, [rel syscall_stack_top]
    and rsp, -16

    push rcx
    push r11

    ; Userspace ABI:
    ;   rax=syscall number, rdi/rsi/rdx/r10/r8/r9=args 0..5
    ; Rust extern "C" ABI:
    ;   rdi/rsi/rdx/rcx/r8/r9=args 0..5
    mov r11, r8
    mov r8, r10
    mov rcx, rdx
    mov rdx, rsi
    mov rsi, rdi
    mov rdi, rax
    mov r9, r11

    call syscall_handler

    pop r11
    pop rcx

    ; Return with IRETQ instead of SYSRET because the current GDT layout has
    ; user code before user data, which is not compatible with SYSRET's fixed
    ; selector derivation rules.
    mov r10, [rel syscall_user_rsp]
    push qword USER_DATA_SELECTOR
    push r10
    push r11
    push qword USER_CODE_SELECTOR
    push rcx
    iretq

global syscall_init
syscall_init:
    ; EFER.SCE = enable SYSCALL/SYSRET instructions.
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1
    wrmsr

    ; STAR: kernel CS is 0x08. The upper user selector field is left matching
    ; the current GDT even though syscall_entry returns via IRETQ.
    mov ecx, 0xC0000081
    rdmsr
    mov eax, 0
    mov edx, 0x00180008
    wrmsr

    ; LSTAR = syscall entry point.
    mov ecx, 0xC0000082
    lea rax, [rel syscall_entry]
    mov rdx, rax
    shr rdx, 32
    wrmsr

    ; SFMASK: mask IF while inside the syscall handler.
    mov ecx, 0xC0000084
    mov eax, 0x00000200
    xor edx, edx
    wrmsr

    ret

section .bss
alignb 16
syscall_user_rsp:
    resq 1
alignb 16
syscall_stack:
    resb 16384
syscall_stack_top:
