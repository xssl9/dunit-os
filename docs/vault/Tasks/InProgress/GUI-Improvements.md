# GUI Mode — legacy maintenance and migration

**Status:** FUNCTIONAL LEGACY / MIGRATION TARGET
**Target:** [[../Future/GUI-Architecture|GUI Server + UI Runtime + Dunit DWM]]

## Current state

GUI Mode boots and provides desktop visuals, windows, panel/dock/launcher/system elements, notifications/quick settings and a GUI terminal functionally corresponding to Terminal Mode. Userspace GUI applications exist and are launched through the current bridge.

The architecture is not final: high-level desktop/compositor/WM/layout logic remains concentrated in kernel GUI code, with hardcoded geometry/colors/application wiring and broad drawing/input syscalls.

## Allowed work before rewrite

- Fix regressions that block current GUI boot or terminal parity.
- Preserve automated GUI boot/screenshot markers.
- Extract pure layout/protocol/data structures and tests when reusable.
- Do not expand final ABI around legacy drawing calls.
- Do not add more permanent widgets/policy to kernel.

## Migration gates

- M1 kernel runtime: preemption, events, shared VM and rights handles.
- M2 GUI protocol/headless state-machine tests.
- M3 userspace GUI Server vertical slice with two clients.
- M4 DWM/UI parity and persistent config.
- Remove legacy GUI from normal boot only after terminal/panel/dock/launcher/notifications/quick settings parity.

## Success

Normal GUI boot no longer executes kernel desktop loop; appearance/structure changes through TOML/DUI/DSS; application crash cannot damage compositor/kernel.
