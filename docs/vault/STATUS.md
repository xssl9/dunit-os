# STATUS

> Snapshot of Dunit OS after the VFS/MemFS, syscall, stdio, and `dufetch` work.

---

## Summary

Dunit OS now has a usable early kernel/runtime foundation. The kernel can boot into terminal mode, initialize a runtime filesystem, run terminal filesystem commands through VFS, enter CPL3 for syscall smoke tests, and perform userspace file IO through process-local file descriptors.

The system is still not a persistent OS yet: no block device layer, no persistent dunitFS, no real userspace terminal, and no isolated process model.

---

## Subsystem Status

| Subsystem | Status | Notes |
|---|---|---|
| Boot | WORKING | Limine boot to terminal/GUI modes |
| HAL | WORKING | GDT, IDT, interrupts, syscall entry foundation |
| Interrupts | PARTIAL | Keyboard IRQ path works; full device IRQ model is not done |
| Memory | PARTIAL | PMM/heap init works; VMM is still minimal |
| Allocator | WORKING | Kernel heap works for current runtime features |
| Scheduler | SKELETON | Current-process foundation only |
| Processes | PARTIAL | PID/current process/cwd/fd table exist |
| IPC | SKELETON | Basic structures exist, not a full userspace IPC layer |
| Syscalls | PARTIAL | ABI + safe-copy + VFS/stdio syscalls work |
| ELF Loader | SKELETON | Build artifacts exist; real exec path remains future work |
| VFS | WORKING | Runtime root FS, path normalization, file ops |
| MemFS | WORKING | Runtime directories/files and read/write semantics |
| DevFS | SKELETON | Stub backend only |
| ProcFS | SKELETON | Stub backend only |
| Terminal Mode | WORKING | Commands, VFS integration, `dufetch` |
| Framebuffer | WORKING | Terminal rendering and boot screen path |
| GUI Mode | PARTIAL | Experimental window/app code |
| Window Management | SKELETON | In-kernel experimental WM, not final architecture |
| Userspace Runtime | PARTIAL | libdunit syscall wrappers + smoke path |
| Drivers | PARTIAL | Keyboard path works; disk/network/USB/sound planned |
| Networking | PLANNED | No stack yet |

---

## Recently Completed

- [[Tasks/Completed/VFS-MemFS|VFS + MemFS Runtime Layer]]
- [[Tasks/Completed/Syscall-ABI|Syscall ABI + Safe User Copy]]
- [[Tasks/Completed/Process-FD-Model|Current Process + FD Table]]
- [[Tasks/Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[Tasks/Completed/Stdio-FD|Minimal Stdio FDs]]
- [[Tasks/Completed/Dufetch|dufetch]]

---

## Next Reasonable Work

1. Userspace terminal foundation: stdin behavior, stdout/stderr path, and a real userspace shell test.
2. ELF exec integration with process model: load one known app and return/fault cleanly.
3. Block-device preparation for persistent dunitFS: disk abstraction before any on-disk filesystem.
