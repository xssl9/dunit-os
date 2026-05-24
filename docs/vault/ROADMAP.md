# 🚦 ROADMAP

> Текущий план проекта. Вырос из [[Origin/VISION|VISION]] (.kiro).
> Каждый таск — отдельная нода в графе.

---

## ✅ Выполнено

- [x] [[Tasks/Completed/HAL|Microkernel OS с HAL (C/Assembly) и Kernel (Rust)]]
- [x] [[Tasks/Completed/Bootloader|Limine bootloader с GUI и Terminal режимами]]
- [x] [[Tasks/Completed/Terminal-Mode|Framebuffer console для Terminal Mode]]
- [x] [[Tasks/Completed/Keyboard-Driver|Interrupt-based keyboard driver]]
- [x] [[Tasks/Completed/Terminal-Mode|Полный набор команд в Terminal Mode]]
- [x] [[Tasks/Completed/Userspace-Programs|Компиляция userspace программ (plank, terminal, file_manager, text_editor, settings, system_monitor)]]

---

## 🔧 В процессе

### Task 4: Terminal Improvements
→ [[Tasks/InProgress/Terminal-Improvements|Terminal Improvements]]

- [x] История команд (↑↓)
- [x] Tab autocomplete
- [x] Команда `exec` для запуска userspace
- [ ] Алиасы команд
- [ ] Переменные окружения
- [ ] Pipe поддержка (`|`)
- [ ] Редиректы (`>`, `>>`)

### Task 5: Drivers
→ [[Tasks/InProgress/Drivers|Drivers]]

- [ ] Sound driver (AC97 / Intel HDA)
- [ ] USB driver (UHCI/OHCI/EHCI/xHCI)
- [ ] Disk driver (ATA/AHCI)
- [ ] Network driver (RTL8139 / E1000)
- [ ] PCI enumeration
- [ ] ACPI support

### Task 6: GUI Improvements
→ [[Tasks/InProgress/GUI-Improvements|GUI Improvements]]

- [ ] Window animations (fade in/out, minimize/maximize)
- [ ] Multiple themes
- [ ] Settings app для смены темы
- [ ] Drag and drop
- [ ] Context menus (right-click)
- [ ] Notifications system
- [ ] System tray

---

## 🔮 Будущее

### Task 7: Network Stack
→ [[Tasks/Future/Network-Stack|Network Stack]]

- [ ] Ethernet layer
- [ ] IP layer
- [ ] TCP/UDP
- [ ] Socket API
- [ ] DNS resolver
- [ ] HTTP client

### Task 8: Package Manager
→ [[Tasks/Future/Package-Manager|Package Manager]]

- [ ] dpkg implementation
- [ ] Package repository
- [ ] Dependency resolution

### Task 9: Filesystem
→ [[Tasks/Future/Filesystem|Filesystem]]

- [ ] ext2/ext3
- [ ] FAT32
- [ ] File permissions
- [ ] Symbolic links
- [ ] Mount/unmount

### Task 10: Advanced Features
→ [[Tasks/Future/Advanced-Features|Advanced Features]]

- [ ] Multi-core (SMP)
- [ ] Power management (ACPI)
- [ ] Swap support
- [ ] Kernel modules
- [ ] GDB stub

### GUI Architecture (Display Server)
→ [[Tasks/Future/GUI-Architecture|GUI Architecture]]

- [ ] Display Server как отдельный процесс
- [ ] Orbital-подобный WM (референс: Redox)
- [ ] IPC между приложениями и Display Server
- [ ] egui интеграция

---

## Build

```bash
make clean
make all
make run          # GUI mode
make run-terminal # Terminal mode
```

## Userspace бинари

`build/userspace/`: plank, terminal, file_manager, text_editor, settings, system_monitor
