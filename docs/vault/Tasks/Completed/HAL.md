# ✅ HAL — Hardware Abstraction Layer

**Статус:** ✅ Выполнено  
**Из плана:** [[../../Origin/VISION|VISION]] → [[../../ROADMAP|ROADMAP]]  
**Требования:** [[../../Origin/REQUIREMENTS|REQ-1, REQ-2, REQ-3]]

---

## Что сделано

- [x] `boot.asm` — entry point, Long Mode check, стек, вызов `hal_init()`
- [x] `gdt.c / gdt.asm` — GDT с Ring 0 и Ring 3 сегментами
- [x] `idt.c / idt.asm` — IDT с 256 обработчиками
- [x] `interrupts.asm` — ISR stubs с сохранением всех регистров
- [x] `ports.c` — `inb/outb/inw/outw/inl/outl`
- [x] `context_switch.asm` — переключение контекста
- [x] `syscall.asm` — точка входа системных вызовов
- [x] FFI граница с Rust через `extern "C"`

## Скриншоты / Артефакты

> Перетащи скрин сюда (Obsidian поддерживает drag & drop)

## Заметки

_Место для заметок команды_
