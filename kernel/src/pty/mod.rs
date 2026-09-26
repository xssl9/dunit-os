//! PTY-like endpoint: a bidirectional byte channel between a userspace terminal
//! emulator (the *master*) and a spawned child process whose stdin/stdout it
//! drives (the *slave*).
//!
//! The kernel had no stream/pipe primitive — only bounded 256-byte datagram IPC
//! and shared-memory regions — and interactive stdin only worked for the single
//! foreground process fed by a kernel-side `ExecInput` provider (the GUI path
//! passed `NoExecInput`, so exec'd interactive programs could never read input
//! from a userspace terminal). A `Pty` fills that gap with two byte rings:
//!
//! * `to_slave`  — master writes keystrokes, the slave reads them as stdin.
//! * `to_master` — the slave writes stdout/stderr, the master reads them to paint.
//!
//! Both sides poll cooperatively (read returns "would block" when empty and the
//! caller yields), matching the OS's cooperative scheduler; no wait/wake wiring
//! is needed. Routing is keyed by pid: `sys_read`/`sys_write` consult the table
//! before the legacy terminal-stdin / console paths (see `syscall::sys_read`,
//! `sys_write`). A master owns its ptys; slave attachment records the child pid.

use crate::process::ProcessId;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, Ordering};

/// Per-direction ring capacity. Large enough for a screenful of scrollback plus
/// a typed line; overflow is dropped (terminal backpressure), never blocks.
pub const PTY_RING_CAP: usize = 8192;

/// Result of a slave-side stdin read.
pub enum SlaveRead {
    /// Bytes were copied into the caller buffer.
    Data(usize),
    /// The ring is empty but the master is alive — caller should retry (EAGAIN).
    WouldBlock,
    /// The ring is empty and the master is gone — end of input (EOF).
    Eof,
}

struct Pty {
    master: ProcessId,
    slave: Option<ProcessId>,
    to_slave: VecDeque<u8>,
    to_master: VecDeque<u8>,
}

/// Синглтон таблицы PTY. Тот же паттерн, что у `IpcManager`/`PROCESS_TABLE`:
/// `UnsafeCell`-newtype без `static mut`, доступ кооперативный на одном CPU.
struct PtyTableCell(UnsafeCell<Option<BTreeMap<u32, Pty>>>);
unsafe impl Sync for PtyTableCell {}
static PTY_TABLE: PtyTableCell = PtyTableCell(UnsafeCell::new(None));
static NEXT_PTY_ID: AtomicU64 = AtomicU64::new(1);

fn table() -> &'static mut BTreeMap<u32, Pty> {
    unsafe {
        let slot = PTY_TABLE.0.get();
        if (*slot).is_none() {
            *slot = Some(BTreeMap::new());
        }
        (*slot).as_mut().unwrap()
    }
}

fn ring_push(ring: &mut VecDeque<u8>, data: &[u8]) -> usize {
    let space = PTY_RING_CAP.saturating_sub(ring.len());
    let n = space.min(data.len());
    ring.extend(data[..n].iter().copied());
    n
}

fn ring_pop(ring: &mut VecDeque<u8>, out: &mut [u8]) -> usize {
    let n = ring.len().min(out.len());
    for slot in out.iter_mut().take(n) {
        *slot = ring.pop_front().unwrap();
    }
    n
}

fn find_by_slave(pid: ProcessId) -> Option<u32> {
    table()
        .iter()
        .find(|(_, pty)| pty.slave == Some(pid))
        .map(|(id, _)| *id)
}

/// Create a new PTY owned by `master`. Returns its id (always >= 1).
pub fn create(master: ProcessId) -> u32 {
    let id = NEXT_PTY_ID.fetch_add(1, Ordering::SeqCst) as u32;
    table().insert(
        id,
        Pty {
            master,
            slave: None,
            to_slave: VecDeque::new(),
            to_master: VecDeque::new(),
        },
    );
    id
}

/// Record `slave` as the child attached to `id`. Fails if the pty is unknown or
/// `master` is not its owner.
pub fn attach_slave(id: u32, master: ProcessId, slave: ProcessId) -> bool {
    match table().get_mut(&id) {
        Some(pty) if pty.master == master => {
            pty.slave = Some(slave);
            true
        }
        _ => false,
    }
}

/// Master → slave: enqueue keystrokes for the child's stdin. Returns bytes
/// accepted, or `None` if the pty is unknown / not owned by `master`.
pub fn master_write(id: u32, master: ProcessId, data: &[u8]) -> Option<usize> {
    match table().get_mut(&id) {
        Some(pty) if pty.master == master => Some(ring_push(&mut pty.to_slave, data)),
        _ => None,
    }
}

/// Master ← slave: drain the child's stdout. Returns `(bytes, hangup)` where
/// `hangup` is true once the slave has exited and its output is fully drained.
/// `None` if the pty is unknown / not owned by `master`.
pub fn master_read(id: u32, master: ProcessId, out: &mut [u8]) -> Option<(usize, bool)> {
    match table().get_mut(&id) {
        Some(pty) if pty.master == master => {
            let n = ring_pop(&mut pty.to_master, out);
            let slave_gone = match pty.slave {
                Some(slave) => !crate::process::process_exists(slave),
                None => false,
            };
            Some((n, slave_gone && pty.to_master.is_empty()))
        }
        _ => None,
    }
}

/// Master closes its pty. Fails if unknown / not owned by `master`.
pub fn close(id: u32, master: ProcessId) -> bool {
    match table().get(&id) {
        Some(pty) if pty.master == master => {
            table().remove(&id);
            true
        }
        _ => false,
    }
}

/// Slave-side stdin read, keyed by the reading process's pid. Returns `None`
/// when `pid` is not a pty slave (so `sys_read` falls back to the legacy
/// terminal-stdin path).
pub fn slave_read_stdin(pid: ProcessId, out: &mut [u8]) -> Option<SlaveRead> {
    let id = find_by_slave(pid)?;
    let pty = table().get_mut(&id).unwrap();
    let n = ring_pop(&mut pty.to_slave, out);
    if n > 0 {
        return Some(SlaveRead::Data(n));
    }
    if crate::process::process_exists(pty.master) {
        Some(SlaveRead::WouldBlock)
    } else {
        Some(SlaveRead::Eof)
    }
}

/// Slave-side stdout write, keyed by the writing process's pid. Returns true if
/// `pid` is a pty slave and the bytes were routed to its master (so `sys_write`
/// skips the console/foreground sink); false to fall through.
pub fn slave_write_stdout(pid: ProcessId, data: &[u8]) -> bool {
    match find_by_slave(pid) {
        Some(id) => {
            let pty = table().get_mut(&id).unwrap();
            ring_push(&mut pty.to_master, data);
            true
        }
        None => false,
    }
}

/// Exit hook: drop any ptys owned by `pid` (the master died). Slave death is
/// observed lazily by `master_read` via `process_exists`, so nothing to do there.
pub fn on_process_exit(pid: ProcessId) {
    let dead: Vec<u32> = table()
        .iter()
        .filter(|(_, pty)| pty.master == pid)
        .map(|(id, _)| *id)
        .collect();
    for id in dead {
        table().remove(&id);
    }
}
