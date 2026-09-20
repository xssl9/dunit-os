# Terminal and userspace shell migration

**Status:** WORKING RECOVERY TERMINAL / USERSPACE MIGRATION PLANNED
**Previous:** [[../Completed/Terminal-Mode|Terminal Mode]]

## Current state

- Kernel terminal has history, completion and VFS-backed commands.
- Foreground userspace programs receive stdin/stdout and can be executed from Terminal Mode.
- GUI Mode has a full terminal frontend, but command execution still delegates to kernel shell logic.

## Target

- Kernel terminal remains a small recovery console.
- Common userspace shell/session engine serves Terminal and GUI terminal.
- PTY-like endpoint separates terminal frontend from process/session/job control.
- Pipes, descriptor duplication/redirection, environment/PATH and blocking input are native runtime contracts.

## Tasks

- [ ] Common userspace command parser and builtin boundary.
- [ ] Environment and executable lookup.
- [ ] Pipes, `dup`/inheritance and file redirection.
- [ ] Blocking/pollable input and PTY-like API.
- [ ] Sessions/process groups/job control only after process model supports them.
- [ ] GUI terminal becomes ordinary GUI client over the same shell/session service.

## Acceptance

The same command/program behaves consistently in Terminal and GUI terminals; kernel shell is not required during normal desktop session; recovery terminal still works if userspace GUI fails.
