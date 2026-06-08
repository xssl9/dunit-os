pub mod scheduler;

use alloc::collections::BTreeMap;
use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

pub type ProcessFd = u32;

pub const FIRST_PROCESS_FD: ProcessFd = 3;
pub const MAX_PROCESS_FD: ProcessFd = 1024;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static mut CURRENT_PROCESS: Option<Process> = None;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Dead,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CpuContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

impl CpuContext {
    pub const fn new() -> Self {
        Self {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0,
        }
    }
}

pub struct Process {
    pub pid: ProcessId,
    pub state: ProcessState,
    pub context: CpuContext,
    pub is_kernel: bool,
    pub cwd: String,
    fd_table: BTreeMap<ProcessFd, FdEntry>,
    next_fd: ProcessFd,
}

impl Process {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel: false,
            cwd: String::from("/"),
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
        }
    }

    pub fn new_kernel(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel: true,
            cwd: String::from("/"),
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
        }
    }

    pub fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }

    pub fn allocate_fd(&mut self, entry: FdEntry) -> Result<ProcessFd, ProcessError> {
        for _ in FIRST_PROCESS_FD..MAX_PROCESS_FD {
            let fd = self.next_fd;
            self.next_fd += 1;
            if self.next_fd >= MAX_PROCESS_FD {
                self.next_fd = FIRST_PROCESS_FD;
            }

            if !self.fd_table.contains_key(&fd) {
                self.fd_table.insert(fd, entry);
                return Ok(fd);
            }
        }

        Err(ProcessError::FdTableFull)
    }

    pub fn get_fd(&self, fd: ProcessFd) -> Option<&FdEntry> {
        self.fd_table.get(&fd)
    }

    pub fn close_fd(&mut self, fd: ProcessFd) -> Result<FdEntry, ProcessError> {
        self.fd_table.remove(&fd).ok_or(ProcessError::InvalidFd)
    }

    pub fn fd_count(&self) -> usize {
        self.fd_table.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    NoCurrentProcess,
    InvalidFd,
    FdTableFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdTarget {
    Vfs(crate::fs::vfs::FileDescriptor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdEntry {
    pub target: FdTarget,
}

impl FdEntry {
    pub const fn vfs(fd: crate::fs::vfs::FileDescriptor) -> Self {
        Self {
            target: FdTarget::Vfs(fd),
        }
    }
}

pub fn allocate_pid() -> ProcessId {
    ProcessId(NEXT_PID.fetch_add(1, Ordering::SeqCst))
}

pub fn init_current_kernel_process() {
    unsafe {
        if CURRENT_PROCESS.is_none() {
            let pid = allocate_pid();
            let mut process = Process::new_kernel(pid);
            process.state = ProcessState::Running;
            CURRENT_PROCESS = Some(process);
        }
    }
}

pub fn current_process() -> Option<&'static Process> {
    unsafe { CURRENT_PROCESS.as_ref() }
}

pub fn current_process_mut() -> Option<&'static mut Process> {
    unsafe { CURRENT_PROCESS.as_mut() }
}

pub fn allocate_fd(entry: FdEntry) -> Result<ProcessFd, ProcessError> {
    current_process_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .allocate_fd(entry)
}

pub fn get_fd(fd: ProcessFd) -> Option<&'static FdEntry> {
    current_process()?.get_fd(fd)
}

pub fn close_fd(fd: ProcessFd) -> Result<FdEntry, ProcessError> {
    current_process_mut()
        .ok_or(ProcessError::NoCurrentProcess)?
        .close_fd(fd)
}
