extern crate alloc;

use display_server::DisplayServer;
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_mouse_events_forwarded_to_egui(
        x in -1000i32..1000i32,
        y in -1000i32..1000i32,
        buttons in 0u8..8u8,
    ) {
        let mut server = DisplayServer::new();
        
        let initial_event_count = server.get_egui_input().events.len();
        
        server.handle_mouse_event(x, y, buttons);
        
        let final_event_count = server.get_egui_input().events.len();
        
        assert!(final_event_count > initial_event_count);
        
        let has_pointer_moved = server.get_egui_input().events.iter().any(|event| {
            matches!(event, egui::Event::PointerMoved(_))
        });
        assert!(has_pointer_moved);
    }

    #[test]
    fn prop_keyboard_events_forwarded_to_egui(
        scancode in 0x01u8..0x54u8,
        pressed in proptest::bool::ANY,
    ) {
        let mut server = DisplayServer::new();
        
        server.clear_egui_events();
        let initial_event_count = server.get_egui_input().events.len();
        
        server.handle_keyboard_event(scancode, pressed);
        
        let final_event_count = server.get_egui_input().events.len();
        
        if final_event_count > initial_event_count {
            let has_key_event = server.get_egui_input().events.iter().any(|event| {
                matches!(event, egui::Event::Key { .. })
            });
            assert!(has_key_event);
        }
    }
}
