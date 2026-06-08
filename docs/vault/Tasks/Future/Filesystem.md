# Persistent dunitFS / Block-backed Filesystem

**Status:** Future  
**Depends on:** [[../Completed/VFS-MemFS|VFS + MemFS Runtime Layer]] · [[../InProgress/Drivers|Disk Driver]]

---

## What Is Already Done

The first runtime filesystem stage is complete:

- VFS contract.
- Root MemFS mounted as `/`.
- Runtime file/directory operations.
- Kernel terminal filesystem commands through VFS.
- Userspace `open/read/write/close` syscalls through VFS.

See [[../Completed/VFS-MemFS|VFS + MemFS Runtime Layer]] and [[../Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]].

---

## What This Future Node Means Now

This node is no longer "filesystem from zero". It now tracks persistent storage work:

- block device abstraction
- disk driver integration
- persistent dunitFS design
- on-disk inode/node format
- mount table beyond root MemFS
- file permissions
- symlinks
- optional ext2/FAT32 research

---

## Blockers

- No block device layer.
- No ATA/AHCI driver.
- No persistent allocation model.
- No crash/recovery story.

---

## Non-goals For Next Step

- Do not replace MemFS.
- Do not add journaling yet.
- Do not implement ext2/FAT32 before block device basics.
