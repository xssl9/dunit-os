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
- `userspace/libdunit/src/rights_numbers.rs` — Rust `pub const RIGHT_<NAME>`
- `abi/include/dunit/syscall.h` — C `#define SYS_<NAME>` (consumed by the musl fork)
- `abi/include/dunit/errno.h` — C `#define DUNIT_E<NAME>` (== POSIX errno magnitudes)
- `abi/include/dunit/rights.h` — C `#define DUNIT_RIGHT_<NAME>` (handle capability bits)

The kernel's `enum Syscall`, its `E<NAME>` error consts (`kernel/src/syscall/mod.rs`)
and its `RIGHT_<NAME>` bits (`kernel/src/handle.rs`) are the implementor; the
generator refuses to run if any disagrees with its manifest, so the kernel,
libdunit and the C sysroot stay locked together.

## Error numbers

`abi/errno.abi` lists each error as `<positive magnitude> <NAME>`. Dunit syscalls
report failure by returning the negated value in `rax` (`EINVAL` → `-22`); the
magnitudes equal POSIX errno, so the Dunit-error → POSIX-errno mapping is identity.

## Handle rights

`abi/rights.abi` lists each capability bit as `<bit index> <NAME>`. A handle's
rights only ever narrow (never widen) when transferred between processes.

## Capability query

`sys_abi_query` (syscall `AbiQuery`) lets a program negotiate features explicitly
instead of inferring them from a kernel version. libdunit exposes it as
`abi_query(query, arg)` / `abi_version()` with selectors:

- `ABI_QUERY_VERSION` (0) → ABI major version (`0` for v0)
- `ABI_QUERY_SYSCALL_COUNT` (1) → size of the syscall number space (highest + 1)
- `ABI_QUERY_HAS_SYSCALL` (2) → `1` if syscall number `arg` is enabled in this
  build, else `0` (so a cfg-gated syscall like the boot-smoke `SmokeDone` reports
  `0` in a production kernel)

## Status / roadmap

- [x] Syscall-number manifest + Rust/C generation + kernel consistency check.
- [x] Error-number manifest + Rust/C generation + `error_name()` + kernel check (M6.3).
- [x] Handle-rights manifest + Rust/C generation + kernel check (M6.3). Reserved fds `0/1/2` + inheritance still to document.
- [x] Capability/feature query (`sys_abi_query`) + `abi_test` smoke (M6.3).
- [ ] Scalar widths/alignment for `time_t`/`off_t`/`ino_t`/`pid_t`/pointers (M6.3).
- [ ] Documented process-entry stack (`argc/argv/envp`/auxv) + crt1 (M6.3).
- [ ] ELF contract: types, load bias, `PT_TLS`/`PT_GNU_RELRO`/`PT_INTERP` (M6.3).

See `DUNIT_OS_TECHNICAL_ROADMAP.md` §M6 for the full plan.
