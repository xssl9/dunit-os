# DunitFS v2 и persistence

**Status:** V1 PROTOTYPE / V2 PLANNED
**Depends on:** [[../Completed/VFS-MemFS|VFS + MemFS]] · [[../Completed/Block-Storage-v1|Block Storage]]
**Related:** [[Installed-System|Normal Installed System]]

## Current state

- VFS/MemFS является текущим root filesystem.
- AHCI и legacy VirtIO block devices существуют.
- DunitFS v1 автоматически монтируется как `/persist`.
- V1 имеет superblock/CRC metadata, fixed 64 nodes и contiguous allocation.
- Persistence существования файла подтверждена после полного reboot.

Это полезный prototype, но не безопасный system/user filesystem: нет полноценных directories, rename/unlink, permissions, timestamps, fsync contract, journal/metadata COW и fsck.

## DunitFS v2 scope

- [ ] Versioned superblock, backup/generation and feature flags.
- [ ] Allocation bitmap/extents и безопасное disk-full поведение.
- [ ] Directories, stable identifiers, rename/unlink and orphan cleanup.
- [ ] Timestamps, ownership/permissions and executable metadata.
- [ ] Explicit `fsync`/flush ordering contract.
- [ ] Journal или metadata COW с bounded recovery.
- [ ] `fsck.dunit`, read-only degraded mount и recovery report.
- [ ] Mount table and clear root/system/data policies.

## Target hierarchy

```text
/system/bin        trusted programs
/system/lib        libc/runtime, shared libraries later
/system/share      themes, UI definitions, icons, defaults
/system/services   service manifests
/apps/<id>         application bundles
/users/<uid>/home  documents
/users/<uid>/config
/users/<uid>/data
/var/log           bounded logs
/var/lib           service state
/run               volatile state
```

## Acceptance

- Write + fsync + full VM stop/start + remount + content hash.
- Directory/rename/unlink correctness across reboot.
- Disk-full and corrupted metadata produce bounded errors/recovery.
- Interrupted metadata update recovers old or new state, not arbitrary mixture.
- BIOS/UEFI × AHCI/VirtIO installed images pass persistence matrix.

## Non-goals for first v2

- POSIX completeness, snapshots and distributed/network FS.
- ext2/FAT root compatibility as a substitute for DunitFS correctness.
- Using DunitFS v1 as the only copy of important user data.
