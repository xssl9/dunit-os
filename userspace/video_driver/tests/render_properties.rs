use proptest::prelude::*;

fn copy_to_framebuffer(framebuffer: &mut [u8], back_buffer: &[u8]) {
    let len = framebuffer.len().min(back_buffer.len());
    framebuffer[..len].copy_from_slice(&back_buffer[..len]);
}

fn handle_render_frame_logic(
    framebuffer: &mut [u8],
    back_buffer: &mut [u8],
    shared_buffer: &[u8],
    fb_size: usize,
) {
    let copy_size = fb_size.min(shared_buffer.len());
    back_buffer[..copy_size].copy_from_slice(&shared_buffer[..copy_size]);
    copy_to_framebuffer(framebuffer, back_buffer);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        fork: false,
        .. ProptestConfig::default()
    })]
    
    #[test]
    fn prop_framebuffer_render_completion(
        width in 800u32..801,
        height in 600u32..601,
        fill_byte in 0u8..255
    ) {
        let pitch = width * 4;
        let fb_size = (pitch * height) as usize;
        
        let mut framebuffer = vec![0u8; fb_size];
        let mut back_buffer = vec![0u8; fb_size];
        let shared_buffer = vec![fill_byte; fb_size];
        
        handle_render_frame_logic(&mut framebuffer, &mut back_buffer, &shared_buffer, fb_size);
        
        for (i, &byte) in framebuffer.iter().enumerate().take(100) {
            assert_eq!(byte, fill_byte, "Mismatch at byte {}", i);
        }
    }
    
    #[test]
    fn prop_partial_render(
        width in 800u32..801,
        height in 600u32..601,
        fill_byte in 0u8..255,
        partial_size_factor in 0.5f32..0.6
    ) {
        let pitch = width * 4;
        let fb_size = (pitch * height) as usize;
        let partial_size = ((fb_size as f32) * partial_size_factor) as usize;
        
        let mut framebuffer = vec![0u8; fb_size];
        let mut back_buffer = vec![0u8; fb_size];
        let shared_buffer = vec![fill_byte; partial_size];
        
        handle_render_frame_logic(&mut framebuffer, &mut back_buffer, &shared_buffer, fb_size);
        
        for i in 0..partial_size.min(100) {
            assert_eq!(framebuffer[i], fill_byte, "Mismatch at byte {}", i);
        }
        
        for i in partial_size..partial_size.min(fb_size).min(partial_size + 100) {
            assert_eq!(framebuffer[i], 0, "Byte {} should remain zero", i);
        }
    }
    
    #[test]
    fn prop_multiple_renders(
        width in 800u32..801,
        height in 600u32..601,
        num_renders in 2usize..3
    ) {
        let pitch = width * 4;
        let fb_size = (pitch * height) as usize;
        
        let mut framebuffer = vec![0u8; fb_size];
        let mut back_buffer = vec![0u8; fb_size];
        
        for render_idx in 0..num_renders {
            let fill_byte = (render_idx % 256) as u8;
            let shared_buffer = vec![fill_byte; fb_size];
            
            handle_render_frame_logic(&mut framebuffer, &mut back_buffer, &shared_buffer, fb_size);
            
            for &byte in framebuffer.iter().take(100) {
                assert_eq!(byte, fill_byte);
            }
        }
    }
}
