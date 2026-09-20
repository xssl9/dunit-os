# Advanced Features

**Status:** LATER / DEPENDENCY-GATED
**Roadmap:** [[../../ROADMAP|ROADMAP]]

## Later tracks

- SMP/APIC and per-CPU scheduler state after stable UP preemption/lock audit.
- GPU acceleration behind GUI backend after correct software compositor.
- Dynamic loader/`.so` after stable VM, ABI, filesystem and static musl.
- Full POSIX signals, cancellation and optional `fork`/COW after demonstrated demand.
- Audio: HDA DMA -> userspace mixer -> per-app streams/permissions.
- USB: xHCI enumeration/descriptors/endpoints -> HID -> mass storage/hotplug.
- ACPI: tables, reboot/poweroff, battery/thermal, then suspend/resume.
- Swap only after robust VM and persistent storage.
- GDB stub, tracing/profiling and crash dump pipeline.
- Accessibility, text shaping, IME and broader localization.

## Ordering rules

- No SMP before race-free single-core preemption.
- No GPU acceleration before userspace GUI Server parity.
- No dynamic linking before static C toolchain and stable ABI.
- No network package repository before trust/time/TLS/transactions.
- No broad hardware claim without end-to-end I/O on published support tier.

## Product boundary

Dunit can become usable without SMP, GPU acceleration, dynamic libraries, Wi-Fi or a full browser. These features must not block the first installed desktop release for QEMU and one reference PC.
