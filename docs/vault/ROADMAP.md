# ROADMAP

> The vault is the project task graph. Completed nodes are under `Tasks/Completed`, active work under `Tasks/InProgress`, and future architecture under `Tasks/Future`.

---

## Done / Working

- [x] [[Tasks/Completed/Bootloader|Limine bootloader with GUI and terminal boot modes]]
- [x] [[Tasks/Completed/HAL|HAL foundation: GDT, IDT, interrupts, syscall entry]]
- [x] [[Tasks/Completed/Keyboard-Driver|Interrupt-based keyboard driver]]
- [x] [[Tasks/Completed/Terminal-Mode|Kernel terminal mode with framebuffer console]]
- [x] [[Tasks/Completed/VFS-MemFS|Runtime VFS + root MemFS]]
- [x] [[Tasks/Completed/Syscall-ABI|Syscall ABI hardening + bounded user copy]]
- [x] [[Tasks/Completed/Process-FD-Model|Minimal current process, PID, cwd, fd table]]
- [x] [[Tasks/Completed/Userspace-VFS-Syscalls|Userspace Open/Read/Write/Close through VFS]]
- [x] [[Tasks/Completed/Stdio-FD|Minimal stdin/stdout/stderr fd reservation]]
- [x] [[Tasks/Completed/Dufetch|dufetch terminal system summary]]
- [x] [[Tasks/Completed/Userspace-Programs|Userspace program build pipeline]]

---

## In Progress

### Userspace Runtime v1

→ [[Tasks/InProgress/Userspace-Runtime-v1|Userspace Runtime v1]]

- [x] Userspace ELF binaries embedded under `/app`
- [x] Foreground `exec` with argv/envp and exit/fault reporting
- [x] `spawn` prepares Ready child processes
- [x] Cooperative `yield` can run Ready children and resume parents
- [x] `wait` observes real child exit/fault status after execution
- [x] Basic parent/child IPC round trip
- [x] `runtime_stress` regression app
- [ ] Canonical automated runtime regression through `build_and_run_multipass.py`
- [ ] Documentation fully aligned with current runtime behavior
- [ ] Host-side kernel test workflow fixed or documented

### Terminal Improvements

→ [[Tasks/InProgress/Terminal-Improvements|Terminal Improvements]]

- [x] Command history
- [x] Tab autocomplete
- [x] Real VFS-backed `ls/pwd/cd/mkdir/touch/cat/echo/rm/tree`
- [x] `dufetch`
- [x] Basic `echo > file` and `echo >> file`
- [ ] Aliases
- [ ] Environment variables
- [ ] Pipes
- [ ] Full stdin input model for userspace terminal

### Drivers

→ [[Tasks/InProgress/Drivers|Drivers]]

- [x] PS/2 keyboard path for terminal mode
- [ ] PCI enumeration
- [ ] Disk driver: ATA/AHCI
- [ ] Network driver: RTL8139/E1000
- [ ] USB driver
- [ ] Sound driver
- [ ] ACPI support

### GUI Improvements

→ [[Tasks/InProgress/GUI-Improvements|GUI Improvements]]

- [ ] Window animations
- [ ] Multiple themes
- [ ] Drag and drop
- [ ] Context menus
- [ ] Notifications
- [ ] System tray

---

## Planned

### Persistent Filesystem

→ [[Tasks/Future/Filesystem|Persistent dunitFS / block-backed FS]]

- [ ] Block device abstraction
- [ ] Persistent dunitFS design
- [ ] Mount table beyond root MemFS
- [ ] File permissions
- [ ] Symlinks
- [ ] ext2/FAT32 compatibility research

### Userspace Execution

- [x] Real ELF exec path for embedded `/app` applications
- [x] Per-process address-space objects for userspace records
- [x] Per-process kernel stacks for syscall entry
- [x] Cooperative scheduler integration through `yield`
- [ ] Blocking wait/input semantics
- [ ] Timer preemption hardening
- [ ] Long-running background process model

### Network Stack

→ [[Tasks/Future/Network-Stack|Network Stack]]

- [ ] Ethernet layer
- [ ] IP layer
- [ ] TCP/UDP
- [ ] Socket API
- [ ] DNS resolver
- [ ] HTTP client

### Package Manager

→ [[Tasks/Future/Package-Manager|Package Manager]]

- [ ] Package metadata format
- [ ] Repository format
- [ ] Dependency resolution

### Advanced Features

→ [[Tasks/Future/Advanced-Features|Advanced Features]]

- [ ] SMP
- [ ] Power management
- [ ] Swap
- [ ] Kernel modules
- [ ] GDB stub

---

## Build / Test

Preferred autonomous workflow on Windows:

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --qemu-timeout 60 \
  --qemu-log qemu_runtime_stress.log \
  --qemu-test-commands "exec runtime_stress" \
  --expect-log "runtime_stress: OK" \
  --expect-log "exec: /app/runtime_stress returned code=0"
```

`build_and_run_multipass.py` is the only supported autonomous launch and test
entrypoint. It builds the ISO, starts QEMU, injects terminal commands, stops QEMU
on timeout, and analyzes the serial log.
