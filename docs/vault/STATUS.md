# STATUS

> Проверенный snapshot Dunit OS / Green Tea Kernel. Последнее обновление: 2026-09-21.

## Краткое резюме

Dunit OS уже является ранним вертикальным прототипом самостоятельной ОС, а не только boot demo. Она загружается через BIOS/UEFI, имеет ring 3 ELF userspace, процессы, потоки и системные вызовы, VFS/MemFS, block devices, DunitFS v1, Terminal Mode и функциональный legacy GUI Mode. UP round-robin включён по умолчанию; FXSAVE/FXRSTOR сохраняет x87/SSE, `FS.base` сохраняет static TLS каждого TID, PIT предоставляет monotonic clock/deadline, timer/IPC wait queues блокируют потоки. Основные ограничения — отсутствие futex-like синхронизации и полной VM, in-kernel desktop architecture, embedded root/userspace, слабая crash consistency DunitFS и отсутствие packet networking.

## Статус подсистем

| Подсистема | Статус | Фактическая граница |
|---|---|---|
| Limine / BIOS / UEFI | WORKING | ISO и disk boot проверены; оба firmware paths работают |
| HAL / interrupts | WORKING FOUNDATION | GDT, IDT, syscall entry, PIT/PIC, keyboard/mouse; APIC/SMP later |
| PMM / VMM / heap | PARTIAL | `munmap/mprotect`, shared VM frames и guard pages проверены `[VM-TEST] OK`; нужны file mappings, demand paging, COW и rights |
| Scheduler | PARTIAL | UP round-robin default-on; TID, XMM, static TLS и timer/IPC waits проверены gated smoke; нет AVX/SMP/futex-like wait |
| Processes | PARTIAL | PID/parent-child, общий address space/fd table для потоков, thread create/exit/nonblocking join; нет production exec/fork model |
| ELF userspace | WORKING FOUNDATION | embedded ELF applications запускаются из `/app`; installed disk loading не завершён |
| Syscalls / user copy | WORKING FOUNDATION | register ABI и mapped-range checks работают; ABI ещё не стабилизирован/versioned |
| IPC | PROTOTYPE | bounded byte queues и parent/child round trip; нужны handles/events/shared VM/rights |
| VFS / MemFS | WORKING | runtime root, paths и file APIs работают |
| DevFS / ProcFS | PARTIAL | диагностика и базовые nodes, не полный object/filesystem contract |
| AHCI | WORKING IN QEMU | SATA disk discovery/I/O path подтверждён |
| VirtIO block | PARTIAL | legacy transport работает; modern VirtIO later |
| DunitFS v1 | PROTOTYPE | `/persist`, fixed 64 nodes, CRC metadata; без directories/journal/fsck/permissions |
| Installer / GPT | PARTIAL | BIOS/UEFI disk image и boot path работают; root всё ещё embedded MemFS |
| Terminal Mode | WORKING | kernel recovery shell, VFS commands, userspace exec |
| GUI Mode | FUNCTIONAL LEGACY | desktop/terminal/apps работают; compositor/DWM/layout находятся в kernel-centric implementation |
| GUI Server / DWM | INTEGRATED, PARTIAL | userspace `gui_server` компоузит untrusted-клиентов по gui-v1, автозапускается как десктоп (`limine_dwm.conf`); есть draggable окна + панель/таскбар + часы; осталось parity (launcher-спавн приложений, dock, quick settings, notifications, workspaces, switcher, TOML settings) |
| Input | PARTIAL | PS/2 path работает; xHCI foundation без полной enumeration/HID transfers |
| Networking | DISCOVERY ONLY | E1000 MMIO/MAC probe; нет RX/TX, Ethernet/IP/TCP/socket API |
| libc | PLANNED | primary target — full Dunit musl fork, static-first |
| Audio / ACPI power | NOT IMPLEMENTED | отдельные later services |

## Подтверждённые runtime-сценарии

- Terminal ISO boot до `root@dunit:~#`.
- GUI ISO boot до `[GUI] two-pass blur cache ready`.
- `runtime_stress`, IPC и file API programs завершаются с code `0`.
- `preempt_test` доказывает timer preemption CPU-bound parent/child без `yield`; marker проверяется только в boot-smoke profile.
- BIOS disk boot видит AHCI disk и автоматически монтирует DunitFS.
- UEFI disk boot через OVMF доходит до того же storage/runtime path.
- Файл в `/persist` существует после остановки и повторной загрузки того же disk image.

## Что нельзя считать готовым

- `/persist` не равен persistent root.
- Disk boot не означает, что system/apps загружаются с DunitFS.
- Наличие NIC в PCI inventory не означает networking.
- Наличие xHCI command ring не означает USB enumeration/HID.
- Текущий GUI не является целевой userspace GUI architecture.
- Наличие scheduler code не означает preemptive multitasking.
- Наличие исходного `display_server` не означает интегрированный GUI Server.

## Ближайший технический порядок

1. [[Tasks/InProgress/Kernel-Runtime-Prerequisites|Preemption, threads, blocking events, shared VM and rights-bearing handles]].
2. GUI protocol/headless tests параллельно kernel runtime.
3. [DONE/В РАБОТЕ] Userspace GUI Server vertical slice готов (компоузит untrusted-клиентов, автозапуск как десктоп, интерактивные окна + панель); идёт достижение DWM parity и config-driven Dunit DWM.
4. [[Tasks/Future/Installed-System|Disk-root init, transactional install and first boot]].
5. [[Tasks/Future/Filesystem|Recoverable DunitFS v2 and persistence matrix]].
6. [[Tasks/Future/Libc-Musl|Static-first Dunit musl fork]].
7. [[Tasks/Future/Network-Stack|E1000 -> netd -> native/musl sockets -> TLS/HTTP]].

## Источник подробностей

Полный технический анализ, зависимости и acceptance criteria: [DUNIT_OS_TECHNICAL_ROADMAP.md](../../DUNIT_OS_TECHNICAL_ROADMAP.md).
