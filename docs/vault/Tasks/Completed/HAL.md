# HAL Foundation

**Status:** COMPLETED FOUNDATION
**Current state:** [[../../STATUS|STATUS]]

## Delivered

- C/NASM boot boundary and Rust FFI.
- GDT with ring 0/ring 3 segments.
- IDT/ISR stubs and exception paths.
- Port I/O and interrupt setup.
- Syscall entry/return assembly.
- Context-switch assembly foundation.
- Limine handoff integration.

## Boundary

HAL foundation is working, but APIC/SMP, complete FPU/SIMD thread switching, modern interrupt routing and broad hardware abstraction remain later work. Existence of context-switch/preemption code is not proof of default-on preemptive scheduling.
