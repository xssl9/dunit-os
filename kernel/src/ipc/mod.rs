use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::process::ProcessId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SharedMemoryId(pub u64);

pub struct SharedMemoryRegion {
    pub id: SharedMemoryId,
    pub physical_addr: u64,
    pub size: usize,
    pub owners: Vec<ProcessId>,
}

impl SharedMemoryRegion {
    pub fn new(id: SharedMemoryId, physical_addr: u64, size: usize) -> Self {
        Self {
            id,
            physical_addr,
            size,
            owners: Vec::new(),
        }
    }

    pub fn add_owner(&mut self, pid: ProcessId) {
        if !self.owners.contains(&pid) {
            self.owners.push(pid);
        }
    }

    pub fn remove_owner(&mut self, pid: ProcessId) {
        self.owners.retain(|&p| p != pid);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MessageType {
    MouseEvent { x: i32, y: i32, buttons: u8 },
    KeyboardEvent { scancode: u8, pressed: bool },
    RenderFrame { buffer_id: u64 },
    WindowCreate { width: u32, height: u32 },
    WindowClose { window_id: u32 },
}

#[derive(Debug, Clone)]
pub struct Message {
    pub sender: ProcessId,
    pub msg_type: MessageType,
    pub data: [u8; 256],
}

impl Message {
    pub fn new(sender: ProcessId, msg_type: MessageType) -> Self {
        Self {
            sender,
            msg_type,
            data: [0; 256],
        }
    }

    pub fn with_data(sender: ProcessId, msg_type: MessageType, data: &[u8]) -> Self {
        let mut msg = Self::new(sender, msg_type);
        let len = data.len().min(256);
        msg.data[..len].copy_from_slice(&data[..len]);
        msg
    }
}

pub struct IpcManager {
    shared_regions: BTreeMap<SharedMemoryId, SharedMemoryRegion>,
    message_queues: BTreeMap<ProcessId, Vec<Message>>,
    next_shared_id: u64,
}

impl IpcManager {
    pub const fn new() -> Self {
        Self {
            shared_regions: BTreeMap::new(),
            message_queues: BTreeMap::new(),
            next_shared_id: 1,
        }
    }

    pub fn create_shared_memory(&mut self, size: usize) -> Option<SharedMemoryId> {
        use crate::memory::pmm::get_pmm;

        let pmm = get_pmm()?;
        let num_pages = (size + 4095) / 4096;
        
        let mut physical_frames = Vec::new();
        for _ in 0..num_pages {
            match pmm.alloc_frame() {
                Some(frame) => physical_frames.push(frame),
                None => {
                    for frame in physical_frames {
                        pmm.free_frame(frame);
                    }
                    return None;
                }
            }
        }

        let id = SharedMemoryId(self.next_shared_id);
        self.next_shared_id += 1;

        let physical_addr = physical_frames[0].as_usize() as u64;
        let region = SharedMemoryRegion::new(id, physical_addr, size);
        self.shared_regions.insert(id, region);

        Some(id)
    }

    pub fn attach_shared_memory(&mut self, id: SharedMemoryId, pid: ProcessId) -> Option<u64> {
        let region = self.shared_regions.get_mut(&id)?;
        region.add_owner(pid);
        Some(region.physical_addr)
    }

    pub fn detach_shared_memory(&mut self, id: SharedMemoryId, pid: ProcessId) {
        if let Some(region) = self.shared_regions.get_mut(&id) {
            region.remove_owner(pid);
            
            if region.owners.is_empty() {
                self.shared_regions.remove(&id);
            }
        }
    }

    pub fn get_shared_region(&self, id: SharedMemoryId) -> Option<&SharedMemoryRegion> {
        self.shared_regions.get(&id)
    }

    pub fn send_message(&mut self, target: ProcessId, msg: Message) -> Result<(), ()> {
        let queue = self.message_queues.entry(target).or_insert_with(Vec::new);
        queue.push(msg);
        Ok(())
    }

    pub fn receive_message(&mut self, pid: ProcessId) -> Option<Message> {
        let queue = self.message_queues.get_mut(&pid)?;
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    }

    pub fn has_messages(&self, pid: ProcessId) -> bool {
        self.message_queues
            .get(&pid)
            .map(|q| !q.is_empty())
            .unwrap_or(false)
    }

    pub fn clear_messages(&mut self, pid: ProcessId) {
        self.message_queues.remove(&pid);
    }
}

static mut IPC_MANAGER_INSTANCE: Option<IpcManager> = None;

pub fn init() {
    // Временно пропускаем инициализацию IPC
    // TODO: инициализировать после настройки аллокатора
}

pub fn get_ipc_manager() -> Option<&'static mut IpcManager> {
    unsafe { IPC_MANAGER_INSTANCE.as_mut() }
}
