<p align="center">
  <img src="logo.bmp" width="150"/>
</p>

<h1 align="center">Dunit OS — Green Tea</h1>

<p align="center">
  <strong>x86_64 microkernel OS written in Rust with tiling window manager and process isolation</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust%20%7C%20C%20%7C%20Assembly-orange?style=flat-square" alt="Language"/>
  <img src="https://img.shields.io/badge/arch-x86__64-blue?style=flat-square" alt="Architecture"/>
  <img src="https://img.shields.io/badge/status-In%20Development-yellow?style=flat-square" alt="Status"/>
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License"/>
</p>

---

## What Works

- ✅ **HAL Layer** — GDT, IDT, interrupts, context switching, syscall entry (C/NASM)
- ✅ **Limine Bootloader** — GUI and Terminal boot modes
- ✅ **Framebuffer Console** — Direct pixel rendering with custom font
- ✅ **Interrupt-based Drivers** — Keyboard and mouse with IRQ handling
- ✅ **Terminal Mode** — Command history (↑↓), tab autocomplete, `exec` command
- ✅ **Userspace Programs** — 6 system apps compile and run: plank, terminal, file_manager, text_editor, settings, system_monitor
- ✅ **Process Management** — Scheduler, IPC, system calls
- ✅ **Memory Management** — PMM, VMM with paging
- ✅ **Virtual File System** — VFS with DevFS, MemFS, ProcFS
- ✅ **ELF Loader** — Load and execute userspace binaries

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Userspace (Rust)                     │
│  plank | terminal | file_manager | text_editor | ...    │
├─────────────────────────────────────────────────────────┤
│                  Microkernel (Rust)                     │
│  Scheduler | VMM/PMM | VFS | IPC | Drivers | ELF       │
├─────────────────────────────────────────────────────────┤
│              HAL (C + Assembly)                         │
│  GDT | IDT | Interrupts | Context Switch | Syscalls     │
└─────────────────────────────────────────────────────────┘
```

**Three-layer design:**

1. **HAL** — Hardware Abstraction Layer in C and NASM
   - Low-level CPU setup (GDT, IDT)
   - Interrupt handling and PIC remapping (IRQ0-15 → INT 32-47)
   - Context switching and syscall entry points
   - Port I/O primitives (inb/outb/inw/outw/inl/outl)

2. **Kernel** — Microkernel in Rust (`no_std`, `x86_64-unknown-none`)
   - Physical and virtual memory managers
   - Process scheduler with round-robin
   - Virtual file system with multiple backends
   - Device drivers (keyboard, mouse)
   - IPC via shared memory and message passing
   - ELF binary loader

3. **Userspace** — Applications in Rust (custom JSON target)
   - System applications (panel, terminal, file manager, etc.)
   - libdunit — userspace library for syscalls and IPC

---

## Philosophy

**Microkernel** — Minimal kernel, drivers and services in userspace.

**Tiling Window Manager** — No overlapping windows. Each window occupies its own tile. Clean, predictable layout.

**Process Isolation** — Every process runs independently with its own address space. IPC via explicit message passing.

**DunitFS Structure:**
```
/kernel    — Kernel modules and core binaries
/proc      — Process information (ProcFS)
/app       — User applications
/cfg       — System configuration files
/usr       — User data and programs
/tmp       — Temporary files
```

---

## Build & Run

**Prerequisites:**
- Rust nightly toolchain
- GCC cross-compiler for x86_64
- NASM assembler
- QEMU
- xorriso

**Build:**
```bash
make clean
make all
```

**Run in GUI mode:**
```bash
make run
```

**Run in Terminal mode:**
```bash
make run-terminal
```

**Create ISO:**
```bash
make iso
# Output: build/microkernel.iso
```

---

## Roadmap

### ✅ Completed

- [x] Microkernel with HAL (C/Assembly) and Kernel (Rust)
- [x] Limine bootloader with GUI and Terminal modes
- [x] Framebuffer console for Terminal mode
- [x] Interrupt-based keyboard driver
- [x] Full command set in Terminal mode
- [x] Userspace program compilation and execution

### 🚧 In Progress

- [ ] Terminal improvements: aliases, environment variables, pipes, redirects
- [ ] Additional drivers: PCI enumeration, disk (ATA/AHCI), network (RTL8139/E1000), sound (AC97/HDA), USB
- [ ] GUI improvements: window animations, themes, drag-and-drop, context menus, system tray

### 🔮 Planned

- [ ] **Network Stack** — TCP/IP with smoltcp, socket API, DNS resolver
- [ ] **Filesystem** — ext2/FAT32 support, file permissions, symbolic links
- [ ] **Package Manager** — dpkg implementation with dependency resolution
- [ ] **Display Server** — Orbital-style compositor (Redox OS reference)
- [ ] **Advanced Features** — SMP (multi-core), ACPI power management, swap, kernel modules, GDB stub

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  <strong>Built with Rust 🦀 and low-level magic ✨</strong>
</p>
