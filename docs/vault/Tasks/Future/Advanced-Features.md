# 🔮 Advanced Features

**Статус:** 🔮 Будущее  
**Из плана:** [[../../ROADMAP|ROADMAP]] → Task 10

---

## Чеклист

- [ ] **Multi-core (SMP)** — APIC, CPU affinity, spinlocks
- [ ] **Power management (ACPI)** — suspend, shutdown
- [ ] **Swap support** — swapfile / swap partition
- [ ] **Kernel modules** — динамическая загрузка .ko
- [ ] **GDB stub** — удалённая отладка через serial

## Заметки

### SMP
Нужен LAPIC (Local APIC) вместо PIC, BSP инициализирует AP через SIPI.

### GDB stub
Serial port уже используется для stdio в QEMU (`-serial stdio`) — можно использовать для GDB remote protocol.

## Скриншоты

> Место для скринов
