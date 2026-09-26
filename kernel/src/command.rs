use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::vfs::{self, FileType, OpenFlags, VfsError};
use crate::process::{self, ProcessError, ProcessExit, ProcessId, ProcessOutputSink};

pub enum ExecRunError {
    MissingPath,
    Vfs(String, VfsError),
    ProcessCreate,
    ElfLaunch,
    Interrupted,
    StdinUnsupported,
}

pub trait ExecInput {
    fn collect_stdin(&mut self, pid: ProcessId) -> ExecInputResult;
}

pub enum ExecInputResult {
    Provided,
    Interrupted,
    Unsupported,
}

pub struct NoExecInput;

impl ExecInput for NoExecInput {
    fn collect_stdin(&mut self, _pid: ProcessId) -> ExecInputResult {
        ExecInputResult::Unsupported
    }
}

pub fn resolve_exec_path(cwd: &str, path: &str) -> Result<String, VfsError> {
    let vfs = vfs::get_vfs().ok_or(VfsError::IoError)?;
    if path.contains('/') {
        let normalized = vfs.normalize_at(cwd, path)?;
        let stat = vfs.stat_at("/", &normalized)?;
        if stat.file_type == FileType::Directory {
            return Err(VfsError::IsADirectory);
        }
        return Ok(normalized);
    }

    let mut candidate = String::from("/app/");
    candidate.push_str(path);
    let stat = vfs.stat_at("/", &candidate)?;
    if stat.file_type == FileType::Directory {
        return Err(VfsError::IsADirectory);
    }
    Ok(candidate)
}

pub fn read_vfs_file(cwd: &str, path: &str) -> Result<Vec<u8>, VfsError> {
    let vfs = vfs::get_vfs().ok_or(VfsError::IoError)?;
    let fd = vfs.open_at(cwd, path, OpenFlags::READ)?;
    let mut data = Vec::new();
    let mut buf = [0u8; 512];

    loop {
        match vfs.read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(error) => {
                let _ = vfs.close(fd);
                return Err(error);
            }
        }
    }

    vfs.close(fd)?;
    Ok(data)
}

pub fn parse_exec_argv(
    command_line: &str,
    cwd: &str,
) -> Result<(String, Vec<String>), ExecRunError> {
    if command_line.trim().is_empty() {
        return Err(ExecRunError::MissingPath);
    }

    let mut argv = Vec::new();
    for part in command_line.split_whitespace() {
        argv.push(String::from(part));
    }
    if argv.is_empty() {
        return Err(ExecRunError::MissingPath);
    }

    let requested = argv[0].clone();
    let normalized = match resolve_exec_path(cwd, &requested) {
        Ok(path) => path,
        Err(error) => {
            let mut shown = String::new();
            if requested.contains('/') {
                shown.push_str(&requested);
            } else {
                shown.push_str("/app/");
                shown.push_str(&requested);
            }
            return Err(ExecRunError::Vfs(shown, error));
        }
    };
    let argv0 = normalized
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(requested.as_str());
    argv[0] = String::from(argv0);
    Ok((normalized, argv))
}

pub fn run_foreground_exec(
    cwd: &str,
    command_line: &str,
    sink: ProcessOutputSink,
    input: &mut impl ExecInput,
) -> Result<(String, ProcessExit), ExecRunError> {
    let (normalized, argv) = parse_exec_argv(command_line, cwd)?;

    crate::serial_write("[EXEC] loading ");
    crate::serial_write(&normalized);
    crate::serial_write("\r\n");

    let data = read_vfs_file(cwd, &normalized)
        .map_err(|error| ExecRunError::Vfs(normalized.clone(), error))?;
    let pid = process::create_user_process_record(normalized.clone(), true)
        .map_err(|_| ExecRunError::ProcessCreate)?;

    if crate::elf::prepare_process_elf(pid, &data, &argv).is_err() {
        let _ = process::autoreap_process(pid, "exec-prepare-failed");
        return Err(ExecRunError::ElfLaunch);
    }

    crate::serial_write("[ELF-TEST] userspace app started\r\n");
    process::set_foreground_process(Some(pid), sink);
    let run_result = loop {
        match process::enter_user_process(pid) {
            Ok(exit) => break Ok(exit),
            Err(ProcessError::SchedulerUnavailable)
                if process::is_pid_runnable(pid) || process::is_pid_blocked(pid) => {
                if process::is_pid_blocked(pid) {
                    unsafe {
                        core::arch::asm!("sti; hlt", options(nomem, nostack));
                    }
                    continue;
                }
                if process::terminal_stdin_waiting_pid() == Some(pid) {
                    match input.collect_stdin(pid) {
                        ExecInputResult::Provided => {}
                        ExecInputResult::Interrupted => {
                            process::set_foreground_process(None, ProcessOutputSink::SerialOnly);
                            let _ = process::autoreap_process(pid, "exec-interrupted");
                            return Err(ExecRunError::Interrupted);
                        }
                        ExecInputResult::Unsupported => {
                            process::set_foreground_process(None, ProcessOutputSink::SerialOnly);
                            let _ = process::autoreap_process(pid, "exec-stdin-unsupported");
                            return Err(ExecRunError::StdinUnsupported);
                        }
                    }
                }
            }
            Err(_) => break Err(ExecRunError::ElfLaunch),
        }
    };
    process::set_foreground_process(None, ProcessOutputSink::SerialOnly);

    let exit = run_result?;
    crate::serial_write("[EXEC] ");
    crate::serial_write(&normalized);
    match exit.status {
        process::ProcessExitStatus::Exited(code) => {
            crate::serial_write(" returned code=");
            serial_write_i32(code);
        }
        process::ProcessExitStatus::Fault(fault) => {
            crate::serial_write(" killed by ");
            crate::serial_write(fault.reason());
        }
    }
    crate::serial_write("\r\n");

    if let Some(vfs) = vfs::get_vfs() {
        if vfs.open_file_count() == 0 {
            crate::serial_write("[PROCESS-RUN] VFS handles clean\r\n");
        } else {
            crate::serial_write("[PROCESS-RUN] VFS handles leaked\r\n");
        }
    }

    Ok((normalized, exit))
}

/// Prove default-on preemption and SSE isolation with CPU-bound parent/child.
#[cfg(feature = "boot-smoke-tests")]
pub fn run_preemption_smoke() -> bool {
    crate::serial_write("[PREEMPT-TEST] START\r\n");

    process::reset_preemption_count();

    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "preempt_test", ProcessOutputSink::SerialOnly, &mut input);

    let preempts = process::preemption_count();

    let ok = match result {
        Ok((_, exit)) => {
            let _ = process::autoreap_process(exit.pid, "preempt-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0)) && preempts > 0
        }
        Err(_) => false,
    };

    if ok {
        crate::serial_write("[PREEMPT-TEST] OK preempts=");
        serial_write_u32(preempts as u32);
        crate::serial_write("\r\n");
    } else {
        crate::serial_write("[PREEMPT-TEST] FAIL preempts=");
        serial_write_u32(preempts as u32);
        crate::serial_write("\r\n");
    }
    ok
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_thread_smoke() -> bool {
    crate::serial_write("[THREAD-TEST] START\r\n");
    process::reset_preemption_count();
    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "thread_test", ProcessOutputSink::SerialOnly, &mut input);
    let preempts = process::preemption_count();
    let ok = match result {
        Ok((_, exit)) => {
            let clean = process::thread_count_for_pid(exit.pid) == 0;
            let _ = process::autoreap_process(exit.pid, "thread-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0)) && clean && preempts > 0
        }
        Err(_) => false,
    };
    crate::serial_write(if ok { "[THREAD-TEST] OK preempts=" } else { "[THREAD-TEST] FAIL preempts=" });
    serial_write_u32(preempts as u32);
    crate::serial_write("\r\n");
    ok
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_wait_smoke() -> bool {
    crate::serial_write("[WAIT-TEST] START\r\n");
    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "wait_test", ProcessOutputSink::SerialOnly, &mut input);
    let ok = match result {
        Ok((_, exit)) => {
            let clean = process::thread_count_for_pid(exit.pid) == 0;
            let _ = process::autoreap_process(exit.pid, "wait-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0)) && clean
        }
        Err(_) => false,
    };
    crate::serial_write(if ok {
        "[WAIT-TEST] OK\r\n"
    } else {
        "[WAIT-TEST] FAIL\r\n"
    });
    ok
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_vm_smoke() -> bool {
    crate::serial_write("[VM-TEST] START\r\n");
    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "vm_test", ProcessOutputSink::SerialOnly, &mut input);
    let ok = match result {
        Ok((_, exit)) => {
            let _ = process::autoreap_process(exit.pid, "vm-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0))
        }
        Err(_) => false,
    };
    crate::serial_write(if ok { "[VM-TEST] OK\r\n" } else { "[VM-TEST] FAIL\r\n" });
    ok
}

#[cfg(feature = "boot-smoke-tests")]
pub fn run_tls_smoke() -> bool {
    crate::serial_write("[TLS-TEST] START\r\n");
    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "tls_test", ProcessOutputSink::SerialOnly, &mut input);
    let ok = match result {
        Ok((_, exit)) => {
            let _ = process::autoreap_process(exit.pid, "tls-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0))
        }
        Err(_) => false,
    };
    crate::serial_write(if ok { "[TLS-TEST] OK\r\n" } else { "[TLS-TEST] FAIL\r\n" });
    ok
}

/// Exercise the PTY endpoint end to end: `pty_test` (master) creates a pty,
/// `pty_spawn`s `pty_echo` (slave) on it, writes a line, and reads the echo back
/// off the master side — proving master↔slave stdin/stdout streaming works.
#[cfg(feature = "boot-smoke-tests")]
pub fn run_pty_smoke() -> bool {
    crate::serial_write("[PTY-TEST] START\r\n");
    let mut input = NoExecInput;
    let result = run_foreground_exec("/", "pty_test", ProcessOutputSink::SerialOnly, &mut input);
    let ok = match result {
        Ok((_, exit)) => {
            let _ = process::autoreap_process(exit.pid, "pty-smoke");
            matches!(exit.status, process::ProcessExitStatus::Exited(0))
        }
        Err(_) => false,
    };
    crate::serial_write(if ok { "[PTY-TEST] OK\r\n" } else { "[PTY-TEST] FAIL\r\n" });
    ok
}

fn serial_write_i32(value: i32) {
    if value < 0 {
        crate::serial_write("-");
        serial_write_u32(value.wrapping_neg() as u32);
    } else {
        serial_write_u32(value as u32);
    }
}

fn serial_write_u32(mut value: u32) {
    let mut buf = [0u8; 10];
    let mut index = buf.len();

    if value == 0 {
        crate::serial_write("0");
        return;
    }

    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for byte in &buf[index..] {
        let ch = [*byte];
        let s = unsafe { core::str::from_utf8_unchecked(&ch) };
        crate::serial_write(s);
    }
}
