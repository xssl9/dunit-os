# Userspace VFS Syscalls

**Status:** Done / smoke-tested  
**Links:** [[../../STATUS|STATUS]] · [[VFS-MemFS|VFS + MemFS]] · [[Syscall-ABI|Syscall ABI]]

---

## Implemented

- `Open(5)`
- `Read(3)`
- `Write(4)`
- `Close(6)`

---

## Behavior

- `open(path_ptr, path_len, flags)` uses process cwd for relative paths.
- `open` returns a process-local fd.
- `read` copies data from VFS into a user buffer.
- `write` copies user buffer into kernel memory before writing.
- `close` closes the VFS handle before removing the process fd.

---

## Smoke Coverage

The CPL3 smoke test checks:

- create/write/read/close success path
- read from write-only fd fails
- write to read-only fd fails
- truncate clears old content
- append writes at EOF
- invalid close fails

Expected serial evidence:

```text
[SYSCALL-FS-TEST] OK
[SYSCALL-FS-SEMANTICS-TEST] OK
```
