# 🔮 GUI Architecture — Display Server

**Статус:** 🔮 Будущее  
**Источник идеи:** [[../../Origin/VISION|VISION]] (.kiro) + Redox Orbital  
**Смотри сначала:** [[../InProgress/GUI-Improvements|GUI Improvements (текущее)]]  
**Требования:** [[../../Origin/REQUIREMENTS|REQ-8, REQ-9]]

---

## Проблема текущего подхода

Сейчас GUI рисует напрямую в framebuffer — это как inline стили в HTML.  
Цель — сделать **Display Server** (аналог CSS-классов / layout engine):
приложения не знают про пиксели, только про окна и события.

---

## Референс — Redox Orbital

Redox решил это через:
- **Orbital** — display server, работает как userspace процесс
- Приложения открывают файл `/scheme/orbital` через VFS
- Получают "окно" — буфер + события (mouse, keyboard)
- Нет X11, нет wayland — своя минимальная схема

Брать код напрямую нельзя (другой IPC), но **архитектура** — хороший референс.

---

## Наш план

```
Приложение
    │ syscall / IPC
    ▼
Display Server (userspace, Ring 3)
    │ shared memory (window buffer)
    │ message: RenderFrame
    ▼
Video Driver (userspace, Ring 3)
    │ blit
    ▼
Framebuffer (физическая память)
```

---

## Чеклист

- [ ] Определить IPC протокол (message types)
- [ ] Реализовать shared memory для window buffers
- [ ] Display Server: create_window / destroy_window
- [ ] Display Server: compositor (z-order)
- [ ] Интеграция egui для рендера рамок/декораций
- [ ] Video Driver: double buffering + swap
- [ ] Mouse cursor через Display Server
- [ ] Focus management

---

## Зависимости

- [[../InProgress/Drivers|Drivers]] — нужен IPC в ядре
- [[../Future/Filesystem|Filesystem]] — для `/scheme/orbital`-подобного механизма (опционально)

---

## Скриншоты / Мокапы

> Место для скринов и набросков

## Заметки

_Место для заметок команды_
