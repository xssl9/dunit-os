# Dunit Userspace ABI v0

The stable contract between userspace and the Green Tea Kernel, versioned
independently of any single language binding. This is the foundation M6 (the
Dunit musl fork) builds on: the kernel speaks native Dunit syscalls — never Linux
numbers or structs — and both `libdunit` (Rust) and the C sysroot are generated
from the same source so they can never drift.

## Syscall numbers — single source of truth

`abi/syscalls.abi` lists every syscall as `<number> <CanonicalName>`. It is
authoritative. Regenerate the consumers after any edit:

    python3 tools/gen_abi.py          # regenerate + verify kernel agrees
    python3 tools/gen_abi.py --check  # verify only, write nothing (CI guard)

Generated, committed, never hand-edited:

- `userspace/libdunit/src/syscall_numbers.rs` — Rust `pub const SYSCALL_<NAME>`
- `userspace/libdunit/src/errno_numbers.rs` — Rust `pub const E<NAME>` + `error_name()`
- `abi/include/dunit/syscall.h` — C `#define SYS_<NAME>` (consumed by the musl fork)
- `abi/include/dunit/errno.h` — C `#define DUNIT_E<NAME>` (== POSIX errno magnitudes)

The kernel's `enum Syscall` and its `E<NAME>` error consts (`kernel/src/syscall/mod.rs`)
are the implementor; the generator refuses to run if either disagrees with its
manifest, so the kernel, libdunit and the C sysroot stay locked together.

## Error numbers

`abi/errno.abi` lists each error as `<positive magnitude> <NAME>`. Dunit syscalls
report failure by returning the negated value in `rax` (`EINVAL` → `-22`); the
magnitudes equal POSIX errno, so the Dunit-error → POSIX-errno mapping is identity.

## Status / roadmap

- [x] Syscall-number manifest + Rust/C generation + kernel consistency check.
- [x] Error-number manifest + Rust/C generation + `error_name()` + kernel check (M6.3).
- [ ] Handle/fd lifetime, rights, inheritance, reserved `0/1/2` (M6.3).
- [ ] Scalar widths/alignment for `time_t`/`off_t`/`ino_t`/`pid_t`/pointers (M6.3).
- [ ] Documented process-entry stack (`argc/argv/envp`/auxv) + crt1 (M6.3).
- [ ] ELF contract: types, load bias, `PT_TLS`/`PT_GNU_RELRO`/`PT_INTERP` (M6.3).

See `DUNIT_OS_TECHNICAL_ROADMAP.md` §M6 for the full plan.
