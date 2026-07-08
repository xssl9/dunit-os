# Drivers

**Status:** PARTIAL / in progress  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**Related requirements:** [[../../Origin/REQUIREMENTS|REQ-7, REQ-12]]

---

## Current State

Dunit OS has enough low-level hardware support to boot, draw to the framebuffer, receive keyboard input, initialize timer/PIC paths, and run the kernel terminal. This is enough for the current VFS/MemFS and syscall smoke workflows.

What is still missing is a general driver model and broader hardware/network
coverage. The first QEMU disk-backed block driver exists as legacy virtio-blk
`vd0`.

---

## Working

- Framebuffer output for boot UI, terminal mode, and experimental GUI mode.
- Keyboard input for terminal interaction and automated QEMU command injection.
- Shared mouse input state used by PS/2 mouse packets and the early USB HID
  mouse parser path.
- PCI config-space scan that detects and logs USB controllers.
- PCI BAR decoding plus MMIO/bus-master enable path for device drivers.
- Cached PCI inventory populated during driver init, so diagnostics and drivers
  can reuse discovered devices without rescanning config space every time.
- PCI capability walking for MSI/MSI-X discovery plus IRQ line/pin diagnostics.
- PCI BAR sizing helper for future block/network/MMIO drivers.
- Terminal `lspci` diagnostics for PCI device inventory and USB controller
  detection.
- xHCI host-controller bring-up: MMIO capability probe, controller halt/reset,
  slot configuration, port power/status logging.
- xHCI command/event ring foundation with DMA pages, DCBAA/CRCR/ERST setup,
  doorbell ringing, and successful `Enable Slot` command completion.
- Terminal `usb` diagnostics for xHCI controller count, initialized controller
  count, connected port count, and last init error.
- Minimal device registry with `/dev` MemFS device nodes for framebuffer, input,
  PCI, and detected xHCI controllers.
- Minimal block device layer with registered block devices, block geometry,
  sector read/write dispatch, and a RAM-backed `ramblk0` smoke device exposed
  at `/dev/ramblk0`.
- Terminal `blk` and `blkread` diagnostics for block inventory and sector-read
  smoke testing.
- Legacy virtio-blk PCI driver for QEMU transitional devices, registering
  disk-backed `vd0` through the block layer.
- Terminal `blkwrite` writes a deterministic sector pattern for automated
  read-after-write testing.
- `build_and_run_multipass.py --disk virtio` creates/attaches the raw QEMU disk
  image used by the block regression.
- QEMU `qemu-xhci` + `usb-mouse` boot verified: controller initializes and logs
  the connected USB mouse port, then completes `Enable Slot` with slot 1.
- Tracked terminal QEMU path uses the same USB devices:
  `make run-terminal QEMU_USB_INPUT="-device qemu-xhci -device usb-mouse"`.
- Serial output used by logs and userspace stdout/stderr smoke checks.
- Basic port IO support through the HAL layer.
- Timer/PIC initialization paths used by the boot flow.

---

## Skeleton / Planned

- PCI IRQ routing beyond diagnostics and MSI/MSI-X enable/configuration.
- ACPI support.
- ATA/AHCI disk driver after the QEMU-friendly virtio path.
- Network driver, likely E1000 or RTL8139 first.
- USB stack beyond first command path: device contexts, address-device,
  descriptor enumeration, and real HID polling/interrupt transfers.
- Sound driver.
- A cleaner device registration layer for future DevFS integration.

---

## Development Order

```text
PCI enumeration hardening
    -> block device abstraction (done)
    -> QEMU virtio-blk disk driver (done)
    -> mountable dunitFS prototype
    -> ATA/AHCI disk driver
    -> network driver -> [[../Future/Network-Stack|Network Stack]]
    -> USB xHCI device contexts and enumeration
    -> USB HID mouse path
    -> sound later

ACPI
    -> power management
    -> SMP groundwork later
```

---

## Blockers

- Persistent dunitFS can now target `vd0`, but it still needs an on-disk format,
  allocator, mount path, and VFS bridge before it can leave MemFS/RAM.
- Networking needs a real NIC driver before a TCP/IP stack is useful.
- DevFS is still a skeleton, but the first registered device nodes are exposed
  through `/dev` via MemFS.
- USB devices are not enumerated yet: xHCI input/device contexts, address-device,
  descriptor reads, and HID interrupt polling are still required after the first
  command-ring path.

---

## Regression

Use the autonomous multipass/QEMU path; do not launch QEMU manually:

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --disk virtio \
  --qemu-timeout 60 \
  --qemu-log qemu_block_v1.log \
  --qemu-test-commands "blk;blkread vd0 0;blkwrite vd0 3;blkread vd0 3" \
  --expect-log "vd0" \
  --expect-log "virtio-blk" \
  --expect-log "vd0 lba=3 written=512" \
  --expect-log "DUNIT-BLOCK-STOR" \
  --expect-log "AGE-V1"
```

Useful interactive diagnostics after boot:

```text
lspci
usb
ls /dev
devs
blk
blkread ramblk0 0
blkread vd0 0
blkwrite vd0 3
blkread vd0 3
ps
```

---

## Links

- [[../Completed/HAL|HAL]]
- [[../Completed/VFS-MemFS|VFS + MemFS]]
- [[../Future/Filesystem|Persistent dunitFS]]
