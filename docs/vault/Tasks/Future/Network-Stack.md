# Network Stack

**Status:** PLANNED  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**Related requirements:** [[../../Origin/REQUIREMENTS|REQ-12]]  
**Depends on:** [[../InProgress/Drivers|Drivers]]

---

## Current State

Networking is not implemented yet. There is no working NIC driver, no socket API, and no TCP/IP stack wired into the kernel or userspace runtime.

---

## Planned Scope

- Ethernet layer.
- IPv4 first.
- ARP.
- UDP.
- TCP later.
- Socket-like syscall/API design.
- DNS resolver after UDP exists.
- Minimal HTTP client only after the stack is usable.

---

## Possible Library

`smoltcp` is a reasonable no_std reference/library candidate, but it should only be evaluated after a NIC driver and packet IO boundary exist.

---

## Blockers

- No E1000/RTL8139 driver yet.
- No socket syscall contract.
- No userspace network runtime.
- No package manager or repository transport layer.

---

## Links

- [[../InProgress/Drivers|Drivers]]
- [[../Future/Package-Manager|Package Manager]]
