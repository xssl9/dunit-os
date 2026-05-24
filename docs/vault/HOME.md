# 🖥️ Dunit OS (Green Tea)

> Микроядерная ОС на Rust для x86_64 с GUI и Terminal режимами.

---

## Навигация

| | |
|---|---|
| 🌱 [[Origin/VISION\|VISION]] | Исходная архитектурная идея |
| 📋 [[Origin/REQUIREMENTS\|REQUIREMENTS]] | Требования из .kiro |
| 🗺️ [[Origin/DESIGN\|DESIGN]] | Design document из .kiro |
| 🚦 [[ROADMAP\|ROADMAP]] | Текущий план (из TODO.md) |
| 🤖 [[AI-Context/CONTEXT\|AI CONTEXT]] | Файл для ИИ-ассистентов |

---

## Статус проекта

```
HAL (C/Asm)     ████████████████████ ✅ done
Kernel (Rust)   ██████████████░░░░░░ ~70%
Userspace       ████████░░░░░░░░░░░░ ~40%
Drivers         ░░░░░░░░░░░░░░░░░░░░ todo
Network Stack   ░░░░░░░░░░░░░░░░░░░░ todo
Filesystem      ░░░░░░░░░░░░░░░░░░░░ todo
```

---

## Стек

- **HAL** — C + NASM
- **Kernel** — Rust (`no_std`)
- **Userspace** — Rust (кастомный таргет `x86_64-unknown-none`)
- **Bootloader** — Limine
- **QEMU** — запуск и тестирование

## Быстрые ссылки по категориям

### ✅ Сделано
- [[Tasks/Completed/HAL|HAL]]
- [[Tasks/Completed/Bootloader|Bootloader + Limine]]
- [[Tasks/Completed/Terminal-Mode|Terminal Mode]]
- [[Tasks/Completed/Keyboard-Driver|Keyboard Driver]]
- [[Tasks/Completed/Userspace-Programs|Userspace Programs]]

### 🔧 В процессе
- [[Tasks/InProgress/Terminal-Improvements|Terminal Improvements]]
- [[Tasks/InProgress/Drivers|Drivers]]
- [[Tasks/InProgress/GUI-Improvements|GUI Improvements]]

### 🔮 Будущее
- [[Tasks/Future/Network-Stack|Network Stack]]
- [[Tasks/Future/Filesystem|Filesystem]]
- [[Tasks/Future/Package-Manager|Package Manager]]
- [[Tasks/Future/Advanced-Features|Advanced Features (SMP, ACPI)]]
- [[Tasks/Future/GUI-Architecture|GUI Architecture (Display Server)]]
