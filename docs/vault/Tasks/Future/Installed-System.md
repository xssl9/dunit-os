# Normal Installed System

**Status:** PARTIAL BOOT PATH / PLANNED DISK ROOT
**Depends on:** [[Filesystem|DunitFS v2]] · [[../InProgress/Drivers|Block Drivers]]

## Current state

GPT/installer components, BIOS/UEFI boot payloads and disk boot work. AHCI disk is detected and DunitFS can mount as `/persist`. Однако root остаётся MemFS, applications/assets попадают в kernel/image build, `/boot/userspace` не является источником normal exec, а initrd практически пуст.

## Target installation flow

```text
select/validate disk
 -> partition GPT
 -> create ESP + Dunit system/data layout
 -> format DunitFS
 -> copy system/apps/config defaults
 -> verify hashes
 -> install Limine BIOS/UEFI data
 -> sync and reboot
 -> disk-root init
 -> first-boot user provisioning
 -> write data/config
 -> full stop/start
 -> verify persistence
```

## Tasks

- [ ] Define v1 partition policy: ESP + system/data or ESP + unified root with read-mostly `/system`.
- [ ] Load `init`, services, apps and assets from disk, removing `include_bytes!` as normal runtime source.
- [ ] Add service manager and first-boot provisioning.
- [ ] Make installer transactional and verify every copied artifact before enabling boot.
- [ ] Separate Live, Minimal, Installed and DWM manifests.
- [ ] Live image is read-only installer/recovery; installed image mounts persistent root and does not require installer services.
- [ ] Provide rollback/recovery behavior for interrupted install and invalid system volume.

## Acceptance

- BIOS and UEFI boot the installed disk without ISO.
- Kernel/userspace/apps/config come from persistent system layout.
- User environment survives repeated full shutdown/start.
- Corrupt system/data mount falls back to explicit recovery/read-only mode.
- Installer cannot silently target an ambiguous/wrong disk.
