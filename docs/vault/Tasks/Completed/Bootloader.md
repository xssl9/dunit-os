# Bootloader / Limine

**Status:** COMPLETED FOUNDATION
**Current state:** [[../../STATUS|STATUS]]

## Delivered

- Limine boot and memory/framebuffer handoff.
- Separate Terminal and GUI boot configurations.
- BIOS ISO and disk paths.
- UEFI `BOOTX64.EFI`/OVMF path.
- Installer BIOS/ESP payload integration.
- Deterministic test configs `limine_test_terminal.conf` and `limine_test_gui.conf`.

## Verified boundary

BIOS and UEFI both reach Green Tea Kernel and mounted storage in automated scenarios. This does not by itself mean normal installed root: system/apps are still embedded and [[../Future/Installed-System|disk-root installation]] remains future work.

## Test rule

Launch only through `tools/qemu_test.py`; `make run` is not the autonomous verification path.
