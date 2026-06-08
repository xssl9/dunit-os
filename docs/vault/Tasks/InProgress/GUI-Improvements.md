# GUI Improvements

**Status:** PARTIAL / in progress  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**See also:** [[../Future/GUI-Architecture|GUI Architecture]]

---

## Current State

Dunit OS can initialize the framebuffer and has an experimental GUI path. The reliable interactive mode today is still **kernel terminal mode**, not GUI mode.

The GUI should be treated as an experimental surface until process execution, IPC, input routing, and a display-server model are designed.

---

## Working

- Framebuffer is available.
- Boot UI and terminal rendering work on top of framebuffer output.
- GUI code exists as an experimental direction.

---

## Not Ready Yet

- Window manager is still a skeleton.
- GUI applications are not real userspace processes.
- No userspace display server.
- No GUI IPC contract.
- No window/event protocol.
- No persistent user configuration for themes/settings.

---

## Future Work

- Window create/destroy primitives.
- Input focus and event routing.
- Basic compositor and z-order.
- Context menus and notifications.
- Theme/settings storage after persistent filesystem exists.

---

## Dependencies

- [[../Completed/Syscall-ABI|Syscall ABI]]
- [[../Completed/Process-FD-Model|Process + FD Model]]
- [[../Future/GUI-Architecture|GUI Architecture]]
- IPC foundation before real userspace GUI apps.
