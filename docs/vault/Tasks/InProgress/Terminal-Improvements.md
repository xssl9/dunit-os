# Terminal Improvements

**Status:** In progress  
**Previous:** [[../Completed/Terminal-Mode|Terminal Mode]]

---

## Done In Kernel Terminal

- [x] Command history
- [x] Tab autocomplete
- [x] VFS-backed filesystem commands
- [x] `echo >` and `echo >>`
- [x] `tree`
- [x] `dufetch`

---

## Still To Do

- [ ] Aliases, for example `alias ll='ls -la'`
- [ ] Environment variables: `$PATH`, `$HOME`
- [ ] Pipes: `cmd1 | cmd2`
- [ ] Input redirection: `<`
- [ ] Real userspace terminal process
- [x] Foreground keyboard-backed stdin for userspace exec
- [ ] Shell-grade stdin, job control, and blocking input semantics

---

## Notes

The kernel terminal is usable, but it is still not the final shell
architecture. Foreground userspace programs can receive terminal stdin, but the
next major step is a real userspace shell that talks through stdio and VFS
syscalls.

Related completed foundations:

- [[../Completed/Stdio-FD|Minimal Stdio FDs]]
- [[../Completed/Userspace-VFS-Syscalls|Userspace VFS Syscalls]]
- [[../Completed/Process-FD-Model|Current Process + FD Table]]
