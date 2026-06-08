# dufetch

**Status:** Done / terminal command  
**Links:** [[../../STATUS|STATUS]] · [[Terminal-Mode|Terminal Mode]]

---

## What Works

`dufetch` is a kernel terminal command that prints:

- Dunit OS ASCII logo.
- OS name.
- Kernel version string.
- Architecture.
- Terminal mode.
- Shell name.
- Filesystem summary.
- Current process PID.
- Current terminal cwd.
- PMM memory stats when available.
- Framebuffer display info.

---

## Test

Verified through:

```powershell
python build_and_run_multipass.py --qemu-timeout 40 --qemu-log qemu_dufetch.log --qemu-test-commands "dufetch;pwd;ls"
```
