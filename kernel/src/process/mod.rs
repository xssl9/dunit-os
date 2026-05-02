pub mod scheduler;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

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
}

impl Process {
    pub fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel: false,
        }
    }

    pub fn new_kernel(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
            is_kernel: true,
        }
    }

    pub fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }
}
