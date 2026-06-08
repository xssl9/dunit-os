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
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9

    ; Userspace ABI:
    ;   rax=syscall number, rdi/rsi/rdx/r10/r8/r9=args 0..5
    ; Rust extern "C" ABI:
    ;   rdi/rsi/rdx/rcx/r8/r9=args 0..5, additional args on stack
    ; The first Rust argument is the syscall number, so userspace arg5 is
    ; passed as the first stack argument.
    mov r10, [rsp]       ; user arg5
    mov r9, [rsp + 8]    ; user arg4
    mov r8, [rsp + 16]   ; user arg3
    mov rcx, [rsp + 24]  ; user arg2
    mov rdx, [rsp + 32]  ; user arg1
    mov rsi, [rsp + 40]  ; user arg0
    mov rdi, rax

    sub rsp, 8
    push r10

    call syscall_handler

    add rsp, 16

    pop r9
    pop r8
    pop r10
    pop rdx
    pop rsi
    pop rdi
    pop r11
    pop rcx

    cmp rax, [rel syscall_smoke_return_magic]
    jne .return_to_user
    cmp qword [rel syscall_smoke_active], 1
    jne .return_to_user
    mov qword [rel syscall_smoke_active], 0
    mov rsp, [rel syscall_smoke_kernel_rsp]
    sti
    ret

.return_to_user:
    ; Return with IRETQ instead of SYSRET because the current GDT layout has
    ; user code before user data, which is not compatible with SYSRET's fixed
    ; selector derivation rules.
    push qword USER_DATA_SELECTOR
    push qword [rel syscall_user_rsp]
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

global run_user_syscall_smoke
run_user_syscall_smoke:
    mov [rel syscall_smoke_kernel_rsp], rsp
    mov qword [rel syscall_smoke_active], 1

    push qword USER_DATA_SELECTOR
    push rsi
    pushfq
    pop rax
    or rax, 0x200
    push rax
    push qword USER_CODE_SELECTOR
    push rdi
    iretq

section .rodata
align 8
syscall_smoke_return_magic:
    dq 0x0051595343414C4C

section .bss
alignb 16
syscall_user_rsp:
    resq 1
syscall_smoke_kernel_rsp:
    resq 1
syscall_smoke_active:
    resq 1
alignb 16
syscall_stack:
    resb 16384
syscall_stack_top:
