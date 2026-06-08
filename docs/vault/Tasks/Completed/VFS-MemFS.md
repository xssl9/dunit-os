# VFS + MemFS Runtime Layer

**Status:** Done / working runtime layer  
**Links:** [[../../STATUS|STATUS]] · [[../../ROADMAP|ROADMAP]] · [[../Future/Filesystem|Persistent dunitFS future work]]

---

## What Works

- VFS initializes during boot.
- Root MemFS is mounted as `/`.
- Path normalization supports absolute paths, relative paths, `.`, and `..`.
- Kernel terminal commands use VFS APIs instead of fake filesystem handlers.
- MemFS supports:
  - directories
  - files
  - `open`
  - `read`
  - `write`
  - `close`
  - `readdir`
  - `create`
  - `mkdir`
  - `remove`
  - `truncate`
  - `stat`
- Base runtime tree exists:
  - `/kernel`
  - `/proc`
  - `/app`
  - `/cfg`
  - `/usr`
  - `/tmp`

---

## Semantics Locked In

- `READ`, `WRITE`, `CREATE`, `TRUNC`, `APPEND` are bitmask flags.
- `TRUNC` requires write access.
- `APPEND` writes at EOF on every write.
- Read on write-only fd returns an error.
- Write on read-only fd returns an error.
- VFS does not duplicate backend file offset; MemFS handle owns file position.

---

## Not Done Here

- Persistent storage.
- Block devices.
- Journaling.
- Permissions.
- Symlinks.
- Page cache.
- Full mount table.
- Real DevFS/ProcFS backends.
