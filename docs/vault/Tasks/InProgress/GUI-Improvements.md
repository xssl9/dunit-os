# GUI Improvements

**Status:** PARTIAL / in progress  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**See also:** [[../Future/GUI-Architecture|GUI Architecture]]

---

## Current State

Dunit OS can initialize the framebuffer and has an experimental GUI path. The reliable interactive mode today is still **kernel terminal mode**, not GUI mode.

The GUI should be treated as an experimental surface until Userspace Runtime v1,
IPC, input routing, and a display-server model are hardened.

---

## Working

- Framebuffer is available.
- Boot UI and terminal rendering work on top of framebuffer output.
- GUI code exists as an experimental direction.
- Some GUI-oriented userspace apps are built and embedded under `/app`.

---

## Not Ready Yet

- Window manager is still a skeleton.
- GUI applications are not on a stable app/runtime contract yet.
- No persistent userspace display server.
- GUI IPC messages exist experimentally, but the contract is not final.
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
- [[Userspace-Runtime-v1|Userspace Runtime v1]]
- [[../Future/GUI-Architecture|GUI Architecture]]
- IPC foundation before real userspace GUI apps.
