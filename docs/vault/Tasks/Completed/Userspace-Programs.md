# Userspace Program Builds

**Status:** Done / build pipeline works  
**Links:** [[../../STATUS|STATUS]] · [[Userspace-VFS-Syscalls|Userspace VFS Syscalls]]

---

## Programs Built Into The ISO

| Program | Build status | Runtime status |
|---|---|---|
| `plank` | builds | GUI/runtime integration is experimental |
| `terminal` | builds | not yet the main shell |
| `file_manager` | builds | UI/demo stage |
| `text_editor` | builds | UI/demo stage |
| `settings` | builds | UI/demo stage |
| `system_monitor` | builds | UI/demo stage |

---

## Runtime Foundation Now Available

Userspace has more than just builds now:

- libdunit syscall wrappers.
- hardened syscall register ABI.
- userspace `open/read/write/close`.
- userspace stdout/stderr helpers.
- CPL3 smoke path that returns to kernel.

See:

- [[Syscall-ABI|Syscall ABI + Safe User Copy]]
- [[Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[Stdio-FD|Minimal Stdio FDs]]

---

## Target

All binaries build for:

```text
userspace/x86_64-unknown-none.json
```

Output directory:

```text
build/userspace/
```

---

## Not Done

- Real ELF exec flow for these apps.
- Per-process address spaces.
- Full userspace terminal.
