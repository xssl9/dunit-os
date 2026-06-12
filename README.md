<p align="center">
  <img src="image.png" width="700" alt="Dunit OS logo"/>
</p>

# Dunit OS

Dunit OS is a small x86_64 hobby operating system built around a Rust kernel,
a C/NASM hardware layer, a Limine boot flow, and a growing userspace runtime.

It is not a polished desktop OS yet. The current system is a terminal-first OS
prototype with real userspace ELF execution, a memory-backed filesystem,
syscalls, process records, recoverable userspace faults, and early cooperative
scheduler groundwork.

## Current State

What works today:

- Limine boot with Terminal Mode first and GUI Mode still available.
- HAL in C/NASM: GDT, IDT, interrupt entry, syscall entry, context switch stubs,
  port I/O, and low-level boot handoff.
- Rust `no_std` kernel with PMM, VMM, heap, address-space setup, and basic fault
  recovery for userspace.
- Framebuffer-backed kernel terminal with command parsing, history, autocomplete,
  and honest system commands.
- VFS with MemFS as the root filesystem.
- `/app` userspace ELF binaries embedded into MemFS.
- `/assets` image assets, including `dr15.bmp` and `logo.bmp`.
- Userspace syscall ABI for read/write/open/close, framebuffer drawing,
  spawn/wait foundation, pid, cwd/chdir, sleep, debug log, and cooperative yield.
- Userspace exec ABI with `argc`, `argv`, and `envp`.
- PATH lookup through `/app`, so both `exec /app/elf_demo` and `exec elf_demo`
  style commands are supported.
- stdio fd foundation: stdin returns EOF, stdout/stderr write to the terminal log.
- Process table with real records, parent/child relation, wait/reap behavior,
  terminal exec autoreap, exit codes, and fault statuses.
- Runnable spawn foundation: `spawn` prepares an ELF child into Ready state, but
  does not perform real child context switching yet.
- Cooperative scheduler foundation: validated PID ready queue and `yield`
  syscall that reports no real switch yet.

## Included Userspace Apps

Current system apps in `/app`:

- `elf_demo` - minimal userspace hello-world.
- `fs_test` - VFS syscall smoke test.
- `exit_test` - process exit test.
- `args_test` - argv ABI test.
- `cwd_test` - getcwd/chdir ABI test.
- `path_test` - PATH and spawn/wait contract test.
- `stdin_test` - stdin EOF foundation test.
- `scheduler_test` - scheduler/yield foundation test.
- `spawn_ready_test` - runnable spawn foundation test.
- `image_demo` - framebuffer drawing demo.
- `bmp_viewer` - BMP renderer; defaults to `/assets/logo.bmp`.
- `fault_pf` - recoverable page fault test.
- `fault_ud` - recoverable invalid opcode test.

Example terminal commands:

```text
help
dufetch
ls /app
ls /assets
exec args_test one two
exec bmp_viewer
exec bmp_viewer /assets/dr15.bmp
exec fault_pf
ps
pwd
```

## Architecture

```text
                 userspace Rust ELF apps
        args_test | fs_test | bmp_viewer | ...
                            |
                         libdunit
                            |
                  syscall ABI / exec ABI
                            |
                  Rust kernel subsystems
      process table | VFS/MemFS | ELF | PMM/VMM | terminal
                            |
                         C/NASM HAL
          boot handoff | GDT | IDT | interrupts | syscall entry
                            |
                         Limine/QEMU
```

The project is still intentionally single-process-at-a-time for real execution.
The process table and scheduler queue are being shaped so real cooperative
multitasking can be added without fake success behavior.

## Boot Modes

`limine.conf` is the normal interactive boot menu:

```text
timeout: 5

/Dunit OS - GUI Mode
    resolution: 1600x900x32

/Dunit OS - Terminal Mode
    cmdline: mode=terminal
```

Automated tests use separate configs so they never depend on the normal boot
menu:

- `limine_test_terminal.conf`: terminal mode, timeout 0.
- `limine_test_gui.conf`: GUI mode, timeout 0.

Terminal Mode is the reliable development path. GUI Mode exists, but it is not
the focus of the current runtime milestones.

## Honest Limitations

Not implemented yet:

- Real cooperative context switching between userspace processes.
- Timer preemption.
- SMP.
- Disk-backed filesystem.
- Network stack.
- Userspace terminal/shell process.
- Full libc.
- ACPI/QEMU shutdown.
- Real RTC/date source.

Current foundation behavior:

- `spawn` prepares a Ready child, but execution is not scheduled yet.
- `wait` on Ready/Running children returns `EAGAIN` instead of faking success.
- `yield` reports a candidate process but returns "switch not implemented".
- `stdin` is EOF-only.
- `/assets/logo.bmp` and `/assets/dr15.bmp` are embedded as small generated BMP
  previews to keep kernel heap pressure reasonable.

## Roadmap

### 1. Cooperative Multitasking

- Store all runnable processes in the process table.
- Keep scheduler queue as PID-only ownership metadata.
- Save/restore userspace CPU context on `yield`.
- Run Ready child processes created by `spawn`.
- Make `wait` block or return clean nonblocking statuses by contract.

### 2. Runtime Contracts

- Tighten syscall error codes and userspace wrappers.
- Expand stdin beyond EOF-only behavior.
- Add better process introspection for `ps`.
- Keep fault diagnostics recoverable and readable.

### 3. Filesystem Growth

- Move beyond embedded MemFS assets.
- Add a disk-backed filesystem path.
- Add mount/unmount semantics.
- Keep `/app` and `/assets` as early boot/system locations.

### 4. Terminal And Tools

- Make terminal commands less kernel-hardcoded over time.
- Add more userspace tools.
- Add file inspection/editing primitives.
- Improve automated regression coverage.

### 5. GUI Later

- Revisit GUI mode after scheduler/runtime contracts are strong.
- Prefer real userspace GUI processes over fake desktop state.
- Add input, rendering, and window/compositor contracts gradually.

## Repository Map

```text
hal/                         C/NASM hardware layer
kernel/                      Rust no_std kernel
userspace/libdunit/          Userspace syscall/startup helper library
userspace/system_apps/       Small Rust ELF apps embedded into /app
docs/                        Design notes and milestone context
build_and_run_multipass.py   Canonical build/test automation
limine.conf                  Normal interactive boot menu
limine_test_terminal.conf    Automated terminal test boot config
limine_test_gui.conf         Automated GUI test boot config
```

## License

MIT License.
