# GUI Server, UI Runtime и Dunit DWM

**Status:** PLANNED REWRITE
**Roadmap:** [[../../ROADMAP|ROADMAP]]
**Depends on:** [[../InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime Prerequisites]] · [[Installed-System|Installed System]]

## Почему нужен rewrite

Текущий GUI функционален, включая GUI-терминал, но desktop/compositor/window policy/layout/widgets сосредоточены в `kernel/src/ui_loop.rs` и `kernel/src/window_manager.rs`. Геометрия, цвета, приложения и wallpaper dimensions частично hardcoded; raw drawing/input paths слишком широки. Это нельзя стабилизировать как долгосрочный application ABI.

## Целевая граница

```text
Green Tea Kernel
  memory, processes/threads, IPC, shared VM, handles
  framebuffer/display backend, input devices, synchronization

GUI Server (userspace)
  surfaces, shared buffers, composition, focus, input routing

Dunit UI Runtime
  DUI tree, layout, widgets, DSS style/motion, themes/events

Dunit DWM
  desktop, panel, dock, launcher, quick settings, notifications

Applications
  terminal, file manager, settings, calculator, browser UI
```

Только GUI Server получает `DISPLAY_MASTER`/input master rights. DWM — privileged policy client, но не владелец client buffers. Обычные приложения получают protocol endpoints и shared surface handles.

## GUI protocol v1

- Version/capability handshake.
- Create/destroy surface and role assignment.
- Attach shared XRGB/ARGB buffer, damage and commit.
- Configure/ack resize-state serials.
- Pointer, keyboard, text input and focus events.
- Title/app ID/close requests.
- Explicit object lifetime, bounds, quotas and errors.
- Privileged DWM operations separated from client operations.

## Config-driven UI

| Назначение | Формат |
|---|---|
| Settings, manifests, panel/dock composition | versioned TOML subset |
| Component tree | declarative DUI |
| Appearance/themes | bounded DSS stylesheet |
| Motion | declarative DSS/DUI blocks |

UI Runtime предоставляет retained tree, stable IDs, Row/Column/Grid/Stack/Scroll, constraints, widgets, capture/bubble events, keyboard focus, style cascade и property animation. Reload выполняется `parse -> validate -> resolve -> dry layout -> atomic swap`; ошибка оставляет last-known-good.

## Milestones

- [ ] M2: protocol spec + headless model/property/fuzz tests.
- [ ] M3: userspace `gui-server`, two clients, shared surfaces, composition and input.
- [ ] M4: DUI/DSS/TOML runtime and DWM feature parity.
- [ ] Перенести GUI terminal command execution в userspace shell/session/PTY path.
- [ ] Сохранить panel/dock/theme/window/session settings на user volume.
- [ ] Удалить legacy GUI из normal boot после parity; оставить узкий recovery path при необходимости.

## Acceptance

- Kernel не содержит dock/panel/launcher/widgets/themes/layout parser.
- Два untrusted clients композитятся одновременно и не видят чужие buffers/input.
- Crash client/DWM/GUI Server имеет ограниченный blast radius и recovery path.
- Desktop меняется через config без kernel rebuild.
- Terminal, panel, dock, launcher, notifications, quick settings и wallpaper имеют parity.
- Golden layouts проходят на 1024×768, 1600×900 и HiDPI profile.

## Не делать

- Не фиксировать immediate-mode `DRAW_RECT/DRAW_TEXT` как final GUI ABI.
- Не давать приложениям raw framebuffer/input.
- Не строить полный HTML/CSS/JS runtime вместо небольшого DUI/DSS.
- Не удалять legacy GUI до рабочего vertical slice и feature parity.
