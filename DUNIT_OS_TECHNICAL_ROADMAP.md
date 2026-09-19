# Dunit OS / Green Tea Kernel — техническое ревью и roadmap

> Срез репозитория: commit `5093e64`, 20 сентября 2026 года. Документ основан на исходном коде и автоматических QEMU-прогонах, а не на названиях файлов или старых заметках в `docs/`.

## 1. Название и цель проекта

**Dunit OS** — самостоятельная x86_64 ОС с ядром **Green Tea Kernel**, собственным ABI, userspace, DunitFS и будущим desktop-стеком. POSIX и musl рассматриваются как слой переносимости исходного кода, а не как цель Linux-совместимости.

Целевая система:

```text
Limine (BIOS/UEFI)
  -> Green Tea Kernel
       memory / VM / scheduler / processes / IPC / VFS / drivers / security
       framebuffer + raw input primitives (не desktop widgets)
  -> init / service manager
       -> GUI Server + compositor
       -> Dunit DWM
       -> terminal, shell and applications
       -> Dunit UI Runtime
  -> DunitFS system and user volumes
  -> Dunit ABI + libdunit + Dunit musl fork
```

## 2. Краткое резюме текущего состояния

Dunit уже больше, чем boot-screen: она загружается через Limine, имеет ring-3 ELF-процессы с отдельными address spaces, syscall ABI, VFS/MemFS, файловые дескрипторы, cooperative `spawn/yield/wait`, IPC-очереди, обработку userspace faults, framebuffer-терминал, работающий GUI-прототип, AHCI/VirtIO block I/O, GPT, DunitFS и BIOS/UEFI disk image.

Однако это ещё не production desktop OS:

- планирование кооперативное; timer preemption выключен по умолчанию;
- нет threads/TLS/signals/futex-подобного ожидания и полноценного process `exec`/`fork`;
- GUI shell, compositor, WM, layout, decorations и системные панели живут в ядре (`kernel/src/ui_loop.rs`, 4458 строк);
- userspace `display_server` и `video_driver` — неинтегрированные прототипы, отсутствующие в `USERSPACE_APPS` Makefile;
- установленная система по-прежнему использует MemFS как `/`; DunitFS автоматически монтируется только в `/persist`;
- приложения и assets встраиваются в kernel image через `include_bytes!`, а скопированные installer-ом `/boot/userspace/*` не загружаются;
- DunitFS v1 ограничена 64 nodes, contiguous extents и не имеет journal/fsck/permissions/atomic rename;
- сеть — discovery E1000 без packet I/O; audio и ACPI power отсутствуют; USB не дошёл до enumeration/class-driver path.

Главный вывод: **перед выносом GUI нужны preemptive scheduling, блокирующий IPC, shared-memory handles и устойчивый service/process lifecycle**. GUI-протокол можно проектировать параллельно, но нельзя объявлять userspace GUI готовым до этих kernel prerequisites.

## 3. Методика и результаты проверки

Единственная точка запуска и тестирования: `tools/qemu_test.py`.

Проверено автоматически:

- Terminal ISO: boot smoke + `runtime_stress`, `ipc_parent`, `file_api_test` — успешно;
- GUI ISO 1600×900: дошёл до `[GUI] two-pass blur cache ready`, framebuffer screenshot получен — успешно;
- установленный BIOS disk image: загрузка с HDD, AHCI `sda`, DunitFS auto-mount — успешно;
- установленный UEFI disk image под OVMF: загрузка с ESP, AHCI `sda`, DunitFS auto-mount — успешно;
- persistence: `touch /persist/probe.txt`, полный stop/start QEMU, затем `cat /persist/probe.txt` — файл найден после reboot;
- payload modules закономерно отсутствуют при disk boot: installer payload нужен Live ISO, не установленной системе.

Релевантный tail Terminal Mode:

```text
[STDOUT] runtime_stress: fault OK
[STDOUT] runtime_stress: OK
[PROCESS-RUN] exited pid=4 code=0
[EXEC] /app/runtime_stress returned code=0
[PROCESS-RUN] VFS handles clean
[STDOUT] ipc_parent: OK
[EXEC] /app/ipc_parent returned code=0
[STDOUT] file_api_test: OK
```

Релевантный tail GUI Mode:

```text
[GUI] Starting built-in desktop loop
[GUI] renderer init start
[GUI] back buffer enabled
[GUI] dirty cursor redraw enabled
[GUI] wallpaper loaded from VFS
[GUI] rebuilding two-pass blur cache
[GUI] two-pass blur cache ready
```

Релевантный tail installed BIOS/UEFI boot:

```text
[AHCI] SATA disk port=0 sectors=524288
[AHCI] controllers found=1 initialized=1 disks=1
[DUNITFS] auto-mounted sda at /persist
[ OK ] Dunit OS (Green Tea) ready
[TERM-007] Starting main loop
```

Persistence после reboot:

```text
[DUNITFS] auto-mounted sda at /persist
root@dunit:~# cat /persist/probe.txt

root@dunit:~#
```

Проверен именно жизненный цикл пустого файла. Автотест содержимого, crash-consistency и power-loss recovery ещё отсутствует и обязателен ниже.

## 4. Техническое ревью по подсистемам

### 4.1 Boot, Limine, BIOS, UEFI и режимы

- **Реализовано:** Limine handoff, HHDM, memory map, framebuffer и modules в `hal/src/boot_main.c:166`; выбор Terminal Mode по `mode=terminal` в `hal/src/boot_main.c:205`; меню в `limine.conf`; deterministic configs `limine_test_terminal.conf` и `limine_test_gui.conf`.
- **Реализовано:** host installer создаёт GPT, 64 MiB FAT32 ESP, DunitFS и ставит Limine BIOS stages (`tools/install_disk.py:96`, `:191`, `:227`, `:296`). Один image загрузился и через SeaBIOS, и через OVMF.
- **Частично:** in-system installer пишет ESP payload, GPT и BIOS stages, но принимает только writable AHCI (`kernel/src/storage/installer.rs:78`); VirtIO install отвергается.
- **Проблема:** `initrd::init()` создаёт пустой объект (`kernel/src/initrd.rs`), хотя boot log утверждает, что archive найден и распакован. Это вводящий в заблуждение лог.
- **Проблема:** kernel не читает `/boot/userspace`; binaries в `/app` запекаются в kernel через `kernel/src/fs/vfs.rs:8-38`.

### 4.2 Память и процессы

- **Рабочая база:** multi-region PMM, HHDM, растущая PMM-backed kernel heap; `AddressSpace` с owned user frames, map/unmap и kernel-half sync (`kernel/src/memory/vmm.rs:143`, `:202`, `:227`).
- **Рабочая база:** ELF parser/loader, W^X flags, user stack и `argc/argv/envp` (`kernel/src/elf/mod.rs:209`, `:391`). Userspace faults завершают процесс, не kernel.
- **Частично:** anonymous private `mmap` существует (`kernel/src/syscall/mod.rs:807`, `Process::map_anonymous` в `kernel/src/process/mod.rs:353`), но нет `munmap`, `mprotect`, file mappings, shared mappings, demand paging или COW.
- **Не реализовано:** syscall `fork` и process-image `exec` возвращают `ENOSYS` (`kernel/src/syscall/mod.rs:491`, `:495`). Текущий `spawn` — Dunit-native primitive и пригоден как основной API; `fork` не обязан быть первым.
- **Техдолг:** много `static mut` singleton state и 91 compiler warning в test build; существующие `SpinLock`/`IrqSafeSpinLock` надо распространить на VFS, DunitFS, IPC, terminal, WM и driver registries.

### 4.3 Планировщик, threads, TLS и синхронизация

- **Реализовано:** PID ready queue, saved CPU context, transitions Ready/Running/Blocked/Dead/Reaped, `yield`, parent/child wait/reap.
- **Прототип:** scheduler прямо логирует `cooperative only, timer-preemption=off smp=off` (`kernel/src/process/scheduler.rs:80`). PIT tick и experimental save/schedule hook есть, но `PREEMPTION_ENABLED=false` (`kernel/src/process/mod.rs:41`, `:1278`).
- **Не реализовано:** kernel threads как полноценные schedulable entities, userspace threads, TLS register setup, per-thread kernel stack/FPU state, blocking wait queues, priorities, time accounting, SMP.
- **Следствие:** GUI Server как настоящий long-running userspace service и musl pthreads пока нельзя считать надёжными.

### 4.4 Syscalls и ABI

- **Реализовано:** 29 номеров Dunit ABI (`kernel/src/syscall/mod.rs:9`): file I/O, anonymous mmap, byte IPC, framebuffer/input, spawn/wait/kill/sleep/yield, cwd, stats, readdir/stat.
- **Удачно:** user-copy проверяет присутствие и writable/user flags каждой страницы до доступа.
- **Частично:** `sleep` использует PIT tick wait, но архитектуре нужны blocked sleepers и timer queue; stdio/terminal foreground policy всё ещё kernel-centric.
- **Проблема безопасности:** raw framebuffer syscalls доступны обычным приложениям; будущий GUI требует capability/handle, доступный только GUI Server.
- **ABI-долг:** нет versioned ABI document, handle rights, poll/event wait, dup/pipe, seek, metadata, clocks, threads/TLS, robust errno contract.

### 4.5 IPC

- **Работает:** bounded byte messages до 256 байт, per-PID queues, sender identity; parent/child round trip подтверждён (`kernel/src/ipc/mod.rs:5`, `:218`, `:233`).
- **Прототип:** receive возвращает `EAGAIN`, очередь — `Vec` с `remove(0)`, нет блокирующего wait, backpressure notification, endpoint object, rights transfer и service discovery.
- **Небезопасный prototype:** shared-memory API хранит один `physical_addr`, теряет список выделенных frames и возвращает physical address (`kernel/src/ipc/mod.rs:108`, `:137`); syscall surface для create/attach отсутствует. Это нельзя использовать как GUI buffer protocol.

### 4.6 VFS, MemFS, DevFS/ProcFS и DunitFS

- **Работает:** path normalization, mounts, open/read/write/close/create/mkdir/remove/truncate/readdir/stat, per-process fd table; MemFS — root (`kernel/src/fs/vfs.rs:487`).
- **Частично:** DevFS/ProcFS существуют, но не являются полноценными независимыми mounts/device interfaces.
- **Работает как prototype:** DunitFS v1 superblock + CRC, 64 fixed metadata nodes, files/directories, persistent read/write и auto-mount (`kernel/src/fs/dunitfs.rs:10`, `:13`, `:75`, `:132`).
- **Ограничения DunitFS:** нет free-space bitmap, generations, journal/COW, atomic rename, unlink directory, permissions, ownership, timestamps, symlinks, hardlinks, fsync contract, fsck/repair. Contiguous relocation в `ensure_capacity` (`:393`) создаёт write-amplification и слабую crash consistency.
- **Баг проектирования:** `auto_mount` пробует каждую GPT partition, не сверяя DunitFS type GUID; invalid superblock лишь фильтрует результат.

### 4.7 Block storage, GPT и установка

- **Работает в QEMU:** PCI discovery, AHCI DMA read/write/flush, SATA registration; legacy VirtIO block polling; generic block registry; GPT primary/backup read/write with CRC.
- **Работает:** host image path создаёт ESP + DUNIT-ROOT; installed boot и `/persist` подтверждены.
- **Частично:** in-system installer зависит от ISO modules и только AHCI; нет transactional install, rollback, upgrade slots или post-install first-boot state.
- **Ключевой разрыв:** установленная система загружает kernel с ESP, но root и `/app` остаются embedded MemFS. DUNIT-ROOT — только data volume.

### 4.8 Terminal Mode и GUI Mode

**Terminal Mode**

- framebuffer console и kernel shell реально работают; history/autocomplete, VFS, diagnostics и foreground ELF exec подтверждены;
- shell и terminal loop находятся в ядре (`kernel/src/lib.rs:750`), поэтому это bootstrap/recovery console, а не конечный userspace shell;
- это наиболее стабильный режим разработки.

**GUI Mode**

- реально загружается, имеет back buffer, damage redraw, wallpaper, pointer/keyboard, окна, dock, launcher, quick settings, notifications, brightness и animations (`kernel/src/ui_loop.rs:2554`, `:3108`, `:3972`);
- userspace GUI apps создают окна и посылают draw/event messages через `libdunit`; kernel shell принимает их в `process_gui_messages` (`kernel/src/ui_loop.rs:1564`);
- GUI terminal — userspace input frontend, но команды исполняет kernel `shell::run_command` через `execute_gui_terminal_command` (`kernel/src/ui_loop.rs:1502`). По доступным командам он близок Terminal Mode, но это не независимый userspace terminal/shell и stdin для interactive child apps ограничен;
- текущий protocol привязан к magic PID 1 (`userspace/libdunit/src/lib.rs:254`) и immediate drawing (`DRAW_TEXT`, `DRAW_RECT`), а не к surfaces/buffer commits;
- геометрия, цвета, приложения и wallpaper dimensions hardcoded (`kernel/src/ui_loop.rs:30-39`, `kernel/src/window_manager.rs:57`). Единственный config — два shortcuts в ad-hoc файле (`kernel/src/fs/vfs.rs:469`).

**Неинтегрированные заготовки**

- `userspace/display_server/src/lib.rs:78` содержит тестируемую in-memory модель windows/focus/composition, но binary `main` создаёт объект и сразу возвращает (`src/main.rs:9`);
- `userspace/video_driver` предполагает несуществующие C symbols и backbuffer за концом framebuffer (`src/lib.rs:119`); `_start` пуст (`src/main.rs:12`);
- их следует использовать как источник тестов/API-идей, но не объявлять основой production GUI без переработки.

### 4.9 Драйверы и будущая платформа

- framebuffer, PS/2 keyboard/mouse, PCI, AHCI и basic block I/O — рабочая QEMU-база;
- VirtIO block — legacy polling, не modern PCI transport и не interrupt-driven;
- E1000 читает status/MAC, но `packet-io=not-implemented` (`kernel/src/drivers/net.rs`);
- xHCI делает controller/ring/Enable Slot bring-up, но USB enumeration, descriptors, endpoints и class drivers отсутствуют;
- audio отсутствует;
- ACPI, battery, thermal, suspend/resume и shutdown отсутствуют; `poweroff` честно сообщает not implemented.

## 5. Что production-like, частично и prototype

| Уровень | Компоненты | Оценка |
|---|---|---|
| Production-like для hobby/QEMU CI | Limine BIOS boot, Terminal boot, PMM/VMM base, ELF smoke, safe user-copy, MemFS/VFS basics, AHCI disk discovery | Повторяемо работает, но ещё не hardened для hostile workloads/SMP |
| Интегрировано, но частично | processes, syscalls, scheduler, IPC, DunitFS, GPT/install, UEFI, GUI apps | Реальные end-to-end пути есть, семантика и recovery неполны |
| Prototype | in-kernel desktop/compositor/WM, shared memory, VirtIO legacy, xHCI, network discovery | Нельзя стабилизировать как public API в текущем виде |
| Skeleton/dead-end | userspace display_server binary, video_driver binary, initrd loader, kthreads | Требует интеграции или замены |
| Не реализовано | preemptive default, userspace threads/TLS, signals, futex, dynamic loader, audio, TCP/IP, ACPI power, package manager | Roadmap ниже |

## 6. Реализованные компоненты

- [x] BIOS/UEFI Limine boot, Terminal/GUI mode selection and framebuffer handoff.
- [x] PMM/VMM/heap, isolated ring-3 ELF execution and recoverable userspace faults.
- [x] Dunit syscall ABI, VFS/MemFS, per-process FDs, cwd, argv/envp and file APIs.
- [x] Cooperative process lifecycle, IPC byte messages and automated runtime regression.
- [x] PCI, AHCI block I/O, GPT, host/in-system installer foundations and DunitFS `/persist` mount.
- [x] In-kernel GUI prototype plus separate userspace GUI application processes.

## 7. Незавершённые компоненты

- [ ] Default-on preemption, userspace threads/TLS and blocking synchronization.
- [ ] Safe shared-memory objects and capability-based IPC handles.
- [ ] Userspace GUI Server/compositor/DWM and declarative UI Runtime.
- [ ] Disk-backed system root, first boot, user profiles, recovery-capable DunitFS.
- [ ] Dunit musl fork, native C toolchain and static userspace baseline.
- [ ] Packet networking, audio, complete USB, ACPI/power and package/application platform.

## 8. Удачные решения и главные архитектурные проблемы

Удачные решения:

- Rust `no_std` kernel + компактный C/NASM HAL дают ясную границу hardware entry/ядро;
- Dunit-native `spawn/yield/wait` позволяет развиваться без преждевременного `fork`;
- отдельные address spaces, recoverable userspace faults и safe-copy — правильный фундамент;
- VFS mount abstraction уже позволяет заменить embedded root постепенно;
- общий block registry отделяет DunitFS/GPT от AHCI/VirtIO;
- test-only Limine configs и автоматический QEMU harness обеспечивают воспроизводимость.

Мешающие решения:

- 4458-строчный `ui_loop.rs` объединяет shell, protocol, compositor, DWM и widgets в kernel;
- raw pixel/input syscalls обходят ownership/security GUI Server;
- magic PID 1 вместо service registry/endpoint capability;
- embedded apps/assets связывают kernel rebuild с userspace update;
- cooperative execution заставляет GUI loop вручную «запускать приложение один раз»;
- global `static mut` и single-CPU assumptions мешают preemption/SMP;
- DunitFS data model не годится как системный root без versioning/recovery.

## 9. Целевая архитектура Green Tea Kernel

Kernel оставляет только mechanisms:

- PMM/VM: address spaces, `map/unmap/protect`, shared VM objects, W^X, guard pages;
- processes/threads: preemptive scheduler, wait queues, clocks, TLS base, credentials;
- object/handle IPC: endpoints, events, shared buffers, rights transfer;
- VFS and block primitives;
- framebuffer/display handle и input event devices;
- capability checks: только GUI Server получает display/input master rights;
- crash reporting, logs и service supervision hooks.

Kernel не должен содержать dock, panel, launcher, decorations, widgets, themes, layout parser или application-specific drawing.

## 10. Архитектура GUI Server, compositor и протокола

Процессная модель:

```text
init (PID 1)
  |- gui-server       owns display/input master handles
  |- dunit-dwm        privileged shell client
  |- terminal         ordinary GUI client + PTY/session
  |- notificationd    optional service after v1
  `- applications     untrusted clients
```

GUI protocol v1 должен быть binary, versioned и transport-neutral:

- `HELLO(version, features)` / `WELCOME(client_id, formats)`;
- `CREATE_SURFACE(role, width, height, format)` -> surface handle;
- `ATTACH_BUFFER(surface, shm_handle, offset, stride, damage[])`;
- `COMMIT(surface, serial)`; server releases buffer asynchronously;
- `CONFIGURE(surface, size, scale, state, serial)` / `ACK_CONFIGURE`;
- `POINTER_*`, `KEY_*`, `TEXT_INPUT`, focus enter/leave;
- `SET_TITLE`, `SET_APP_ID`, `REQUEST_CLOSE`;
- privileged DWM operations separated from ordinary client operations.

Surface format v1: premultiplied XRGB8888/ARGB8888, explicit width/height/stride, bounded damage rectangles, double/triple buffering. Client never receives the physical framebuffer address.

Compositor:

- z-order scene graph, clipping, occlusion and damage tracking;
- focus policy delegated to DWM policy API, enforcement remains server-side;
- input hit-testing against committed scene;
- frame callbacks rather than busy redraw;
- deterministic software compositor first; GPU acceleration later behind backend trait.

Failure policy: malformed message disconnects client, stale handle returns protocol error, crashed app destroys only its surfaces, crashed DWM does not crash GUI Server, crashed GUI Server is restarted by init or falls back to Terminal Mode.

## 11. Dunit UI Runtime и config-driven desktop

Рекомендуемое разделение форматов:

| Назначение | Формат | Причина |
|---|---|---|
| Settings, app metadata, panel/dock composition | TOML subset | Читаем человеком, простая schema/version, подходит persistence |
| UI component tree | DUI — небольшой declarative markup | TOML/JSON неудобны для глубокого дерева, а HTML слишком велик |
| Appearance | DSS — CSS-подобный ограниченный stylesheet | selectors, states, variables и themes без полной сложности CSS |
| Motion | declarative blocks в DSS/DUI | duration/easing/properties; никакого arbitrary script в v1 |

Не использовать полный CSS/HTML/JS engine на раннем этапе. JSON оставить для tooling/IPC diagnostics, не как основной hand-edited config.

UI Runtime:

- retained component tree с stable IDs;
- `Row`, `Column`, `Stack`, `Grid`, `Scroll`, constraints, min/max/preferred size;
- widgets: text, icon, button, input, list, menu, slider, progress, notification;
- event capture/bubble, focus navigation, keyboard actions;
- style cascade: defaults -> theme -> component class -> state;
- animation clock и property interpolation;
- parse/validate off-screen, atomic tree swap; на ошибке оставить last-known-good и показать diagnostic;
- filesystem watch через будущий event API; до него explicit reload action;
- runtime не знает о dock/launcher: DWM собирает их как обычные UI trees.

Пример файлов:

```text
/system/share/dunit-ui/defaults.dss
/system/share/dwm/desktop.dui
/system/share/dwm/default.toml
/users/<uid>/config/dwm/config.toml
/users/<uid>/config/dwm/theme.dss
/users/<uid>/config/dwm/desktop.dui
```

## 12. Архитектура Dunit DWM

Dunit DWM — отдельный policy shell поверх GUI Server:

- desktop/workspaces, panel, dock, launcher, quick settings, notifications, window switcher;
- app registry из manifests, а не enum;
- placement/focus/workspace rules из TOML;
- wallpaper loader и scaling в userspace;
- brightness UI вызывает power/display service, не затемняет software framebuffer как финальное решение;
- terminal, file manager, calculator и monitor — отдельные clients;
- DWM не рисует client contents и не владеет чужими buffers.

## 13. Что нужно улучшить в kernel до DWM

Минимальный gate для DWM: M1 должен дать preemption, threads, blocking events, безопасные shared VM objects и rights-bearing handles. `gui-server` должен запускаться и перезапускаться как service, а не вызываться из вечного kernel loop. Raw input/framebuffer выдаются только ему; приложения получают protocol endpoints. До этого допустимы host-side protocol/layout tests и legacy GUI maintenance, но не фиксация нового desktop ABI.

## 14. Milestones и task cards

### M0 — зафиксировать контракты и убрать ложный статус (must-have)

- [ ] Написать `docs/architecture/current-state.md` и Dunit ABI v1 table.
- [ ] Исправить boot logs initrd/shared-memory/context-switch, чтобы они отражали измеренное состояние.
- [ ] Сделать compiler warnings и QEMU markers частью CI budget.

**Цель/причина:** создать честный baseline; без него нельзя отличить integration от файла-заготовки. **Подсистемы:** boot, docs, tests. **Зависимости:** нет. **Результат:** versioned snapshot. **Готовность:** каждый WORKING claim имеет QEMU test. **Тесты:** Terminal/GUI/BIOS/UEFI matrix. **Риск:** документация снова устареет; снижать executable status report.

### M1 — kernel runtime prerequisites (must-have, перед GUI migration)

- [ ] Включить и стабилизировать preemptive round-robin на PIT, затем abstraction clocksource/timer.
- [ ] Сделать schedulable threads: TID, per-thread context/kernel stack/FPU state, process-owned address space.
- [ ] Добавить wait queues и blocking sleep/IPC/event; убрать polling GUI apps.
- [ ] Реализовать `munmap`, `mprotect`, shared VM object, guard pages и correct teardown.
- [ ] Добавить TLS ABI: set/get thread pointer (`FS.base` на x86_64), initial TLS image.
- [ ] Добавить futex-like Dunit primitive `wait_on_word/wake` без копирования Linux ABI.
- [ ] Перевести глобальные mutable singletons на locks/owned services.
- [ ] Ввести handle table с rights (`READ/WRITE/MAP/SIGNAL/TRANSFER/DISPLAY_MASTER`).

**Цель/причина:** GUI services и musl threads требуют concurrency, blocking и shared buffers. **Подсистемы:** process, scheduler, interrupts, VMM, IPC, HAL. **Зависимости:** M0. **Результат:** несколько независимых long-running processes/threads. **Готовность:** CPU-bound child preempted без `yield`; blocked client не расходует CPU; kill освобождает mappings/handles. **Тесты:** starvation, preemption stress, thread TLS isolation, mmap/unmap/protect, wait/wake race, fault injection. **Риски:** IRQ/lock deadlocks, FPU leakage, lost wakeups; сначала UP, затем SMP.

### M2 — GUI protocol + headless reference server (можно параллельно с M1)

- [ ] Специфицировать protocol v1, state machines, object lifetimes, limits и errors.
- [ ] Реализовать host/headless protocol tests без framebuffer.
- [ ] Перенести полезные pure structures/tests из `userspace/display_server`, не его текущий `egui` binary lifecycle.
- [ ] Реализовать surface/buffer/focus/input models и fuzz/property tests.

**Цель/причина:** отделить контракт от текущего kernel UI. **Подсистемы:** новый `protocol/`, display-server tests. **Зависимости:** финальная интеграция зависит от M1, проектирование нет. **Результат:** стабильная protocol crate/spec. **Готовность:** invalid clients не влияют на server; protocol replay детерминирован. **Тесты:** malformed length, stale IDs, buffer bounds, focus ordering. **Риски:** заморозить слишком широкий API; v1 оставить минимальным.

### M3 — userspace GUI Server и compositor (must-have)

- [ ] Добавить display/input master handles и shared-buffer syscalls.
- [ ] Собрать `gui-server` как обычный ELF из Makefile/image manifest.
- [ ] Реализовать software compositor, damage, frame callbacks, focus/input routing.
- [ ] Запустить два untrusted client processes одновременно.
- [ ] Оставить старый kernel GUI под `legacy_gui` feature до parity, затем удалить.

**Цель/причина:** удалить desktop policy из kernel. **Подсистемы:** kernel display/input/IPC, userspace gui-server, build. **Зависимости:** M1, M2. **Результат:** kernel предоставляет только mechanisms. **Готовность:** `kernel/src/ui_loop.rs` не участвует в normal GUI boot; server crash не рушит kernel. **Тесты:** overlapping surfaces, resize storms, app crash, buffer reuse, input isolation, 10-minute idle/load. **Риски:** copies/performance; сначала correctness и bounded buffers.

### M4 — UI Runtime и Dunit DWM parity (must-have)

- [ ] Реализовать DUI parser/tree, layout, base widgets, DSS theme и motion engine.
- [ ] Реализовать schema-versioned TOML settings и last-known-good reload.
- [ ] Собрать DWM: desktop, panel, dock, launcher, quick settings, notifications, workspaces, Alt/Super switcher.
- [ ] Перенести terminal, calculator, file manager, stats на client API.
- [ ] Перенести GUI terminal command execution в userspace shell/session; добавить PTY-like endpoint.
- [ ] Сохранить/восстановить window/session settings в user config.

**Цель/причина:** полный отказ от hardcoded desktop. **Подсистемы:** ui-runtime, DWM, apps, VFS. **Зависимости:** M3 и writable installed user config. **Результат:** внешний вид меняется без kernel rebuild. **Готовность:** все перечисленные функции parity; испорченный config не мешает login. **Тесты:** golden layout at 1024×768/1600×900/HiDPI, keyboard-only navigation, reload, malformed config, animation timing. **Риски:** scope explosion; accessibility/text shaping оставить отдельными incremental milestones, но API учесть.

### M5 — normal installed system и persistence (must-have; storage foundation можно параллельно M1-M3)

- [ ] Определить GPT policy: ESP + Dunit System + Dunit Data; для v1 допустимы ESP + единый DunitFS root, но `/system` должен быть read-mostly.
- [ ] Убрать `include_bytes!` applications/assets; загрузить init и app manifests с disk root.
- [ ] Реализовать real init/service manager и first-boot provisioning.
- [ ] Расширить DunitFS: allocation bitmap/extents, directories, rename, unlink, timestamps, permissions, fsync, superblock generations, journal или metadata COW.
- [ ] Добавить `fsck.dunit`, read-only degraded mount и recovery report.
- [ ] Сделать installer transactional: validate -> partition -> format -> copy -> verify -> boot config -> sync.
- [ ] Поддержать AHCI и VirtIO target; отдельно тестировать BIOS и UEFI.
- [ ] Разделить Live и installed manifests; Live имеет installer/recovery, installed — persistent root и first boot.

**Цель/причина:** `/persist` рядом с embedded root не является законченной установкой. **Подсистемы:** installer, GPT, DunitFS, VFS, boot, init, build images. **Зависимости:** базовая часть параллельна GUI; DWM config persistence зависит от неё. **Результат:** system/users/apps обновляются с диска. **Готовность:** create/write/fsync/reboot/read; interrupted metadata update восстанавливается; kernel не содержит app binaries. **Тесты:** two-reboot content hash, 1000-file/large-file tests, disk-full, corrupted CRC, power-cut points, BIOS/UEFI × AHCI/VirtIO. **Риски:** filesystem corruption; нельзя использовать DunitFS v1 как единственную копию важных данных.

Рекомендуемая иерархия:

```text
/system/bin        trusted system programs
/system/lib        Dunit runtime, libc, shared libraries later
/system/share      themes, DUI/DSS, icons, defaults
/system/services   service manifests
/apps/<id>         application bundles/manifests
/users/<uid>/home  documents
/users/<uid>/config
/users/<uid>/data
/var/log           bounded logs
/var/lib           service state
/run               volatile runtime state
/devices           device namespace
/proc              diagnostics
```

### M6 — Dunit musl fork, static-first (should-have после M1/M5)

Основной кандидат фиксируется: **musl fork**. relibc/newlib допустимы только как сравнительные fallback, если spike найдёт фундаментальный блокер.

- [ ] Создать `toolchains/dunit-musl/` и target tuple `x86_64-dunit`.
- [ ] Сохранить Dunit syscall numbers/ABI; написать musl arch/syscall adaptation layer, не Linux emulation layer.
- [ ] Phase A: crt1, `_start`, exit, write, errno, string, malloc поверх Dunit mmap — static binaries only.
- [ ] Phase B: files, cwd, stat/readdir, clocks, environment, spawn/wait; предпочесть `posix_spawn`-подобный Dunit path.
- [ ] Phase C: pthread/TLS поверх M1 threads + wait/wake primitive.
- [ ] Phase D: limited signals только после явной Dunit signal model; не копировать Linux semantics автоматически.
- [ ] Phase E: sockets после native network API mapping.
- [ ] Dynamic loader и `.so` — отдельный milestone после stable ABI, filesystem и memory protection.

**Цель/причина:** C ecosystem без превращения kernel в Linux. **Подсистемы:** musl fork, ABI, VM, threads, VFS. **Зависимости:** M1; static filesystem programs требуют M5. **Результат:** native Dunit-linked C programs. **Готовность:** static hello/file/malloc/env/thread tests; upstream musl tests subset с documented skips. **Тесты:** allocator stress, TLS uniqueness, mutex contention, cancellation policy, locale-disabled profile. **Риски:** скрытые Linux assumptions, fork/signals/dlopen; поддерживать explicit porting delta.

Минимальные kernel primitives для первого порта musl:

- process exit, robust read/write/open/close/seek/stat and errno;
- anonymous `mmap` + `munmap` + `mprotect`;
- clocks and blocking sleep;
- TLS base setup;
- threads create/exit/join;
- atomic wait/wake;
- spawn/exec-image contract and environment;
- entropy source до crypto/network use.

`fork` не блокирует static-first port: сначала Dunit-native spawn/`posix_spawn`; `fork` реализовать позже через COW только если реальная software demand оправдает сложность.

### M7 — platform services и ecosystem (later, часть можно параллельно после M1)

- [ ] Networking: E1000 RX/TX rings -> Ethernet/ARP/IPv4/ICMP/UDP -> TCP/DNS/DHCP -> native socket handles.
- [ ] Audio: HDA discovery/DMA -> mixer service -> per-app streams/permissions.
- [ ] USB: xHCI enumeration -> descriptors/endpoints -> HID keyboard/mouse -> mass storage; hotplug events.
- [ ] Power: ACPI tables, reboot/poweroff, battery/thermal, затем suspend/resume.
- [ ] Packages: signed bundle manifest, architecture/ABI version, install transaction, rollback; network repository только после network + trust model.
- [ ] Application model: app ID, manifest, permissions, resources, data dirs, GUI protocol version, optional package dependencies.
- [ ] SDK: headers, Dunit musl toolchain, UI compiler/linter, package builder, emulator test template.

**Цель/причина:** превратить desktop prototype в платформу. **Зависимости:** M1, M5, ABI; packages из сети зависят от crypto/network/time. **Готовность:** subsystem-specific end-to-end tests и permission denial tests. **Риски:** hardware matrix; QEMU reference devices first, затем реальные machines.

## 15. План полной переработки GUI Mode

Практическая последовательность — M2 protocol model, затем один M3 vertical slice «два клиента -> shared surfaces -> compositor -> input/focus», после него M4 parity. Старый GUI остаётся feature-gated только до появления terminal + panel + dock + launcher + notifications + quick settings. После parity удаляются `kernel/src/ui_loop.rs`, `kernel/src/window_manager.rs` и GUI-specific syscall drawing; reusable framebuffer/damage primitives переносятся в userspace либо остаются узким backend API.

## 16. План конфигурационной UI-системы

Три схемы (`config.toml`, `*.dui`, `*.dss`) получают независимые version fields, size/depth limits и offline linter. DWM применяет конфигурацию транзакционно: parse -> schema validate -> resource resolve -> layout dry run -> atomic swap. Ошибка сохраняет last-known-good, пишет diagnostic и никогда не валит desktop. System defaults неизменяемы; user overlay хранится на persistent user volume.

## 17. План перехода от Live Boot к установленной системе

M5 заменяет embedded `/app` и assets на disk-root manifest, запускает `init` с диска и чётко различает profiles: Live монтирует read-only image и предлагает installer/recovery, Installed монтирует persistent system/data и не требует installer modules. Installation success означает не запись ESP, а успешный first boot, создание user environment, запись данных и повторную проверку после reboot.

## 18. План persistence и файловой структуры

DunitFS v2 должна иметь recoverable metadata update, `fsync` contract, allocation tracking и `fsck`. System defaults лежат в `/system/share`; изменяемые host state — `/var`; user settings/data — `/users/<uid>/{config,data,home}`; `/run` всегда volatile. Acceptance test записывает random content и config, синхронизирует, полностью останавливает VM, загружает тот же image и сверяет hashes и DWM settings.

## 19. План портирования libc

M6 фиксирует musl fork как основной путь, Dunit-native syscall adapter и static-first порядок. Критический path: VM/unmap/protect -> TLS -> threads -> wait/wake -> pthread subset. Signals, `fork` и dynamic loader не входят в первый порт; их семантика проектируется только по подтверждённому demand. POSIX headers/API являются source-compatibility facade над Dunit ABI.

## 20. Критерии завершения каждого этапа

- **M0:** status claims трассируются к test marker и исходнику.
- **M1:** preemption/thread/TLS/wait-wake stress проходит без races и leaks.
- **M2:** protocol conformance/fuzz suite детерминированно зелёная.
- **M3:** userspace GUI Server композитит два изолированных клиента и переживает их crash.
- **M4:** feature parity и config reload без kernel GUI code.
- **M5:** BIOS/UEFI installed root + content/config persistence + recovery test.
- **M6:** static C hello/file/malloc/thread suite на Dunit musl fork.
- **M7:** каждый service имеет end-to-end I/O и permission test, не только discovery.

## 21. Dunit Minimal и Dunit DWM images

- **Dunit Minimal:** kernel, init, recovery terminal, storage/fs tools, network diagnostics optional; без GUI assets/server/DWM. Используется CI, recovery и low-resource systems.
- **Dunit DWM:** Minimal + GUI Server, Dunit UI Runtime, DWM, fonts/themes, terminal/file manager/settings.
- Оба образа используют один kernel ABI и package format; различаются manifests, не `#ifdef` архитектурой.
- Live images дополнительно содержат installer и read-only recovery assets; installed images используют persistent root.

## 22. Зависимости между этапами и параллельная разработка

```text
M0 baseline
 |- M1 scheduler/threads/IPC/VM -----> M3 GUI Server -----> M4 DWM/UI
 |                    `--------------> M6 musl static -> pthread -> dynamic later
 |- M2 protocol/headless tests ------> M3
 `- M5 filesystem/installer ---------> M4 config persistence + M6 disk userspace
                                      `-> M7 packages/services
```

Можно параллельно:

- GUI protocol/headless compositor tests и kernel scheduler work;
- DunitFS v2 design/fsck tooling и shared-memory/IPC work;
- DUI/DSS parser/layout host tests и GUI Server integration;
- musl source audit/toolchain spike и M1 implementation;
- network/audio/USB design после handle/event model, не раньше.

Нельзя параллелить ценой фиксации временного ABI: GUI shared buffers, pthread/TLS и package manifests должны ждать утверждённых kernel object/ABI contracts.

## 23. Приоритеты: must-have / should-have / later

**Must-have:** M0; preemption/threads/wait queues/handles/shared VM; GUI protocol/server; DWM parity; DunitFS recovery + disk-root/init; persistence matrix.

**Should-have:** static musl port, config hot reload, workspaces/session restore, VirtIO modern block, ACPI shutdown, basic networking.

**Later:** dynamic loader, full signals/fork, SMP, GPU acceleration, TCP maturity, audio mixer, USB mass storage, signed repositories, suspend/resume.

## 24. Risk register

| Риск | Вероятность/влияние | Мера |
|---|---|---|
| Preemption вскрывает `static mut` races | высокая/критическое | lock audit, IRQ-safe rules, stress before default-on |
| DunitFS corruption | высокая/критическое | v2 metadata transaction, fsck, power-cut tests, backup superblock |
| GUI protocol premature freeze | средняя/высокое | minimal v1, capability negotiation, headless conformance suite |
| Shared buffer leaks/aliasing | высокая/высокое | kernel-owned VM objects, reference counts, rights and bounds |
| GUI rewrite stalls visible progress | средняя/высокое | legacy GUI feature until parity; vertical slices |
| musl drags kernel toward Linux | средняя/высокое | Dunit ABI adapter, documented POSIX layer, no Linux syscall numbers |
| fork/signals consume roadmap | высокая/среднее | static + spawn first; demand-driven semantics |
| Installer destroys wrong disk | низкая/критическое | device identity, size/model confirmation, dry-run plan, explicit destructive token |
| Hardware scope explosion | высокая/среднее | reference QEMU devices and published support tiers |

## 25. Technical debt register

- [ ] Разбить `kernel/src/lib.rs`, `syscall/mod.rs`, `process/mod.rs`, `ui_loop.rs` по ответственностям.
- [ ] Устранить `static mut` references и compiler warnings.
- [ ] Заменить raw VFS trait pointers/global path buffer безопасной ownership/locking model.
- [ ] Удалить пустой initrd либо реализовать archive loading и честные logs.
- [ ] Унифицировать Terminal и GUI shell через userspace session/PTY, а kernel оставить recovery console.
- [ ] Версионировать Dunit ABI, filesystem, GUI protocol, app manifest и config schemas.
- [ ] Удалить dead `display_server`/`video_driver` paths после переноса полезных тестов.
- [ ] Добавить structured log levels и bounded persistent logs.

## 26. План тестирования

Каждый runtime test запускается только через `tools/qemu_test.py`.

Матрица:

- boot: ISO/disk × Terminal/GUI × BIOS/UEFI;
- storage: AHCI/VirtIO × clean/corrupt/full × two-reboot/power-cut;
- process: spawn/wait/kill/fault/preempt/thread/TLS;
- IPC: queue full, client crash, endpoint close, rights transfer, shared buffer lifecycle;
- GUI: headless protocol suite + QEMU screenshot markers + interactive event injection;
- libc: static C conformance tiers; dynamic tier отдельно;
- longevity: 10 min smoke per PR, multi-hour nightly stress.

Обязательные gates milestone:

- serial log не содержит `PANIC`, `FAIL`, unexpected fault или timeout;
- marker доказывает не только boot, но и subsystem completion;
- screenshot используется только вместе с machine-readable state/serial marker;
- persistence считается доказанной лишь после stop/start, повторного mount и проверки content hash;
- corrupt input tests не требуют участия пользователя и QEMU всегда завершается harness-ом.

## 27. Что нельзя делать слишком рано

- не переносить dock/widgets/styles в kernel;
- не объявлять текущий `display_server` интегрированным;
- не стабилизировать immediate-mode `DRAW_RECT/DRAW_TEXT` как долгосрочный GUI ABI;
- не давать приложениям raw framebuffer/input;
- не делать полный CSS/HTML/JS runtime до базового DUI/DSS;
- не начинать dynamic loader до stable VM/ABI/filesystem;
- не делать `fork` главным условием musl v1;
- не считать `/persist` эквивалентом installed root;
- не добавлять package network repository без signatures, time, TLS/crypto и transactions;
- не начинать SMP до корректного UP preemption и lock audit;
- не заявлять hardware support по PCI detection без end-to-end I/O test.

## 28. Предлагаемая структура каталогов

```text
kernel/
  arch/x86_64/  memory/  process/  ipc/  vfs/  drivers/  abi/
userspace/
  init/  services/gui-server/  services/inputd/  services/audiod/
  dwm/  ui-runtime/  shell/  terminal/  apps/  libdunit/
protocols/
  gui-v1/  service-v1/  package-v1/
filesystems/dunitfs/
toolchains/dunit-musl/
images/minimal/  images/dwm/  images/live/
configs/defaults/
tests/qemu/  tests/protocol/  tests/persistence/  tests/libc/
tools/qemu_test.py
```

## 29. Дальнейшее развитие проекта

После M6/M7: стабилизировать SDK и ABI release cadence, добавить signed app bundles, accessibility/text shaping/IME, hardware compatibility tiers, reproducible images и upgrade rollback. GPU acceleration, SMP, dynamic linking, suspend/resume и broad POSIX coverage развиваются независимо и не должны блокировать надёжную UP software-composited desktop v1.

## 30. Итоговая целевая архитектура Dunit Desktop OS

Завершённая архитектура — не Linux distribution и не GUI внутри kernel. Green Tea Kernel предоставляет собственные Dunit objects, ABI, scheduling, VM, IPC, VFS и device primitives. `init` запускает userspace services. GUI Server единолично владеет display/input handles и композитит shared surfaces. Независимый Dunit DWM задаёт desktop policy через Dunit UI Runtime и versioned DUI/DSS/TOML. Установленная система грузит programs/config/data с проверяемой DunitFS, а Dunit musl fork предоставляет source portability поверх Dunit ABI.

Ближайший правильный порядок: **M0 honest baseline -> M1 preemption/threads/blocking IPC/shared VM -> M3 GUI Server vertical slice**, параллельно **M2 protocol tests** и **M5 DunitFS/install v2**; затем **M4 DWM parity** и **M6 static-first musl**. Это сохраняет видимый GUI прогресс, но не цементирует сегодняшние kernel hardcodes.
