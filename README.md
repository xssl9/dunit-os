# 🚀 Dunit OS

<div align="center">

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Language](https://img.shields.io/badge/language-Rust%20%7C%20C%20%7C%20Assembly-orange.svg)
![Platform](https://img.shields.io/badge/platform-x86__64-lightgrey.svg)

**A modern microkernel operating system written in Rust**

[English](#english) | [Русский](#russian)

</div>

---

<a name="english"></a>
## 🇬🇧 English

### 📖 About

**Dunit OS** is a microkernel operating system built from scratch with a focus on modularity and safety. The kernel is written in Rust, while the Hardware Abstraction Layer (HAL) uses C and Assembly for low-level operations.

### ✨ Features

- 🔧 **Microkernel Architecture** - Clean separation between HAL (C/Assembly) and Kernel (Rust)
- 🎨 **Dual Boot Modes** - GUI mode with window manager and Terminal mode with framebuffer console
- 💻 **Userspace Programs** - 6 system applications (Plank panel, Terminal, File Manager, Text Editor, Settings, System Monitor)
- 🔄 **Process Management** - Scheduler, IPC (Inter-Process Communication), system calls
- 💾 **Memory Management** - Physical Memory Manager (PMM), Virtual Memory Manager (VMM)
- 📁 **Virtual File System** - VFS with DevFS, MemFS, ProcFS support
- ⌨️ **Hardware Drivers** - Interrupt-based keyboard and mouse drivers
- 🔍 **ELF Loader** - Load and execute userspace ELF binaries
- 📜 **Terminal Features** - Command history (↑/↓), tab autocomplete, `exec` command

### 🏗️ Architecture

```
dunit-os/
├── hal/              # Hardware Abstraction Layer (C/Assembly)
│   └── src/          # GDT, IDT, interrupts, context switching, syscalls
├── kernel/           # Microkernel (Rust)
│   └── src/          # Memory, processes, IPC, VFS, drivers, ELF loader
├── userspace/        # User applications (Rust)
│   ├── system_apps/  # plank, terminal, file_manager, text_editor, settings, system_monitor
│   └── libdunit/     # Userspace library
├── limine/           # Limine bootloader
└── build/            # Build artifacts
```

### 🛠️ Building

**Prerequisites:**
- Rust nightly toolchain
- GCC cross-compiler for x86_64
- NASM assembler
- QEMU (for testing)
- xorriso (for ISO creation)

**Build commands:**
```bash
# Clean build
make clean

# Build kernel and userspace
make all

# Run in GUI mode
make run

# Run in Terminal mode
make run-terminal
```

### 🎯 Roadmap

#### ✅ Completed
- [x] Microkernel with HAL
- [x] Limine bootloader integration
- [x] Framebuffer console
- [x] Keyboard/mouse drivers
- [x] Userspace program compilation
- [x] Command history and autocomplete

#### 🚧 In Progress
- [ ] Command aliases and environment variables
- [ ] Pipe and redirection support
- [ ] Sound, USB, disk, network drivers
- [ ] Window animations and themes
- [ ] Network stack (TCP/IP)
- [ ] Package manager (dpkg)
- [ ] ext2/FAT32 filesystem support
- [ ] Multi-core support (SMP)

### 📝 License

This project is licensed under the MIT License.

### 🤝 Contributing

Contributions are welcome! Feel free to open issues or submit pull requests.

---

<a name="russian"></a>
## 🇷🇺 Русский

### 📖 О проекте

**Dunit OS** — это микроядерная операционная система, написанная с нуля с акцентом на модульность и безопасность. Ядро написано на Rust, а уровень аппаратной абстракции (HAL) использует C и Assembly для низкоуровневых операций.

### ✨ Возможности

- 🔧 **Микроядерная архитектура** - Чёткое разделение между HAL (C/Assembly) и ядром (Rust)
- 🎨 **Два режима загрузки** - GUI режим с оконным менеджером и Terminal режим с framebuffer консолью
- 💻 **Пользовательские программы** - 6 системных приложений (панель Plank, терминал, файловый менеджер, текстовый редактор, настройки, системный монитор)
- 🔄 **Управление процессами** - Планировщик, IPC (межпроцессное взаимодействие), системные вызовы
- 💾 **Управление памятью** - Менеджер физической памяти (PMM), менеджер виртуальной памяти (VMM)
- 📁 **Виртуальная файловая система** - VFS с поддержкой DevFS, MemFS, ProcFS
- ⌨️ **Драйверы устройств** - Драйверы клавиатуры и мыши на прерываниях
- 🔍 **Загрузчик ELF** - Загрузка и выполнение пользовательских ELF-бинарников
- 📜 **Функции терминала** - История команд (↑/↓), автодополнение по Tab, команда `exec`

### 🏗️ Архитектура

```
dunit-os/
├── hal/              # Уровень аппаратной абстракции (C/Assembly)
│   └── src/          # GDT, IDT, прерывания, переключение контекста, системные вызовы
├── kernel/           # Микроядро (Rust)
│   └── src/          # Память, процессы, IPC, VFS, драйверы, загрузчик ELF
├── userspace/        # Пользовательские приложения (Rust)
│   ├── system_apps/  # plank, terminal, file_manager, text_editor, settings, system_monitor
│   └── libdunit/     # Библиотека для userspace
├── limine/           # Загрузчик Limine
└── build/            # Артефакты сборки
```

### 🛠️ Сборка

**Требования:**
- Rust nightly toolchain
- Кросс-компилятор GCC для x86_64
- Ассемблер NASM
- QEMU (для тестирования)
- xorriso (для создания ISO)

**Команды сборки:**
```bash
# Очистка
make clean

# Сборка ядра и userspace
make all

# Запуск в GUI режиме
make run

# Запуск в Terminal режиме
make run-terminal
```

### 🎯 Дорожная карта

#### ✅ Завершено
- [x] Микроядро с HAL
- [x] Интеграция загрузчика Limine
- [x] Framebuffer консоль
- [x] Драйверы клавиатуры/мыши
- [x] Компиляция пользовательских программ
- [x] История команд и автодополнение

#### 🚧 В разработке
- [ ] Алиасы команд и переменные окружения
- [ ] Поддержка pipe и редиректов
- [ ] Драйверы звука, USB, дисков, сети
- [ ] Анимации окон и темы
- [ ] Сетевой стек (TCP/IP)
- [ ] Менеджер пакетов (dpkg)
- [ ] Поддержка файловых систем ext2/FAT32
- [ ] Поддержка многоядерности (SMP)

### 📝 Лицензия

Проект распространяется под лицензией MIT.

### 🤝 Участие в разработке

Приветствуются любые вклады! Не стесняйтесь открывать issues или отправлять pull requests.

---

<div align="center">

**Made with ❤️ and Rust**

</div>
