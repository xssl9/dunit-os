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

- UP round-robin, schedulable user threads, timer/IPC wait queues и x86_64 static TLS включены; общая система событий и futex-like синхронизация ещё отсутствуют;
- нет dynamic TLS/DTV, signals, futex-подобного ожидания и полноценного process `exec`/`fork`;
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
- **Реализовано после исходного среза:** anonymous private `mmap`, `munmap`, `mprotect`, guard pages и shared VM object с физическими страницами и refcount. `[VM-TEST] OK` проверяет aliasing между процессами, page fault на guard page, partial unmap и освобождение frame после teardown.
- **Не реализовано:** file mappings, demand paging, COW и права доступа на shared VM IDs (позже через handle table).
- **Не реализовано:** syscall `fork` и process-image `exec` возвращают `ENOSYS` (`kernel/src/syscall/mod.rs:491`, `:495`). Текущий `spawn` — Dunit-native primitive и пригоден как основной API; `fork` не обязан быть первым.
- **Техдолг:** много `static mut` singleton state и 91 compiler warning в test build; существующие `SpinLock`/`IrqSafeSpinLock` надо распространить на VFS, DunitFS, IPC, terminal, WM и driver registries.

### 4.3 Планировщик, threads, TLS и синхронизация

- **Реализовано:** PID ready queue, saved CPU context, transitions Ready/Running/Blocked/Dead/Reaped, `yield`, parent/child wait/reap.
- **Реализовано после исходного среза:** UP round-robin включён по умолчанию; x87/MMX/SSE сохраняются через FXSAVE/FXRSTOR, PIT предоставляет monotonic clock/deadline abstraction. QEMU smoke проверяет вытеснение и XMM isolation.
- **Реализовано после исходного среза:** schedulable userspace TID, отдельные GPR/FPU/kernel stack, общее process-owned address space и fd table; create/exit/nonblocking join/get_tid. QEMU smoke проверяет общую память, XMM isolation, thread fault и teardown при выходе процесса.
- **Реализовано после исходного среза:** wait queue переводит потоки в Blocked; PIT будит `sleep` и timed IPC event wait, отправка IPC будит ожидающие потоки. GUI apps ждут IPC-события без цикла `yield`; QEMU `[WAIT-TEST] OK` проверяет таймаут, sleep и IPC wakeup.
- **Реализовано после исходного среза:** `FS.base` переключается для каждого TID, syscalls 39–40 задают/читают thread pointer, ELF `PT_TLS` создаёт отдельный initial TLS/TCB для main и новых потоков. `[TLS-TEST] OK` проверяет `.tdata/.tbss` и изоляцию при вытеснении.
- **Не реализовано:** dynamic thread vector для dynamic TLS, универсальный event/handle readiness, priorities, time accounting, SMP и полноценный kernel-thread scheduler.
- **Следствие:** GUI Server как настоящий long-running userspace service и musl pthreads пока нельзя считать надёжными.

### 4.4 Syscalls и ABI

- **Реализовано:** Dunit ABI (`kernel/src/syscall/mod.rs:9`): file I/O, anonymous mmap, byte IPC, framebuffer/input, spawn/wait/kill/sleep/yield, cwd, stats, readdir/stat и thread create/join/exit/get_tid (номера 29–32).
- **Удачно:** user-copy проверяет присутствие и writable/user flags каждой страницы до доступа.
- **Частично:** `sleep` блокирует поток до PIT deadline; stdio/terminal foreground policy всё ещё kernel-centric.
- **Проблема безопасности:** raw framebuffer syscalls доступны обычным приложениям; будущий GUI требует capability/handle, доступный только GUI Server.
- **ABI-долг:** нет generated/versioned ABI manifest, handle rights, poll/event wait, dup/pipe, seek, metadata, realtime clocks, dynamic TLS/DTV и robust errno contract.

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
| Не реализовано | TLS, signals, futex, dynamic loader, audio, TCP/IP, ACPI power, package manager | Roadmap ниже |

## 6. Реализованные компоненты

- [x] BIOS/UEFI Limine boot, Terminal/GUI mode selection and framebuffer handoff.
- [x] PMM/VMM/heap, isolated ring-3 ELF execution and recoverable userspace faults.
- [x] Dunit syscall ABI, VFS/MemFS, per-process FDs, cwd, argv/envp and file APIs.
- [x] Cooperative process lifecycle, IPC byte messages and automated runtime regression.
- [x] PCI, AHCI block I/O, GPT, host/in-system installer foundations and DunitFS `/persist` mount.
- [x] In-kernel GUI prototype plus separate userspace GUI application processes.

## 7. Незавершённые компоненты

- [x] Default-on preemption, userspace threads и blocking timer/IPC wait queues (M1 QEMU smokes).
- [ ] TLS и futex-like синхронизация для pthread subset.
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

- [x] Написать `docs/architecture/current-state.md` и Dunit ABI v1 table.
- [x] Исправить boot logs initrd/shared-memory/context-switch, чтобы они отражали измеренное состояние.
- [x] Сделать compiler warnings и QEMU markers частью CI budget.

**Цель/причина:** создать честный baseline; без него нельзя отличить integration от файла-заготовки. **Подсистемы:** boot, docs, tests. **Зависимости:** нет. **Результат:** versioned snapshot. **Готовность:** каждый WORKING claim имеет QEMU test. **Тесты:** Terminal/GUI/BIOS/UEFI matrix. **Риск:** документация снова устареет; снижать executable status report.

### M1 — kernel runtime prerequisites (must-have, перед GUI migration)

- [x] Доказать, что preemptive round-robin на PIT реально вытесняет CPU-bound child без `yield` (smoke-хук `[PREEMPT-TEST] OK`).
- [x] Включить round-robin по умолчанию, сохранять FPU/SSE-состояние и добавить abstraction clocksource/timer. Проверено QEMU boot smoke: CPU-bound parent/child с разными XMM значениями, `runtime_stress`, `ipc_parent`.
- [x] Сделать schedulable threads: TID, per-thread context/kernel stack/FPU state, process-owned address space. QEMU `[THREAD-TEST] OK`: два потока делят память/PID, сохраняют разные XMM значения, join возвращает статусы, thread fault изолирован, unjoined поток удаляется при выходе владельца.
- [x] Добавить wait queues и blocking sleep/IPC event wait; убрать polling GUI apps. `[WAIT-TEST] OK` проверяет timeout, timer wake и IPC wake; общий handle/event readiness остаётся отдельной задачей.
- [x] Реализовать `munmap`, `mprotect`, shared VM object, guard pages и correct teardown. QEMU `[VM-TEST] OK`: partial unmap, W^X, guard fault, shared aliasing и возврат frame в PMM после закрытия/выхода peer.
- [x] Добавить TLS ABI: set/get thread pointer (`FS.base` на x86_64), initial `PT_TLS` image. `[TLS-TEST] OK`: Variant II TCB, `.tdata/.tbss`, отдельный TLS main/threads и сохранение при вытеснении.
- [x] Добавить futex-like Dunit primitive `wait_on_word/wake` без копирования Linux ABI. Dunit-native: ключ `(owner pid, user vaddr)`, `FutexWait=41`/`FutexWake=42`, compare-and-park атомарен под IRQ guard + WAIT_QUEUE (без lost wakeup), `read_user_u32` проверяет PRESENT|USER. QEMU `futex_test: OK`: EAGAIN на несовпадении, timeout, wake/join worker'а.
- [x] Перевести глобальные mutable singletons на locks/owned services. Все `static mut` ядра убраны: write-once синглтоны → `OnceCell`/`UnsafeCell`-newtype с `unsafe impl Sync`; состояние, разделяемое с IRQ (буфер клавиатуры) → `IrqSafeSpinLock`; данные под уже существующими spinlock'ами (MMIO, PCI, registry, block, mouse-пакет, ring событий мыши) свёрнуты в одну структуру за `UnsafeCell` под тем же локом; простые скаляры-флаги (параметры фреймбуфера, флаги init) → атомики; кооперативные синглтоны, раздающие `&'static mut` (VMM/PMM/VFS/process table/IPC/WM/initrd/kthreads/dunitfs), → `UnsafeCell`-newtype с сохранением семантики UP. Остались только 2 символа `extern "C"` от HAL (`boot_mem_regions/boot_mem_region_count`), читаемые один раз через `read_volatile`. Регрессии зелёные: boot до терминала, `[TLS-TEST] OK`, `futex_test: OK`.
- [x] Ввести handle table с rights (`READ/WRITE/MAP/SIGNAL/TRANSFER/DISPLAY_MASTER`). Per-process `HandleTable` (kernel/src/handle.rs): непрозрачный `Handle` -> объект ядра + битовая маска прав. Объекты: `Memory` (READ/WRITE/MAP), `Endpoint` (SIGNAL), `Display` (DISPLAY_MASTER, эксклюзивно на систему). Инвариант capability: права можно лишь сужать через `dup`/`transfer`, расширение отклоняется. Syscalls 43–54 (create_memory/create_endpoint/read/write/map/dup/rights/close/signal/take_signals/display_acquire/transfer) + обёртки в libdunit. Teardown освобождает дисплей и очищает таблицу при выходе процесса. QEMU `handle_test: OK`: все 6 прав, READ/WRITE/MAP по объекту памяти, SIGNAL через endpoint + приём, эксклюзивный DISPLAY_MASTER, TRANSFER (перенос + отказ без права), сужение прав и невалидность хэндла после close/transfer.

**Цель/причина:** GUI services и musl threads требуют concurrency, blocking и shared buffers. **Подсистемы:** process, scheduler, interrupts, VMM, IPC, HAL. **Зависимости:** M0. **Результат:** несколько независимых long-running processes/threads. **Готовность:** CPU-bound child preempted без `yield`; blocked client не расходует CPU; kill освобождает mappings/handles. **Тесты:** starvation, preemption stress, thread TLS isolation, mmap/unmap/protect, wait/wake race, fault injection. **Риски:** IRQ/lock deadlocks, FPU leakage, lost wakeups; сначала UP, затем SMP.

### M2 — GUI protocol + headless reference server (можно параллельно с M1)

- [x] Специфицировать protocol v1, state machines, object lifetimes, limits и errors. Контракт v1.0: [protocols/gui-v1/README.md](protocols/gui-v1/README.md) — wire layout, negotiation, surface/buffer/configure/focus lifecycle, quotas, errors и нормативные сценарии. Это спецификация для следующих пунктов M2; decoder, headless tests и kernel binding M3 ещё не реализованы, public ABI не заморожен.
- [x] Реализовать host/headless protocol tests без framebuffer. Крейт [protocols/gui-v1](protocols/gui-v1) (`no_std + alloc`, ноль внешних зависимостей): реальный wire-кодек (`src/wire.rs`) и headless reference server (`src/server.rs`) с порядком валидации §10 (framing/attachments → version → serial → opcode direction/policy → connection state → object namespace → enum/geometry/quotas). 37 host-тестов `cargo test`: framing/malformed rejection, negotiation/HELLO timeout/serial monotonicity, object namespace (fresh/stale/reused/cross-connection/type), import bounds/limits и детерминированный byte-identical replay. Поведенческие state machines (surface/buffer/configure/commit/focus/input events) и fuzz/property остаются пунктом 4 ниже.
- [x] Перенести полезные pure structures/tests из `userspace/display_server`, не его текущий `egui` binary lifecycle. Модель [`protocols/gui-v1/src/wm.rs`](protocols/gui-v1/src/wm.rs): чистый, детерминированный window/focus/composition-слой (window stack, focus policy с авто-фокусом и переназначением при destroy, top-most hit-testing, click-drag, CPU-композитор в ARGB `Vec<u32>`) — `egui`-lifecycle (`Context`/`RawInput`, scancode→egui-key) отброшен. Property-тесты display_server перенесены в [`tests/wm.rs`](protocols/gui-v1/tests/wm.rs) как детерминированные развёртки (proptest недоступен offline): creation/drag/focus/compositing/cursor + hit-test стекинга и скрытых окон. Общий host-набор крейта — 45 тестов, зелёный.
- [x] Реализовать surface/buffer/focus/input models и fuzz/property tests. В [protocols/gui-v1/src/server.rs](protocols/gui-v1/src/server.rs) добавлены поведенческие state machines: surface configure/ack/commit lifecycle с configure/commit токенами, buffer ownership (`AVAILABLE→PENDING→BUSY` + `BUFFER_RELEASE`), frame callbacks завершаемые на composition tick, и single-seat focus/input роутер (pointer/keyboard enter/leave, motion/button/axis, key/text, implicit grab, подавление release неудерживаемых keys, held-key overflow). Policy-решения (focus assignment, reconfigure) входят как verified external events (§9). Новый [tests/behavior.rs](protocols/gui-v1/tests/behavior.rs) детерминированно покрывает нормативные сценарии §11.1–4, §11.9, §11.10 плюс seeded byte/event fuzz (no panic, per-connection serial monotonicity); [tests/limits.rs](protocols/gui-v1/tests/limits.rs) закрывает оставшиеся edge-сценарии §11.7/§11.8/§11.12 — damage-rect boundary/max/max+1, `damage_count`/array mismatch → MALFORMED, backing end==cap/overflow, surface dims min/max/max+1, per-connection backing budget, fixed little-endian/no-padding wire round-trip и no-partial-state + retry под новым serial. Всего 61 host-тест `cargo test` зелёный; QEMU boot regression без изменений. Дублирующий import memory-object (§7) отложен до M3 kernel binding (transport shim не несёт memory-object identity), помечено в коде.

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

Основной кандидат фиксируется: **полный fork musl**. relibc/newlib допустимы только как сравнительные fallback, если ограниченный spike обнаружит фундаментальный блокер. musl проектировалась поверх Linux syscall layer, поэтому Dunit выполняет **порт на новую ОС**, а не только добавляет новую CPU-архитектуру. Готовый Linux `libc.a` и простая замена syscall numbers непригодны: отличаются контракты процессов, файлов, VM, threads, signals, структуры данных и ошибки.

#### M6.1. Что является портом, а что не является Linux-совместимостью

Целевая цепочка:

```text
C/POSIX source API
        |
        v
Dunit musl fork: libc semantics + Dunit OS adaptation layer
        |
        v
versioned Dunit userspace ABI / libdunit low-level contract
        |
        v
Green Tea Kernel native syscalls and objects
```

- Green Tea Kernel не принимает Linux syscall numbers и не экспортирует Linux kernel structs.
- Публичные заголовки musl предоставляют переносимость исходного C-кода; это facade, а не kernel ABI.
- Внутренний adapter переводит POSIX flags, `errno`, paths, `stat`, directory entries, clocks и process operations в собственные Dunit contracts.
- x86_64 System V calling convention, ELF и стандартный initial stack допустимы как независимые отраслевые ABI; их использование не означает Linux compatibility.
- Если функция пока не поддерживается, она возвращает документированную ошибку (`ENOSYS`, `ENOTSUP`) или исключается из заявленного профиля. Заглушка, ложно возвращающая success, запрещена.

#### M6.2. Исходники и сопровождение fork

- [ ] Создать отдельный репозиторий `dunit-musl` как fork полного upstream source tree, не копировать выборочные `.c` и headers.
- [ ] Зафиксировать исходный upstream release/tag и commit в `UPSTREAM.md`.
- [ ] Подключить fork в `toolchains/dunit-musl/` как pinned submodule либо воспроизводимый pinned checkout.
- [ ] Вести Dunit-изменения тематическими commits: `build`, `crt`, `syscall`, `fs`, `vm`, `thread`, `signal`, `network`, `ldso`.
- [ ] Хранить таблицу отличий от upstream и регулярно переносить bug/security fixes; обновление версии должно проходить полный libc test gate.
- [ ] Не удалять из fork временно неподдерживаемые подсистемы: исключать их build profile, чтобы позднее не восстанавливать дерево вручную.
- [ ] Проверить лицензионные notices musl и генерировать перечень исходных версий в SDK/image metadata.

**Цель:** управляемый fork с проверяемым происхождением. **Причина:** маленький vendor snapshot быстро потеряет исправления и станет непереносимым. **Подсистемы:** source management, build, release. **Зависимости:** M0. **Результат:** любой Dunit release указывает точный upstream и Dunit commit. **Готовность:** clean checkout воспроизводит тот же `libc.a`; upstream delta автоматически формируется в CI. **Тесты:** clean/offline rebuild, checksum comparison, upgrade rehearsal. **Риски:** большой вечный diff; минимизировать OS-specific changes и не переформатировать upstream files.

#### M6.3. Dunit ABI прежде libc

До интеграции musl выпустить `Dunit Userspace ABI v0`:

- [ ] Зафиксировать syscall calling convention: `rax` number; `rdi/rsi/rdx/r10/r8/r9` arguments; `rax` result.
- [ ] Версионировать syscall numbers независимо от Rust `libdunit`; генерировать Rust и C definitions из одного manifest.
- [ ] Определить signed error return range и стабильное преобразование Dunit errors в POSIX `errno`.
- [ ] Определить lifetime и права handles/fds, наследование при spawn, reserved descriptors `0/1/2`.
- [ ] Определить endian, width и alignment для scalar types, `time_t`, `off_t`, `ino_t`, `pid_t`, pointers и atomics.
- [ ] Не раскрывать kernel-private `Stat`, process или directory structs: использовать versioned wire structs и переводить их в публичные musl types.
- [ ] Определить process-entry ABI: initial stack, `argc/argv/envp`, auxiliary vector, page size, random seed и program headers.
- [ ] Определить ELF contract: accepted types/segments, load bias, stack permissions, `PT_TLS`, `PT_GNU_RELRO`, `PT_INTERP` policy.
- [ ] Добавить feature/capability query вместо определения поведения по версии kernel.

Текущий Dunit `_start(argc, argv, envp)` через регистры годится для прототипов, но до musl следует перейти к документированному initial stack. Dunit-specific `crt1` разбирает его и вызывает `__libc_start_main`; kernel не должен знать устройство musl.

**Готовность ABI v0:** независимый C test без musl получает arguments/environment/auxv, вызывает raw syscalls и корректно завершает процесс. Изменения ABI до v1 допустимы только с bump версии и обновлением conformance tests.

#### M6.4. Cross-toolchain и sysroot

- [ ] Ввести логическое target имя `x86_64-dunit`; не выдавать Dunit binary за `x86_64-linux-musl`.
- [ ] Сначала использовать LLVM/Clang + LLD или небольшой compiler-driver wrapper; полный GCC port не является условием первого C binary.
- [ ] Научить compiler driver выбирать Dunit linker script, crt objects, include paths, library paths и default static mode.
- [ ] Собрать из fork `crt1.o`, `crti.o`, `crtn.o`, `libc.a` и public headers в staging sysroot.
- [ ] Не смешивать host headers/libraries с Dunit sysroot; сборка с найденным host `/usr/include` должна падать.
- [ ] Создать команды `x86_64-dunit-cc`, `x86_64-dunit-ar`, `x86_64-dunit-strip` и SDK manifest с ABI/libc versions.
- [ ] Добавить CMake toolchain file, Meson cross file и Autoconf cache только после стабильного прямого compiler flow.

Предлагаемый sysroot:

```text
toolchains/sysroot/
├── usr/include/          # musl public headers + Dunit extensions
├── usr/lib/
│   ├── crt1.o
│   ├── crti.o
│   ├── crtn.o
│   └── libc.a
├── system/include/dunit/ # native Dunit API outside POSIX namespace
└── share/dunit-sdk/      # ABI manifest, supported profile, licenses
```

**Готовность:** `x86_64-dunit-cc -static hello.c -o hello` создаёт ELF без host dependencies; `readelf` подтверждает ожидаемые segments/symbols, а Green Tea Kernel запускает его.

#### M6.5. Разделение кода внутри musl fork

x86_64 arithmetic, atomics, string/memory implementations и большая часть ISO C остаются upstream. Dunit-specific код концентрируется в узких границах:

```text
arch/x86_64/              CPU ABI, atomics, TLS access; минимум Dunit delta
crt/ + ldso/              Dunit process entry; dynamic loader позже
src/internal/             syscall/error/capability bridge
src/dunit/                native adapters and structure translation
src/thread/               Dunit thread/TLS/wait-wake backend
src/process/              Dunit spawn/wait; fork-dependent paths gated
src/mman/, src/fs/        native VM/VFS mappings
```

- [ ] Сохранить upstream generic implementations там, где они не зависят от Linux semantics.
- [ ] Не размазывать `#ifdef __dunit__` по всему дереву: вводить небольшие backend interfaces и Dunit translation units.
- [ ] Аудировать прямые `SYS_*`, Linux ioctls, `/proc`, `/dev`, fixed paths, cancellation points и kernel struct layouts.
- [ ] Генерировать `bits/alltypes.h` и public ABI types только после утверждения их размеров.
- [ ] Dunit-specific public functions помещать в отдельные headers/namespace, не загрязнять POSIX имена.

#### M6.6. Фазы реализации

##### M6-A — freestanding bootstrap и static hello

- [ ] Реализовать Dunit `crt1`/`_start`, libc initialization, `main` dispatch и termination.
- [ ] Подключить `exit`, `_Exit`, `write`, минимальный error translation и descriptors `0/1/2`.
- [ ] Собрать static-only musl profile: ISO C string/memory/math/conversion и минимальный stdio path.
- [ ] Проверить constructors/destructors, `.bss`, stack alignment, argc/argv/envp и return from `main`.

**Результат:** статический `hello.c` и программа с arguments. **Готовность:** оба ELF собираются только через Dunit sysroot, загружаются штатным loader и завершаются с проверяемым exit code. **Тесты:** empty args, long args, environment, constructors, invalid user pointers, stdout/stderr. **Риски:** несовместимый entry stack, утечка host headers, ошибочная stack alignment.

##### M6-B — VM, allocator, time и entropy

- [x] Реализовать anonymous/private `mmap`, `munmap`, `mprotect`; alignment 4 KiB, zero-fill, partial unmap и запрет W+X проверены `[VM-TEST] OK`.
- [ ] Адаптировать allocator musl без требования Linux `brk`; optional hints вроде `madvise` могут быть no-op только если контракт это разрешает.
- [ ] Добавить realtime clock и time conversion поверх существующих monotonic clock/blocking sleep; realtime должен иметь явный источник/статус validity.
- [ ] Добавить kernel entropy interface для stack guards и будущей криптографии; слабый PRNG не маркировать как secure.

**Результат:** стабильные `malloc/calloc/realloc/free`, clocks и random seed. **Готовность:** allocator переживает fragmentation/OOM и возвращает корректный `errno`; protection faults детерминированны. **Тесты:** allocator stress, zero/huge allocations, map/unmap/protect boundaries, OOM, monotonicity, entropy availability. **Риски:** VM leaks, use-after-unmap, executable writable memory.

##### M6-C — files, directories и installed environment

- [ ] Покрыть `open/read/write/close/seek`, `stat/fstat`, cwd, directories, rename/unlink/mkdir и `fsync`.
- [ ] Переводить POSIX open/mode flags и Dunit metadata через проверяемый adapter; не передавать public musl structs в kernel.
- [ ] Реализовать buffered `FILE`, standard streams и terminal capability query.
- [ ] Определить Dunit paths для users, configuration, temporary files, DNS и account database; не копировать Linux FHS автоматически.
- [ ] Загружать C programs и libc artifacts с installed root, а не через `include_bytes!`.

**Результат:** C utility работает с DunitFS после установки. **Готовность:** create/write/fsync/reboot/read с тем же hash; stdio error/EOF semantics проходят тесты. **Тесты:** large/empty/sparse-policy files, Unicode bytes in paths, disk-full, rename atomicity, corrupted storage, terminal redirection. **Риски:** несоответствие `stat/dirent`, неполный fsync contract, скрытые fixed paths musl.

##### M6-D — process model без обязательного `fork`

- [ ] Спроектировать `spawn/exec-image/wait/kill` с arguments, environment, cwd и явным inheritance handles.
- [ ] Переписать musl `posix_spawn` backend на прямой Dunit spawn contract, не эмулировать его через Linux `clone/vfork`.
- [ ] Реализовать pipes, descriptor duplication и redirection до заявления shell/process compatibility.
- [ ] Возвращать `ENOSYS` для `fork`, пока нет корректных COW, multithreaded semantics и `pthread_atfork`.

**Результат:** C parent запускает C child и получает exit status. **Готовность:** spawn file actions, environment replacement и pipe redirection работают без fd leaks. **Тесты:** missing executable, permission denial, child fault, concurrent children, inherited/closed handles. **Риски:** deadlocks при spawn, неявная передача privileged handles, попытка приложений обойти отсутствие fork.

##### M6-E — threads, TLS и synchronization

- [ ] Green Tea Kernel создаёт schedulable user threads с отдельными kernel/user stacks и общим address space.
- [x] Поддержать установку/переключение x86_64 `FS.base` и initial static `PT_TLS` image; dynamic thread vector contract остаётся частью будущего dynamic TLS/ldso profile.
- [ ] Реализовать thread create/exit/join/detach и безопасное освобождение stack/TLS после завершения.
- [ ] Реализовать Dunit `wait_on_word/wake` с atomic compare-and-block, timeout и защитой от lost wakeups; не копировать Linux futex ABI.
- [ ] Адаптировать musl pthread create/join, mutex, condvar, rwlock, once и per-thread `errno`.
- [ ] Сначала запретить asynchronous cancellation; cancellation points включать только вместе с формально определённой interruption model.

**Результат:** настоящий pthread subset поверх native Dunit threads. **Готовность:** уникальный TLS/errno у каждого потока, blocked mutex не расходует CPU, join/detach не течёт. **Тесты:** TLS isolation, 1000 create/join cycles, mutex/condvar contention, timeout race, owner death policy, preemption stress, allocator under threads. **Риски:** ABI-зависимый layout musl TCB, lost wakeups, освобождение живого TLS, scheduler/VM lock inversion.

##### M6-F — signals, networking и dynamic linking (отдельные gates)

- [ ] Signals начинать только после Dunit signal/event model: delivery frame, masks, restart/interruption, alt stack и thread targeting.
- [ ] Не заявлять `pthread_cancel`, timers и полноценные signals, пока их скрытые зависимости не проходят race tests.
- [ ] Sockets добавлять после native Dunit network handles; adapter переводит POSIX socket API, но kernel сохраняет собственную object model.
- [ ] Dynamic loader вынести в отдельный milestone: `PT_INTERP`, DSO mapping, relocations, symbol lookup, RELRO, TLS modules, `dlopen/dlsym/dlclose`.
- [ ] До dynamic tier все официальные Dunit C packages собираются static; отсутствие `.so` не считается дефектом M6-A–E.

`fork` не блокирует static-first port: сначала Dunit-native spawn/`posix_spawn`; `fork` реализовать позже через COW только если реальный software demand оправдает сложность.

#### M6.7. Минимальные kernel primitives и порядок появления

| Уровень | Обязательные primitives | Что открывает |
|---|---|---|
| A | exit, write, stable entry ABI, errors | crt, hello, basic stdio |
| B | mmap/munmap/mprotect, clocks, sleep, entropy | malloc, time, guards |
| C | complete file/dir metadata and fsync | stdio, installed C tools |
| D | spawn/exec-image/wait, pipes, dup/inheritance | process utilities |
| E | preemption, thread lifecycle, FS.base/TLS, wait/wake | pthread subset |
| F | events/signals, sockets, DSO VM support | broader POSIX, network, `.so` |

`mmap` должен быть нормальным VM contract, а не allocator-specific syscall. TLS base является состоянием **потока**, а не процесса. `wait_on_word` обязан атомарно проверять значение и усыплять поток. Эти три решения одновременно нужны GUI services и musl, поэтому реализуются в M1 один раз как общие kernel primitives.

#### M6.8. Поддерживаемый libc/POSIX profile

Для каждого release публиковать машинно-читаемый capability manifest и таблицу:

| Профиль | Содержание | Статус первого порта |
|---|---|---|
| ISO C core | strings, memory, math, conversion, stdio subset | must-have |
| Dunit static runtime | crt, errno, allocator, environment, files, clocks | must-have |
| POSIX process subset | spawn, wait, pipes, fd actions | should-have |
| pthread subset | TLS, create/join, mutex/condvar/once | после M1 |
| signals/fork | только после формальной process model | later |
| sockets | после native network stack | later |
| dynamic linking | loader/DSO/TLS modules | separate milestone |

Заявление «musl портирована» запрещено без указания profile/version. M6 завершается на static C + filesystem + pthread subset; broad POSIX и dynamic linking имеют отдельные критерии.

#### M6.9. Тестирование и release gate

- [ ] Добавить `tests/libc/` с маленькими self-checking ELF: startup, args/env, stdio, allocator, files, clocks, spawn, TLS, pthread.
- [ ] Запускать все kernel/OS тесты только через `tools/qemu_test.py`; каждый тест выдаёт стабильный serial marker и exit status.
- [ ] Подключить релевантный subset `libc-test`; unsupported cases перечислять с причиной и владельцем, а не молча исключать.
- [ ] Добавить negative tests: invalid pointers, invalid UTF-8-neutral path bytes, bad fds, OOM, interrupted waits, child crash.
- [ ] Проверять ELF статически: target machine, program headers, executable stack, unexpected `NEEDED`/`INTERP`, unresolved symbols.
- [ ] Для каждого ABI change пересобирать весь sysroot и все C apps; бинарная совместимость проверяется отдельным previous-release suite после ABI v1.
- [ ] Разделить gates: single-thread static, filesystem/persistence, pthread/preemption, network, dynamic.

**Общий результат M6:** native Dunit-linked C programs и SDK без Linux kernel ABI. **Критерий завершения:** reproducible `x86_64-dunit` sysroot; static hello/args/env/file/malloc/spawn/thread suites проходят в QEMU; persistence case переживает power cycle; unsupported POSIX surface документирован. **Главные риски:** скрытые Linux assumptions, слишком ранняя заморозка ABI, некорректный TLS, попытка одновременно реализовать signals/fork/ldso. **Меры:** static-first, capability profiles, узкий adapter, фазовые test gates и контролируемый upstream delta.

### M7 — networking, platform services и ecosystem (later, часть можно параллельно после M1)

#### M7.1. Честное текущее состояние networking

`kernel/src/drivers/net.rs` сейчас выполняет только PCI discovery: распознаёт E1000/RTL8139/legacy VirtIO IDs, для E1000 временно отображает MMIO, читает status и MAC, затем снимает mapping. RX/TX descriptors, packet buffers, interrupts, Ethernet frames и protocol stack отсутствуют; serial log прямо печатает `packet-io=not-implemented` и `stack=not-implemented`. `userspace/system_apps/gui_stats` также корректно показывает `Network (discovery only)`. Поэтому наличие `net0` в registry не считается сетевой поддержкой.

Первый reference target — **E1000 в QEMU**. VirtIO-net добавляется после доказанного end-to-end packet path и modern VirtIO transport; RTL8139 остаётся later/compatibility, а не параллельным отвлечением.

#### M7.2. Целевая архитектура: driver в kernel, stack в `netd`

```text
applications / browser network service / package client
                    |
          native Dunit socket protocol
                    |
             libdunit / musl adapter
                    |
          IPC + readiness event handles
                    |
        netd (userspace network service)
  Ethernet, ARP/NDP, IP, ICMP, UDP, TCP, DHCP, DNS
                    |
      shared bounded RX/TX packet queues
                    |
 Green Tea Kernel NIC driver + IRQ + DMA/IOMMU policy
                    |
              E1000 / VirtIO-net
```

Green Tea Kernel отвечает только за PCI/MMIO, DMA-safe memory, NIC reset/configuration, IRQ moderation, bounded descriptor rings, packet ownership и capability-gated transfer в `netd`. IP, TCP, DNS и policy не должны разрастаться внутри ядра. `netd` — supervised privileged service; приложения не получают MMIO, DMA buffers или raw Ethernet по умолчанию.

- [ ] Добавить `NET_DEVICE`/`PACKET_QUEUE` handles с rights `RX`, `TX`, `CONTROL`, `RAW`, `TRANSFER`.
- [ ] Передавать пакеты между driver и `netd` через bounded shared rings и event handles, без syscall на каждый byte.
- [ ] Зафиксировать ownership state каждого buffer: `DRIVER_RX -> NETD -> FREE` и `APP/NETD_TX -> DRIVER -> COMPLETE`.
- [ ] При падении `netd` kernel останавливает queue, отзывает mappings, сбрасывает NIC и позволяет service manager запустить его заново.
- [ ] Raw packet capability выдавать только diagnostics/network services; обычное приложение получает socket endpoint с ограниченными правами.
- [ ] Конфигурацию интерфейсов и policy хранить вне kernel; driver не знает DHCP, DNS, routes или proxy.

**Цель:** модульный native stack без Linux network ABI. **Причина:** TCP/DNS в kernel усложнят recovery, безопасность и независимое обновление. **Подсистемы:** PCI, VMM/DMA, interrupts, IPC/shared memory, handles, init, `netd`. **Зависимости:** M1 handles/events/shared VM и service supervision. **Результат:** перезапускаемый userspace stack над узким packet I/O backend. **Готовность:** crash/restart `netd` не повреждает kernel и возвращает интерфейс в рабочее состояние. **Риски:** копирование и context-switch overhead; решать shared rings/batching, не переносом всего stack в kernel.

#### M7.3. N0 — E1000 packet I/O foundation

- [ ] Реализовать hardware reset, EEPROM/RAL-MAC selection, link state и documented register initialization.
- [ ] Выделять физически пригодные DMA regions через общий DMA API; не использовать произвольные virtual pointers как bus addresses.
- [ ] Реализовать RX/TX descriptor rings, head/tail management, buffer recycling и memory barriers.
- [ ] Настроить MSI/MSI-X при наличии, с fallback на legacy interrupt; polling разрешён только как диагностический режим.
- [ ] Добавить interrupt moderation/NAPI-like bounded drain без копирования Linux API.
- [ ] Обработать link up/down, queue stall, malformed/oversized frames, ring exhaustion и reset recovery.
- [ ] Публиковать counters: packets/bytes, drops by reason, checksum errors, queue full, resets, link transitions.

**Результат:** driver отправляет и принимает реальные Ethernet frames. **Готовность:** двунаправленный frame loop через QEMU network backend; 10-minute flood не зависает и не течёт. **Тесты:** ring wrap, RX starvation, TX completion, link toggle, invalid length, interrupt storm, forced reset. **Риски:** DMA corruption и races; guard regions, ownership assertions и bounded descriptors обязательны.

#### M7.4. N1 — link/network layers: Ethernet, ARP, IPv4 и ICMP

- [ ] Ethernet II parser/builder: destination/source MAC, EtherType, minimum/maximum frame size и padding.
- [ ] Drop неизвестных/повреждённых frames до глубокого parsing; каждый length проверять до чтения header.
- [ ] ARP cache со states `INCOMPLETE/REACHABLE/STALE`, bounded entries, retry/expiry и защита от бесконтрольного poisoning.
- [ ] IPv4 validation: version/IHL, total length, header checksum, TTL, protocol, source/destination и interface ownership.
- [ ] Реализовать routing table с connected/default routes и longest-prefix match; не зашивать один gateway в код.
- [ ] Зафиксировать fragmentation policy: v1 может не фрагментировать TX, но обязан корректно вернуть MTU error; RX reassembly — bounded по bytes/fragments/time либо явно later.
- [ ] ICMPv4 echo request/reply и необходимые destination-unreachable/time-exceeded сообщения; rate limiting обязателен.
- [ ] Реализовать loopback как логический interface без обращения к физическому NIC.

**Результат:** статически настроенный Dunit host отвечает на ping и пингует gateway. **Готовность:** ARP resolution + ICMP echo проходят при cache miss/hit/expiry; malformed packets не валят `netd`. **Тесты:** bad checksum/length/IHL, TTL zero, unknown EtherType/protocol, ARP timeout, route miss, MTU boundary, packet fuzzing. **Риски:** parser vulnerabilities и unbounded reassembly; использовать checked cursor API и жёсткие budgets.

#### M7.5. N2 — UDP, DHCPv4 и DNS resolver

- [ ] UDP demultiplexing по local address/port, ephemeral port allocation, checksum и bounded receive queues.
- [ ] Native datagram endpoint: bind/connect/send-to/receive-from, nonblocking mode, timeout и readiness events.
- [ ] DHCPv4 state machine `INIT -> SELECTING -> REQUESTING -> BOUND -> RENEWING/REBINDING`, lease timers и link-change reset.
- [ ] Валидировать DHCP offers/options; применять address, prefix, gateway и DNS atomically, с rollback на ошибке.
- [ ] Сохранять lease только как optimization; после reboot/link change проверять его заново и не считать вечным.
- [ ] DNS stub resolver: A/AAAA/CNAME, UDP query IDs, compression-pointer bounds, retry/timeout, bounded TTL cache и TCP fallback для truncated replies.
- [ ] Источники конфигурации: static system profile, DHCP и user/admin override с явно определённым precedence.

**Результат:** интерфейс получает адрес автоматически, разрешает имя и обменивается UDP datagrams. **Готовность:** DNS lookup не требует hardcoded server; lease renew и timeout корректны. **Тесты:** DHCP loss/NAK/renew, duplicate address response, DNS malformed compression, NXDOMAIN, timeout/retry/cache expiry, UDP queue overflow. **Риски:** spoofing и parser loops; transaction validation, budgets и privileges.

#### M7.6. N3 — TCP transport

- [ ] Реализовать отдельную TCP state machine для active/passive open и states от `CLOSED` до `TIME_WAIT`.
- [ ] Проверять 4-tuple, sequence/acknowledgement numbers, flags, header length, checksum и receive window.
- [ ] Реализовать SYN retransmission, data retransmission, RTT/RTO estimator, duplicate ACK handling и bounded retry policy.
- [ ] Добавить sliding send/receive windows, out-of-order queue с memory limit, FIN/RST handling и half-close.
- [ ] Начать с простого проверяемого congestion control (например, Reno-подобного); congestion control не может отсутствовать в Internet-facing release.
- [ ] Поддержать listen backlog, accept queue, ephemeral ports, `SO_REUSEADDR`, keepalive policy и `TCP_NODELAY`; редкие options later.
- [ ] Все timers вести от monotonic clock и обрабатывать wrap/large jumps детерминированно.

**Результат:** надёжный byte stream для локальных и внешних соединений. **Готовность:** клиент и сервер передают multi-megabyte stream с искусственными loss/reorder/duplication; connection teardown не течёт. **Тесты:** handshake timeout, simultaneous close, reset, zero window, retransmission, reordered segments, TIME_WAIT pressure, SYN/backlog exhaustion. **Риски:** сложность TCP и resource exhaustion; state/property tests, per-socket/global quotas и сначала один congestion algorithm.

#### M7.7. N4 — native Dunit socket API и event model

Native API является service protocol/object model, а не копией Linux syscall table:

```text
net.open(family, type, protocol) -> socket handle
net.bind(handle, endpoint)
net.connect(handle, endpoint)
net.listen(handle, backlog)
net.accept(handle) -> socket handle + peer
net.send/receive(handle, buffers, flags)
net.send_to/receive_from(handle, endpoint, buffers, flags)
net.shutdown(handle, direction)
net.get/set_option(handle, typed option)
resolver.lookup(name, family, flags) -> address list
event.wait([socket readiness, IPC, timer, file], deadline)
```

- [ ] Версионировать socket/resolver protocol независимо от TCP implementation.
- [ ] Представлять IPv4/IPv6 endpoints typed structures, не строками внутри protocol.
- [ ] Определить partial I/O, EOF, reset, timeout, would-block и cancellation semantics.
- [ ] Интегрировать readiness в общий Dunit event wait; не добавлять busy polling в GUI/browser.
- [ ] Разделить права `CONNECT`, `BIND_LOW_PORT`, `LISTEN`, `RAW`, `CONFIGURE_INTERFACE` и `OBSERVE_ALL`.
- [ ] Добавить per-process quotas на sockets, queued bytes, DNS requests и listening backlog.
- [ ] Поддержать capability revocation и deterministic errors при рестарте `netd`.

**Результат:** Rust/Dunit-native приложения используют сеть без POSIX и без доступа к NIC. **Готовность:** echo client/server работают через public `libdunit` API; один зависший клиент не блокирует `netd`. **Тесты:** readiness edge cases, close during wait, partial send, quota denial, handle transfer, service restart. **Риски:** premature ABI freeze; сначала versioned protocol + conformance suite.

#### M7.8. N5 — подключение к Dunit musl

```text
POSIX C API (`socket`, `connect`, `send`, `poll`, `getaddrinfo`)
        -> Dunit musl adapter
        -> libdunit native socket/resolver protocol
        -> netd
```

- [ ] Реализовать musl mappings для `socket/bind/connect/listen/accept`, send/receive family, `shutdown` и close.
- [ ] Переводить POSIX `sockaddr`, flags/options и errors в versioned Dunit types; kernel и `netd` не принимают Linux structs.
- [ ] Реализовать blocking/nonblocking behaviour поверх readiness events, включая timeout и interruption policy.
- [ ] Подключить `poll`/`pselect`-подобный libc слой к общему Dunit event wait, чтобы socket, pipe, IPC и timer ожидались вместе.
- [ ] Перенести `getaddrinfo/getnameinfo` на resolver protocol; не заставлять каждое приложение самостоятельно разбирать DNS.
- [ ] Документировать supported socket options и возвращать `ENOPROTOOPT` для неподдерживаемых, а не fake success.

**Результат:** одна и та же native сеть обслуживает Dunit-native и C/POSIX applications. **Зависимости:** M6 static runtime + N4 API. **Готовность:** статические C echo/DNS/HTTP clients проходят без Linux ABI. **Тесты:** POSIX error mapping, IPv4/IPv6 address conversion, nonblocking connect, poll timeout/readiness, resolver concurrency. **Риски:** расхождение POSIX и native semantics; contract tests запускаются против обоих APIs.

#### M7.9. N6 — TLS, HTTP и путь к GUI-браузеру

TLS и HTTP не помещаются в kernel или NIC driver. Они работают в отдельном userspace network service браузера либо в общей high-level библиотеке:

```text
browser-ui (GUI client)
    |
browser content processes
    |
browser-network-service
  URL policy, proxy, cache, cookies, HTTP, TLS, certificates
    |
musl sockets или native Dunit socket API
    |
netd -> NIC driver
```

- [ ] До TLS предоставить cryptographically secure entropy, корректное realtime clock и обновляемое read-only certificate store.
- [ ] Портировать проверенную TLS/crypto library; не писать собственную криптографию как часть network stack.
- [ ] Ввести trust-store update/version/rollback и hostname/certificate validation tests.
- [ ] Реализовать HTTP/1.1 client с redirects, chunked/content-length framing, compression limits и connection reuse; HTTP/2 позже поверх доказанного TLS/TCP.
- [ ] Вынести browser networking в отдельный sandboxed process; renderer/content process не получает unrestricted sockets.
- [ ] Network service применяет origin/proxy/download policy, quotas и передаёт браузеру responses через IPC/shared buffers.
- [ ] Persistent browser cache/cookies живут в user data directories и используют size limits/atomic updates; это не состояние `netd`.
- [ ] GUI Browser является обычным GUI client: создаёт surfaces через GUI Server и не получает framebuffer/input/network master capabilities.
- [ ] Сначала сделать headless HTTP fetch + minimal GUI response viewer; полноценный HTML/CSS/JS browser engine — отдельный поздний проект.

**Результат:** архитектура сети изначально пригодна для браузера, но browser engine не блокирует network stack. **Готовность первого browser-network slice:** GUI-приложение просит sandboxed network service загрузить HTTPS resource, получает body/status через IPC и отображает результат через GUI Server. **Тесты:** invalid/expired/wrong-host certificate, redirect loop, truncated/chunked response, decompression bomb limit, cache corruption, network-service crash/restart. **Риски:** огромный security surface; process isolation, established crypto/TLS, strict parsers и no ambient network access.

#### M7.10. N7 — IPv6 и multi-interface (после стабильного IPv4 vertical slice)

- [ ] Ethernet multicast, IPv6 header validation, ICMPv6, Neighbor Discovery и Duplicate Address Detection.
- [ ] Link-local addresses, SLAAC, router advertisements и DNS configuration; DHCPv6 только по подтверждённой необходимости.
- [ ] IPv6 UDP/TCP через тот же transport/socket API, без отдельного application path.
- [ ] Несколько interfaces, per-interface addresses/routes/DNS, route metrics и link failover.
- [ ] Happy Eyeballs policy реализовать в resolver/client layer после стабильного dual-stack.

**Готовность:** IPv4-only, IPv6-only и dual-stack tests используют одни application APIs. **Риски:** преждевременное удвоение scope; packet/address abstractions проектируются dual-stack заранее, реализация IPv6 следует после N1–N6.

#### M7.11. Network configuration, observability и security

Предлагаемое состояние:

```text
/system/share/network/defaults.toml       immutable defaults
/system/services/netd.toml                service manifest/capabilities
/users/<uid>/config/network/              allowed user preferences
/var/lib/netd/                            leases and bounded service state
/var/log/netd/                            bounded/rotated diagnostics
/run/netd/                                volatile endpoints/status
```

- [ ] Schema-versioned static/DHCP configuration с validate -> stage -> atomic apply -> rollback.
- [ ] Status API: interfaces, addresses, routes, DNS, link state и counters без раскрытия чужого traffic.
- [ ] Diagnostic tools: `ip`-подобный status, route, ping, DNS lookup, TCP connect и privileged bounded packet capture.
- [ ] Default deny для raw sockets, interface configuration и low ports; application manifests запрашивают network scopes.
- [ ] Ограничить packet sizes, fragment queues, socket buffers, DNS cache, TCP states и log volume.
- [ ] Все parsers fuzz/property-test; входной packet никогда не приводит к panic всего `netd`, kernel или desktop.
- [ ] Не заявлять firewall/VPN как готовые до появления routing/hooks/key storage; предусмотреть policy hooks без ранней реализации сложного framework.

#### M7.12. Network milestone order и acceptance matrix

```text
N0 E1000 RX/TX
 -> N1 Ethernet/ARP/IPv4/ICMP
 -> N2 UDP/DHCP/DNS
 -> N3 TCP
 -> N4 native sockets/events
 -> N5 musl sockets
 -> N6 TLS/HTTP/browser-network slice
 -> N7 IPv6/multi-interface
```

N4 API проектируется параллельно N1–N3, но фиксируется только после работающих semantics. N5 musl adapter и N6 HTTP tests можно разрабатывать против host mock/reference server, затем переключить на `netd` conformance endpoint.

Обязательная автоматическая матрица через `tools/qemu_test.py`:

- QEMU E1000: link/reset, ARP, ping, UDP, DHCP, DNS, TCP client/server;
- deterministic isolated test network, без зависимости CI от публичного Internet;
- loss/reorder/duplicate/corruption/latency injection;
- malformed packet corpus и bounded fuzz runs;
- `netd` crash/restart и NIC reset во время активных sockets;
- static musl C socket client рядом с native Rust client;
- HTTPS test против локального deterministic server и тестового trust root;
- GUI response viewer через GUI Server без прямых network/display privileges;
- soak: множество connect/close, DNS cache expiry, TCP retransmission и memory/handle leak accounting.

**Критерий завершения networking v1:** E1000 выполняет реальный packet I/O; `netd` получает DHCP address, резолвит DNS и держит TCP stream; native и static-musl clients проходят одинаковые conformance cases; локальный HTTPS resource загружается через sandboxed browser-network service; reboot сохраняет только разрешённую configuration/state; malformed traffic и падение `netd` не рушат kernel/GUI. Публичный Internet smoke допустим как optional manual/release test, но не заменяет deterministic CI.

#### M7.13. Остальные platform services

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

M6 фиксирует полный musl fork как основной путь, Dunit-native syscall adapter и static-first порядок. Работа начинается не с `printf`, а с versioned userspace ABI, process-entry contract, воспроизводимого sysroot и custom `crt1`. Затем последовательно вводятся bootstrap, VM/allocator, filesystem, spawn и pthread/TLS. Критический kernel path: `mmap/munmap/mprotect -> preemption -> thread lifecycle -> FS.base/TLS -> atomic wait/wake -> pthread subset`.

Практический первый deliverable — команда `x86_64-dunit-cc -static hello.c -o hello`, создающая ELF без host/Linux dependencies. Финальный deliverable M6 — документированный static libc profile с files, process spawn и pthread subset. Signals, `fork` и dynamic loader не входят в первый порт; их семантика проектируется только по подтверждённому demand. POSIX headers/API являются source-compatibility facade над Dunit ABI.

Upstream-ориентиры для реализации и проверки assumptions:

- [musl supported platforms](https://wiki.musl-libc.org/supported-platforms) — подтверждает ориентацию upstream на Linux syscall layer;
- [musl porting notes](https://wiki.musl-libc.org/porting) — архитектурные porting resources и рекомендация `libc-test`;
- [musl getting started](https://wiki.musl-libc.org/getting-started) — static-only build через `--disable-shared` как штатный режим.

Эти материалы не задают архитектуру Dunit: Dunit-specific OS layer, ABI profiles, process model и kernel primitives определяются данным roadmap и отдельной ABI-спецификацией.

## 20. Критерии завершения каждого этапа

- **M0:** status claims трассируются к test marker и исходнику.
- **M1:** preemption/thread/TLS/wait-wake stress проходит без races и leaks.
- **M2:** protocol conformance/fuzz suite детерминированно зелёная.
- **M3:** userspace GUI Server композитит два изолированных клиента и переживает их crash.
- **M4:** feature parity и config reload без kernel GUI code.
- **M5:** BIOS/UEFI installed root + content/config persistence + recovery test.
- **M6:** static C hello/file/malloc/thread suite на Dunit musl fork.
- **M7 networking v1:** E1000 packet I/O -> `netd` DHCP/DNS/TCP -> native sockets -> static-musl client -> local HTTPS browser-network slice проходит end-to-end; остальные services имеют собственные I/O и permission gates, не только discovery.

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
                                      `-> M7 netd/packages/services
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
- network: E1000/link -> ARP/ICMP -> DHCP/DNS/UDP/TCP -> native/musl sockets -> local TLS/HTTP; loss/reorder/malformed traffic и `netd` restart;
- browser-network: sandbox/capability denial, test trust root, certificate/redirect/framing/cache failures, GUI response viewer через GUI Server;
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
  init/  services/gui-server/  services/inputd/  services/netd/  services/audiod/
  services/browser-network/  dwm/  ui-runtime/  shell/  terminal/  apps/  libdunit/
protocols/
  gui-v1/  net-v1/  resolver-v1/  service-v1/  package-v1/
filesystems/dunitfs/
toolchains/
  dunit-musl/  sysroot/  wrappers/  cmake/  meson/
images/minimal/  images/dwm/  images/live/
configs/defaults/
tests/qemu/  tests/protocol/  tests/persistence/  tests/libc/
tools/qemu_test.py
```

Полное дерево musl живёт в отдельном fork-репозитории `dunit-musl`; `toolchains/dunit-musl/` фиксирует конкретный commit этого fork. Сгенерированный sysroot не хранится как непрозрачный binary blob: он воспроизводимо собирается из pinned compiler, musl source и Dunit ABI headers.

## 29. Дальнейшее развитие проекта

После M6/M7: стабилизировать SDK и ABI release cadence, добавить signed app bundles, accessibility/text shaping/IME, hardware compatibility tiers, reproducible images и upgrade rollback. GPU acceleration, SMP, dynamic linking, suspend/resume и broad POSIX coverage развиваются независимо и не должны блокировать надёжную UP software-composited desktop v1.

## 30. Итоговая целевая архитектура Dunit Desktop OS

Завершённая архитектура — не Linux distribution и не GUI внутри kernel. Green Tea Kernel предоставляет собственные Dunit objects, ABI, scheduling, VM, IPC, VFS и device primitives. `init` запускает userspace services. GUI Server единолично владеет display/input handles и композитит shared surfaces. Независимый Dunit DWM задаёт desktop policy через Dunit UI Runtime и versioned DUI/DSS/TOML. Kernel NIC drivers отдают bounded packet queues отдельному `netd`; native и musl socket APIs сходятся на одном versioned network protocol. Будущий browser использует sandboxed browser-network service для TLS/HTTP и остаётся обычным GUI client без raw display/network privileges. Установленная система грузит programs/config/data с проверяемой DunitFS, а Dunit musl fork предоставляет source portability поверх Dunit ABI.

Ближайший правильный порядок: **M0 honest baseline -> M1 preemption/threads/blocking IPC/shared VM -> M3 GUI Server vertical slice**, параллельно **M2 protocol tests** и **M5 DunitFS/install v2**; затем **M4 DWM parity** и **M6 static-first musl**. Это сохраняет видимый GUI прогресс, но не цементирует сегодняшние kernel hardcodes.
