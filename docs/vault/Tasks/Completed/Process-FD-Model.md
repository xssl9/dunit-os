# Process and FD Foundation

**Status:** WORKING FOUNDATION / RUNTIME EXPANSION REQUIRED

## Delivered

- PID and parent/child records.
- Per-process cwd and fd table with reserved `0/1/2`.
- Per-process address-space objects and kernel syscall stacks.
- Ready/running/dead/fault state and wait/reap reporting.
- Cleanup paths exercised by repeated spawn/fault/kill runtime tests.

## Boundary

Execution remains cooperative and is not a production scheduler. There are no full userspace threads/TLS, process groups, blocking events, generic handle rights or complete exec-image inheritance. See [[../InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime Prerequisites]].
