# Current Process + FD Table

**Status:** Done / minimal foundation  
**Links:** [[../../STATUS|STATUS]] · [[Syscall-ABI|Syscall ABI]] · [[Userspace-VFS-Syscalls|Userspace VFS Syscalls]]

---

## What Works

- PID allocation.
- Minimal current process state.
- Process cwd initialized to `/`.
- Process-local fd table.
- fd allocation starts at `3`.
- fd `0`, `1`, and `2` are reserved for stdio.
- APIs exist:
  - `current_process()`
  - `current_process_mut()`
  - `allocate_fd()`
  - `get_fd()`
  - `close_fd()`

---

## Scope

This is not a full scheduler/process model. It is the smallest current-process layer needed for VFS syscalls and future userspace work.

---

## Not Done

- Per-process address spaces.
- Per-process kernel stacks.
- Full scheduler integration.
- Process lifecycle/exit cleanup.
- Real process cwd syscalls.
