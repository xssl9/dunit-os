# 🔧 Drivers

**Статус:** 🔧 В процессе  
**Из плана:** [[../../ROADMAP|ROADMAP]] → Task 5  
**Требования:** [[../../Origin/REQUIREMENTS|REQ-7, REQ-12]]

---

## Чеклист

- [ ] **PCI enumeration** ← стартовая точка, нужна для остального
- [ ] **ACPI support**
- [ ] Disk driver (ATA/AHCI)
- [ ] Network driver (RTL8139 / E1000)
- [ ] Sound driver (AC97 / Intel HDA)
- [ ] USB driver (UHCI/OHCI/EHCI/xHCI)

---

## Порядок реализации

```
PCI enumeration
    └── Disk (ATA/AHCI)
    └── Network (E1000) → [[../Future/Network-Stack|Network Stack]]
    └── Sound (AC97)
    └── USB
ACPI → power management
```

---

## PCI — заметки

PCI enumeration — перебор bus/device/function через I/O порты `0xCF8`/`0xCFC`. Уже есть `ports.c` в [[../Completed/HAL|HAL]].

```rust
// Базовый скелет
fn pci_read(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x80000000u32
        | (bus as u32) << 16
        | (device as u32) << 11
        | (func as u32) << 8
        | (offset & 0xFC) as u32;
    // outl(0xCF8, addr); inl(0xCFC)
}
```

---

## Скриншоты

> Место для скринов

## Зависимости

- [[../Completed/HAL|HAL]] — порты уже есть
- [[../Future/Filesystem|Filesystem]] — нужна для disk driver
