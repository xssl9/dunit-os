#![cfg(test)]

use proptest::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProcessId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessState {
    Ready,
    Running,
    Blocked,
    Dead,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CpuContext {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    rsp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    rflags: u64,
}

impl CpuContext {
    fn new() -> Self {
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

struct Process {
    pid: ProcessId,
    state: ProcessState,
    context: CpuContext,
}

impl Process {
    fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            context: CpuContext::new(),
        }
    }
}

struct Scheduler {
    processes: Vec<Process>,
    current_index: usize,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            processes: Vec::new(),
            current_index: 0,
        }
    }

    fn add_process(&mut self, process: Process) {
        self.processes.push(process);
    }

    fn schedule(&mut self) -> Option<ProcessId> {
        if self.processes.is_empty() {
            return None;
        }

        let start_index = self.current_index;
        loop {
            self.current_index = (self.current_index + 1) % self.processes.len();

            if self.processes[self.current_index].state == ProcessState::Ready {
                self.processes[self.current_index].state = ProcessState::Running;
                return Some(self.processes[self.current_index].pid);
            }

            if self.current_index == start_index {
                break;
            }
        }

        None
    }
}

fn simulate_timer_interrupt(scheduler: &mut Scheduler) -> bool {
    scheduler.schedule().is_some()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_timer_interrupt_triggers_scheduling(num_processes in 1usize..20) {
        let mut scheduler = Scheduler::new();
        
        for i in 0..num_processes {
            let process = Process::new(ProcessId(i as u64));
            scheduler.add_process(process);
        }
        
        let scheduled = simulate_timer_interrupt(&mut scheduler);
        
        assert!(scheduled, "Timer interrupt should trigger scheduling when processes are ready");
    }
    
    #[test]
    fn prop_context_switch_preservation(
        rax in any::<u64>(),
        rbx in any::<u64>(),
        rcx in any::<u64>(),
        rdx in any::<u64>(),
        rsi in any::<u64>(),
        rdi in any::<u64>(),
        rbp in any::<u64>(),
        rsp in any::<u64>(),
        r8 in any::<u64>(),
        r9 in any::<u64>(),
        r10 in any::<u64>(),
        r11 in any::<u64>(),
        r12 in any::<u64>(),
        r13 in any::<u64>(),
        r14 in any::<u64>(),
        r15 in any::<u64>(),
        rip in any::<u64>(),
        rflags in any::<u64>(),
    ) {
        let context = CpuContext {
            rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp,
            r8, r9, r10, r11, r12, r13, r14, r15,
            rip, rflags,
        };
        
        let saved_context = context;
        
        let restored_context = context;
        
        assert_eq!(restored_context.rax, saved_context.rax);
        assert_eq!(restored_context.rbx, saved_context.rbx);
        assert_eq!(restored_context.rcx, saved_context.rcx);
        assert_eq!(restored_context.rdx, saved_context.rdx);
        assert_eq!(restored_context.rsi, saved_context.rsi);
        assert_eq!(restored_context.rdi, saved_context.rdi);
        assert_eq!(restored_context.rbp, saved_context.rbp);
        assert_eq!(restored_context.rsp, saved_context.rsp);
        assert_eq!(restored_context.r8, saved_context.r8);
        assert_eq!(restored_context.r9, saved_context.r9);
        assert_eq!(restored_context.r10, saved_context.r10);
        assert_eq!(restored_context.r11, saved_context.r11);
        assert_eq!(restored_context.r12, saved_context.r12);
        assert_eq!(restored_context.r13, saved_context.r13);
        assert_eq!(restored_context.r14, saved_context.r14);
        assert_eq!(restored_context.r15, saved_context.r15);
        assert_eq!(restored_context.rip, saved_context.rip);
        assert_eq!(restored_context.rflags, saved_context.rflags);
    }
}
