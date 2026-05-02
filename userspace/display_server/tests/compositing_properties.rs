extern crate alloc;

use display_server::DisplayServer;
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_compositing_includes_all_visible_windows(
        num_windows in 1usize..5usize,
        screen_width in 800u32..1920u32,
        screen_height in 600u32..1080u32,
    ) {
        let mut server = DisplayServer::new();
        let mut window_ids = alloc::vec::Vec::new();

        for i in 0..num_windows {
            let x = (i * 100) as i32;
            let y = (i * 100) as i32;
            let id = server.create_window(
                i as u32 + 1,
                x,
                y,
                200,
                200,
                i as u64 + 1,
            );
            window_ids.push(id);
        }

        let framebuffer = server.render_frame(screen_width, screen_height);

        assert_eq!(framebuffer.len(), (screen_width * screen_height) as usize);

        for &window_id in &window_ids {
            let window = server.get_window(window_id).expect("Window should exist");
            
            if !window.visible {
                continue;
            }

            let check_x = window.x + 10;
            let check_y = window.y + 10;

            if check_x >= 0
                && check_x < screen_width as i32
                && check_y >= 0
                && check_y < screen_height as i32
            {
                let idx = (check_y as u32 * screen_width + check_x as u32) as usize;
                let pixel = framebuffer[idx];
                
                assert_ne!(pixel, 0);
            }
        }
    }

    #[test]
    fn prop_compositing_renders_cursor(
        mouse_x in 0i32..800i32,
        mouse_y in 0i32..600i32,
        screen_width in 800u32..1920u32,
        screen_height in 600u32..1080u32,
    ) {
        let mut server = DisplayServer::new();

        server.handle_mouse_event(mouse_x, mouse_y, 0);

        let framebuffer = server.render_frame(screen_width, screen_height);

        if mouse_x >= 0
            && mouse_x < screen_width as i32
            && mouse_y >= 0
            && mouse_y < screen_height as i32
        {
            let idx = (mouse_y as u32 * screen_width + mouse_x as u32) as usize;
            if idx < framebuffer.len() {
                let pixel = framebuffer[idx];
                assert_eq!(pixel, 0xFF000000);
            }
        }
    }
}
