use proptest::prelude::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ProcessId(u64);

#[derive(Debug, Clone, Copy, PartialEq)]
enum MessageType {
    MouseEvent { x: i32, y: i32, buttons: u8 },
    KeyboardEvent { scancode: u8, pressed: bool },
    RenderFrame { buffer_id: u64 },
    WindowCreate { width: u32, height: u32 },
    WindowClose { window_id: u32 },
}

#[derive(Debug, Clone)]
struct Message {
    sender: ProcessId,
    msg_type: MessageType,
    data: [u8; 256],
}

impl Message {
    fn new(sender: ProcessId, msg_type: MessageType) -> Self {
        Self {
            sender,
            msg_type,
            data: [0; 256],
        }
    }

    fn with_data(sender: ProcessId, msg_type: MessageType, data: &[u8]) -> Self {
        let mut msg = Self::new(sender, msg_type);
        let len = data.len().min(256);
        msg.data[..len].copy_from_slice(&data[..len]);
        msg
    }
}

struct IpcManager {
    message_queues: BTreeMap<ProcessId, Vec<Message>>,
}

impl IpcManager {
    fn new() -> Self {
        Self {
            message_queues: BTreeMap::new(),
        }
    }

    fn send_message(&mut self, target: ProcessId, msg: Message) -> Result<(), ()> {
        let queue = self.message_queues.entry(target).or_insert_with(Vec::new);
        queue.push(msg);
        Ok(())
    }

    fn receive_message(&mut self, pid: ProcessId) -> Option<Message> {
        let queue = self.message_queues.get_mut(&pid)?;
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    }

    fn has_messages(&self, pid: ProcessId) -> bool {
        self.message_queues
            .get(&pid)
            .map(|q| !q.is_empty())
            .unwrap_or(false)
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_ipc_message_delivery(
        sender_id in 1u64..1000,
        target_id in 1u64..1000,
        x in -1000i32..1000,
        y in -1000i32..1000,
        buttons in 0u8..8
    ) {
        let mut ipc = IpcManager::new();

        let sender = ProcessId(sender_id);
        let target = ProcessId(target_id);

        let msg_type = MessageType::MouseEvent { x, y, buttons };
        let msg = Message::new(sender, msg_type);

        let original_sender = msg.sender;
        let original_x = if let MessageType::MouseEvent { x, .. } = msg.msg_type { x } else { 0 };
        let original_y = if let MessageType::MouseEvent { y, .. } = msg.msg_type { y } else { 0 };
        let original_buttons = if let MessageType::MouseEvent { buttons, .. } = msg.msg_type { buttons } else { 0 };

        assert!(ipc.send_message(target, msg).is_ok());

        assert!(ipc.has_messages(target));

        if let Some(received) = ipc.receive_message(target) {
            assert_eq!(received.sender, original_sender);

            if let MessageType::MouseEvent { x: rx, y: ry, buttons: rb } = received.msg_type {
                assert_eq!(rx, original_x);
                assert_eq!(ry, original_y);
                assert_eq!(rb, original_buttons);
            } else {
                panic!("Message type mismatch");
            }
        } else {
            panic!("Message not received");
        }

        assert!(!ipc.has_messages(target));
    }

    #[test]
    fn prop_message_with_data(
        sender_id in 1u64..1000,
        target_id in 1u64..1000,
        data_byte in 0u8..255,
        data_len in 1usize..256
    ) {
        let mut ipc = IpcManager::new();

        let sender = ProcessId(sender_id);
        let target = ProcessId(target_id);

        let data = vec![data_byte; data_len];
        let msg_type = MessageType::RenderFrame { buffer_id: 42 };
        let msg = Message::with_data(sender, msg_type, &data);

        assert!(ipc.send_message(target, msg).is_ok());

        if let Some(received) = ipc.receive_message(target) {
            assert_eq!(received.sender, sender);
            assert_eq!(&received.data[..data_len], &data[..]);
        } else {
            panic!("Message not received");
        }
    }

    #[test]
    fn prop_multiple_messages_fifo(
        sender_id in 1u64..1000,
        target_id in 1u64..1000,
        num_messages in 1usize..20
    ) {
        let mut ipc = IpcManager::new();

        let sender = ProcessId(sender_id);
        let target = ProcessId(target_id);

        let mut sent_ids = Vec::new();
        for i in 0..num_messages {
            let msg_type = MessageType::WindowClose { window_id: i as u32 };
            let msg = Message::new(sender, msg_type);
            sent_ids.push(i as u32);
            assert!(ipc.send_message(target, msg).is_ok());
        }

        for expected_id in sent_ids {
            if let Some(received) = ipc.receive_message(target) {
                if let MessageType::WindowClose { window_id } = received.msg_type {
                    assert_eq!(window_id, expected_id);
                } else {
                    panic!("Message type mismatch");
                }
            } else {
                panic!("Message not received");
            }
        }

        assert!(!ipc.has_messages(target));
    }
}
