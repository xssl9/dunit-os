# AI CONTEXT - Dunit OS

> Read this first before changing the project. The vault replaces a flat task.md: use [[../STATUS|STATUS]], [[../ROADMAP|ROADMAP]], and task nodes as the current source of truth.

Last updated: 2026-06-09

---

## What This Is

**Dunit OS (Green Tea)** is an x86_64 OS project with a Rust `no_std` kernel, C/NASM HAL pieces, a kernel terminal, experimental GUI code, a runtime VFS/MemFS layer, and early userspace syscall support.

Repository: `https://github.com/susopki/dunit-os`

---

## Current Working State

- Boots through Limine into terminal mode.
- HAL initializes enough for GDT/IDT/interrupts/syscall entry.
- PMM/VMM/heap init path works for the current boot flow.
- Kernel terminal is usable and has VFS-backed commands.
- `dufetch` prints Dunit OS ASCII logo and system summary.
- VFS is initialized at boot.
- Root MemFS is mounted as `/`.
- MemFS supports runtime directories/files and basic file operations.
- Current process model exists: PID, cwd, fd table.
- fd `0/1/2` are reserved for stdin/stdout/stderr.
- Userspace syscall ABI uses:
  - `rax` syscall number
  - `rdi/rsi/rdx/r10/r8/r9` args
  - `rax` return value
  - `rcx/r11` clobbered by `syscall`
- Safe user copy helpers are bounded but still range-check-only.
- CPL3 smoke can enter userspace, perform syscalls, and return.
- Userspace `open/read/write/close` go through process fd table into VFS.
- Userspace `write(1/2)` writes to serial as minimal stdio.

---

## Important Completed Nodes

- [[../Tasks/Completed/VFS-MemFS|VFS + MemFS Runtime Layer]]
- [[../Tasks/Completed/Syscall-ABI|Syscall ABI + Safe User Copy]]
- [[../Tasks/Completed/Process-FD-Model|Current Process + FD Table]]
- [[../Tasks/Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[../Tasks/Completed/Stdio-FD|Minimal Stdio FDs]]
- [[../Tasks/Completed/Dufetch|dufetch]]
- [[../Tasks/Completed/Terminal-Mode|Kernel Terminal Mode]]

---

## Still Not Done

- Persistent dunitFS.
- Block device abstraction.
- Disk driver.
- Real mount table beyond root MemFS.
- Real userspace terminal using stdio.
- Real ELF exec path with process isolation.
- Per-process address spaces and per-process kernel stacks.
- Full scheduler/preemption model.
- Network stack.
- GUI display server architecture.

---

## Do Not Assume

- Do not assume persistent storage exists.
- Do not assume `/dev` or `/proc` have real backends.
- Do not assume userspace apps are fully executed as isolated processes.
- Do not treat current safe-copy as page-table validation. It is bounded range checking until page-fault recovery/user address spaces exist.
- Do not rewrite scheduler, HAL, drivers, GUI, or memory management unless the task explicitly requires it.

---

## Preferred Test Workflow

Use `build_and_run_multipass.py` as the main autonomous verification path from Windows:

```powershell
python build_and_run_multipass.py --qemu-timeout 40 --qemu-log qemu_test.log --qemu-test-commands "dufetch;pwd;ls"
```

Expected boot evidence in serial logs:

```text
[MEMFS] mounted as /
[SYSCALL-TEST] userspace syscall OK
[STDOUT] [STDOUT-TEST] hello from userspace
[SYSCALL-FS-TEST] OK
[SYSCALL-FS-SEMANTICS-TEST] OK
root@dunit:~#
```

---

## Architecture Notes For Future Work

1. VFS/MemFS is a runtime filesystem layer only. Persistent dunitFS is future work.
2. Current fd table is process-local, but there is only a minimal current-process model.
3. Kernel terminal cwd is separate from process cwd.
4. Stdio exists only as reserved fd targets and serial output for stdout/stderr.
5. Userspace file IO is ready enough for the next stage, but stdin and real app launch are still blockers for a userspace terminal.

---

## Vault Reading Order

1. [[../STATUS|STATUS]]
2. [[../ROADMAP|ROADMAP]]
3. Relevant task node under `Tasks/Completed`, `Tasks/InProgress`, or `Tasks/Future`
4. Code
