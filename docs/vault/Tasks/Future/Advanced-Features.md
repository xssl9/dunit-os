# Advanced Features

**Status:** PLANNED  
**Roadmap:** [[../../ROADMAP|ROADMAP]]

---

## Current State

These items are deliberately out of scope for the current VFS/syscall/process foundation work. They should stay planned until the kernel has stable process execution, persistent storage, and better driver coverage.

---

## Planned Items

- SMP / multi-core support.
- ACPI power management.
- Swap support.
- Kernel modules.
- GDB remote stub.
- More complete panic/fault diagnostics.
- Better tracing/profiling hooks.

---

## Notes

### SMP

Requires APIC/LAPIC work, per-CPU state, locking strategy, and careful scheduler changes. This should not be mixed into the current single-core process/syscall work.

### Swap

Requires persistent storage and a more mature virtual memory model.

### GDB Stub

Serial output is already important for logs and smoke tests. A GDB stub can reuse serial concepts later, but it needs a deliberate debug protocol boundary.

---

## Dependencies

- [[../InProgress/Drivers|Drivers]]
- [[../Future/Filesystem|Persistent dunitFS]]
- Scheduler/process model maturity.
