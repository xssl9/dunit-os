# STATUS

> Snapshot of Dunit OS during the Userspace Runtime v1 stabilization work.

---

## Summary

Dunit OS now has a usable early kernel/runtime foundation. The kernel can boot
into terminal mode, initialize VFS/MemFS, execute embedded userspace ELF
programs, run child processes cooperatively through `spawn`/`yield`, observe
exit/fault statuses through `wait`, and perform VFS/stdin/stdout/IPC syscalls
from userspace.

The system is still not a persistent OS yet: the block layer is a volatile
`ramblk0` smoke device, there is no disk-backed dunitFS, no real userspace shell,
and timer preemption/background process semantics are not hardened.

---

## Subsystem Status

| Subsystem | Status | Notes |
|---|---|---|
| Boot | WORKING | Limine boot to terminal/GUI modes |
| HAL | WORKING | GDT, IDT, interrupts, syscall entry foundation |
| Interrupts | PARTIAL | Keyboard, mouse, timer/PIC paths exist; full IRQ routing is not done |
| Memory | PARTIAL | PMM/heap init works; per-process address-space objects exist |
| Allocator | WORKING | Kernel heap works for current runtime features |
| Scheduler | PARTIAL | Cooperative Ready queue, `yield`, context save/restore, experimental preempt hook |
| Processes | PARTIAL | PID table, parent/child, fd table, cwd, wait/reap, exit/fault status |
| IPC | PARTIAL | Basic message queues support parent/child round trips |
| Syscalls | PARTIAL | ABI + safe-copy + VFS/stdio/process/input/IPС/sysinfo syscalls work |
| ELF Loader | WORKING | Embedded `/app` ELF exec path with argv/envp and per-process context |
| VFS | WORKING | Runtime root FS, path normalization, file ops |
| MemFS | WORKING | Runtime directories/files and read/write semantics |
| DevFS | PARTIAL | `/dev` nodes exist; most devices are diagnostics/stubs |
| ProcFS | PARTIAL | `/proc/processes` and `/proc/meminfo` exist |
| Terminal Mode | WORKING | Commands, VFS integration, `dufetch` |
| Framebuffer | WORKING | Terminal rendering and boot screen path |
| GUI Mode | PARTIAL | Experimental window/app code |
| Window Management | SKELETON | In-kernel experimental WM, not final architecture |
| Userspace Runtime | PARTIAL | Foreground exec, cooperative child execution, stdin/stdout, VFS, IPC |
| Drivers | PARTIAL | Keyboard, mouse, PCI diagnostics, xHCI bring-up, ramblk0 block smoke |
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

1. Finish [[Tasks/InProgress/Userspace-Runtime-v1|Userspace Runtime v1]]:
   make `runtime_stress` the canonical automated QEMU regression and keep docs
   aligned with code.
2. Userspace terminal foundation: move command parsing out of the kernel after
   stdin/wait contracts are stable enough.
3. Persistent filesystem preparation: grow from `ramblk0` smoke media toward a
   real disk-backed block driver and mountable dunitFS.
