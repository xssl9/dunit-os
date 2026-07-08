# Persistent dunitFS / Block-backed Filesystem

**Status:** Future  
**Depends on:** [[../Completed/VFS-MemFS|VFS + MemFS Runtime Layer]] · [[../Completed/Block-Storage-v1|Block Storage v1]]

---

## What Is Already Done

The first runtime filesystem stage is complete:

- VFS contract.
- Root MemFS mounted as `/`.
- Runtime file/directory operations.
- Kernel terminal filesystem commands through VFS.
- Userspace `open/read/write/close` syscalls through VFS.
- Block device abstraction with `ramblk0` and QEMU legacy virtio-blk `vd0`.
- Automated read/write sector regression through `build_and_run_multipass.py
  --disk virtio`.

See [[../Completed/VFS-MemFS|VFS + MemFS Runtime Layer]] and [[../Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]].

---

## What This Future Node Means Now

This node is no longer "filesystem from zero". It now tracks persistent storage work:

- persistent dunitFS design on top of `vd0`
- on-disk inode/node format
- mount table beyond root MemFS
- file permissions
- symlinks
- optional ext2/FAT32 research

---

## Blockers

- No mountable on-disk dunitFS yet.
- No persistent allocation model.
- No crash/recovery story.

---

## Non-goals For Next Step

- Do not replace MemFS.
- Do not add journaling yet.
- Do not implement ext2/FAT32 before block device basics.
