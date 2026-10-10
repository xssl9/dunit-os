/* Dunit crt0 — freestanding C entry stub (no libc).
 *
 * Validates the Dunit Userspace ABI v0 process-entry contract from C: the kernel
 * ELF loader hands us a SysV-style initial stack (argc at [rsp], then argv[],
 * NULL, envp[], NULL). We read argc/argv/envp straight off the stack — NOT from
 * registers — so this exercises the stack layout a future musl crt1 will parse.
 * Then we 16-align the stack, call main(argc, argv, envp), and exit via the
 * Exit syscall (number 0) with main's return value. This is the shape the
 * eventual Dunit musl crt1 generalises.
 */
    .text
    .globl _start
    .type _start, @function
_start:
    mov     (%rsp), %rdi            /* argc                                   */
    lea     8(%rsp), %rsi           /* argv  = &stack[8]                      */
    lea     16(%rsp,%rdi,8), %rdx   /* envp  = &stack[16 + argc*8]            */
    and     $-16, %rsp              /* 16-align; `call` then leaves 8 mod 16  */
    xor     %ebp, %ebp              /* outermost frame                        */
    call    main
    mov     %eax, %edi              /* exit status = main() return            */
    xor     %eax, %eax              /* rax = SYS_EXIT (0)                      */
    syscall
.Lhang:
    hlt
    jmp     .Lhang
    .size _start, . - _start

/* No executable stack. */
    .section .note.GNU-stack,"",@progbits
