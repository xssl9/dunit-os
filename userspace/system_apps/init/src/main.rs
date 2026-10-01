#![no_std]
#![no_main]

//! Dunit init — PID 1 of the GUI session and the userspace service manager (M5 item 3).
//!
//! In GUI mode the kernel launches `/app/init` as the foreground process instead
//! of spawning the compositor directly. init is pure userspace *policy*: the
//! kernel knows nothing about which services exist. init:
//!   1. first-boot provisioning — on a freshly installed /persist it creates the
//!      user-config root and drops a `/persist/.provisioned` marker (idempotent;
//!      skipped on every subsequent boot).
//!   2. service discovery — reads every `/system/services/*.toml` manifest
//!      (one service per file: name/exec/restart/critical/max_restarts/fallback).
//!   3. supervision — spawns each service and polls it with the non-blocking
//!      wait() syscall, applying the restart policy (never / on-failure / always)
//!      with a per-service restart budget, an optional fallback, and a CRITICAL
//!      marker when a critical service stays down.
//!
//! init must NEVER exit (it is PID 1) and must NEVER touch display/input — those
//! are acquired by the compositor via its own syscalls, independent of which
//! process is in the foreground, so `gui_server` running as init's child still
//! owns the screen. Every lifecycle event is logged with an `[INIT]` prefix so
//! tools/qemu_test.py can assert the behaviour from the serial log.

use core::panic::PanicInfo;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use libdunit::{DirEntry, WaitStatus};

const SERVICES_DIR: &str = "/system/services";
const PROVISION_MARKER: &str = "/persist/.provisioned";
const PERSIST_CONFIG_DIR: &str = "/persist/config";
const PROVISION_CONTENT: &str = "schema=1\n";
const POLL_INTERVAL_MS: u64 = 40;
const DEFAULT_MAX_RESTARTS: u32 = 5;
const MAX_SERVICES: usize = 32;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("[INIT] PANIC in init (PID 1)");
    loop {
        core::hint::spin_loop();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunState {
    Idle,
    Running(u32),
    Stopped,
}

struct Service {
    name: String,
    exec: String,
    policy: RestartPolicy,
    critical: bool,
    max_restarts: u32,
    fallback: Option<String>,
    state: RunState,
    restarts: u32,
    fallback_spawned: bool,
}

// ---- small helpers ---------------------------------------------------------

/// Emit one `[INIT] <body>` log line in a SINGLE `write_stdout`. The kernel's
/// stdout mirror (sys_write -> write_stdio) appends a newline to any write that
/// does not already end in one, so a logical line split across several writes is
/// torn apart on the serial log — which would break marker matching. We build
/// the whole line (with its trailing '\n') and print it in one call.
fn emit(body: &str) {
    let mut line = String::from("[INIT] ");
    line.push_str(body);
    line.push('\n');
    libdunit::print(&line);
}

fn log(msg: &str) {
    emit(msg);
}

fn log_svc(name: &str, msg: &str) {
    let mut body = String::from(name);
    body.push_str(": ");
    body.push_str(msg);
    emit(&body);
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    s.push_str(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

fn push_i32(s: &mut String, v: i32) {
    if v < 0 {
        s.push('-');
    }
    push_u32(s, v.unsigned_abs());
}

fn unquote(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for c in s.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - b'0') as u32)?;
    }
    Some(v)
}

// ---- manifest parsing ------------------------------------------------------

/// Parse one `/system/services/*.toml` manifest. The format is a flat set of
/// `key = value` lines (no sections); unknown keys are ignored for forward
/// compatibility. Returns `None` for a manifest with no `exec` — such a file is
/// malformed and is skipped with a warning rather than crashing init.
fn parse_service(text: &str) -> Option<Service> {
    let mut name = String::new();
    let mut exec = String::new();
    let mut policy = RestartPolicy::OnFailure;
    let mut critical = false;
    let mut max_restarts = DEFAULT_MAX_RESTARTS;
    let mut fallback: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, val) = match line.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        let key = key.trim();
        let val = unquote(val.trim());
        match key {
            "name" => name = String::from(val),
            "exec" => exec = String::from(val),
            "restart" => {
                policy = match val {
                    "never" => RestartPolicy::Never,
                    "always" => RestartPolicy::Always,
                    _ => RestartPolicy::OnFailure,
                }
            }
            "critical" => critical = val == "true" || val == "1",
            "max_restarts" => {
                if let Some(n) = parse_u32(val) {
                    max_restarts = n;
                }
            }
            "fallback" => {
                if !val.is_empty() {
                    fallback = Some(String::from(val));
                }
            }
            _ => {}
        }
    }

    if exec.is_empty() {
        return None;
    }
    if name.is_empty() {
        name = exec.clone();
    }
    Some(Service {
        name,
        exec,
        policy,
        critical,
        max_restarts,
        fallback,
        state: RunState::Idle,
        restarts: 0,
        fallback_spawned: false,
    })
}

/// Discover and register every service manifest under `/system/services`.
/// Files are processed in sorted order so the serial log is deterministic.
fn load_services() -> Vec<Service> {
    let mut services: Vec<Service> = Vec::new();
    let mut entries = [DirEntry::empty(); MAX_SERVICES];
    let n = libdunit::readdir(SERVICES_DIR, &mut entries);
    if n < 0 {
        log("no /system/services directory; running as an idle supervisor");
        return services;
    }
    let count = (n as usize).min(MAX_SERVICES);

    let mut names: Vec<String> = Vec::new();
    for entry in entries.iter().take(count) {
        if entry.file_type != libdunit::FILE_TYPE_FILE {
            continue;
        }
        let nm = entry.name();
        if nm.ends_with(".toml") {
            names.push(String::from(nm));
        }
    }
    names.sort_unstable();

    for nm in &names {
        let mut path = String::from(SERVICES_DIR);
        path.push('/');
        path.push_str(nm);
        match libdunit::read_to_string(&path) {
            Ok(text) => match parse_service(&text) {
                Some(svc) => {
                    log_svc(&svc.name, "registered");
                    services.push(svc);
                }
                None => {
                    let mut body = String::from("skipping malformed service manifest ");
                    body.push_str(nm);
                    emit(&body);
                }
            },
            Err(_) => {
                let mut body = String::from("cannot read service manifest ");
                body.push_str(nm);
                emit(&body);
            }
        }
    }
    services
}

// ---- first-boot provisioning ----------------------------------------------

fn provision() {
    if libdunit::file_exists(PROVISION_MARKER) {
        log("/persist already provisioned");
        return;
    }
    log("first boot: provisioning /persist");
    // Create the persistent user-config root. mkdir is best-effort here: the
    // authoritative gate is the marker file written below, so an "already
    // exists" style failure must not abort provisioning.
    let _ = libdunit::mkdir(PERSIST_CONFIG_DIR);
    match libdunit::write_string(PROVISION_MARKER, PROVISION_CONTENT) {
        Ok(()) => log("provisioning complete; wrote /persist/.provisioned"),
        Err(_) => log("WARN provisioning marker write failed (/persist read-only?)"),
    }
}

// ---- supervision -----------------------------------------------------------

fn start_service(svc: &mut Service) {
    let pid = libdunit::spawn(&svc.exec);
    if pid < 0 {
        // Count spawn failures against the budget so a bad exec path cannot spin.
        svc.restarts += 1;
        if svc.restarts >= svc.max_restarts {
            log_svc(&svc.name, "giving up after repeated spawn failures");
            svc.state = RunState::Stopped;
            if svc.critical {
                log_svc(&svc.name, "CRITICAL service is down");
            }
        } else {
            log_svc(&svc.name, "spawn failed; will retry");
            svc.state = RunState::Idle;
        }
        return;
    }
    let mut m = String::from("spawn pid=");
    push_u32(&mut m, pid as u32);
    log_svc(&svc.name, &m);
    svc.state = RunState::Running(pid as u32);
}

fn launch_fallback(svc: &mut Service) {
    if svc.fallback_spawned {
        return;
    }
    let fb = match &svc.fallback {
        Some(fb) => fb.clone(),
        None => return,
    };
    if libdunit::spawn(&fb) >= 0 {
        svc.fallback_spawned = true;
        let mut m = String::from("launched fallback ");
        m.push_str(&fb);
        log_svc(&svc.name, &m);
    } else {
        let mut m = String::from("fallback spawn failed ");
        m.push_str(&fb);
        log_svc(&svc.name, &m);
    }
}

fn on_exit(svc: &mut Service, st: &WaitStatus) {
    if st.spawn_prepared() {
        // Reaped before it ever ran. spawn() leaves children Ready, so this is
        // not expected in steady state; recover without charging the budget.
        log_svc(&svc.name, "reaped before run; restarting");
        svc.state = RunState::Idle;
        return;
    }

    let success = st.exited() && st.code == 0;
    if success {
        log_svc(&svc.name, "exited cleanly (code 0)");
    } else if st.faulted() {
        log_svc(&svc.name, "faulted");
    } else {
        let mut m = String::from("exited code=");
        push_i32(&mut m, st.code);
        m.push_str(" (failure)");
        log_svc(&svc.name, &m);
    }

    let want_restart = match svc.policy {
        RestartPolicy::Never => false,
        RestartPolicy::Always => true,
        RestartPolicy::OnFailure => !success,
    };

    if want_restart && svc.restarts < svc.max_restarts {
        svc.restarts += 1;
        let mut m = String::from("restart (");
        push_u32(&mut m, svc.restarts);
        m.push('/');
        push_u32(&mut m, svc.max_restarts);
        m.push(')');
        log_svc(&svc.name, &m);
        svc.state = RunState::Idle;
        return;
    }

    // Terminal: policy says stop, or the restart budget is exhausted.
    if want_restart {
        log_svc(&svc.name, "restart budget exhausted");
        launch_fallback(svc);
    } else {
        log_svc(&svc.name, "stopped (restart policy)");
    }
    if svc.critical && !success {
        log_svc(&svc.name, "CRITICAL service is down");
    }
    svc.state = RunState::Stopped;
}

fn step(svc: &mut Service) {
    match svc.state {
        RunState::Idle => start_service(svc),
        RunState::Running(pid) => {
            let mut st = WaitStatus::empty();
            let r = libdunit::wait(pid, &mut st);
            if r == libdunit::EAGAIN {
                return; // still running
            }
            if r == pid as isize {
                on_exit(svc, &st);
            } else {
                // ECHILD/ENOENT or other unexpected error: the child is gone.
                // Treat as a failure exit so the restart policy still applies.
                log_svc(&svc.name, "wait error; treating as failure");
                on_exit(svc, &WaitStatus { kind: 1, code: -1 });
            }
        }
        RunState::Stopped => {}
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    log("start: PID 1 userspace service manager");
    provision();

    let mut services = load_services();
    if services.is_empty() {
        log("no services registered; idling");
    } else {
        let mut m = String::from("supervising ");
        push_u32(&mut m, services.len() as u32);
        m.push_str(" service(s)");
        log(&m);
    }

    loop {
        for svc in services.iter_mut() {
            step(svc);
        }
        libdunit::sleep_ms(POLL_INTERVAL_MS);
    }
}

