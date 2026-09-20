# Dunit OS / Green Tea Kernel

> Независимая x86_64 operating system: собственное ядро, userspace ABI, DunitFS, Terminal Mode и развиваемый desktop stack. Это навигационная страница актуального vault.

## Навигация

| Раздел | Ссылка |
|---|---|
| Проверенное текущее состояние | [[STATUS|STATUS]] |
| Активный план и зависимости | [[ROADMAP|ROADMAP]] |
| Полный технический roadmap | [DUNIT_OS_TECHNICAL_ROADMAP.md](../../DUNIT_OS_TECHNICAL_ROADMAP.md) |
| Контекст для AI/разработчика | [[AI-Context/CONTEXT|AI CONTEXT]] |
| Исходная идея проекта | [[Origin/VISION|VISION]] |
| Архивный design | [[Origin/DESIGN|DESIGN]] |
| Исходные требования | [[Origin/REQUIREMENTS|REQUIREMENTS]] |

## Состояние на 2026-09-20

```text
Boot BIOS/UEFI       ##################-- работает
Terminal Mode        ##################-- работает
Ring 3 / ELF         ################---- работает, требует ABI stabilization
Processes / IPC      ##############------ cooperative foundation
VFS / MemFS          ################---- работает
DunitFS / persistence##########---------- v1 работает, normal installed root ещё нет
AHCI / VirtIO block  ##############------ QEMU path работает
GUI Mode             ##############------ функционален, но архитектура legacy/in-kernel
GUI Server / DWM      ##------------------ спроектированы, userspace rewrite впереди
Networking           ##------------------ PCI/E1000 discovery only
musl libc             -------------------- подробный static-first план
USB / audio / power   ###----------------- ранняя основа / planned
```

## Что реально работает

- Limine загружает Terminal Mode и GUI Mode через BIOS и UEFI.
- Green Tea Kernel получает memory map/framebuffer, поднимает PMM/VMM/heap, GDT/IDT, interrupts и syscall entry.
- Ring 3 ELF-программы запускаются с `argc/argv/envp`, завершаются и изолированно fault-ятся.
- Есть PID/parent-child lifecycle, cwd/fd tables, cooperative spawn/yield/wait и byte-queue IPC.
- Safe user-copy проверяет отображённые userspace ranges; runtime stress проверяет invalid pointers и recoverable faults.
- VFS/MemFS, `/app`, `/proc`, `/dev`, filesystem/sysinfo/process/input IPC syscalls и `libdunit` работают.
- Terminal Mode имеет VFS-команды и запускает userspace applications.
- GUI Mode запускает полноценный GUI-терминал и отдельные userspace GUI clients, но compositor/DWM/layout всё ещё тесно связаны с kernel GUI loop.
- AHCI и legacy VirtIO block paths существуют; DunitFS v1 автоматически монтируется как `/persist` на подходящем диске.
- Disk image загружается через BIOS/UEFI; persistence пустого файла подтверждена после полного reboot.
- E1000 discovery читает MMIO status/MAC, но packet RX/TX и protocol stack отсутствуют.
- Единственная разрешённая точка автоматического запуска и тестирования: `tools/qemu_test.py`.

## Ключевая оговорка

Текущая disk boot не является законченной установленной системой: root остаётся MemFS, userspace binaries/assets встроены в kernel/image build, а `/persist` — отдельный ограниченный DunitFS v1. Normal installed system требует disk-root `init`, системных каталогов, recoverable DunitFS v2 и transactional installer.

## Узлы vault

### Работающие основания

- [[Tasks/Completed/Bootloader|Bootloader + Limine]]
- [[Tasks/Completed/HAL|HAL foundation]]
- [[Tasks/Completed/Terminal-Mode|Terminal Mode]]
- [[Tasks/Completed/VFS-MemFS|VFS + MemFS]]
- [[Tasks/Completed/Syscall-ABI|Syscall ABI]]
- [[Tasks/Completed/Process-FD-Model|Process + FD foundation]]
- [[Tasks/Completed/Userspace-VFS-Syscalls|Userspace VFS syscalls]]
- [[Tasks/Completed/Userspace-Programs|Userspace programs]]
- [[Tasks/Completed/Block-Storage-v1|Block storage v1]]

### Активные архитектурные направления

- [[Tasks/InProgress/Userspace-Runtime-v1|Userspace runtime]]
- [[Tasks/InProgress/Drivers|Drivers]]
- [[Tasks/InProgress/GUI-Improvements|Legacy GUI maintenance / migration]]
- [[Tasks/InProgress/Terminal-Improvements|Terminal to userspace shell]]
- [[Tasks/InProgress/Kernel-Runtime-Prerequisites|Preemption, threads, VM, handles]]

### Следующие большие системы

- [[Tasks/Future/GUI-Architecture|GUI Server + UI Runtime + Dunit DWM]]
- [[Tasks/Future/Installed-System|Normal installed system]]
- [[Tasks/Future/Filesystem|DunitFS v2 + persistence]]
- [[Tasks/Future/Libc-Musl|Dunit musl fork]]
- [[Tasks/Future/Network-Stack|netd + protocols + musl/browser integration]]
- [[Tasks/Future/Package-Manager|Application/package model]]
- [[Tasks/Future/Advanced-Features|Later platform features]]

## Технологический стек

- **Boot:** Limine, BIOS + UEFI
- **HAL:** C + NASM
- **Kernel:** Rust `no_std`
- **Userspace:** Rust custom target; future C via Dunit musl fork
- **Storage:** VFS/MemFS, AHCI, legacy VirtIO block, DunitFS v1
- **GUI target:** userspace GUI Server, software compositor, DUI/DSS/TOML UI stack
- **Network target:** kernel NIC backend + userspace `netd`
- **Build/test:** Make + QEMU, только `tools/qemu_test.py`
