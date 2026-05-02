extern crate alloc;

use display_server::DisplayServer;
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_window_position_updates_on_drag(
        initial_x in -500i32..500i32,
        initial_y in -500i32..500i32,
        drag_start_x in 0i32..100i32,
        drag_start_y in 0i32..100i32,
        drag_end_x in 0i32..100i32,
        drag_end_y in 0i32..100i32,
    ) {
        let mut server = DisplayServer::new();
        
        let window_id = server.create_window(1, initial_x, initial_y, 200, 200, 1);
        
        let click_x = initial_x + drag_start_x;
        let click_y = initial_y + drag_start_y;
        
        server.handle_mouse_event(click_x, click_y, 0x01);
        
        let new_mouse_x = initial_x + drag_end_x;
        let new_mouse_y = initial_y + drag_end_y;
        server.handle_mouse_event(new_mouse_x, new_mouse_y, 0x01);
        
        let window = server.get_window(window_id).expect("Window should exist");
        
        let expected_x = new_mouse_x - drag_start_x;
        let expected_y = new_mouse_y - drag_start_y;
        
        assert_eq!(window.x, expected_x);
        assert_eq!(window.y, expected_y);
    }

    #[test]
    fn prop_window_focus_management(
        num_windows in 2usize..10usize,
        focus_index in 0usize..9usize,
    ) {
        let focus_index = focus_index % num_windows;
        
        let mut server = DisplayServer::new();
        let mut window_ids = alloc::vec::Vec::new();
        
        for i in 0..num_windows {
            let id = server.create_window(
                i as u32 + 1,
                (i * 100) as i32,
                (i * 100) as i32,
                100,
                100,
                i as u64 + 1,
            );
            window_ids.push(id);
        }
        
        let target_window_id = window_ids[focus_index];
        server.set_focus(target_window_id);
        
        assert_eq!(server.get_focused_window(), Some(target_window_id));
        
        let focused_window = server.get_window(target_window_id).expect("Window should exist");
        assert!(focused_window.focused);
        
        for (i, &id) in window_ids.iter().enumerate() {
            let window = server.get_window(id).expect("Window should exist");
            if i == focus_index {
                assert!(window.focused);
            } else {
                assert!(!window.focused);
            }
        }
    }
}
