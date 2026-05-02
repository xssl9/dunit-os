#![no_std]

use core::slice;

#[repr(C)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MessageType {
    MouseEvent { x: i32, y: i32, buttons: u8 },
    KeyboardEvent { scancode: u8, pressed: bool },
    RenderFrame { buffer_id: u64 },
    WindowCreate { width: u32, height: u32 },
    WindowClose { window_id: u32 },
}

#[repr(C)]
#[derive(Clone)]
pub struct Message {
    pub sender: u64,
    pub msg_type: MessageType,
    pub data: [u8; 256],
}

pub struct VideoDriver {
    framebuffer: &'static mut [u8],
    back_buffer: &'static mut [u8],
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u8,
}

impl VideoDriver {
    pub fn init(info: FramebufferInfo) -> Self {
        let fb_size = (info.pitch * info.height) as usize;
        
        let framebuffer = unsafe {
            slice::from_raw_parts_mut(info.addr as *mut u8, fb_size)
        };
        
        let back_buffer = unsafe {
            slice::from_raw_parts_mut((info.addr + fb_size as u64) as *mut u8, fb_size)
        };
        
        for byte in back_buffer.iter_mut() {
            *byte = 0;
        }
        
        Self {
            framebuffer,
            back_buffer,
            width: info.width,
            height: info.height,
            pitch: info.pitch,
            bpp: info.bpp,
        }
    }
    
    pub fn swap_buffers(&mut self) {
        let len = self.back_buffer.len();
        self.framebuffer[..len].copy_from_slice(&self.back_buffer[..len]);
    }
    
    pub fn blit(&mut self, x: u32, y: u32, width: u32, height: u32, data: &[u8]) {
        let bytes_per_pixel = (self.bpp / 8) as usize;
        
        for row in 0..height {
            let dst_y = y + row;
            if dst_y >= self.height {
                break;
            }
            
            let dst_offset = (dst_y * self.pitch + x * bytes_per_pixel as u32) as usize;
            let src_offset = (row * width * bytes_per_pixel as u32) as usize;
            let row_bytes = (width * bytes_per_pixel as u32) as usize;
            
            if dst_offset + row_bytes <= self.back_buffer.len() 
                && src_offset + row_bytes <= data.len() {
                self.back_buffer[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&data[src_offset..src_offset + row_bytes]);
            }
        }
    }
    
    pub fn clear(&mut self, color: u32) {
        let bytes_per_pixel = (self.bpp / 8) as usize;
        let color_bytes = color.to_le_bytes();
        
        for y in 0..self.height {
            for x in 0..self.width {
                let offset = (y * self.pitch + x * bytes_per_pixel as u32) as usize;
                if offset + bytes_per_pixel <= self.back_buffer.len() {
                    for i in 0..bytes_per_pixel {
                        self.back_buffer[offset + i] = color_bytes[i];
                    }
                }
            }
        }
    }
    
    pub fn get_back_buffer(&self) -> &[u8] {
        &self.back_buffer
    }
    
    pub fn get_back_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.back_buffer
    }
    
    pub fn handle_render_frame(&mut self, buffer_id: u64) {
        extern "C" {
            fn syscall_attach_shared_memory(id: u64) -> u64;
        }
        
        let shared_addr = unsafe { syscall_attach_shared_memory(buffer_id) };
        
        if shared_addr == 0 {
            return;
        }
        
        let fb_size = (self.pitch * self.height) as usize;
        let shared_buffer = unsafe {
            slice::from_raw_parts(shared_addr as *const u8, fb_size)
        };
        
        self.back_buffer[..fb_size].copy_from_slice(&shared_buffer[..fb_size]);
        
        self.swap_buffers();
    }
    
    pub fn handle_render_frame_with_buffer(&mut self, shared_buffer: &[u8]) {
        let fb_size = (self.pitch * self.height) as usize;
        let copy_size = fb_size.min(shared_buffer.len());
        
        self.back_buffer[..copy_size].copy_from_slice(&shared_buffer[..copy_size]);
        self.swap_buffers();
    }
    
    pub fn get_framebuffer(&self) -> &[u8] {
        self.framebuffer
    }
    
    pub fn process_messages(&mut self) {
        extern "C" {
            fn syscall_receive_message(msg: *mut Message) -> i64;
        }
        
        loop {
            let mut msg = Message {
                sender: 0,
                msg_type: MessageType::RenderFrame { buffer_id: 0 },
                data: [0; 256],
            };
            
            let result = unsafe { syscall_receive_message(&mut msg as *mut Message) };
            
            if result < 0 {
                break;
            }
            
            match msg.msg_type {
                MessageType::RenderFrame { buffer_id } => {
                    self.handle_render_frame(buffer_id);
                }
                _ => {}
            }
        }
    }
}

