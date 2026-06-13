section .text
bits 64

extern syscall_handler

%define USER_CODE_SELECTOR 0x1B
%define USER_DATA_SELECTOR 0x23

global syscall_entry
syscall_entry:
    ; SYSCALL leaves the user return RIP in RCX and RFLAGS in R11.
    ; It does not switch stacks, so use the selected process kernel stack when
    ; the kernel has installed one, otherwise fall back to a private bootstrap
    ; syscall stack. This is still a single-active-process policy until the
    ; scheduler/TSS path can select stacks per running process.
    mov [rel syscall_user_rsp], rsp
    mov [rel syscall_saved_rax], rax
    mov [rel syscall_saved_rbx], rbx
    mov [rel syscall_saved_rcx], rcx
    mov [rel syscall_saved_rdx], rdx
    mov [rel syscall_saved_rsi], rsi
    mov [rel syscall_saved_rdi], rdi
    mov [rel syscall_saved_rbp], rbp
    mov [rel syscall_saved_r8], r8
    mov [rel syscall_saved_r9], r9
    mov [rel syscall_saved_r10], r10
    mov [rel syscall_saved_r11], r11
    mov [rel syscall_saved_r12], r12
    mov [rel syscall_saved_r13], r13
    mov [rel syscall_saved_r14], r14
    mov [rel syscall_saved_r15], r15
    mov rsp, [rel syscall_selected_stack_top]
    test rsp, rsp
    jnz .stack_selected
    lea rsp, [rel syscall_stack_top]
.stack_selected:
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
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
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

global syscall_set_kernel_stack_top
syscall_set_kernel_stack_top:
    and rdi, -16
    mov [rel syscall_selected_stack_top], rdi
    ret

global syscall_reset_kernel_stack
syscall_reset_kernel_stack:
    mov qword [rel syscall_selected_stack_top], 0
    ret

global syscall_get_escape_kernel_rsp
syscall_get_escape_kernel_rsp:
    mov rax, [rel syscall_smoke_kernel_rsp]
    ret

global syscall_get_escape_active
syscall_get_escape_active:
    mov rax, [rel syscall_smoke_active]
    ret

global syscall_restore_escape_state
syscall_restore_escape_state:
    mov [rel syscall_smoke_kernel_rsp], rdi
    mov [rel syscall_smoke_active], rsi
    ret

global syscall_capture_user_context
syscall_capture_user_context:
    ; Rust extern "C":
    ;   rdi = *mut CpuContext
    ;   rsi = resumed syscall return value
    mov [rdi + 0], rsi
    mov rax, [rel syscall_saved_rbx]
    mov [rdi + 8], rax
    mov rax, [rel syscall_saved_rcx]
    mov [rdi + 16], rax
    mov rax, [rel syscall_saved_rdx]
    mov [rdi + 24], rax
    mov rax, [rel syscall_saved_rsi]
    mov [rdi + 32], rax
    mov rax, [rel syscall_saved_rdi]
    mov [rdi + 40], rax
    mov rax, [rel syscall_saved_rbp]
    mov [rdi + 48], rax
    mov rax, [rel syscall_user_rsp]
    mov [rdi + 56], rax
    mov rax, [rel syscall_saved_r8]
    mov [rdi + 64], rax
    mov rax, [rel syscall_saved_r9]
    mov [rdi + 72], rax
    mov rax, [rel syscall_saved_r10]
    mov [rdi + 80], rax
    mov rax, [rel syscall_saved_r11]
    mov [rdi + 88], rax
    mov rax, [rel syscall_saved_r12]
    mov [rdi + 96], rax
    mov rax, [rel syscall_saved_r13]
    mov [rdi + 104], rax
    mov rax, [rel syscall_saved_r14]
    mov [rdi + 112], rax
    mov rax, [rel syscall_saved_r15]
    mov [rdi + 120], rax
    mov rax, [rel syscall_saved_rcx]
    mov [rdi + 128], rax
    mov rax, [rel syscall_saved_r11]
    mov [rdi + 136], rax
    ret

global syscall_escape_user_fault
syscall_escape_user_fault:
    cmp qword [rel syscall_smoke_active], 1
    jne .halt
    mov qword [rel syscall_smoke_active], 0
    mov rsp, [rel syscall_smoke_kernel_rsp]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    sti
    ret
.halt:
    cli
    hlt
    jmp .halt

global run_user_syscall_smoke
run_user_syscall_smoke:
    ; Called from Rust as an extern "C" function. The user payload can clobber
    ; SysV callee-saved registers before returning through the syscall
    ; trampoline, so preserve the kernel caller's callee-saved state here.
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov [rel syscall_smoke_kernel_rsp], rsp
    mov qword [rel syscall_smoke_active], 1
    mov rax, rdi
    mov rbx, rsi

    push qword USER_DATA_SELECTOR
    push rbx
    pushfq
    pop rcx
    or rcx, 0x200
    push rcx
    push qword USER_CODE_SELECTOR
    push rax
    xor rax, rax
    xor rcx, rcx
    xor rdx, rdx
    xor rsi, rsi
    xor rdi, rdi
    xor r8, r8
    xor r9, r9
    xor r10, r10
    xor r11, r11
    iretq

global run_user_process
run_user_process:
    ; Rust extern "C":
    ;   rdi=entry, rsi=user_stack, rdx=argc, rcx=argv, r8=envp
    ; Userspace entry ABI:
    ;   stack points at the argc/argv/envp block,
    ;   rdi=argc, rsi=argv, rdx=envp for tiny no-libc programs.
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov [rel syscall_smoke_kernel_rsp], rsp
    mov qword [rel syscall_smoke_active], 1
    mov rax, rdi
    mov rbx, rsi
    mov r12, rdx
    mov r13, rcx
    mov r14, r8

    push qword USER_DATA_SELECTOR
    push rbx
    pushfq
    pop rcx
    or rcx, 0x200
    push rcx
    push qword USER_CODE_SELECTOR
    push rax
    xor rax, rax
    xor rcx, rcx
    mov rdi, r12
    mov rsi, r13
    mov rdx, r14
    xor r8, r8
    xor r9, r9
    xor r10, r10
    xor r11, r11
    xor r12, r12
    xor r13, r13
    xor r14, r14
    iretq

global run_user_context
run_user_context:
    ; Rust extern "C":
    ;   rdi = *const CpuContext
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov [rel syscall_smoke_kernel_rsp], rsp
    mov qword [rel syscall_smoke_active], 1
    mov rbx, rdi

    mov rax, [rbx + 0]
    mov rcx, [rbx + 16]
    mov rdx, [rbx + 24]
    mov rsi, [rbx + 32]
    mov rdi, [rbx + 40]
    mov rbp, [rbx + 48]
    mov r8, [rbx + 64]
    mov r9, [rbx + 72]
    mov r10, [rbx + 80]
    mov r11, [rbx + 88]
    mov r12, [rbx + 96]
    mov r13, [rbx + 104]
    mov r14, [rbx + 112]
    mov r15, [rbx + 120]

    push qword USER_DATA_SELECTOR
    push qword [rbx + 56]
    push qword [rbx + 136]
    push qword USER_CODE_SELECTOR
    push qword [rbx + 128]
    mov rbx, [rbx + 8]
    iretq

section .rodata
align 8
syscall_smoke_return_magic:
    dq 0x0051595343414C4C

section .bss
alignb 16
syscall_user_rsp:
    resq 1
syscall_selected_stack_top:
    resq 1
syscall_smoke_kernel_rsp:
    resq 1
syscall_smoke_active:
    resq 1
syscall_saved_rax:
    resq 1
syscall_saved_rbx:
    resq 1
syscall_saved_rcx:
    resq 1
syscall_saved_rdx:
    resq 1
syscall_saved_rsi:
    resq 1
syscall_saved_rdi:
    resq 1
syscall_saved_rbp:
    resq 1
syscall_saved_r8:
    resq 1
syscall_saved_r9:
    resq 1
syscall_saved_r10:
    resq 1
syscall_saved_r11:
    resq 1
syscall_saved_r12:
    resq 1
syscall_saved_r13:
    resq 1
syscall_saved_r14:
    resq 1
syscall_saved_r15:
    resq 1
alignb 16
syscall_stack:
    resb 16384
syscall_stack_top:
