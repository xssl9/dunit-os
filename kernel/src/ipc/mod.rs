use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::process::ProcessId;

pub const MAX_MESSAGE_SIZE: usize = 256;
pub const MAX_QUEUE_MESSAGES: usize = 128;

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
    pub len: usize,
    pub data: [u8; MAX_MESSAGE_SIZE],
}

impl Message {
    pub fn new(sender: ProcessId, msg_type: MessageType) -> Self {
        Self {
            sender,
            msg_type,
            len: 0,
            data: [0; MAX_MESSAGE_SIZE],
        }
    }

    pub fn with_data(sender: ProcessId, msg_type: MessageType, data: &[u8]) -> Self {
        let mut msg = Self::new(sender, msg_type);
        let len = data.len().min(MAX_MESSAGE_SIZE);
        msg.data[..len].copy_from_slice(&data[..len]);
        msg.len = len;
        msg
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    InvalidTarget,
    MessageTooLarge,
    QueueFull,
    NoMessage,
    Unavailable,
}

pub struct IpcManager {
    shared_regions: BTreeMap<SharedMemoryId, SharedMemoryRegion>,
    message_queues: BTreeMap<ProcessId, Vec<Message>>,
    next_shared_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct IpcStats {
    pub queue_count: u64,
    pub queued_messages: u64,
    pub shared_regions: u64,
    pub max_queue_messages: u64,
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

    pub fn send_message(&mut self, target: ProcessId, msg: Message) -> Result<(), IpcError> {
        if !crate::process::process_exists(target) {
            return Err(IpcError::InvalidTarget);
        }
        let queue = self.message_queues.entry(target).or_insert_with(Vec::new);
        if queue.len() >= MAX_QUEUE_MESSAGES {
            return Err(IpcError::QueueFull);
        }
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

    pub fn stats(&self) -> IpcStats {
        let mut queued_messages = 0u64;
        for queue in self.message_queues.values() {
            queued_messages += queue.len() as u64;
        }
        IpcStats {
            queue_count: self.message_queues.len() as u64,
            queued_messages,
            shared_regions: self.shared_regions.len() as u64,
            max_queue_messages: MAX_QUEUE_MESSAGES as u64,
        }
    }
}

static mut IPC_MANAGER_INSTANCE: Option<IpcManager> = None;

pub fn init() {
    unsafe {
        IPC_MANAGER_INSTANCE = Some(IpcManager::new());
    }
    crate::memory::serial_write("[IPC] manager ready: message-queue smoke only\r\n");
    // Временно пропускаем инициализацию IPC
    // TODO: инициализировать после настройки аллокатора
}

pub fn get_ipc_manager() -> Option<&'static mut IpcManager> {
    unsafe { IPC_MANAGER_INSTANCE.as_mut() }
}

pub fn send_bytes(sender: ProcessId, target: ProcessId, data: &[u8]) -> Result<(), IpcError> {
    if data.is_empty() || data.len() > MAX_MESSAGE_SIZE {
        return Err(IpcError::MessageTooLarge);
    }
    let manager = get_ipc_manager().ok_or(IpcError::Unavailable)?;
    manager.send_message(
        target,
        Message::with_data(sender, MessageType::WindowClose { window_id: 0 }, data),
    )
}

pub fn recv_bytes(pid: ProcessId, out: &mut [u8]) -> Result<usize, IpcError> {
    let manager = get_ipc_manager().ok_or(IpcError::Unavailable)?;
    let queue = manager.message_queues.get_mut(&pid).ok_or(IpcError::NoMessage)?;
    if queue.is_empty() {
        return Err(IpcError::NoMessage);
    }
    let len = queue[0].len;
    if out.len() < len {
        return Err(IpcError::MessageTooLarge);
    }
    let message = queue.remove(0);
    out[..len].copy_from_slice(&message.data[..len]);
    Ok(len)
}

pub fn ipc_stats() -> IpcStats {
    get_ipc_manager()
        .map(|manager| manager.stats())
        .unwrap_or_default()
}
