# 📐 DESIGN

> Полный design document из `.kiro/specs/microkernel-os/design.md`
> Это архивный документ — актуальный план в [[../ROADMAP|ROADMAP]]

---

## Языковые границы

```
ASM → C          : C calling convention
C → Rust         : FFI через extern "C"
Rust → C         : #[no_mangle] extern "C"
```

---

## Memory Layout

**Kernel Space (Higher Half):**
```
0xFFFFFFFF_FFFFFFFF  ┌──────────────┐
                     │   Reserved   │
0xFFFFFFFF_80000000  ├──────────────┤
                     │ Kernel Code  │
                     │ Kernel Data  │
                     │ Kernel Heap  │
0xFFFF8000_00000000  ├──────────────┤
                     │  Page Tables │
0xFFFF0000_00000000  └──────────────┘
```

**User Space (Lower Half):**
```
0x00007FFF_FFFFFFFF  ┌──────────────┐
                     │    Stack ↓   │
                     ├──────────────┤
                     │     Free     │
                     ├──────────────┤
                     │    Heap ↑    │
0x0000000000400000   ├──────────────┤
                     │ Code + Data  │
0x0000000000000000   └──────────────┘
```

---

## Process State Machine

```
Ready ──schedule()──► Running
  ▲                      │
  │                   block()
unblock()                │
  │                      ▼
  └──────────────── Blocked
                         │
                       exit()
                         ▼
                       Dead
```

---

## IPC — MessageType

```rust
pub enum MessageType {
    MouseEvent { x: i32, y: i32, buttons: u8 },
    KeyboardEvent { scancode: u8, pressed: bool },
    RenderFrame { buffer_id: SharedMemoryId },
    WindowCreate { width: u32, height: u32 },
    WindowClose { window_id: u32 },
}
```

---

## VFS Structure

```
/
├── dev/   (fb0, kbd, mouse)
├── bin/   (init, netsurf, terminal)
└── proc/  (1/, 2/, ...)
```

---

## Compositing алгоритм (Display Server)

1. Очистить back buffer
2. Для каждого видимого окна (back → front):
   - Читать из shared memory окна
   - Нарисовать рамку через egui
   - Скопировать содержимое в позицию (x, y)
3. Нарисовать курсор мыши
4. Отправить `RenderFrame` в Video Driver

---

## Связи

- [[VISION|🌱 VISION]] — исходная идея
- [[REQUIREMENTS|📋 REQUIREMENTS]] — требования
- [[../ROADMAP|🚦 ROADMAP]] — текущий план
- [[../Tasks/Future/GUI-Architecture|🔮 GUI Architecture]] — будущий Display Server
