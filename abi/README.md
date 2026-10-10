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

## Process-entry ABI (current)

On entry the kernel ELF loader (`kernel/src/elf/mod.rs`) hands a program a
SysV-style initial stack plus register args:

```
rsp -> argc            (u64)
       argv[0..argc]   (*const u8), then NULL
       envp[0..]       (*const u8), then NULL
       auxv[0..]       { a_type: u64, a_val: u64 }, then { AT_NULL, 0 }
       (padding, then the NUL-terminated argv/env strings + AT_RANDOM bytes)
```

`%rsp` is `8 mod 16` (the x86-64 SysV *function*-entry shape a no-libc Rust
`_start` expects after a `call`); `%rdi=argc`, `%rsi=argv`, `%rdx=envp`; fds
`0/1/2` = stdin/stdout/stderr. The auxiliary vector carries the static-first
subset `AT_PAGESZ` (4096), `AT_SECURE` (0) and `AT_RANDOM` (→ 16 bytes).

A libc `crt1` re-aligns itself (musl's `_start` does `and $-16,%rsp`), so the
`8 mod 16` convention serves both a no-libc Rust `_start` and a libc crt1 — no
separate `rsp` 0-mod-16 switch is needed, and the auxv is the piece a crt1 reads
after the envp NULL. `AT_PHDR`/`AT_PHENT`/`AT_PHNUM`/`AT_ENTRY` (for TLS from
program headers) are added when the libc port needs them.

## ELF contract (current)

The loader accepts ELF64, little-endian, `EM_X86_64`, **`ET_EXEC` only** (fixed-
address static executables — matches M6's static-first stance; no PIE/`ET_DYN`).
Segments honoured: `PT_LOAD` (mapped) and `PT_TLS` (TLS image, Variant II). No
`PT_INTERP` / dynamic linker, no load bias, no `PT_GNU_RELRO` yet.

## ABI v0 conformance (freestanding C, no musl)

The roadmap's ABI v0 "done" gate is met: `userspace/ctests/` holds a freestanding
C program (`hello.c`) with a Dunit `crt0` (`crt0.s`) — no libc. The host C
compiler builds it against these generated headers; the kernel ELF loader runs it
as `/app/c_hello`. Its `crt0` reads `argc/argv/envp` straight off the SysV initial
stack (not the registers), calls `main`, makes raw `write`/`exit` syscalls, checks
the errno contract (an out-of-range fd returns `-EBADF`), and exits with `main`'s
value. `crt0.s` is the seed the eventual Dunit musl `crt1` generalises. Test:
`tools/qemu_test.py --build --markers-file tools/m6_c_abi_markers.json`.

## Status / roadmap

- [x] Syscall-number manifest + Rust/C generation + kernel consistency check.
- [x] Error-number manifest + Rust/C generation + `error_name()` + kernel check (M6.3).
- [x] Handle-rights manifest + Rust/C generation + kernel check (M6.3). Reserved fds `0/1/2` + inheritance still to document.
- [x] Capability/feature query (`sys_abi_query`) + `abi_test` smoke (M6.3).
- [x] Process-entry ABI + ELF contract documented (current shape above).
- [x] **ABI v0 conformance gate: freestanding C program (own crt0) reads args/env, raw syscalls, errno, exit — verified (M6.3).**
- [x] **Process-entry auxv** (`AT_PAGESZ`/`AT_SECURE`/`AT_RANDOM`/`AT_NULL`), verified from C (`c_hello` walks envp→auxv). Stack is now musl-crt1-ready; crt1 self-aligns, so no `rsp` 0-mod-16 switch needed (M6.3).
- [ ] `AT_PHDR`/`AT_ENTRY` auxv entries for TLS-from-program-headers, when the libc port needs them.
- [ ] Scalar widths/alignment for `time_t`/`off_t`/`ino_t`/`pid_t`/pointers, versioned wire structs (M6.3).
- [x] M6.2 `dunit-musl` fork repo created (`github.com/coreformdev/dunit-musl`, musl v1.2.6+67 `b1efda5b`, MIT) and wired here as the `toolchains/dunit-musl` submodule, with provenance (`UPSTREAM.md`), porting plan (`PORTING.md`) and CI.
- [x] M6.4 `x86_64-dunit` sysroot — `toolchains/dunit-musl/tools/dunit/build-libc.sh` produces a static `libc.a` + crt objects + headers; `make userspace` builds it once and links the `MUSL_CTESTS` against it. (A packaged `dunit-cc` wrapper is still a convenience TODO.)
- [x] M6.2 libc port — **`build:`/`syscall:`/`crt`/`fs:`(write+files)/`vm:` landed and verified on the kernel.** Static musl programs run over the Dunit ABI: `musl_hello` (crt1 + `__init_tls` + native `SetThreadPointer` + `write`), `musl_stdio` (`printf` via `__stdio_write` → native `Write`), `musl_malloc` (mallocng over native `Mmap`), `musl_file` (`open`/read/write/close via a native-`Open` flag/arg adapter, incl. a write+read roundtrip in `/persist`). Unmapped Linux syscalls return documented `-ENOSYS` sentinels (M6.1). See `userspace/ctests/musl_*.c` and `tools/m6_musl_*_markers.json`.
- [x] M6.2 libc port — **`thread:` works.** The native `__clone` + per-thread `SetThreadPointer` trampoline, a futex dispatcher onto `FutexWait`/`FutexWake`, `exit`→`ThreadExit` vs `exit_group`→`Exit`, and a new kernel **`SetTidAddress`** (75) clear-child-tid primitive (zeroes the registered word + `futex_wake` on thread exit) that releases musl's thread-list lock. `musl_thread` runs two pthreads with a contended mutex and joins them (counter=10000). **The static C acceptance suite `hello`/`stdio`/`malloc`/`file`/`thread` is complete.**
- [ ] M6.2 libc port — stdio read path (`__stdio_read`/`readv` → native `Read`) and `stat`/`getdents` struct translation; `signal:`/`network:`/`ldso:` stay `ENOSYS`.

See `DUNIT_OS_TECHNICAL_ROADMAP.md` §M6 for the full plan.
