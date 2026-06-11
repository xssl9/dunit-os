pub mod scheduler;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use crate::memory::vmm::{ActiveAddressSpace, AddressSpace, PageFlags, VirtualAddress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

pub type ProcessFd = u32;

pub const FIRST_PROCESS_FD: ProcessFd = 3;
pub const MAX_PROCESS_FD: ProcessFd = 1024;
pub const DEFAULT_KERNEL_STACK_SIZE: usize = 0x40000;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);
static mut PROCESS_TABLE: Option<Vec<ProcessRecord>> = None;
static CURRENT_PID: AtomicU64 = AtomicU64::new(0);
static PROCESS_EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static PROCESS_EXIT_CODE: AtomicI32 = AtomicI32::new(0);
static PROCESS_EXIT_KIND: AtomicI32 = AtomicI32::new(0);

pub const WAIT_KIND_EMPTY: i32 = -1;
pub const WAIT_KIND_SPAWN_PREPARED: i32 = -2;

/// Process lifecycle for the non-preemptive foundation:
///
/// Prepared: process object exists and owns its address space, kernel stack,
/// fd table, cwd and pid, but no executable image has run yet.
/// Ready: executable context has been prepared and a future scheduler may run it.
/// Running: CURRENT_PID points at the process table record while the CPU is in
/// user mode. The process object still lives inside PROCESS_TABLE.
/// Dead: execution finished or faulted; wait observes the real status only when
/// has_run is true. Prepared/not-started children keep an explicit wait status.
/// Reaped: terminal exec or a future wait path consumed the heavyweight
/// ownership. Metadata stays behind briefly for diagnostics, but it is not
/// runnable or waitable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Prepared,
    Ready,
    Running,
    Blocked,
    Dead,
    Reaped,
}

pub struct ProcessRecord {
    pub pid: ProcessId,
    pub parent: Option<ProcessId>,
    pub state: ProcessState,
    pub status: Option<ProcessExitStatus>,
    pub has_run: bool,
    pub waitable: bool,
    pub path: String,
    process: Option<Process>,
}

#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: ProcessId,
    pub parent: Option<ProcessId>,
    pub state: ProcessState,
    pub status: Option<ProcessExitStatus>,
    pub has_run: bool,
    pub waitable: bool,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitRecord {
    pub pid: ProcessId,
    pub kind: i32,
    pub code: i32,
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
    pub status: Option<ProcessExitStatus>,
    address_space: Option<AddressSpace>,
    kernel_stack: Option<Vec<u8>>,
    pub kernel_stack_top: usize,
    pub entry_argc: usize,
    pub entry_argv: usize,
    pub entry_envp: usize,
    fd_table: BTreeMap<ProcessFd, FdEntry>,
    next_fd: ProcessFd,
}

impl Process {
    pub fn new(pid: ProcessId) -> Self {
        match Self::new_user(pid) {
            Ok(process) => process,
            Err(_) => Self::new_without_address_space(pid, false),
        }
    }

    fn new_without_address_space(pid: ProcessId, is_kernel: bool) -> Self {
        let mut process = Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel,
            cwd: String::from("/"),
            status: None,
            address_space: None,
            kernel_stack: None,
            kernel_stack_top: 0,
            entry_argc: 0,
            entry_argv: 0,
            entry_envp: 0,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
        };
        process.reserve_stdio();
        process
    }

    pub fn new_kernel(pid: ProcessId) -> Self {
        Self::new_without_address_space(pid, true)
    }

    pub fn new_user(pid: ProcessId) -> Result<Self, ProcessError> {
        let mut kernel_stack = vec![0u8; DEFAULT_KERNEL_STACK_SIZE];
        let kernel_stack_top = (kernel_stack.as_mut_ptr() as usize + kernel_stack.len()) & !0xF;
        let mut process = Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel: false,
            cwd: String::from("/"),
            status: None,
            address_space: Some(AddressSpace::new().map_err(|_| ProcessError::AddressSpaceCreateFailed)?),
            kernel_stack: Some(kernel_stack),
            kernel_stack_top,
            entry_argc: 0,
            entry_argv: 0,
            entry_envp: 0,
            fd_table: BTreeMap::new(),
            next_fd: FIRST_PROCESS_FD,
        };
        process.reserve_stdio();
        Ok(process)
    }

    pub fn address_space(&self) -> Option<&AddressSpace> {
        self.address_space.as_ref()
    }

    pub fn address_space_mut(&mut self) -> Option<&mut AddressSpace> {
        self.address_space.as_mut()
    }

    pub fn has_kernel_stack(&self) -> bool {
        self.kernel_stack.is_some() && self.kernel_stack_top != 0
    }

    pub fn kernel_stack_top(&self) -> Option<usize> {
        if self.has_kernel_stack() {
            Some(self.kernel_stack_top)
        } else {
            None
        }
    }

    pub unsafe fn install_syscall_stack(&self) -> Result<(), ProcessError> {
        let stack_top = self.kernel_stack_top().ok_or(ProcessError::NoKernelStack)?;
        crate::hal::syscall_set_kernel_stack_top(stack_top as u64);
        Ok(())
    }

    pub unsafe fn reset_syscall_stack_policy() {
        crate::hal::syscall_reset_kernel_stack();
    }

    pub unsafe fn activate_address_space(&self) -> Result<ActiveAddressSpace, ProcessError> {
        self.address_space()
            .ok_or(ProcessError::NoAddressSpace)
            .map(|address_space| address_space.activate())
    }

    pub fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }

    pub fn exit(&mut self, code: i32) {
        self.status = Some(ProcessExitStatus::Exited(code));
        self.terminate();
    }

    pub fn fault(&mut self, fault: ProcessFault) {
        self.status = Some(ProcessExitStatus::Fault(fault));
        self.terminate();
    }

    pub fn cleanup_fds(&mut self) -> usize {
        let fd_table = core::mem::take(&mut self.fd_table);
        let mut closed = 0;

        for (_, entry) in fd_table {
            if let FdTarget::Vfs(vfs_fd) = entry.target {
                if let Some(vfs) = crate::fs::vfs::get_vfs() {
                    let _ = vfs.close(vfs_fd);
                    closed += 1;
                }
            }
        }

        closed
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

    fn reserve_stdio(&mut self) {
        self.fd_table.insert(0, FdEntry::stdin());
        self.fd_table.insert(1, FdEntry::stdout());
        self.fd_table.insert(2, FdEntry::stderr());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    NoCurrentProcess,
    NoSuchProcess,
    NotChild,
    InvalidFd,
    FdTableFull,
    NoAddressSpace,
    AddressSpaceCreateFailed,
    NoKernelStack,
    InvalidUserContext,
    ProcessNotPrepared,
    ProcessAlreadyExists,
    NotRunnable,
    SchedulerUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    pub pid: ProcessId,
    pub status: ProcessExitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitStatus {
    Exited(i32),
    Fault(ProcessFault),
}

impl ProcessExitStatus {
    pub const fn exit_code(self) -> i32 {
        match self {
            ProcessExitStatus::Exited(code) => code,
            ProcessExitStatus::Fault(fault) => fault.exit_code(),
        }
    }

    pub const fn kind_code(self) -> i32 {
        match self {
            ProcessExitStatus::Exited(_) => 0,
            ProcessExitStatus::Fault(fault) => fault_kind_to_i32(fault),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessFault {
    PageFault,
    GeneralProtection,
    InvalidOpcode,
    DivideByZero,
    Unknown,
}

impl ProcessFault {
    pub const fn exit_code(self) -> i32 {
        match self {
            ProcessFault::PageFault => -14,
            ProcessFault::GeneralProtection => -11,
            ProcessFault::InvalidOpcode => -4,
            ProcessFault::DivideByZero => -8,
            ProcessFault::Unknown => -1,
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            ProcessFault::PageFault => "page fault",
            ProcessFault::GeneralProtection => "general protection fault",
            ProcessFault::InvalidOpcode => "invalid opcode",
            ProcessFault::DivideByZero => "divide by zero",
            ProcessFault::Unknown => "user fault",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdTarget {
    Stdin,
    Stdout,
    Stderr,
    Vfs(crate::fs::vfs::FileDescriptor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdEntry {
    pub target: FdTarget,
}

impl FdEntry {
    pub const fn stdin() -> Self {
        Self {
            target: FdTarget::Stdin,
        }
    }

    pub const fn stdout() -> Self {
        Self {
            target: FdTarget::Stdout,
        }
    }

    pub const fn stderr() -> Self {
        Self {
            target: FdTarget::Stderr,
        }
    }

    pub const fn vfs(fd: crate::fs::vfs::FileDescriptor) -> Self {
        Self {
            target: FdTarget::Vfs(fd),
        }
    }
}

pub fn allocate_pid() -> ProcessId {
    ProcessId(NEXT_PID.fetch_add(1, Ordering::SeqCst))
}

fn process_table_mut() -> &'static mut Vec<ProcessRecord> {
    unsafe {
        if PROCESS_TABLE.is_none() {
            PROCESS_TABLE = Some(Vec::new());
        }
        PROCESS_TABLE.as_mut().unwrap()
    }
}

fn process_record_index(table: &[ProcessRecord], pid: ProcessId) -> Option<usize> {
    table.iter().position(|record| record.pid == pid)
}

fn log_process_transition(pid: ProcessId, from: ProcessState, to: ProcessState, reason: &str) {
    crate::memory::serial_write("[PROCESS] pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write(" state=");
    serial_write_state(from);
    crate::memory::serial_write("->");
    serial_write_state(to);
    if !reason.is_empty() {
        crate::memory::serial_write(" reason=");
        crate::memory::serial_write(reason);
    }
    crate::memory::serial_write("\r\n");
}

fn current_pid() -> Option<ProcessId> {
    let pid = CURRENT_PID.load(Ordering::SeqCst);
    if pid == 0 {
        None
    } else {
        Some(ProcessId(pid))
    }
}

pub fn insert_process_record(
    pid: ProcessId,
    parent: Option<ProcessId>,
    path: String,
    state: ProcessState,
    waitable: bool,
    has_run: bool,
    process: Option<Process>,
) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        record.parent = parent;
        record.path = path;
        record.state = state;
        record.waitable = waitable;
        record.has_run = has_run;
        record.status = None;
        record.process = process;
        return;
    }

    table.push(ProcessRecord {
        pid,
        parent,
        state,
        status: None,
        has_run,
        waitable,
        path,
        process,
    });
}

pub fn create_user_process_record(path: String, waitable: bool) -> Result<ProcessId, ProcessError> {
    let pid = allocate_pid();
    let parent = current_pid();
    let mut process = Process::new_user(pid)?;
    process.state = ProcessState::Prepared;
    if let Some(current) = current_process() {
        process.cwd = current.cwd.clone();
    }

    insert_process_record(
        pid,
        parent,
        path.clone(),
        ProcessState::Prepared,
        waitable,
        false,
        Some(process),
    );

    crate::memory::serial_write("[PROCESS] pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write(" parent=");
    match parent {
        Some(parent_pid) => serial_write_u64(parent_pid.0),
        None => crate::memory::serial_write("none"),
    }
    crate::memory::serial_write(" state=prepared path=");
    crate::memory::serial_write(&path);
    crate::memory::serial_write("\r\n");

    Ok(pid)
}

pub fn with_process_mut<R>(
    pid: ProcessId,
    f: impl FnOnce(&mut Process) -> Result<R, ProcessError>,
) -> Result<R, ProcessError> {
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    let process = record.process.as_mut().ok_or(ProcessError::ProcessNotPrepared)?;
    f(process)
}

pub fn mark_process_prepared_as_ready(pid: ProcessId) -> Result<(), ProcessError> {
    {
        let table = process_table_mut();
        let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
        let record = &mut table[index];
        if record.state == ProcessState::Dead || record.state == ProcessState::Reaped {
            return Err(ProcessError::ProcessNotPrepared);
        }
        if record.process.is_none() {
            return Err(ProcessError::ProcessNotPrepared);
        }
        let from = record.state;
        record.state = ProcessState::Ready;
        if let Some(process) = record.process.as_mut() {
            process.state = ProcessState::Ready;
        }
        if from != ProcessState::Ready {
            log_process_transition(pid, from, ProcessState::Ready, "prepare-ready");
        }
    }
    if crate::process::scheduler::enqueue_ready(pid).is_err() {
        crate::memory::serial_write("[SCHED] enqueue rejected pid=");
        serial_write_u64(pid.0);
        crate::memory::serial_write("\r\n");
        return Err(ProcessError::NotRunnable);
    }
    Ok(())
}

pub fn mark_process_ready(pid: ProcessId) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        if record.state != ProcessState::Dead && record.state != ProcessState::Reaped {
            let from = record.state;
            record.state = ProcessState::Ready;
            if let Some(process) = record.process.as_mut() {
                process.state = ProcessState::Ready;
            }
            if from != ProcessState::Ready {
                log_process_transition(pid, from, ProcessState::Ready, "elf-ready");
            }
            if crate::process::scheduler::enqueue_ready(pid).is_err() {
                crate::memory::serial_write("[SCHED] enqueue rejected pid=");
                serial_write_u64(pid.0);
                crate::memory::serial_write("\r\n");
            }
        }
    }
}

pub fn mark_process_started(pid: ProcessId) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, pid) {
        let record = &mut table[index];
        let from = record.state;
        record.state = ProcessState::Running;
        record.has_run = true;
        if from != ProcessState::Running {
            log_process_transition(pid, from, ProcessState::Running, "enter-user");
        }
    }
}

pub fn mark_process_finished(exit: ProcessExit) {
    let table = process_table_mut();
    if let Some(index) = process_record_index(table, exit.pid) {
        let record = &mut table[index];
        let from = record.state;
        record.state = ProcessState::Dead;
        record.status = Some(exit.status);
        record.has_run = true;
        if let Some(process) = record.process.as_mut() {
            process.state = ProcessState::Dead;
            process.status = Some(exit.status);
        }
        crate::process::scheduler::remove(exit.pid);
        if from != ProcessState::Dead {
            log_process_transition(exit.pid, from, ProcessState::Dead, "finished");
        }
    }
}

pub fn reap_process(pid: ProcessId) -> Result<(), ProcessError> {
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let record = &mut table[index];
    if record.state != ProcessState::Dead {
        return Err(ProcessError::NotRunnable);
    }
    record.process = None;
    record.state = ProcessState::Reaped;
    record.waitable = false;
    crate::process::scheduler::remove(pid);
    crate::memory::serial_write("[PROCESS] pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write(" state=reaped\r\n");
    Ok(())
}

pub fn process_exists(pid: ProcessId) -> bool {
    let table = process_table_mut();
    process_record_index(table, pid).is_some()
}

pub fn is_pid_runnable(pid: ProcessId) -> bool {
    let table = process_table_mut();
    let Some(index) = process_record_index(table, pid) else {
        return false;
    };
    let record = &table[index];
    if record.state != ProcessState::Ready {
        return false;
    }
    let Some(process) = record.process.as_ref() else {
        return false;
    };
    !process.is_kernel
        && process.context.rip != 0
        && process.context.rsp != 0
        && process.address_space().is_some()
        && process.kernel_stack_top().is_some()
}

pub fn wait_for_child(requested_pid: ProcessId) -> Result<WaitRecord, ProcessError> {
    let parent_pid = current_process()
        .ok_or(ProcessError::NoCurrentProcess)?
        .pid;
    let table = process_table_mut();
    let has_requested_process = requested_pid.0 == 0
        || table.iter().any(|record| record.pid == requested_pid);

    let mut child_index = None;
    for (index, record) in table.iter().enumerate() {
        if record.parent != Some(parent_pid) || !record.waitable {
            continue;
        }
        if requested_pid.0 != 0 && record.pid != requested_pid {
            continue;
        }
        child_index = Some(index);
        break;
    }

    let index = match child_index {
        Some(index) => index,
        None if has_requested_process => return Err(ProcessError::NotChild),
        None => return Err(ProcessError::NoSuchProcess),
    };

    let record = &table[index];
    let (kind, code) = match record.status {
        Some(status) if record.has_run => (status.kind_code(), status.exit_code()),
        _ if record.state == ProcessState::Prepared && !record.has_run => (WAIT_KIND_SPAWN_PREPARED, 0),
        _ if matches!(record.state, ProcessState::Ready | ProcessState::Running | ProcessState::Blocked) => {
            return Err(ProcessError::NotRunnable);
        }
        _ => return Err(ProcessError::ProcessNotPrepared),
    };

    let pid = record.pid;
    let from = record.state;
    log_process_transition(pid, from, ProcessState::Reaped, "wait");
    let mut record = table.remove(index);
    if let Some(mut process) = record.process.take() {
        let closed = process.cleanup_fds();
        if closed > 0 {
            crate::memory::serial_write("[PROCESS] reaped pid=");
            serial_write_u64(pid.0);
            crate::memory::serial_write(" cleaned-fds=");
            serial_write_usize(closed);
            crate::memory::serial_write("\r\n");
        }
    }

    Ok(WaitRecord { pid, kind, code })
}

pub fn cleanup_prepared_children(parent_pid: ProcessId) -> usize {
    let table = process_table_mut();
    let mut removed = 0;
    let mut index = 0;
    while index < table.len() {
        let should_remove = table[index].parent == Some(parent_pid)
            && !table[index].has_run
            && table[index].process.is_some();
        if should_remove {
            let pid = table[index].pid;
            let from = table[index].state;
            log_process_transition(pid, from, ProcessState::Reaped, "parent-exit");
            let mut record = table.remove(index);
            if let Some(mut process) = record.process.take() {
                let _ = process.cleanup_fds();
            }
            crate::process::scheduler::remove(pid);
            removed += 1;
        } else {
            index += 1;
        }
    }
    removed
}

pub fn autoreap_process(pid: ProcessId, reason: &str) -> Result<(), ProcessError> {
    let table = process_table_mut();
    let index = process_record_index(table, pid).ok_or(ProcessError::NoSuchProcess)?;
    let from = table[index].state;
    log_process_transition(pid, from, ProcessState::Reaped, reason);
    let mut record = table.remove(index);
    if let Some(mut process) = record.process.take() {
        let _ = process.cleanup_fds();
    }
    Ok(())
}

pub fn snapshot_processes(out: &mut Vec<ProcessSnapshot>) {
    out.clear();
    let table = process_table_mut();
    for record in table.iter() {
        out.push(ProcessSnapshot {
            pid: record.pid,
            parent: record.parent,
            state: record.state,
            status: record.status,
            has_run: record.has_run,
            waitable: record.waitable,
            path: record.path.clone(),
        });
    }
}

pub fn init_current_kernel_process() {
    if CURRENT_PID.load(Ordering::SeqCst) != 0 {
        return;
    }

    let pid = allocate_pid();
    let mut process = Process::new_kernel(pid);
    process.state = ProcessState::Running;
    insert_process_record(
        pid,
        None,
        String::from("kernel"),
        ProcessState::Running,
        false,
        true,
        Some(process),
    );
    CURRENT_PID.store(pid.0, Ordering::SeqCst);
}

pub fn current_process() -> Option<&'static Process> {
    let pid = current_pid()?;
    let table = process_table_mut();
    let index = process_record_index(table, pid)?;
    table[index].process.as_ref()
}

pub fn current_process_mut() -> Option<&'static mut Process> {
    let pid = current_pid()?;
    let table = process_table_mut();
    let index = process_record_index(table, pid)?;
    table[index].process.as_mut()
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

pub fn exit_current_process(code: i32) -> Option<ProcessId> {
    let process = current_process_mut()?;
    process.exit(code);
    Some(process.pid)
}

pub fn request_current_user_exit(code: i32) -> Option<ProcessId> {
    let process = current_process()?;
    if process.is_kernel {
        return None;
    }
    PROCESS_EXIT_CODE.store(code, Ordering::SeqCst);
    PROCESS_EXIT_KIND.store(0, Ordering::SeqCst);
    PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    Some(process.pid)
}

pub fn request_current_user_fault(fault: ProcessFault) -> Option<ProcessId> {
    let process = current_process()?;
    if process.is_kernel {
        return None;
    }
    PROCESS_EXIT_CODE.store(fault.exit_code(), Ordering::SeqCst);
    PROCESS_EXIT_KIND.store(fault_kind_to_i32(fault), Ordering::SeqCst);
    PROCESS_EXIT_REQUESTED.store(true, Ordering::SeqCst);
    Some(process.pid)
}

pub fn user_fault_escape_requested() -> bool {
    PROCESS_EXIT_REQUESTED.load(Ordering::SeqCst)
        && PROCESS_EXIT_KIND.load(Ordering::SeqCst) != 0
}

fn take_process_exit_request() -> Option<(i32, ProcessExitStatus)> {
    if PROCESS_EXIT_REQUESTED.swap(false, Ordering::SeqCst) {
        let code = PROCESS_EXIT_CODE.load(Ordering::SeqCst);
        let kind = PROCESS_EXIT_KIND.swap(0, Ordering::SeqCst);
        let status = match kind {
            0 => ProcessExitStatus::Exited(code),
            value => ProcessExitStatus::Fault(fault_kind_from_i32(value)),
        };
        Some((code, status))
    } else {
        None
    }
}

pub fn enter_user_process(pid: ProcessId) -> Result<ProcessExit, ProcessError> {
    let previous_pid = CURRENT_PID.load(Ordering::SeqCst);
    let (entry, user_stack, entry_argc, entry_argv, entry_envp) = with_process_mut(pid, |process| {
        if process.is_kernel {
            return Err(ProcessError::InvalidUserContext);
        }
        if process.context.rip == 0 || process.context.rsp == 0 {
            return Err(ProcessError::InvalidUserContext);
        }
        if process.address_space().is_none() {
            return Err(ProcessError::NoAddressSpace);
        }
        if process.kernel_stack_top().is_none() {
            return Err(ProcessError::NoKernelStack);
        }

        process.state = ProcessState::Running;
        Ok((
            process.context.rip,
            process.context.rsp,
            process.entry_argc as u64,
            process.entry_argv as u64,
            process.entry_envp as u64,
        ))
    })?;

    crate::memory::serial_write("[PROCESS-RUN] starting pid=");
    serial_write_u64(pid.0);
    crate::memory::serial_write("\r\n");
    mark_process_started(pid);
    CURRENT_PID.store(pid.0, Ordering::SeqCst);

    let run_result = unsafe {
        match current_process() {
            Some(current) => {
                match current.install_syscall_stack() {
                    Ok(()) => {
                        match current.activate_address_space() {
                            Ok(active) => {
                                crate::memory::serial_write("[PROCESS-RUN] entered user mode\r\n");
                                crate::hal::run_user_process(
                                    entry,
                                    user_stack,
                                    entry_argc,
                                    entry_argv,
                                    entry_envp,
                                );
                                drop(active);
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            None => Err(ProcessError::NoCurrentProcess),
        }
    };

    unsafe {
        Process::reset_syscall_stack_policy();
    }

    let mut status = ProcessExitStatus::Fault(ProcessFault::Unknown);
    let closed = with_process_mut(pid, |finished| {
        status = if let Some((_code, requested_status)) = take_process_exit_request() {
            match requested_status {
                ProcessExitStatus::Exited(code) => finished.exit(code),
                ProcessExitStatus::Fault(fault) => finished.fault(fault),
            }
            requested_status
        } else if finished.state != ProcessState::Dead {
            finished.fault(ProcessFault::Unknown);
            ProcessExitStatus::Fault(ProcessFault::Unknown)
        } else {
            finished.status.unwrap_or(ProcessExitStatus::Fault(ProcessFault::Unknown))
        };
        Ok(finished.cleanup_fds())
    })?;
    if closed > 0 {
        crate::memory::serial_write("[PROCESS-RUN] cleaned fds=");
        serial_write_usize(closed);
        crate::memory::serial_write("\r\n");
    }
    let reaped_children = cleanup_prepared_children(pid);
    if reaped_children > 0 {
        crate::memory::serial_write("[PROCESS-RUN] reaped prepared children=");
        serial_write_usize(reaped_children);
        crate::memory::serial_write("\r\n");
    }

    CURRENT_PID.store(previous_pid, Ordering::SeqCst);
    unsafe {
        match current_process() {
            Some(current) if !current.is_kernel && current.kernel_stack_top().is_some() => {
                let _ = current.install_syscall_stack();
            }
            _ => Process::reset_syscall_stack_policy(),
        }
    }

    let exit = ProcessExit { pid, status };
    mark_process_finished(exit);
    run_result.map(|_| exit)
}

const fn fault_kind_to_i32(fault: ProcessFault) -> i32 {
    match fault {
        ProcessFault::PageFault => 1,
        ProcessFault::GeneralProtection => 2,
        ProcessFault::InvalidOpcode => 3,
        ProcessFault::DivideByZero => 4,
        ProcessFault::Unknown => 5,
    }
}

const fn fault_kind_from_i32(value: i32) -> ProcessFault {
    match value {
        1 => ProcessFault::PageFault,
        2 => ProcessFault::GeneralProtection,
        3 => ProcessFault::InvalidOpcode,
        4 => ProcessFault::DivideByZero,
        _ => ProcessFault::Unknown,
    }
}

pub fn run_process_address_space_smoke() -> bool {
    crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] START\r\n");

    let pid = allocate_pid();
    let mut process = match Process::new_user(pid) {
        Ok(process) => process,
        Err(_) => {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] create failed\r\n");
            return false;
        }
    };

    let address_space = match process.address_space.as_mut() {
        Some(address_space) => address_space,
        None => {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] no address space\r\n");
            return false;
        }
    };

    let user_page = VirtualAddress::from_usize(0x0050_0000);
    if address_space
        .map_user_page(user_page, PageFlags::WRITABLE)
        .is_err()
    {
        crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] map failed\r\n");
        return false;
    }

    unsafe {
        let _active = process.activate_address_space().ok();
        if _active.is_none() {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] activate failed\r\n");
            return false;
        }

        let ptr = user_page.as_usize() as *mut u64;
        core::ptr::write_volatile(ptr, 0x5052_4F43_4153_5043);
        if core::ptr::read_volatile(ptr) != 0x5052_4F43_4153_5043 {
            crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] user page check failed\r\n");
            return false;
        }
    }

    crate::memory::serial_write("[PROCESS-ADDRSPACE-TEST] OK\r\n");
    true
}

fn serial_write_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut index = buf.len();

    if value == 0 {
        crate::memory::serial_write("0");
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
        crate::memory::serial_write(s);
    }
}

fn serial_write_usize(value: usize) {
    serial_write_u64(value as u64);
}

fn serial_write_state(state: ProcessState) {
    let name = match state {
        ProcessState::Prepared => "Prepared",
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::Blocked => "Blocked",
        ProcessState::Dead => "Dead",
        ProcessState::Reaped => "Reaped",
    };
    crate::memory::serial_write(name);
}

pub fn run_process_kernel_stack_smoke() -> bool {
    crate::memory::serial_write("[PROCESS-KSTACK-TEST] START\r\n");

    let pid = allocate_pid();
    let process = match Process::new_user(pid) {
        Ok(process) => process,
        Err(_) => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] create failed\r\n");
            return false;
        }
    };

    match process.kernel_stack_top() {
        Some(stack_top) if stack_top & 0xF == 0 => {}
        Some(_) => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] unaligned stack top\r\n");
            return false;
        }
        None => {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] missing kernel stack\r\n");
            return false;
        }
    }

    crate::memory::serial_write("[PROCESS-KSTACK-TEST] stack ready\r\n");

    let ok = unsafe {
        if process.install_syscall_stack().is_err() {
            crate::memory::serial_write("[PROCESS-KSTACK-TEST] install failed\r\n");
            return false;
        }

        let ok = crate::syscall::run_userspace_syscall_smoke();
        Process::reset_syscall_stack_policy();
        ok
    };

    if !ok {
        crate::memory::serial_write("[PROCESS-KSTACK-TEST] syscall smoke failed\r\n");
        return false;
    }

    crate::memory::serial_write("[PROCESS-KSTACK-TEST] OK\r\n");
    true
}
