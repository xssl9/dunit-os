use super::{ProcessError, ProcessId};
use alloc::vec::Vec;

pub struct Scheduler {
    ready_queue: Vec<ProcessId>,
    cursor: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
            cursor: 0,
        }
    }

    pub fn enqueue(&mut self, pid: ProcessId) -> Result<(), ProcessError> {
        if !super::is_pid_runnable(pid) {
            return Err(ProcessError::NotRunnable);
        }
        if !self.ready_queue.contains(&pid) {
            self.ready_queue.push(pid);
        }
        Ok(())
    }

    pub fn remove(&mut self, pid: ProcessId) {
        self.ready_queue.retain(|queued| *queued != pid);
        if self.cursor >= self.ready_queue.len() {
            self.cursor = 0;
        }
    }

    pub fn pick_next(&mut self) -> Option<ProcessId> {
        if self.ready_queue.is_empty() {
            return None;
        }

        let len = self.ready_queue.len();
        let mut checked = 0;
        while checked < len && !self.ready_queue.is_empty() {
            if self.cursor >= self.ready_queue.len() {
                self.cursor = 0;
            }

            let pid = self.ready_queue[self.cursor];
            if super::is_pid_runnable(pid) {
                self.cursor = (self.cursor + 1) % self.ready_queue.len();
                return Some(pid);
            }

            self.ready_queue.remove(self.cursor);
            checked += 1;
        }

        None
    }

    pub fn len(&self) -> usize {
        self.ready_queue.len()
    }
}

static mut SCHEDULER_INSTANCE: Option<Scheduler> = None;

pub fn init() {
    unsafe {
        SCHEDULER_INSTANCE = Some(Scheduler::new());
    }
    crate::memory::serial_write("[SCHED] ready queue initialized\r\n");
}

pub fn enqueue_ready(pid: ProcessId) -> Result<(), ProcessError> {
    unsafe {
        match SCHEDULER_INSTANCE.as_mut() {
            Some(scheduler) => scheduler.enqueue(pid),
            None => Err(ProcessError::SchedulerUnavailable),
        }
    }
}

pub fn remove(pid: ProcessId) {
    unsafe {
        if let Some(scheduler) = SCHEDULER_INSTANCE.as_mut() {
            scheduler.remove(pid);
        }
    }
}

pub fn pick_next_candidate() -> Option<ProcessId> {
    unsafe { SCHEDULER_INSTANCE.as_mut()?.pick_next() }
}

pub fn ready_len() -> usize {
    unsafe {
        SCHEDULER_INSTANCE
            .as_ref()
            .map(|scheduler| scheduler.len())
            .unwrap_or(0)
    }
}
