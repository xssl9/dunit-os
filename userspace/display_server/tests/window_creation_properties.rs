extern crate alloc;

use display_server::{DisplayServer, ProcessId, SharedMemoryId};
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_window_creation_allocates_resources(
        owner_pid in 1u32..1000u32,
        x in -1000i32..1000i32,
        y in -1000i32..1000i32,
        width in 1u32..2000u32,
        height in 1u32..2000u32,
        buffer in 1u64..10000u64,
    ) {
        let mut server = DisplayServer::new();
        
        let window_id = server.create_window(owner_pid, x, y, width, height, buffer);
        
        let window = server.get_window(window_id).expect("Window should exist");
        
        assert_eq!(window.id, window_id);
        assert_eq!(window.owner_pid, owner_pid);
        assert_eq!(window.x, x);
        assert_eq!(window.y, y);
        assert_eq!(window.width, width);
        assert_eq!(window.height, height);
        assert_eq!(window.buffer, buffer);
        assert!(window.visible);
        
        assert_eq!(server.window_count(), 1);
    }
}
