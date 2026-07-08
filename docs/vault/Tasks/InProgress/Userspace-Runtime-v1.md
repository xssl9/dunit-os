# Userspace Runtime v1

**Status:** ACTIVE / stabilization milestone  
**Roadmap:** [[../../ROADMAP|ROADMAP]]

---

## Goal

Turn the current userspace runtime from a working foundation into a stable
contract that future shell, GUI, filesystem, and networking work can rely on.

This milestone is about runtime reliability, not a new feature surface.

---

## Current Working Contract

- Userspace ELF binaries are embedded under `/app` and loaded through VFS.
- `exec` can launch a foreground userspace program and report exit/fault status.
- `spawn` prepares a child process as Ready.
- `yield` can transfer execution to Ready children and resume the parent.
- `wait` observes real child exit/fault status after the child has run.
- Recoverable userspace faults are converted into process fault statuses.
- Process-local fd tables back VFS file IO and stdio.
- Foreground terminal stdin can feed interactive userspace programs.
- Basic IPC send/receive supports parent/child round trips.

---

## Definition Of Done

- `runtime_stress` is the canonical regression application for this milestone.
- The only supported autonomous launch/test entrypoint is
  `build_and_run_multipass.py`.
- A terminal-mode automated run must build the ISO, boot QEMU, inject
  `exec runtime_stress`, parse the serial log, and require:
  - `runtime_stress: OK`
  - `exec: /app/runtime_stress returned code=0`
- README, STATUS, ROADMAP, and AI context describe the same runtime state.
- Host-side kernel tests are either fixed or clearly documented as not the
  runtime verification path.

---

## Canonical Test Command

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --qemu-timeout 60 \
  --qemu-log qemu_runtime_stress.log \
  --qemu-test-commands "exec runtime_stress" \
  --expect-log "runtime_stress: OK" \
  --expect-log "exec: /app/runtime_stress returned code=0"
```

---

## Follow-up Work After v1

- Move the kernel terminal toward a userspace shell.
- Add blocking or pollable wait/input semantics instead of manual yield loops.
- Tighten syscall errno documentation and libdunit wrappers.
- Decide whether timer preemption remains experimental or becomes a v2 target.
