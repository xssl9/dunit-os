# Minimal Stdio FDs

**Status:** Done / minimal  
**Links:** [[../../STATUS|STATUS]] · [[Process-FD-Model|Process FD Model]] · [[Syscall-ABI|Syscall ABI]]

---

## What Works

- Current process reserves:
  - `fd 0` = stdin
  - `fd 1` = stdout
  - `fd 2` = stderr
- First VFS `open()` returns `fd >= 3`.
- `write(1, buf, len)` writes to serial.
- `write(2, buf, len)` writes to serial.
- `read(0, ...)` returns an honest unsupported error for now.
- libdunit has:
  - `write_stdout`
  - `write_stderr`
  - `print`
  - `println`

---

## Smoke Evidence

```text
[STDOUT] [STDOUT-TEST] hello from userspace
```

---

## Not Done

- Real keyboard-backed stdin.
- Terminal line discipline.
- Userspace terminal process.
- Redirection of stdout/stderr to files or pipes.
