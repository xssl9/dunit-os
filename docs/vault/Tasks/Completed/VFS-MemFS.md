# VFS + MemFS Runtime Layer

**Status:** COMPLETED RUNTIME FOUNDATION
**Next:** [[../Future/Filesystem|DunitFS v2]] · [[../Future/Installed-System|Installed System]]

## Delivered

- Root MemFS and normalized absolute/relative paths.
- Files/directories, open/read/write/close/readdir/create/mkdir/remove/truncate/stat.
- Access flags and per-handle offsets.
- Runtime trees including `/app`, `/proc`, `/dev`, `/tmp` and configuration paths.
- Kernel terminal and userspace syscalls use VFS APIs.

## Boundary

MemFS is still the normal root and disappears on reboot. DunitFS v1 mounts separately as `/persist`; it does not replace root. Permissions, robust mounts, disk-root init and crash recovery belong to future storage milestones.
