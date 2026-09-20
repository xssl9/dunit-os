# Userspace Runtime

**Status:** WORKING FOUNDATION / NEEDS V2 RUNTIME PRIMITIVES
**Roadmap:** [[../../ROADMAP|ROADMAP]]

## Working contract

- Embedded `/app` ELF programs run in ring 3 with args/environment.
- Per-process address-space records, kernel stacks, cwd and fd tables exist.
- Exit and recoverable user faults become wait statuses.
- Cooperative spawn/yield/wait works for regression scenarios.
- File/stat/readdir/cwd/stdin/stdout/process/sysinfo/input/IPC wrappers exist in `libdunit`.
- Parent/child IPC round trips and runtime stress are automated.

## Current limits

- Applications are embedded into current image/root, not loaded from normal installed filesystem.
- Timer preemption passes a gated smoke test, but normal scheduler remains cooperative/default-off; long-running independent services are not hardened.
- No userspace threads/TLS, shared VM object model, general events/poll or rights-bearing handles.
- Process entry/ABI is not yet frozen for musl/toolchain compatibility.
- GUI services and shell/session are not yet ordinary supervised services.

## Next contract

Work moves through [[Kernel-Runtime-Prerequisites|Kernel Runtime Prerequisites]]:

- preemption and thread lifecycle;
- blocking events/wait queues;
- shared VM and handle rights;
- stable process entry/auxv/ELF/TLS ABI;
- spawn/exec-image inheritance and installed-disk loading;
- service supervision/restart.

## Canonical regression

```bash
python3 tools/qemu_test.py \
  --build \
  --config limine_test_terminal.conf \
  --cmd "exec runtime_stress" \
  --timeout 120 \
  --cmd-timeout 20 \
  --settle 2 \
  --json
```

Required evidence includes successful boot, `runtime_stress: OK`, exit code `0` and clean VFS handles.
