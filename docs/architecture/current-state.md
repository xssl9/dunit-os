# Dunit OS — фактическое состояние (snapshot M0)

> Честный срез того, что **реально работает** в ядре Green Tea, а не что
> запланировано. Каждое утверждение `WORKING` подкреплено наблюдаемым признаком
> из serial-лога, который выдаёт `tools/qemu_test.py` (единственная точка
> запуска и тестирования). `PARTIAL`/`PROTOTYPE`/`MISSING` означают ровно то,
> что написано: не верить на слово, сверяться с кодом.
>
> - **Дата среза:** 2026-09-20, milestone M0.
> - **Как воспроизвести:** `tools/qemu_test.py` → `build/qemu-serial.full.log`.
> - **Легенда:** ✅ WORKING · 🟡 PARTIAL · 🧪 PROTOTYPE · ⛔ MISSING.

## Как читать этот документ

Признак (`evidence`) — это строка, которую печатает ядро при загрузке или при
прогоне встроенных smoke-тестов. Если строка исчезла или изменилась, а документ
нет — расходится документ, чинить нужно его. Все цитаты ниже взяты из реального
прогона, а не сочинены.

**Это проверяется в CI (бюджет M0):**

- `tools/boot_markers.json` — манифест, связывающий каждый WORKING-claim с
  конкретным serial-маркером и файлом-источником. `tools/qemu_test.py
  --markers-file tools/boot_markers.json` роняет прогон, если хоть один
  required-маркер пропал или появился forbidden (`[PANIC]`, утечка
  VFS-хендлов). Так claim не может быть WORKING без доказательства в загрузке.
- `tools/check_warnings.py` + `tools/warnings_budget.json` — бюджет
  компиляторных warnings (ядро + userspace). Растить его нельзя: CI падает,
  если число warnings превысило зафиксированное. Планка только опускается
  (`--update`).
- Оба шага встроены в `.github/workflows/Build.yml` перед публикацией релиза.

## Загрузка и платформа

| Подсистема | Статус | Признак в serial-логе |
|---|---|---|
| Boot по Limine v5 | ✅ | `[BOOT] START` … `[KERNEL] START` |
| HHDM / higher-half | ✅ | `[KERNEL] HHDM setup` |
| Framebuffer | ✅ | `Framebuffer: 1280x800x32 initialized` |
| GDT | ✅ | `[HAL] OK`, `GDT loaded` |
| IDT + exception handlers | ✅ | `IDT loaded with 256 entries`, `Exception handlers registered` |
| Serial-лог (отладка) | ✅ | весь `build/qemu-serial.full.log` |

## Память

| Подсистема | Статус | Признак / примечание |
|---|---|---|
| Physical Memory Manager | ✅ | `[PMM] OK`, `Physical memory: 339 MiB usable` |
| Virtual Memory Manager | ✅ | `[VMM] OK` |
| Kernel heap (PMM-backed, growable) | ✅ | `[HEAP] OK (PMM-backed, growable)` |
| Переключение адресных пространств | ✅ | `[ADDRSPACE-TEST] OK`, `[PROCESS-ADDRSPACE-TEST] OK` |
| MMIO-аллокатор | ✅ | `[MMIO-ALLOC-TEST] OK` |
| VM mappings (userspace) | 🟡 | anonymous private, `munmap/mprotect`, guard pages и shared objects работают; нет file mappings/COW/rights |

## Планировщик и процессы

| Возможность | Статус | Признак / примечание |
|---|---|---|
| UP round-robin планировщик | ✅ | `[SCHED] UP round-robin init: timer-preemption=on fpu=fxsave smp=off` |
| Context switch (save/restore + `yield`) | ✅ | `Scheduler: cooperative context switch ready` |
| Таймерное вытеснение (preemption) | ✅ | включено по умолчанию; `[PREEMPT-TEST] OK preempts=N` проверяет CPU-bound parent/child без `yield` и сохранность разных XMM значений. FXSAVE/FXRSTOR сохраняет x87/MMX/SSE (без AVX). |
| SMP / многоядерность | ⛔ | `smp=off`; поднимается один CPU |
| Загрузка ELF и запуск процесса | ✅ | `[EXEC] loading /app/runtime_stress`, `[ELF-TEST] userspace app started` |
| `spawn` дочернего процесса | ✅ | `[SPAWN] ready pid=5 path=/app/resumable_child execution=not-started` |
| `wait` и коды возврата | ✅ | `[WAIT] pid=5 kind=0 code=7` |
| Возобновляемый дочерний процесс | ✅ | `resumable_child: A` → `C`, `[PROCESS-RUN] exited pid=5 code=7` |
| `kill` процесса | ✅ | `[WAIT] pid=10 kind=0 code=-9` (runtime_stress: kill OK) |
| Обработка user-fault (page fault) | ✅ | `[USER-FAULT] pid=11 reason=page-fault … err=0x4`, `[WAIT] pid=11 kind=1 code=-14` |
| Schedulable user threads | ✅ | `[THREAD-TEST] OK`: TID, create/exit/nonblocking join, отдельные GPR/kernel stack/FXSAVE, общий PID/address space/fd table; thread fault не завершает процесс |
| Wait queues / blocking sleep / IPC event | ✅ | `[WAIT-TEST] OK`: timeout, blocked sleep, wake при отправке IPC; GUI apps ждут событие без yield polling |
| VM lifecycle | ✅ | `[VM-TEST] OK`: `munmap/mprotect`, W^X, guard fault, shared frame между процессами и освобождение после закрытия/exit |
| TLS / futex | ⛔ | ещё не реализованы (следующие задачи M1) |

Профиль исполнения: UP round-robin с PIT (~100 Гц), один процесс исполняется
в каждый момент времени. Состояние x87/MMX/SSE сохраняется отдельно для каждого
потока; AVX/XSAVE пока не поддержаны. `clock.rs` предоставляет монотонные
тики/наносекунды и deadline. TLS, универсальный event/handle wait и SMP остаются задачами M1.

## IPC

| Возможность | Статус | Признак / примечание |
|---|---|---|
| Байтовые очереди сообщений | ✅ | `[IPC] manager ready: bounded byte message queues (lazy per-PID)` |
| `send` / `receive` между процессами | ✅ | `[IPC] send from=12 to=13 len=7`, `[IPC] recv pid=13 len=7`, `ipc_parent: OK` |
| Границы: ≤256 байт, ≤128 сообщений/очередь | ✅ | `MAX_MESSAGE_SIZE`, `MAX_QUEUE_MESSAGES` |
| Неблокирующий `receive` | ✅ | пустая очередь → `EAGAIN` |
| Блокирующее ожидание IPC-события | ✅ | `wait_event` + повторный `receive`; `libdunit::ipc_recv_blocking` |
| Shared VM object | ✅ | `shared_vm_create/map/close`, refcounted frames; ID пока без rights/handle table |

## Файловая система

| Возможность | Статус | Признак / примечание |
|---|---|---|
| VFS + MemFS как `/` | ✅ | `[MEMFS] mounted as /`, `[VFS] init OK` |
| Встроенные `/app` и `/assets` | ✅ | приложения грузятся из `/app` (`include_bytes!`) |
| syscalls FS (`open/read/write/close/readdir/stat`) | ✅ | `[SYSCALL-FS-TEST] OK`, `[SYSCALL-FS-SEMANTICS-TEST] OK` |
| Initrd-архив | ⛔ | `Initrd: no archive provided; using embedded /app and /assets` — стор есть, архив в boot-путь не подключён |
| DunitFS на диске / `/persist` | 🟡 | код есть; в текущем прогоне диск не смонтирован (`disks=0`) |
| `seek` / `dup` / `pipe` | ⛔ | нет |

## Драйверы и устройства

| Устройство | Статус | Признак / примечание |
|---|---|---|
| PS/2 клавиатура | ✅ | `Keyboard driver ready`, `IRQ 1: Keyboard interrupt enabled` |
| PS/2 мышь | ✅ | `Mouse input driver ready`, `IRQ 12 … enabled` |
| PIT таймер (~100 Гц) | ✅ | `IRQ 0: PIT timer enabled (~100 Hz)` |
| PCI-обнаружение | ✅ | `[PCI] devices detected=6 usb=0 net=1 msi=2 msix=1` |
| Сетевая карта e1000 | 🟡 | обнаружена, MMIO/MAC готовы, **packet-io=not-implemented** |
| Сетевой стек | ⛔ | `[NET] … stack=not-implemented` |
| AHCI (SATA) | 🟡 | контроллер найден (`initialized=1`), но `disks=0` (устройство non-ATA пропущено) |
| xHCI / USB | 🧪 | контроллеры не найдены (`found=0`); HID-парсер готов, энумерации нет |
| virtio-blk | ⛔ | `no legacy virtio-blk device` |

## Userspace

| Возможность | Статус | Признак / примечание |
|---|---|---|
| `libdunit` (no_std обёртки syscalls) | ✅ | все приложения линкуются с ней |
| Терминал / tty | ✅ | `Dunit OS 1.0.0 (Green Tea) tty1`, `root@dunit:~#` |
| Встроенные приложения (`/app/*`) | ✅ | `runtime_stress`, `ipc_parent`, `elf_demo`, `fault_pf` и др. отрабатывают |
| Raw framebuffer/ввод из userspace | 🟡 | доступны напрямую, без capability/разграничения |
| GUI-режим | 🧪 | код ветки есть; текущий прогон идёт в terminal mode |
| Динамическая линковка / PIE | ⛔ | только static non-PIE ELF |

## Что подтверждают встроенные smoke-тесты

Прогон `runtime_stress` последовательно проходит (все с `OK` в логе):
allocator, keyboard decoder, VFS, usercopy, resumable child, IPC, повторный
spawn (×3 `elf_demo`), kill, обработка page fault. Завершается
`[EXEC] /app/runtime_stress returned code=0` без утечки VFS-хендлов
(`VFS handles clean`). Отдельно `ipc_parent` завершается `code=0`.

Это и есть текущая планка «не сломать»: любое изменение ядра должно оставлять
эти строки на месте при прогоне через `tools/qemu_test.py`.

## Крупные пробелы (входы в последующие milestones)

- **M1** — вытеснение по-настоящему, потоки, TLS, futex, полноценный `mmap`.
- **Сеть** — packet I/O для e1000 и сетевой стек (сейчас `not-implemented`).
- **Хранилище** — реальный диск/DunitFS смонтирован и переживает перезагрузку.
- **USB** — энумерация устройств поверх готового HID-парсера.
- **ABI** — versioned манифест вместо ручного дублирования номеров (см.
  [dunit-abi-v1.md](./dunit-abi-v1.md)), capability-модель для framebuffer/ввода.
- **M6** — musl fork поверх замороженного Dunit ABI v1.
