# Syscall ABI Foundation

**Status:** WORKING / NOT YET FROZEN
**Related:** [[../InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime]] · [[../Future/Libc-Musl|musl]]

## Current register ABI

- `rax` — syscall number/result.
- `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9` — six arguments.
- `rcx`, `r11` — clobbered by `syscall`.

## Delivered

- CPL3 syscall entry/return and Rust dispatch.
- Mapped userspace range validation/copy helpers.
- File, process, cwd, stat/readdir, input, IPC, GUI and system-info operations.
- Invalid pointer/unknown syscall regression coverage.

## Boundary

Register convention works, but public ABI still needs versioned number/error manifests, handle rights, process-entry/auxv, clocks, VM, events, threads/TLS and capability discovery before musl/third-party SDK freeze.
