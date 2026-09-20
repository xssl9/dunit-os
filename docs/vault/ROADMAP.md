# ROADMAP

> Vault-level task graph. Полный инженерный план находится в [DUNIT_OS_TECHNICAL_ROADMAP.md](../../DUNIT_OS_TECHNICAL_ROADMAP.md); здесь — короткая карта исполнения и ссылки на живые узлы.

## Целевая последовательность

```text
M0 honest baseline / ABI decisions
  |
  +-> M1 preemption + threads + VM + events + handles
  |      +-> M3 userspace GUI Server -> M4 UI Runtime / Dunit DWM
  |      `-> M6 static musl -> pthread -> broader POSIX later
  |
  +-> M2 GUI protocol/headless conformance -> M3
  |
  `-> M5 DunitFS v2 + installed root
         +-> persistent DWM configuration
         `-> disk-loaded libc/apps/packages

M7: E1000 -> netd -> IPv4/UDP/TCP/DHCP/DNS -> native sockets
    -> musl sockets -> TLS/HTTP -> browser-network service
```

## M0 — честный baseline и контракты

- [x] Техническое ревью и [полный roadmap](../../DUNIT_OS_TECHNICAL_ROADMAP.md).
- [x] [[STATUS|Vault status]] отражает проверенное состояние.
- [x] Добавлены `docs/architecture/current-state.md` и документированный Dunit ABI v1 snapshot.
- [x] CI проверяет warning budget и обязательные/запрещённые QEMU markers.
- [ ] Стабилизировать ABI v1 после M1 contracts и генерировать Rust/C definitions из одного manifest.
- [ ] Завести ADR для process entry, handles/rights, GUI protocol, `netd` и musl strategy.
- [ ] Каждый `WORKING` claim связать с автоматическим serial marker/test.

## M1 — kernel runtime prerequisites (must-have)

→ [[Tasks/InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime Prerequisites]]

- [x] Доказать timer preemption CPU-bound parent/child без `yield` в gated boot smoke.
- [ ] Сделать preemptive round-robin default-on, добавить FPU/SIMD state и clocksource abstraction.
- [ ] Schedulable userspace threads и thread lifecycle.
- [ ] Blocking wait queues/events/IPC вместо polling.
- [ ] `munmap`, `mprotect`, shared VM objects и guard pages.
- [ ] x86_64 `FS.base`/TLS contract.
- [ ] `wait_on_word/wake` synchronization primitive.
- [ ] Handle tables с rights и безопасным transfer.

## M2–M4 — новый GUI stack

→ [[Tasks/Future/GUI-Architecture|GUI Server Architecture]]

- [ ] Versioned binary GUI protocol и headless conformance tests.
- [ ] Userspace `gui-server` с exclusive display/input master handles.
- [ ] Shared client surfaces, software compositor, damage/focus/input routing.
- [ ] Независимый Dunit DWM поверх GUI Server.
- [ ] DUI markup, DSS styles/motion, TOML settings.
- [ ] Убрать hardcoded desktop geometry/colors/apps из kernel.
- [ ] После feature parity удалить legacy `ui_loop`/kernel WM из normal GUI boot.

## M5 — normal installed system

→ [[Tasks/Future/Installed-System|Installed System]] · [[Tasks/Future/Filesystem|DunitFS v2]]

- [ ] Disk-root `/system`, `/apps`, `/users`, `/var`, `/run`.
- [ ] `init` и services загружаются с установленного system volume.
- [ ] Transactional installer: partition -> format -> copy -> verify -> boot config -> sync.
- [ ] DunitFS v2: allocation tracking, directories, rename/unlink, fsync, recovery/fsck.
- [ ] BIOS/UEFI × AHCI/VirtIO persistence/recovery matrix.
- [ ] Отличающиеся manifests для Live, Minimal и DWM images.

## M6 — Dunit musl fork

→ [[Tasks/Future/Libc-Musl|Dunit musl]]

- [ ] Полный upstream fork с pinned version и контролируемым delta.
- [ ] `x86_64-dunit` compiler wrapper/sysroot, crt objects и `libc.a`.
- [ ] Static hello/args/env, затем VM/allocator/files.
- [ ] Dunit-native `posix_spawn` mapping без обязательного `fork`.
- [ ] pthread/TLS поверх M1 threads и wait/wake.
- [ ] Sockets после native `netd` API.
- [ ] Signals, `fork` и dynamic loader — отдельные later gates.

## M7 — networking и platform services

→ [[Tasks/Future/Network-Stack|Network Stack]]

- [ ] E1000 RX/TX rings, IRQ/DMA/reset/counters.
- [ ] Userspace `netd`: Ethernet, ARP, IPv4/ICMP, UDP, DHCP, DNS, TCP.
- [ ] Native versioned socket/resolver protocol + common event wait.
- [ ] Dunit musl POSIX socket adapter.
- [ ] Entropy/time/trust store, proven TLS library и HTTP client.
- [ ] Sandboxed browser-network service + minimal GUI response viewer.
- [ ] IPv6/multi-interface после стабильного IPv4 vertical slice.

## Later / отдельные platform tracks

- [[Tasks/InProgress/Drivers|USB/xHCI, VirtIO modern, hardware matrix]].
- Audio service/HDA.
- ACPI power/reboot/shutdown, затем suspend.
- [[Tasks/Future/Package-Manager|Signed application bundles and package transactions]].
- [[Tasks/Future/Advanced-Features|SMP, GPU acceleration, dynamic linking and advanced diagnostics]].

## Milestone discipline

- Main остаётся bootable и regression-green.
- Разработка идёт vertical slices, а не по чуть-чуть во всех подсистемах.
- Наличие файла/driver discovery не является completion.
- Persistence подтверждается только stop/start + remount + content hash.
- Unsupported API возвращает честную ошибку, а не fake success.
- Один reference QEMU device/один reference PC завершаются раньше широкой hardware matrix.

## Единственный test entrypoint

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

Ручной QEMU и старый `build_and_run_multipass.py` не являются актуальным workflow.
