# Terminal Mode

**Status:** WORKING / RECOVERY TERMINAL

## Delivered

- Framebuffer console and login-style prompt.
- History, tab completion and VFS-backed commands.
- `pwd`, `ls`, `cd`, `mkdir`, `touch`, `cat`, `echo`, `rm`, `tree` and diagnostics.
- Userspace `exec` with stdin/stdout, exit/fault reporting.
- Runtime/process/storage/device diagnostic commands.

## Architecture boundary

Kernel terminal remains useful as recovery environment, but it is not the final shell. [[../InProgress/Terminal-Improvements|Userspace shell/session migration]] will unify Terminal and GUI terminals via stdio/PTY-like contracts.

## Verification

Automated terminal boot must reach `root@dunit:~#` via `tools/qemu_test.py` and then run subsystem-specific commands.
