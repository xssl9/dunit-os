# 🤖 AI CONTEXT — Dunit OS

> Этот файл — для ИИ-ассистентов. Открывай его первым при работе с проектом.
> Последнее обновление: вручную командой

---

## Что это

**Dunit OS (Green Tea)** — микроядерная ОС на Rust для x86_64.  
Разрабатывается двумя людьми. Весь код пишет ИИ под их руководством.

**GitHub:** `https://github.com/susopki/dunit-os`

---

## Стек

| Слой | Технология |
|---|---|
| Bootloader | Limine |
| HAL | C + NASM |
| Kernel | Rust (`no_std`, `x86_64-unknown-none`) |
| Userspace | Rust (кастомный JSON таргет) |
| Запуск | QEMU |
| Линковка | `ld.lld` + `linker.ld` |

---

## Что уже работает

- HAL полностью: GDT, IDT, прерывания, context switch, syscall entry
- Limine bootloader с GUI и Terminal режимами
- Framebuffer console
- Keyboard driver (interrupt-based)
- Terminal с историей и tab autocomplete
- 6 userspace программ компилируются и запускаются: plank, terminal, file_manager, text_editor, settings, system_monitor

---

## Что в процессе

- Terminal: алиасы, env vars, pipe, редиректы
- Drivers: PCI enumeration, disk, network, sound, USB
- GUI: анимации, темы, drag&drop, context меню, system tray

---

## Что планируется

- Network Stack (smoltcp)
- Filesystem (ext2/FAT32)
- Package Manager
- Display Server (Orbital-стиль, не через framebuffer напрямую)
- SMP, ACPI, kernel modules

---

## Важные архитектурные решения

1. **Kernel компилируется как `libkernel.a`** и линкуется с HAL объектами через `ld.lld`
2. **Userspace** собирается отдельно под `x86_64-unknown-none.json` таргет
3. **GUI будущего** — Display Server как userspace процесс (не framebuffer напрямую), референс — Redox Orbital
4. **IPC** — через shared memory + message passing (MessageType: MouseEvent, KeyboardEvent, RenderFrame, WindowCreate)

---

## Граф проекта

```
Origin/VISION (исходный .kiro план)
    └── Origin/REQUIREMENTS
    └── Origin/DESIGN
         └── ROADMAP (текущий план)
              ├── Tasks/Completed/*
              ├── Tasks/InProgress/*
              └── Tasks/Future/*
```

---

## Как читать vault

1. Начни с [[../HOME|HOME]]
2. Посмотри [[../Origin/VISION|VISION]] чтобы понять исходную идею
3. Открой [[../ROADMAP|ROADMAP]] — текущий план с линками на таски
4. Каждый таск — отдельный файл с чеклистом и заметками

---

## Заметки для ИИ

- Код пишется под `no_std` — нет std, нет OS abstractions
- В HAL уже есть: `inb/outb/inw/outw/inl/outl` через inline asm
- Прерывания ремапнуты: IRQ0-15 → INT 32-47 (PIC remap)
- Syscall entry в `syscall.asm`, диспетчеризация в Rust
