# Dunit libc — musl fork

**Status:** PLANNED / STATIC-FIRST
**Depends on:** [[../InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime]] · [[Installed-System|Installed System]]
**Network dependency:** [[Network-Stack|Native sockets]]

## Decision

Основной libc target — полный fork musl. relibc/newlib допустимы только как fallback после технического spike. Dunit сохраняет собственный syscall/object ABI; POSIX headers являются source compatibility layer, а не Linux kernel compatibility.

## Source and toolchain

- [ ] Separate `dunit-musl` fork of complete upstream tree with pinned tag/commit.
- [ ] Controlled upstream delta and regular security/bugfix rebases.
- [ ] `x86_64-dunit` compiler wrapper/target, LLD flow and isolated sysroot.
- [ ] Build `crt1.o`, `crti.o`, `crtn.o`, public headers and static `libc.a`.
- [ ] Prevent accidental host headers/libraries.

## ABI prerequisites

- Versioned syscall numbers/error mapping and generated Rust/C definitions.
- Stable process initial stack, `argc/argv/envp/auxv` and ELF contract.
- `mmap/munmap/mprotect`, clocks/sleep and secure entropy.
- Files/directories/stat/fsync translation without exposing kernel structs.
- Spawn/exec-image/wait/pipes/dup inheritance contract.
- Threads, x86_64 `FS.base` TLS and atomic wait/wake.

## Ordered profiles

1. **A:** custom crt, exit/write/errno, ISO C core, static hello/args/env.
2. **B:** VM, allocator, clocks and entropy.
3. **C:** files/directories, buffered stdio and installed sysroot.
4. **D:** Dunit-native `posix_spawn`, wait, pipes and redirection; no mandatory `fork`.
5. **E:** pthread/TLS, mutex/condvar/once over native threads/wait-wake.
6. **F:** sockets after `netd`; signals and dynamic loader are separate later gates.

## Rules

- Unsupported functions return honest errors; no fake success stubs.
- Do not spread uncontrolled `#ifdef __dunit__`; keep a narrow OS backend/translation layer.
- Kernel never consumes public musl/Linux structs.
- Static linking is the official first package profile.
- `fork`, full signals, cancellation and `.so` do not block libc v1.

## Acceptance

`x86_64-dunit-cc -static hello.c -o hello` produces a host-independent ELF. Startup/args/env, allocator, file/persistence, spawn and pthread test tiers pass through `tools/qemu_test.py`; supported/unsupported POSIX surface is versioned and published.
