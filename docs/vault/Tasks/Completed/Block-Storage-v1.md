# Block Storage Foundation

**Status:** COMPLETED FOUNDATION / EXPANDED SINCE ORIGINAL MILESTONE
**Next:** [[../Future/Filesystem|DunitFS v2]] · [[../Future/Installed-System|Installed System]]

## Delivered

- Common block device registration/read/write/geometry layer.
- RAM block smoke device.
- Legacy VirtIO block path.
- AHCI controller/disk path used by disk boot/persistence tests.
- GPT/installer payload groundwork.
- DunitFS v1 auto-mount as `/persist` on suitable disk.
- Terminal block/storage diagnostics.

## Verified boundary

BIOS and UEFI disk boots reach storage; persistence exists across reboot. However, system root and applications remain embedded/MemFS, and DunitFS v1 lacks production recovery/allocation/directory semantics.

## Regression rule

All disk builds, boots, persistence and firmware variants must use `tools/qemu_test.py`. Historical `build_and_run_multipass.py` examples are obsolete.
