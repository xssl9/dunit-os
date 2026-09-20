# Userspace Program Build and Execution

**Status:** WORKING FOUNDATION

## Current reality

The build produces many Rust `no_std` ELF programs under `build/userspace`, including runtime/process/IPC/file/input/fault tests and GUI clients such as terminal, calculator, stats, file manager and image viewer. They run through the current ELF/process/syscall stack rather than being build-only placeholders.

## Boundary

- Programs are currently assembled into the image and exposed through embedded `/app` content.
- They are not yet loaded from a normal installed `/system/bin` or `/apps` hierarchy.
- GUI clients use the legacy bridge, not final GUI Server protocol.
- C programs wait for [[../Future/Libc-Musl|Dunit musl]].

Target remains `userspace/x86_64-unknown-none.json` until a versioned Dunit target/SDK is introduced.
