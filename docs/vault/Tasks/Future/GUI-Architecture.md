# GUI Architecture - Display Server

**Status:** PLANNED  
**Source idea:** [[../../Origin/VISION|VISION]] + Redox Orbital-style architecture  
**See first:** [[../InProgress/GUI-Improvements|GUI Improvements]]  
**Related requirements:** [[../../Origin/REQUIREMENTS|REQ-8, REQ-9]]

---

## Current State

The current GUI path draws directly through framebuffer-oriented kernel code. That is acceptable for experiments, boot visuals, and early demos, but it is not the final application model.

The reliable user-facing environment today is [[../Completed/Terminal-Mode|Terminal Mode]].

---

## Target Direction

Long term, GUI should move toward a display-server model:

```text
Application
    -> syscall / IPC
Display Server (userspace)
    -> window buffers + events
Video / framebuffer backend
    -> physical framebuffer
```

This would keep applications away from raw pixels and allow windows, focus, events, and composition to become explicit system contracts.

---

## Planned Work

- Define IPC message types.
- Define window create/destroy API.
- Define event delivery for keyboard/mouse.
- Add shared-memory or buffer handoff strategy.
- Add compositor with z-order and focus.
- Add a minimal userspace display server process.
- Connect GUI apps after ELF exec/userspace process launch is real.

---

## Blockers

- IPC is still a skeleton.
- Userspace app execution is not a general runtime yet.
- Scheduler/process model is still minimal.
- No shared-memory API.
- No userspace display server.

---

## Links

- [[../Completed/Syscall-ABI|Syscall ABI]]
- [[../Completed/Process-FD-Model|Process + FD Model]]
- [[../InProgress/GUI-Improvements|GUI Improvements]]
