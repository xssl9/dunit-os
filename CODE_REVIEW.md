# Dunit OS — Code Review

Полное ревью проекта: костыли, затычки, явные ошибки, slop и блокеры дальнейшего
развития ОС. Каждый пункт — под исправление, отмечен `[ ]`.

Отметки серьёзности:
- 🔴 **BLOCKER** — мешает дальнейшему развитию (SMP, стабильность, корректность памяти).
- 🟠 **BUG** — явная ошибка/дефект корректности.
- 🟡 **HACK / затычка** — работает, но временно/неправильно спроектировано.
- ⚪ **SLOP** — мусор, мёртвый код, обман в логах, копипаста.

Легенда файлов сверялась с состоянием репозитория на момент ревью.

---

## 🔴 Блокеры (архитектурные, мешают развитию)

- [ ] **Алиасинг `PROCESS_TABLE` → UB и блокер SMP.**
  `kernel/src/process/mod.rs`: `static mut PROCESS_TABLE: Option<Vec<ProcessRecord>>`
  раздаётся многими функциями без синхронизации. `current_process()` возвращает
  `&'static Process`, `current_process_mut()` — `&'static mut Process`, ссылаясь
  внутрь `Vec`, который в другом месте (`insert_process_record`) делает `push`.
  При реаллокации `Vec` эти ссылки становятся висячими → UB. Это же — фундаментальный
  блокер для SMP. Нужно: индексная адресация вместо долгоживущих ссылок + примитив
  синхронизации (spinlock/`RwLock`), хранение процессов в стабильных слотах.

- [ ] **Гонка `timer_preempt_save_and_schedule` из IRQ-контекста.**
  `kernel/src/process/mod.rs`: мутирует `PROCESS_TABLE` прямо из обработчика таймера,
  без блокировки, в то время как syscall-путь тоже её мутирует. Data race.
  (Сейчас замаскировано тем, что `PREEMPTION_ENABLED = false`, но это и есть причина,
  по которой преемпшн нельзя включить.)

- [x] **Нет page-fault recovery при копировании user-памяти → краш ядра.**
  `kernel/src/syscall/mod.rs`: `copy_buffer_from_user` / `copy_buffer_to_user` /
  `copy_string_from_user` делают сырые `read_volatile`/`write_volatile` по
  пользовательскому указателю. Если страница не отображена — падает само ядро,
  а не процесс. `is_valid_user_pointer` лишь проверяет диапазон
  (`USER_SPACE_START..USER_SPACE_END`), но не проверяет, что страница реально
  замаплена. Нужно: проверять маппинг через `AddressSpace`/таблицы страниц перед
  доступом, либо обрабатывать #PF при копировании.

- [ ] **Лок-фри доступ к глобальному состоянию ядра.**
  Множество подсистем используют `static mut ... : Option<T>` без синхронизации и
  раздают `&'static mut`:
  - `kernel/src/process/scheduler.rs`: `static mut SCHEDULER_INSTANCE`.
  - `kernel/src/fs/vfs.rs`: `static mut VFS_INSTANCE`, `static mut ROOT_MEMFS`,
    `static mut VFS_PATH_BUFFER` (общий буфер пути — реентерабельность/гонки).
  - `kernel/src/memory/vmm.rs`: `static mut HHDM_OFFSET`, `static mut VMM_INSTANCE`.
  - `kernel/src/drivers/net.rs`: `static mut NET_SNAPSHOT`.
  - `kernel/src/drivers/ahci.rs`: `static mut DISKS`.
  Пока однопоточно и кооперативно — «работает», но это системный блокер для SMP и
  преемпшна. Нужна единая стратегия синхронизации.

- [ ] **Аллокатор кучи не PMM-backed и не растёт.**
  `kernel/src/allocator.rs`: фиксированный `static mut KERNEL_HEAP: [u8; 2*1024*1024]`
  в BSS. Куча не может вырасти; при исчерпании — отказ аллокаций. Для растущего числа
  процессов/ФС это потолок. Нужно: backing кучи через PMM с возможностью расширения.

- [x] **PMM использует только один регион памяти.**
  `kernel/src/memory/pmm.rs`: битмап-аллокатор берёт только самый большой usable-регион
  из memory map, игнорируя остальные usable-регионы → теряется доступная RAM. Нужно:
  учитывать все usable-регионы.

- [x] **MMIO-маппинг не попадает в уже созданные адресные пространства.**
  `kernel/src/memory/vmm.rs`: добавлен канонический kernel root. Новые `AddressSpace`
  копируют его верхнюю половину, каждое переключение CR3 синхронизирует её повторно,
  а `map_mmio_region` всегда обновляет канонический root и текущий активный root.

- [ ] **Ключевые syscalls возвращают ENOSYS.**
  `kernel/src/syscall/mod.rs`: `sys_fork`, `sys_exec`, `sys_kill_process`, `sys_mmap` —
  заглушки `ENOSYS`. Без `exec`/`mmap` полноценный userspace (в т.ч. userspace-шелл из
  роадмапа) невозможен. Это функциональные блокеры.

---

## 🟠 Явные ошибки (bugs)

- [x] **`sys_sleep` — busy-wait с риском переполнения.**
  `kernel/src/syscall/mod.rs`: `sys_sleep(ms)` = `for _ in 0..ms*1000 { pause }`.
  Жжёт CPU целиком, не отдаёт управление, `ms*1000` может переполниться, а длительность
  никак не привязана к реальному времени. Нужно: сон через таймер/расписание, отдачей
  управления планировщику.

- [x] **`copy_string_from_user` трактует байты как Latin-1, а не UTF-8.**
  `kernel/src/syscall/mod.rs`: `out.push(byte as char)` — некорректно для многобайтового
  UTF-8 (каждый байт становится отдельным codepoint). То же в userspace:
  `userspace/libdunit/src/lib.rs` — `read_line`/`read_to_string` (`out.push(*byte as char)`).
  Нужно: собирать `Vec<u8>` и валидировать через `from_utf8`.

- [x] **`MemFs::readdir` молча обрезает список до 32 записей.**
  `kernel/src/fs/memfs.rs`: `readdir` использует фиксированный буфер `[DirEntry; 32]`,
  хотя результат отдаётся `Vec`. В `/app` уже ~33 бинарника + ассеты → часть записей
  теряется. Нужно: динамический сбор без фиксированного лимита.

- [x] **Возможная ошибка индексации дисков в `register_disks`.**
  `kernel/src/drivers/ahci.rs`: при заполнении `DISKS` в `bring_up_controller` индекс —
  `AHCI_DISK_COUNT + added`, но `AHCI_DISK_COUNT` инкрементируется только ПОСЛЕ возврата
  из `bring_up_controller`. `register_disks` затем читает `DISKS[0..count]`. При нескольких
  контроллерах/частичных сбоях индексация «слотов» и итоговый count могут разойтись.
  Нужно: единый источник истины по занятым слотам.

- [x] **`memory::init()` молча продолжает при отказе PMM.**
  `kernel/src/memory/mod.rs`: если `pmm::init()` не удался, `init()` тихо возвращается,
  оставляя кучу неинициализированной → падение на первой аллокации. Нужно: явная ошибка
  и остановка/паника с диагностикой.

- [x] **`Process::new()` тихо деградирует без адресного пространства.**
  `kernel/src/process/mod.rs`: при неудаче создания `AddressSpace` молча вызывает
  `new_without_address_space`. Процесс «создан», но без изоляции памяти — скрытый
  неконсистентный статус. Нужно: возвращать ошибку, а не молчаливый фолбэк.

- [x] **Утечка при выравнивании в аллокаторе.**
  `kernel/src/allocator.rs`: `alloc` возвращает выровненный адрес, но `dealloc` трактует
  указатель как начало блока. Padding-байты до выровненного адреса теряются
  (не возвращаются во free-list). Нужно: хранить/восстанавливать реальное начало блока.

- [x] **`uptime_ticks` / `uptime_available` захардкожены в 0.**
  `kernel/src/syscall/mod.rs`: в `SystemStats` поля времени всегда 0 → `dtop`/`dufetch`
  показывают неверный аптайм. Нужно: реальный счётчик тиков таймера.

- [x] **Опечатка в README.**
  `README.md:19`: `"...GUI Mode still available.ч"` — лишняя кириллическая «ч».

---

## 🟡 Костыли и затычки

- [x] **Обманный boot-лог (фейковые данные).**
  `kernel/src/lib.rs`: `kernel_main` печатает неизмеряемые строки как факты:
  `"[ OK ] CPU features: SSE, SSE2, AVX available"`, `"Memory: 512MB RAM detected"`,
  `"Window manager: 5 applications registered"`, `"7 processes running"`. Ни одно из
  значений не измеряется. Нужно: либо измерять реально, либо убрать.

- [x] **Встроенный smoke-тест syscall больше не попадает в production.**
  Inline-asm payload, его syscall, стек, статика и helper-код закрыты
  feature-флагом `boot-smoke-tests`. Makefile включает его только для
  `limine_test_terminal.conf` и `limine_test_gui.conf`; production `limine.conf`
  собирается без smoke-кода и маркеров.

- [x] **`ADDRSPACE-TEST` smoke убран из production-пути.**
  `run_address_space_smoke`, процессные smoke-тесты и их вызовы также
  собираются только с `boot-smoke-tests`.

- [x] **Дублирование логики page-table walk.**
  Ручные обходчики удалены из ELF и syscall-модулей. Процессный userspace использует
  `AddressSpace`, а ограниченные операции раннего ring-3 smoke-теста и kernel/MMIO
  mapping централизованы в `memory::vmm`.

- [x] **Два параллельных пути загрузки ELF.**
  Оставлен один production-путь: `prepare_process_elf` →
  `load_into_process_address_space` → `AddressSpace`. Ручной current-CR3 loader и
  неиспользуемый `ElfLoader` удалены.

- [x] **`VirtualMemoryManager` — мёртвый/сломанный путь.**
  Неинициализируемый `VMM_INSTANCE`, `get_vmm` и весь легаси-тип удалены. Единственный
  API адресных пространств — HHDM-aware `AddressSpace` и функции `memory::vmm`.

- [x] **MMIO-виртуальный диапазон переиспользуется.**
  Bump заменён на bounded allocator с учётом allocations, `unmap_mmio_region`,
  возвратом и coalescing свободных extent-ов. Временные net/xHCI-маппинги
  освобождаются; feature-smoke проверяет повторную выдачу того же адреса.

- [x] **`terminal_cwd()` мутирует состояние в геттере.**
  `kernel/src/lib.rs`: функция-геттер изменяет `static mut TERMINAL_CWD`. Побочный
  эффект в геттере — источник трудноуловимых багов.

- [x] **Захардкоженный автокомплит из 22 команд.**
  `kernel/src/lib.rs`: массив команд для автодополнения задан вручную и расходится с
  реальным набором команд. README сам отмечает цель «сделать команды менее
  kernel-hardcoded». Нужно: единый реестр команд.

- [x] **`scancode_to_char` обрабатывает модификаторы и символы.**
  Декодер PS/2 Set-1 хранит Left/Right Shift и Caps Lock, игнорирует break-коды,
  выдаёт верхний/нижний регистр и полный базовый US-набор знаков.

- [x] **Userspace allocator освобождает и переиспользует память.**
  Фиксированные 64 KiB и no-op `dealloc` заменены на синхронизированный free-list
  allocator с split/coalescing. Он растёт страницами через реальный private-anonymous
  `mmap`; ядро валидирует флаги, адреса и коллизии, а при ошибке откатывает
  частично выделенные страницы. Это база для будущей musl libc.

- [x] **Копипаста цели `userspace:` в Makefile удалена.**
  Единые `USERSPACE_APPS`/`USERSPACE_CARGO_FLAGS` и fail-fast цикл собирают и
  копируют все приложения.

- [x] **`sys_kill_process` = ENOSYS, но `libdunit::kill` присутствует.**
  Userspace выставляет `kill()` (`userspace/libdunit/src/lib.rs`), которого ядро не
  поддерживает — тихий no-op/ошибка для вызывающего. Согласовать ABI.

---

## ⚪ Slop, мёртвый код, шум

- [x] **`kernel/src/main.rs` — мёртвая заглушка.**
  Оротанный файл: VGA-текст `"DUNIT OS WORKS!"` и пустой `#[panic_handler] fn panic()
  -> ! { loop {} }`. Реальная точка входа — `kernel_main` в `lib.rs`. Файл путает.
  Удалить.

- [x] **Четыре копии таблиц глиф-битмапов в `lib.rs`.**
  `kernel/src/lib.rs`: одинаковые `match`-таблицы глифов продублированы в
  `draw_text_direct`, `draw_colored_text`, `draw_error_text_old`, внутреннем
  `draw_text` и `draw_char`. Нужно: единая таблица шрифта.

- [x] **Мёртвые функции отрисовки.**
  `kernel/src/lib.rs`: `draw_error_text_old`, `draw_char`, `draw_text`, `draw_window`
  не используются (или дублируют активные пути). Удалить.

- [x] **Отладочный серийный спам TERM-001..007.**
  `kernel/src/lib.rs`: маркеры `TERM-001`..`TERM-007` в серийный порт. Убрать/спрятать
  за debug-флагом.

- [x] **Busy-wait задержки `for _ in 0..500000 { pause }`.**
  `kernel/src/lib.rs`: «магические» циклы ожидания. Заменить на таймер/явную задержку.

- [x] **Дублирование `serial_write`.**
  `serial_write` объявлен/дублирован в нескольких местах (`kernel/src/lib.rs`,
  `kernel/src/memory/mod.rs`, `kernel/src/fs/vfs.rs` через `extern`). Свести к одному
  модулю логирования.

- [x] **Тяжёлое серийное логирование на горячих путях процессов.**
  `kernel/src/process/mod.rs`: обильные `serial_write` на путях планирования/переключения.
  Замедляет и зашумляет. Спрятать за уровнем логирования.

- [x] **Дублированные `write_hex`/`write_dec`/`write_mac` по драйверам.**
  `kernel/src/drivers/ahci.rs`, `kernel/src/drivers/net.rs` содержат собственные копии
  хелперов форматирования. Вынести в общий util.

- [x] **`net_*` поля в `SystemStats`/dufetch при отсутствии стека.**
  `kernel/src/drivers/net.rs` — только discovery (`stack=not-implemented`). Поля
  `net_total_nics`/`supported`/`mmio_ready`/`mac_ready` пробрасываются в
  `SystemStats` (`userspace/libdunit/src/lib.rs`). Риск ввести в заблуждение о
  наличии сети. Пометить как discovery-only в выводе.

---

## 🔴 Цель: полная переработка GUI — уход от хардкода к TOML-конфигам (в стиле Hyprland)

Сейчас GUI — это сплошной хардкод прямо в ядре. Тема, геометрия окон, набор
приложений, раскладка панели, обои и хоткеи запечены в `const`/`enum` внутри
kernel-кода и не конфигурируются без пересборки ОС. Это архитектурно плохо и
является блокером для развития рабочего стола. Целевое состояние: GUI управляется
декларативными TOML-конфигами (как `hyprland.conf` у Hyprland) — пользователь меняет
тему/биндинги/раскладку/автозапуск без пересборки ядра, а сам композитор/WM живёт в
userspace.

Конкретные точки хардкода (что убрать в конфиг):

- [ ] **Тема/цвета захардкожены в ядре.**
  `kernel/src/gui/ui_loop.rs:9-26`: вся палитра — `const BG = 0x030504`, `PANEL`,
  `TEXT`, `ACCENT`, `WINDOW_BG`, `TERMINAL_BG`, `GLASS*`, `SHADOW` и т.д. Должно
  задаваться в TOML-теме (`[theme] bg = "#030504"` …).

- [ ] **Геометрия и заголовки окон захардкожены.**
  `kernel/src/window_manager.rs:57-81` (`default_window`): позиции/размеры/тайтлы для
  каждого приложения заданы кортежами (`Terminal => (50, 80, 420, 310, "Terminal")` и
  т.п.). Пиксельные оффсеты кнопок закрыть/свернуть/зум тоже магические
  (`window.x + 12/32/52`, `close_at`/`minimize_at`/`zoom_at`). Должно описываться
  правилами окон в конфиге (как `windowrule` в Hyprland).

- [ ] **Набор приложений — фиксированный `enum`, а не конфиг/реестр.**
  `kernel/src/window_manager.rs:19-27`: `enum AppType { Terminal, Calculator, Files,
  Settings, Monitor, Editor }` жёстко зашит. Пути к GUI-бинарям тоже захардкожены:
  `kernel/src/gui/ui_loop.rs:36-42` (`GUI_PING_PATH`, `GUI_TERMINAL_STUB_PATH`,
  `GUI_CALCULATOR_PATH`, `GUI_STATS_PATH`, `GUI_FILE_MANAGER_PATH`). Должно быть:
  список приложений/автозапуск/ярлыки из конфига (`[[app]]` записи).

- [ ] **Обои: путь и размеры прибиты гвоздями.**
  `kernel/src/gui/ui_loop.rs:30-34`: `WALLPAPER_WIDTH = 1600`, `WALLPAPER_HEIGHT = 900`,
  `WALLPAPER_STRIDE`, `WALLPAPER_PATH = "/assets/wallpapers/wallpaper.bmp"`. Фиксированные
  размеры сломаются на другом разрешении. Должно: путь и режим (fit/stretch/tile) из
  конфига, размеры — из самого изображения.

- [ ] **Хоткеи: плоский самопальный формат вместо полноценного конфига.**
  `kernel/src/fs/vfs.rs:469` зашивает дефолт `super+q=close_window\nsuper+enter=open_terminal`
  в `/cfg/gui/shortcuts.conf` (`GUI_SHORTCUTS_CONFIG`), парсинг — в
  `kernel/src/gui/ui_loop.rs` (`GUI_SHORTCUTS_PATH`). Это затычка: формат ad-hoc,
  набор действий ограничен. Должно: биндинги в TOML (`[[bind]] mods=["super"]
  key="q" action="close_window"`), расширяемый список действий.

- [ ] **Границы/ограничения раскладки — магические числа.**
  `kernel/src/window_manager.rs`: `drag_window` фиксирует верхнюю границу `42`
  (высота панели), `zoom_at` — оффсеты `24/54/48/150`, минимумы `260/180`. Панель/гэпы/
  границы должны конфигурироваться (`[layout] gaps`, `panel_height` …).

- [ ] **Композитор/WM живёт в ядре, а не в userspace.**
  `kernel/src/window_manager.rs` + `kernel/src/gui/ui_loop.rs` (~4467 строк) держат
  состояние окон и логику отрисовки прямо в ядре (`static mut WM_INSTANCE`). README
  сам ставит цель: *«Prefer real userspace GUI processes over fake desktop state»*.
  Целевая архитектура Hyprland-стиля: тонкий kernel-фреймбуфер/ввод + userspace-
  композитор, читающий TOML. Это верхнеуровневый блокер, из которого следуют все
  пункты выше.

**Итог по GUI:** текущая реализация — хардкод, и это действительно плохо.
Направление переработки: (1) вынести композитор/WM в userspace; (2) перевести тему,
правила окон, список приложений/автозапуск, обои, хоткеи и параметры раскладки в
TOML-конфиги в `/cfg/gui/`; (3) добавить парсер TOML и горячую перезагрузку конфига.

---

## Заметки к области, не вошедшей в глубокий разбор

- [ ] **GUI/`ui_loop.rs` (~4467 строк) — отдельный аудит (см. раздел про переработку GUI выше).**
  Ключевая цель по GUI вынесена в отдельный раздел «Полная переработка GUI». Помимо
  ухода от хардкода к TOML, `kernel/src/gui/ui_loop.rs` и связанные
  (`window_manager.rs`, `gui/renderer.rs`, `shell.rs`, `command.rs`, `terminal.rs`)
  требуют прохода на предмет фейкового «desktop state», дублирования отрисовки и
  мёртвого кода.

- [ ] **Проверить остальные драйверы/ФС на те же паттерны.**
  `drivers/{pci,virtio_blk,keyboard,mouse,block,registry,usb/*}.rs`,
  `fs/{dunitfs,devfs,procfs}.rs`, `storage/{gpt,mod}.rs`, HAL asm/C. Ожидаемые
  повторяющиеся проблемы: `static mut` без синхронизации, дубли форматтеров,
  busy-wait, отладочный лог. Пройтись после устранения системных блокеров.

---

## Приоритеты исправления (рекомендация)

1. Синхронизация + стабильная адресация процессов (`PROCESS_TABLE`, scheduler) — снимает
   главный блокер SMP/преемпшна и класс UB.
2. Безопасное копирование user↔kernel с проверкой маппинга (устраняет краши ядра от
   userspace-указателей).
3. PMM (все регионы) + растущая PMM-backed куча + shared kernel-half PML4.
4. Убрать smoke-тесты и фейковый boot-лог из продового пути; свести дублирующиеся
   page-walk/форматтеры/глифы к единым API.
5. Функциональные syscalls (`exec`, `mmap`, `kill`, реальный `sleep`), затем чистка slop.
