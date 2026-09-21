# Kernel Runtime Prerequisites

**Status:** MUST-HAVE / NEXT FOUNDATION
**Roadmap:** [[../../ROADMAP|ROADMAP]]
**Unlocks:** [[../Future/GUI-Architecture|GUI Server/DWM]] · [[../Future/Libc-Musl|musl pthread/TLS]] · [[../Future/Network-Stack|netd events]]

## Current boundary

Dunit имеет process/address-space/kernel-stack foundation, spawn/yield/wait, basic IPC и schedulable userspace threads. UP round-robin включён по умолчанию; x87/SSE состояние сохраняется отдельно для каждого TID. Потоки делят process-owned address space и fd table, а GPR, kernel stack и FPU state имеют свои. Монотонное время и deadline доступны через `kernel/src/clock.rs` (источник PIT). Для нескольких надёжных long-running services ещё нужны TLS и blocking wait primitives.

## Required work

- [x] Prove PIT timer preemption with CPU-bound parent/child and no `yield`.
- [x] Default-on UP round-robin with x87/SSE FXSAVE state, PIT clocksource/deadline and user-mode-only IRQ preemption.
- [x] Schedulable thread entity: TID, caller-provided user stack, per-thread kernel stack/registers/FXSAVE state and shared process address space; create/exit/nonblocking join.
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
