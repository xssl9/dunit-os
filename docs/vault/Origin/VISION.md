# 🌱 VISION — Исходная идея

> Это начальная точка проекта. Из этого документа вырос [[../ROADMAP|ROADMAP]].

**Источник:** `.kiro/specs/microkernel-os/` (design document)

---

## Что хотели сделать

Микроядерная ОС для x86_64 с трёхслойной архитектурой:

```
┌─────────────────────────────────────────────┐
│   Applications (NetSurf, Terminal)          │
│   Rust/C (Ring 3 Userspace)                 │
├─────────────────────────────────────────────┤
│   Display Server + WM (egui)  │  relibc     │
│   Rust (Ring 3)               │  C lib      │
├──────────────────┬────────────┴─────────────┤
│  Video Driver    │  Network Driver          │
│  Rust (Ring 3)   │  Rust (Ring 3)           │
├──────────────────┴──────────────────────────┤
│           Kernel (Ring 0) — RUST            │
│  Scheduler · IPC · PMM/VMM · Syscalls       │
├─────────────────────────────────────────────┤
│         HAL (C + Assembly) — Ring 0         │
│  boot.asm · GDT · IDT · ports · cpu         │
└─────────────────────────────────────────────┘
              ↑ Limine Bootloader
```

---

## Ключевые принципы

- **Микроядро** — драйверы и сервисы живут в userspace (Ring 3), общаются через IPC
- **Трёхслойность** — HAL (C/Asm) → Kernel (Rust) → Userspace (Rust/C)
- **Display Server** — отдельный процесс, рендер через **egui**
- **C-совместимость** — через **relibc** из Redox OS
- **Браузер** — NetSurf портированный под egui вместо X11

---

## Boot sequence (задуманный)

1. **Limine** загружает ядро, передаёт framebuffer + memory map
2. `boot.asm` — проверка Long Mode, стек, вызов `hal_init()`
3. **HAL (C)** — GDT, IDT, PIC, включение прерываний
4. **Kernel (Rust)** — PMM, VMM, Global Allocator, планировщик
5. Kernel создаёт первый процесс `init`
6. Init запускает **Video Driver** → **Display Server + WM**
7. Display Server готов принимать окна

---

## Компоненты (исходный план)

| Компонент | Язык | Где живёт |
|---|---|---|
| HAL | C + ASM | Ring 0 |
| PMM / VMM | Rust | Ring 0 |
| Scheduler | Rust | Ring 0 |
| Syscall Handler | Rust + ASM | Ring 0 |
| IPC | Rust | Ring 0 |
| Global Allocator | Rust | Ring 0 |
| Video Driver | Rust | Ring 3 |
| Display Server + WM | Rust + egui | Ring 3 |
| Network Driver (smoltcp + e1000) | Rust | Ring 3 |
| VFS | Rust | Ring 0 |
| relibc | C | Ring 3 |
| NetSurf | C | Ring 3 |

---

## Связи

- [[DESIGN|📐 DESIGN]] — полный design document с кодом
- [[REQUIREMENTS|📋 REQUIREMENTS]] — все требования
- [[../ROADMAP|🚦 ROADMAP]] — как это трансформировалось в реальный план
