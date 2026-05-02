use super::{CpuContext, Process, ProcessId, ProcessState};
use alloc::vec::Vec;

pub struct Scheduler {
    processes: Vec<Process>,
    current_index: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            processes: Vec::new(),
            current_index: 0,
        }
    }

    pub fn add_process(&mut self, process: Process) {
        self.processes.push(process);
    }

    pub fn remove_process(&mut self, pid: ProcessId) {
        self.processes.retain(|p| p.pid != pid);
        if self.current_index >= self.processes.len() && !self.processes.is_empty() {
            self.current_index = 0;
        }
    }

    pub fn schedule(&mut self) -> Option<&mut Process> {
        if self.processes.is_empty() {
            return None;
        }

        let start_index = self.current_index;
        let len = self.processes.len();
        
        for _ in 0..len {
            self.current_index = (self.current_index + 1) % len;
            
            if self.processes[self.current_index].state == ProcessState::Ready {
                self.processes[self.current_index].state = ProcessState::Running;
                return Some(&mut self.processes[self.current_index]);
            }
            
            if self.current_index == start_index {
                break;
            }
        }

        None
    }

    pub fn current_process(&mut self) -> Option<&mut Process> {
        if self.processes.is_empty() {
            return None;
        }
        Some(&mut self.processes[self.current_index])
    }
}

extern "C" {
    fn switch_context_asm(old_context: *mut CpuContext, new_context: *const CpuContext);
}

pub unsafe fn switch_context(from: &mut Process, to: &Process) {
    from.state = ProcessState::Ready;
    switch_context_asm(&mut from.context as *mut CpuContext, &to.context as *const CpuContext);
}

static mut SCHEDULER_INSTANCE: Option<Scheduler> = None;

pub fn init() {
    // Временно пропускаем инициализацию планировщика
    // TODO: инициализировать после настройки аллокатора
}

pub fn get_scheduler() -> Option<&'static mut Scheduler> {
    unsafe { SCHEDULER_INSTANCE.as_mut() }
}
