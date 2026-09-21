# AI CONTEXT — Dunit OS

> Читать перед изменениями. Vault даёт краткий контекст; подробная архитектура и task cards находятся в [DUNIT_OS_TECHNICAL_ROADMAP.md](../../../DUNIT_OS_TECHNICAL_ROADMAP.md).

- **Last updated:** 2026-09-20
- **Repository:** `https://github.com/xssl9/dunit-os`
- **Kernel:** Green Tea Kernel
- **Test rule:** только `tools/qemu_test.py`

## Что это

Dunit OS — самостоятельная x86_64 ОС с Rust `no_std` kernel, C/NASM HAL, собственным syscall ABI, ELF userspace, VFS/MemFS, DunitFS, Terminal Mode и экспериментальным GUI Mode. Цель — собственная desktop platform, не Linux distribution. POSIX/musl являются source portability layer поверх Dunit ABI.

## Проверенное текущее состояние

- Limine BIOS/UEFI boot, ISO и persistent disk image paths.
- Terminal Mode и GUI Mode выбираются boot config.
- PMM/VMM/heap, GDT/IDT, PIT/PIC, syscall entry, framebuffer/input foundation.
- Ring 3 ELF programs, per-process address spaces/kernel stacks, PID/parent-child/cwd/fd state.
- Cooperative spawn/yield/wait, recoverable user faults и byte-queue IPC.
- Timer preemption доказана `preempt_test`: CPU-bound child завершается без `yield`; режим пока gated/default-off и сохраняет только GPR.
- Syscall ABI: `rax` number/result; args `rdi/rsi/rdx/r10/r8/r9`; `rcx/r11` clobbered.
- VFS root MemFS, `/app`, `/proc`, `/dev`, file/stat/readdir/cwd/process/sysinfo/input/GUI wrappers in `libdunit`.
- AHCI и legacy VirtIO block paths; DunitFS v1 auto-mount как `/persist`.
- GUI Mode имеет desktop, windows, panel/dock/launcher/notifications/quick settings и полноценный GUI terminal, но high-level GUI остаётся kernel-centric.
- E1000 driver только обнаруживает controller/MMIO/MAC; packet I/O отсутствует.
- xHCI имеет ранний controller/ring groundwork, но полной enumeration/HID stack нет.

## Главные ограничения

- UP timer preemption включена по умолчанию; FXSAVE/FXRSTOR сохраняет x87/SSE состояние процессов. Schedulable threads, AVX/XSAVE и SMP ещё не реализованы.
- Нет полноценных userspace threads, TLS, wait queues, futex-like wait/wake и SMP.
- Нет `munmap/mprotect`-полноты, shared VM object model и rights-bearing handles.
- Root/system/apps embedded; установленная система не загружает userspace с persistent root.
- DunitFS v1 ограничена fixed nodes/contiguous allocation и не имеет journal/fsck/permissions.
- GUI Server/DWM/UI Runtime ещё не вынесены из kernel legacy path.
- Networking — discovery only; нет Ethernet/IP/TCP/sockets.
- musl fork ещё не создан.

## Обязательные архитектурные границы

1. Kernel предоставляет mechanisms: memory, processes/threads, IPC, handles, framebuffer/display backend, input, devices.
2. GUI Server — отдельный userspace service и единственный владелец display/input master handles.
3. Dunit DWM — отдельный policy client поверх GUI Server.
4. UI Runtime — независимые DUI tree, layout/widgets, DSS styles/motion и TOML settings.
5. NIC driver остаётся в kernel, protocol stack находится в supervised userspace `netd`.
6. Dunit musl fork адаптируется к native ABI; kernel не копирует Linux syscalls/structs.
7. Static-first: signals/fork/dynamic loader не блокируют первый C userspace.

## Не делать предположений

- Не считать `/persist` полноценным installed root.
- Не считать `net0` работающей сетью.
- Не считать xHCI probe поддержкой USB devices.
- Не считать `userspace/display_server` интегрированным server.
- Не считать наличие scheduler/preempt code работающей preemption.
- Не утверждать production readiness по наличию исходного файла.
- Не давать обычным приложениям raw framebuffer/input/NIC/DMA.
- Не переносить dock/widgets/themes/TCP/POSIX compatibility policy в kernel.

## Ближайшая зависимость

```text
preemption + threads + blocking events + shared VM + handles
    -> GUI Server and DWM
    -> musl pthread/TLS

DunitFS v2 + installed root
    -> persistent configuration
    -> disk-loaded libc/apps/packages

E1000 packet I/O + netd
    -> native sockets
    -> musl sockets
    -> TLS/HTTP/browser-network service
```

## Автоматическая проверка

Единственная разрешённая точка запуска:

```bash
python3 tools/qemu_test.py \
  --build \
  --config limine_test_terminal.conf \
  --cmd "exec runtime_stress" \
  --timeout 120 \
  --cmd-timeout 20 \
  --settle 2 \
  --json
```

После проверки анализировать `build/qemu-serial.log`, показывать релевантный tail и подтверждать boot + subsystem marker. QEMU должен завершать harness, участие пользователя запрещено.

## Порядок чтения

1. [[../STATUS|STATUS]]
2. [[../ROADMAP|ROADMAP]]
3. [DUNIT_OS_TECHNICAL_ROADMAP.md](../../../DUNIT_OS_TECHNICAL_ROADMAP.md)
4. Соответствующий task node
5. Код и автоматический тест
