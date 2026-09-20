# Kernel Runtime Prerequisites

**Status:** MUST-HAVE / NEXT FOUNDATION
**Roadmap:** [[../../ROADMAP|ROADMAP]]
**Unlocks:** [[../Future/GUI-Architecture|GUI Server/DWM]] · [[../Future/Libc-Musl|musl pthread/TLS]] · [[../Future/Network-Stack|netd events]]

## Current boundary

Dunit имеет process/address-space/kernel-stack foundation, cooperative spawn/yield/wait, basic IPC и experimental preemption hooks. Это ещё не среда для нескольких надёжных long-running services: timer preemption default-off, threads/TLS и blocking wait primitives отсутствуют.

## Required work

- [ ] Stable UP preemptive round-robin with explicit IRQ/preemption/lock rules.
- [ ] Schedulable thread entity: TID, user/kernel stack, registers, FPU/SIMD and shared process address space.
- [ ] Wait queues, events, deadlines and blocking sleep/IPC/input.
- [ ] `munmap`, `mprotect`, shared VM objects, guard pages and complete teardown.
- [ ] x86_64 thread pointer / `FS.base`, initial `PT_TLS` image and context-switch preservation.
- [ ] Atomic `wait_on_word/wake` with timeout and no lost wakeups.
- [ ] Per-process handle table with rights such as read/write/map/signal/transfer/display/raw-network.
- [ ] Replace unsafe global mutable ownership with explicit locks/services.
- [ ] Define monotonic/realtime clocks and secure entropy interfaces.

## Acceptance

- CPU-bound child is preempted without `yield`.
- Blocked thread consumes no CPU and wakes exactly on event/timeout.
- Multiple threads have isolated TLS and per-thread `errno` groundwork.
- Kill/exit releases mappings, handles, stacks and IPC endpoints.
- Shared buffer lifecycle survives client/server crash.
- Stress covers starvation, lost wakeups, mmap boundaries, FPU state and lock/IRQ ordering.

## Scope control

Implement and harden on one CPU first. SMP, priorities/realtime scheduling and complex signal interruption are later milestones.
