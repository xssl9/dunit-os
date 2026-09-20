# Userspace VFS Syscalls

**Status:** WORKING FOUNDATION

## Implemented surface

- Open/read/write/close.
- cwd/chdir.
- stat/readdir and related file API wrappers.
- Process-local descriptors and safe user-buffer transfer.
- Access/truncate/append/error semantics covered by smoke/stress programs.

## Boundary

The API serves current MemFS and `/persist` paths, but stable libc-quality contracts still need seek/metadata completeness, pipes/dup, fsync, permissions, poll/events and versioned errno/ABI documentation.
