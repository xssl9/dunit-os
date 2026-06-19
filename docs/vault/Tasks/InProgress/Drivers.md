# Drivers

**Status:** PARTIAL / in progress  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**Related requirements:** [[../../Origin/REQUIREMENTS|REQ-7, REQ-12]]

---

## Current State

Dunit OS has enough low-level hardware support to boot, draw to the framebuffer, receive keyboard input, initialize timer/PIC paths, and run the kernel terminal. This is enough for the current VFS/MemFS and syscall smoke workflows.

What is still missing is a general driver model and real hardware/storage/network drivers.

---

## Working

- Framebuffer output for boot UI, terminal mode, and experimental GUI mode.
- Keyboard input for terminal interaction and automated QEMU command injection.
- Shared mouse input state used by PS/2 mouse packets and the early USB HID
  mouse parser path.
- PCI config-space scan that detects and logs USB controllers.
- PCI BAR decoding plus MMIO/bus-master enable path for device drivers.
- xHCI host-controller bring-up: MMIO capability probe, controller halt/reset,
  slot configuration, port power/status logging.
- QEMU `qemu-xhci` + `usb-mouse` boot verified: controller initializes and logs
  the connected USB mouse port.
- Serial output used by logs and userspace stdout/stderr smoke checks.
- Basic port IO support through the HAL layer.
- Timer/PIC initialization paths used by the boot flow.

---

## Skeleton / Planned

- PCI enumeration beyond discovery logs: capabilities, MSI/MSI-X, IRQ routing.
- ACPI support.
- Disk driver, initially ATA/AHCI or a simpler QEMU-friendly target.
- Network driver, likely E1000 or RTL8139 first.
- USB stack: xHCI command ring, event ring, device contexts, enumeration, and
  real HID polling/interrupt transfers.
- Sound driver.
- A cleaner device registration layer for future DevFS integration.

---

## Development Order

```text
PCI enumeration hardening
    -> block device abstraction
    -> disk driver
    -> network driver -> [[../Future/Network-Stack|Network Stack]]
    -> USB xHCI command/event rings
    -> USB HID mouse path
    -> sound later

ACPI
    -> power management
    -> SMP groundwork later
```

---

## Blockers

- Persistent dunitFS needs a block device abstraction before it can leave MemFS/RAM.
- Networking needs a real NIC driver before a TCP/IP stack is useful.
- DevFS is still a skeleton, so devices are not exposed through VFS yet.
- USB devices are not enumerated yet: xHCI command ring, event ring, device
  contexts, and HID interrupt polling are still required after host-controller
  bring-up.

---

## Links

- [[../Completed/HAL|HAL]]
- [[../Completed/VFS-MemFS|VFS + MemFS]]
- [[../Future/Filesystem|Persistent dunitFS]]
