# 📋 REQUIREMENTS

> Исходные требования из `.kiro/specs/microkernel-os/requirements.md`
> Трансформировались в задачи → [[../ROADMAP|ROADMAP]]

---

## REQ-1: Bootloader Integration
**User Story:** Получить framebuffer и memory map от Limine без написания 16-bit asm.

- [ ] Kernel получает управление от Limine
- [ ] Получен валидный framebuffer pointer
- [ ] Получена memory map
- [ ] `_start` реализован с `#[no_mangle]`
- [ ] Работа в x86_64 Long Mode

**Статус:** ✅ Выполнено → [[../Tasks/Completed/Bootloader|Bootloader]]

---

## REQ-2: Memory Management
- [ ] PMM — bitmap-based аллокация физических страниц
- [ ] VMM — маппинг виртуальных страниц на физические
- [ ] GDT настроен для защиты памяти
- [ ] Page tables для трансляции адресов

**Статус:** 🔧 Частично → [[../Tasks/InProgress/Drivers|Kernel]]

---

## REQ-3: Interrupt Handling
- [ ] IDT с обработчиками прерываний
- [ ] Timer interrupt → вызов планировщика
- [ ] Keyboard interrupt → обработка ввода
- [ ] Exception handlers для CPU faults

**Статус:** ✅ Базово выполнено → [[../Tasks/Completed/Keyboard-Driver|Keyboard Driver]]

---

## REQ-4: Process Management & Scheduling
- [ ] Round-robin планировщик
- [ ] Context switching по таймеру
- [ ] Ring 3 userspace
- [ ] Изоляция процессов при крашах

**Статус:** 🔧 В процессе

---

## REQ-5: System Calls
- [ ] `syscall` instruction handler
- [ ] Валидация параметров
- [ ] Exit, Fork, Exec, Read, Write, Open, Close, Mmap
- [ ] Переключение Ring 3 → Ring 0

---

## REQ-6: IPC
- [ ] Shared memory механизм
- [ ] Message passing
- [ ] Поддержка: MouseEvent, KeyboardEvent, RenderFrame, WindowCreate

---

## REQ-7: Video Driver
- [ ] Userspace процесс с доступом к framebuffer
- [ ] Double buffering
- [ ] VBE и GOP протоколы

---

## REQ-8: Display Server + WM
- [ ] Отдельный userspace процесс
- [ ] Drag окон
- [ ] Focus management
- [ ] Композитинг всех окон

---

## REQ-9: GUI Framework (egui)
- [ ] Интеграция egui в Display Server
- [ ] Форвардинг mouse/keyboard событий в egui
- [ ] Рендер egui текстур через Video Driver

---

## REQ-10: VFS
- [ ] `/dev`, `/bin`, `/proc`
- [ ] File descriptors
- [ ] Device files (`/dev/fb0`, `/dev/kbd`)

---

## REQ-11: C Library (relibc)
- [ ] Интеграция relibc из Redox
- [ ] malloc/free через kernel syscalls
- [ ] Стандартные C функции

---

## REQ-12: Network Stack (smoltcp)
- [ ] smoltcp без std
- [ ] e1000 драйвер для QEMU
- [ ] Packet transmission

---

## REQ-13: Browser (NetSurf)
- [ ] Сборка NetSurf с relibc
- [ ] Backend egui вместо X11
- [ ] HTTP через Network Driver

---

## REQ-14: ELF64
- [ ] Парсинг ELF headers
- [ ] Загрузка LOAD сегментов
- [ ] Проверка corrupted ELF

---

## REQ-15: Global Allocator
- [ ] `GlobalAlloc` trait
- [ ] Box, Vec, String в `no_std`
- [ ] First-fit с free list

---

## Связи

- [[VISION|🌱 VISION]] — архитектурная идея
- [[DESIGN|📐 DESIGN]] — полный design doc с кодом
- [[../ROADMAP|🚦 ROADMAP]] — план реализации
