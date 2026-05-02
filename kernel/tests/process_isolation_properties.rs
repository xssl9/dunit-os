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

struct Process {
    pid: ProcessId,
    state: ProcessState,
    is_kernel: bool,
}

impl Process {
    fn new(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            is_kernel: false,
        }
    }

    fn new_kernel(pid: ProcessId) -> Self {
        Self {
            pid,
            state: ProcessState::Ready,
            is_kernel: true,
        }
    }

    fn terminate(&mut self) {
        self.state = ProcessState::Dead;
    }
}

struct Kernel {
    processes: Vec<Process>,
    stable: bool,
}

impl Kernel {
    fn new() -> Self {
        Self {
            processes: Vec::new(),
            stable: true,
        }
    }

    fn add_process(&mut self, process: Process) {
        self.processes.push(process);
    }

    fn simulate_process_crash(&mut self, pid: ProcessId) {
        for process in &mut self.processes {
            if process.pid == pid {
                if process.is_kernel {
                    self.stable = false;
                } else {
                    process.terminate();
                }
                break;
            }
        }
    }

    fn is_stable(&self) -> bool {
        self.stable
    }

    fn other_processes_running(&self, crashed_pid: ProcessId) -> bool {
        self.processes.iter().any(|p| p.pid != crashed_pid && p.state != ProcessState::Dead)
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_process_isolation_on_crash(num_processes in 2usize..10, crash_index in 0usize..9) {
        let mut kernel = Kernel::new();
        
        for i in 0..num_processes {
            let process = Process::new(ProcessId(i as u64));
            kernel.add_process(process);
        }
        
        let crash_index = crash_index % num_processes;
        let crashed_pid = ProcessId(crash_index as u64);
        
        kernel.simulate_process_crash(crashed_pid);
        
        assert!(kernel.is_stable(), "Kernel should remain stable when userspace process crashes");
        assert!(kernel.other_processes_running(crashed_pid), "Other processes should continue running after one crashes");
    }
}
