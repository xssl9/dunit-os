# Dunit OS (Green Tea)

> Microkernel-style OS project for x86_64 with a kernel terminal, framebuffer UI experiments, VFS/MemFS, syscall ABI foundation, and a small userspace runtime.

---

## Navigation

| Area | Link |
|---|---|
| Current state | [[STATUS|STATUS]] |
| Roadmap | [[ROADMAP|ROADMAP]] |
| AI handoff | [[AI-Context/CONTEXT|AI CONTEXT]] |
| Original vision | [[Origin/VISION|VISION]] |
| Original requirements | [[Origin/REQUIREMENTS|REQUIREMENTS]] |
| Original design | [[Origin/DESIGN|DESIGN]] |

---

## Project Status

```
Boot / HAL        #################### done
Terminal Mode     ##################-- working
VFS / MemFS       ################---- working runtime layer
Syscall ABI       ###############----- working foundation
Process model     ###########--------- minimal current process
Userspace runtime ##########---------- syscall wrappers + smoke
GUI Mode          #######------------- skeleton / experimental
Drivers           ####---------------- keyboard + basic platform only
Networking        -------------------- planned
Persistent FS     -------------------- planned
```

---

## What Works Now

- Limine boot into GUI or terminal mode.
- HAL setup: GDT, IDT, interrupts, syscall entry path, basic context switch code.
- Memory init: PMM, VMM stub, heap allocator.
- Kernel terminal with real VFS-backed filesystem commands.
- `dufetch` system summary command.
- Runtime VFS with root MemFS mounted as `/`.
- MemFS files/directories with open/read/write/close/readdir/create/mkdir/stat/remove/truncate.
- Process-local fd table with reserved stdio fds `0/1/2`.
- Userspace syscall ABI smoke from CPL3 back to kernel.
- Userspace `open/read/write/close` syscalls through VFS.
- Userspace `write(1/2)` stdio smoke to serial.

---

## Quick Links By Status

### Done / Working

- [[Tasks/Completed/Bootloader|Bootloader + Limine]]
- [[Tasks/Completed/HAL|HAL]]
- [[Tasks/Completed/Keyboard-Driver|Keyboard Driver]]
- [[Tasks/Completed/Terminal-Mode|Kernel Terminal Mode]]
- [[Tasks/Completed/VFS-MemFS|VFS + MemFS Runtime Layer]]
- [[Tasks/Completed/Syscall-ABI|Syscall ABI + Safe User Copy]]
- [[Tasks/Completed/Process-FD-Model|Current Process + FD Table]]
- [[Tasks/Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[Tasks/Completed/Stdio-FD|Minimal Stdio FDs]]
- [[Tasks/Completed/Dufetch|dufetch]]
- [[Tasks/Completed/Userspace-Programs|Userspace Program Builds]]

### In Progress

- [[Tasks/InProgress/Terminal-Improvements|Terminal Improvements]]
- [[Tasks/InProgress/Drivers|Drivers]]
- [[Tasks/InProgress/GUI-Improvements|GUI Improvements]]

### Planned

- [[Tasks/Future/Filesystem|Persistent dunitFS / block-backed FS]]
- [[Tasks/Future/Network-Stack|Network Stack]]
- [[Tasks/Future/Package-Manager|Package Manager]]
- [[Tasks/Future/Advanced-Features|Advanced Features]]
- [[Tasks/Future/GUI-Architecture|GUI Architecture]]

---

## Stack

- **Bootloader:** Limine
- **HAL:** C + NASM
- **Kernel:** Rust `no_std`
- **Userspace:** Rust, custom `x86_64-unknown-none` target
- **Build/Test:** Multipass Linux VM + QEMU via `build_and_run_multipass.py`
