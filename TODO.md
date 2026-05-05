# Dunit OS Development Plan

## Completed ✓
- [x] Microkernel OS with HAL (C/Assembly) and Kernel (Rust)
- [x] Limine bootloader with GUI and Terminal modes
- [x] Framebuffer console for Terminal Mode
- [x] Interrupt-based keyboard driver
- [x] Full command set in Terminal Mode
- [x] Userspace programs compilation (plank, terminal, file_manager, text_editor, settings, system_monitor)

## In Progress
### Task 4: Improve Terminal Mode
- [x] Add command history (up/down arrows)
- [x] Add tab autocomplete
- [x] Add `exec` command to run userspace programs
- [ ] Add command aliases
- [ ] Add environment variables
- [ ] Add pipe support (|)
- [ ] Add redirection (>, >>)

### Task 5: Drivers
- [ ] Sound driver (AC97 or Intel HDA)
- [ ] USB driver (UHCI/OHCI/EHCI/xHCI)
- [ ] Disk driver (ATA/AHCI)
- [ ] Network driver (RTL8139 or E1000)
- [ ] PCI enumeration
- [ ] ACPI support

### Task 6: GUI Improvements
- [ ] Window animations (fade in/out, minimize/maximize)
- [ ] Multiple themes support
- [ ] Settings app for theme switching
- [ ] Drag and drop support
- [ ] Context menus (right-click)
- [ ] Notifications system
- [ ] System tray

## Future Tasks
### Task 7: Network Stack
- [ ] Ethernet layer
- [ ] IP layer
- [ ] TCP/UDP protocols
- [ ] Socket API
- [ ] DNS resolver
- [ ] HTTP client

### Task 8: Package Manager
- [ ] dpkg implementation
- [ ] Package repository
- [ ] Dependency resolution
- [ ] Package installation/removal
- [ ] Update system

### Task 9: Filesystem
- [ ] ext2/ext3 support
- [ ] FAT32 support
- [ ] File permissions
- [ ] Symbolic links
- [ ] Mount/unmount

### Task 10: Advanced Features
- [ ] Multi-core support (SMP)
- [ ] Power management (ACPI)
- [ ] Swap support
- [ ] Kernel modules
- [ ] Debugging tools (gdb stub)

## Testing Plan
1. **GUI Mode**: Test programs visually (click icons in Plank)
2. **Terminal Mode**: Run processes with commands (e.g., `exec /boot/userspace/terminal`)
3. **Task Manager**: Kill processes with `kill` and verify with `ps`

## Build Instructions
```bash
make clean
make all
make run          # GUI mode
make run-terminal # Terminal mode
```

## Userspace Programs Location
All compiled ELF binaries are in `build/userspace/`:
- plank
- terminal
- file_manager
- text_editor
- settings
- system_monitor
