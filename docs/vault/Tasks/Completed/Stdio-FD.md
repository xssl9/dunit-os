# Stdio FD Foundation

**Status:** WORKING FOUNDATION

- fd `0/1/2` are stdin/stdout/stderr.
- Foreground terminal programs can receive keyboard-backed stdin.
- stdout/stderr are visible through terminal/serial paths.
- `libdunit` exposes read/write and printing helpers.

Still required for a real shell/platform: pipes, `dup`, redirection inheritance, PTY-like endpoints, event readiness and session/job control. See [[../InProgress/Terminal-Improvements|Terminal migration]].
