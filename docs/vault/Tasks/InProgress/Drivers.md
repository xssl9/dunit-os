# Drivers

**Status:** PARTIAL / IN PROGRESS
**Roadmap:** [[../../ROADMAP|ROADMAP]]

## Working or integrated foundation

- Limine framebuffer used by boot, Terminal and legacy GUI.
- PS/2 keyboard/mouse input paths.
- PCI enumeration, BAR decoding, bus-master enable and MSI/MSI-X capability discovery.
- Device registry and diagnostic `/dev` nodes.
- AHCI controller/disk path used by installed disk tests.
- Legacy VirtIO block path and RAM block smoke device.
- DunitFS auto-mount on suitable block device.
- xHCI controller reset/command-event ring groundwork; not full USB support.
- E1000 PCI/MMIO/MAC discovery; not packet I/O.

## Honest boundaries

- xHCI has no complete device enumeration, descriptors/endpoints or real HID interrupt pipeline.
- E1000 has no RX/TX descriptor rings or interrupts; `net0` is discovery only.
- VirtIO is legacy-oriented; modern transport/features need separate work.
- Audio and ACPI power are not implemented.
- Framebuffer is not a GPU acceleration driver.

## Ordered work

1. Keep AHCI/persistence regression stable.
2. Finish E1000 packet I/O for [[../Future/Network-Stack|netd]].
3. Complete xHCI enumeration -> descriptors -> HID keyboard/mouse.
4. Add modern VirtIO block/net where it reduces QEMU/reference-hardware risk.
5. ACPI shutdown/reboot and hardware inventory.
6. HDA/audio service, USB mass storage and broader real-hardware matrix later.

## Acceptance rule

Discovery is never called support. A device is supported only after end-to-end I/O, fault/reset handling, automated QEMU regression where possible and a documented hardware tier.

## Test rule

Only `tools/qemu_test.py` may launch/build QEMU scenarios. Manual QEMU commands and the removed Multipass workflow are obsolete.
