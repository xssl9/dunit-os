# Syscall ABI + Safe User Copy

**Status:** Done / foundation working  
**Links:** [[../../STATUS|STATUS]] · [[Process-FD-Model|Process FD Model]] · [[Userspace-VFS-Syscalls|Userspace VFS Syscalls]]

---

## ABI

- `rax` = syscall number
- `rdi` = arg0
- `rsi` = arg1
- `rdx` = arg2
- `r10` = arg3
- `r8` = arg4
- `r9` = arg5
- `rax` = return value
- `rcx` and `r11` are clobbered by `syscall`.

---

## What Works

- syscall entry path from CPL3 to kernel and return.
- Rust syscall handler dispatch.
- invalid syscall diagnostics.
- bounded user string and buffer copy helpers.
- userspace smoke test enters CPL3 and returns.
- smoke verifies VFS syscalls and stdout.

---

## Limitation

Safe-copy is still range-check-only. It does not yet validate per-process page tables and does not recover from page faults during kernel copies.
