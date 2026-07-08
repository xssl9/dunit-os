# AI CONTEXT - Dunit OS

> Read this first before changing the project. The vault replaces a flat task.md: use [[../STATUS|STATUS]], [[../ROADMAP|ROADMAP]], and task nodes as the current source of truth.

Last updated: 2026-07-08

---

## What This Is

**Dunit OS (Green Tea)** is an x86_64 OS project with a Rust `no_std` kernel, C/NASM HAL pieces, a kernel terminal, experimental GUI code, a runtime VFS/MemFS layer, early userspace syscall support, and QEMU virtio block storage.

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
- Process model exists: PID, parent/child, cwd, fd table, wait/reap status.
- fd `0/1/2` are reserved for stdin/stdout/stderr.
- Userspace syscall ABI uses:
  - `rax` syscall number
  - `rdi/rsi/rdx/r10/r8/r9` args
  - `rax` return value
  - `rcx/r11` clobbered by `syscall`
- Safe user copy helpers are bounded but still range-check-only.
- CPL3 smoke can enter userspace, perform syscalls, and return.
- Foreground `exec` runs embedded `/app` ELF programs with argv/envp.
- `spawn` creates Ready child processes and `yield` can run them cooperatively.
- `wait` reports real child exit/fault status after execution.
- Basic IPC supports parent/child send/receive round trips.
- Userspace `open/read/write/close` go through process fd table into VFS.
- Userspace `write(1/2)` writes to serial as minimal stdio.
- Block device layer registers `ramblk0` and legacy virtio-blk `vd0`.
- `build_and_run_multipass.py --disk virtio` attaches a 1 MiB raw QEMU disk.
- Terminal `blk`, `blkread`, and `blkwrite` verify sector read/write paths.

---

## Important Completed Nodes

- [[../Tasks/Completed/VFS-MemFS|VFS + MemFS Runtime Layer]]
- [[../Tasks/Completed/Syscall-ABI|Syscall ABI + Safe User Copy]]
- [[../Tasks/Completed/Process-FD-Model|Current Process + FD Table]]
- [[../Tasks/Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[../Tasks/Completed/Stdio-FD|Minimal Stdio FDs]]
- [[../Tasks/Completed/Dufetch|dufetch]]
- [[../Tasks/Completed/Terminal-Mode|Kernel Terminal Mode]]
- [[../Tasks/Completed/Block-Storage-v1|Block Storage v1]]

---

## Still Not Done

- Persistent dunitFS.
- Real mount table beyond root MemFS.
- Mountable persistent dunitFS.
- Real userspace terminal/shell using stdio.
- Blocking wait/input semantics.
- Hardened timer preemption/background process model.
- Network stack.
- GUI display server architecture.

---

## Do Not Assume

- Do not assume a persistent filesystem exists. Raw sector IO exists on `vd0`,
  but it is not mounted as files yet.
- Do not assume `/dev` or `/proc` have real backends.
- Do not assume long-running background userspace processes are fully hardened.
- Do not treat current safe-copy as page-table validation. It is bounded range checking until page-fault recovery/user address spaces exist.
- Do not rewrite scheduler, HAL, drivers, GUI, or memory management unless the task explicitly requires it.

---

## Preferred Test Workflow

Use `build_and_run_multipass.py` as the only autonomous launch/test path:

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --qemu-timeout 60 \
  --qemu-log qemu_runtime_stress.log \
  --qemu-test-commands "exec runtime_stress" \
  --expect-log "runtime_stress: OK" \
  --expect-log "exec: /app/runtime_stress returned code=0"
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

Block Storage v1 regression:

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --disk virtio \
  --qemu-timeout 60 \
  --qemu-log qemu_block_v1.log \
  --qemu-test-commands "blk;blkread vd0 0;blkwrite vd0 3;blkread vd0 3" \
  --expect-log "vd0" \
  --expect-log "virtio-blk" \
  --expect-log "vd0 lba=3 written=512" \
  --expect-log "DUNIT-BLOCK-STOR" \
  --expect-log "AGE-V1"
```

---

## Architecture Notes For Future Work

1. VFS/MemFS is a runtime filesystem layer only. Persistent dunitFS is future work.
2. Current fd table is process-local, but there is only a minimal current-process model.
3. Kernel terminal cwd is separate from process cwd.
4. Stdio exists only as reserved fd targets and serial output for stdout/stderr.
5. Userspace file IO is ready enough for the next stage, but stdin and real app launch are still blockers for a userspace terminal.
6. `runtime_stress` is the canonical runtime regression app for VFS,
   resumable child execution, IPC, repeated spawn/wait, and recoverable faults.
7. `vd0` is a raw block device. The next storage milestone should build a
   mountable dunitFS layer above it instead of adding another storage backend.

---

## Vault Reading Order

1. [[../STATUS|STATUS]]
2. [[../ROADMAP|ROADMAP]]
3. Relevant task node under `Tasks/Completed`, `Tasks/InProgress`, or `Tasks/Future`
4. Code
