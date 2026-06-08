# Terminal Mode

**Status:** Done / working kernel terminal  
**Links:** [[../../STATUS|STATUS]] · [[../InProgress/Terminal-Improvements|Terminal Improvements]] · [[Dufetch|dufetch]]

---

## What Works

- Framebuffer console.
- Login-style terminal header.
- Command history.
- Tab autocomplete.
- VFS-backed filesystem commands.
- `dufetch` system summary.
- Basic process/system demo commands.

---

## VFS-backed Commands

| Command | Status |
|---|---|
| `pwd` | working |
| `ls` | working through VFS `readdir` |
| `cd` | working with terminal cwd |
| `mkdir` | working through VFS |
| `touch` | working through VFS |
| `cat` | working through VFS |
| `echo text > file` | working |
| `echo text >> file` | working |
| `rm file` | working for files |
| `tree` | working |

---

## Demo Commands

| Command | Status |
|---|---|
| `dufetch` | working |
| `uname` / `uname -a` | simple static output |
| `free` | static/demo output |
| `ps` / `top` | static/demo output |
| `whoami` | static/demo output |

---

## Important Separation

Kernel terminal cwd is separate from future process cwd. The current process cwd exists for VFS syscalls, but terminal `cd` does not yet become a userspace shell `chdir`.

---

## Remaining Terminal Work

→ [[../InProgress/Terminal-Improvements|Terminal Improvements]]
