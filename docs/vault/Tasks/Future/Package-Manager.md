# Package Manager

**Status:** PLANNED  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**Depends on:** [[../Future/Filesystem|Persistent dunitFS]], [[../Future/Network-Stack|Network Stack]]

---

## Current State

Package management is not active runtime functionality yet. Dunit OS can build userspace components, but it cannot install packages into a persistent root filesystem.

---

## Planned Scope

- Package metadata format.
- Package repository layout.
- Install/remove operations.
- Dependency resolution.
- Local package database stored on persistent dunitFS.
- Optional network repository sync after networking exists.

---

## Not In Scope Yet

- dpkg compatibility.
- Online repositories.
- System updates.
- Signature verification.

Those only make sense after persistent storage and networking are real.

---

## Blockers

- Persistent dunitFS is not implemented.
- Block device layer is not implemented.
- Networking is planned, not working.
- Userspace process execution is still not a full app runtime.
