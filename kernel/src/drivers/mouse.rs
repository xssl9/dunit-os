use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static PACKET_SIZE: AtomicUsize = AtomicUsize::new(3);
static mut PACKET: [u8; 4] = [0; 4];
static mut PACKET_INDEX: usize = 0;
static PACKET_LOCK: AtomicBool = AtomicBool::new(false);

pub fn init() {
    unsafe {
        wait_write();
        outb(0x64, 0xA8);

        wait_write();
        outb(0x64, 0x20);
        wait_read();
        let mut status = inb(0x60);
        status |= 0x02;
        status &= !0x20;

        wait_write();
        outb(0x64, 0x60);
        wait_write();
        outb(0x60, status);

        mouse_write(0xF6);
        let _ = mouse_read_ack();

        mouse_set_sample_rate(200);
        mouse_set_sample_rate(100);
        mouse_set_sample_rate(80);
        mouse_write(0xF2);
        let _ = mouse_read_ack();
        let device_id = mouse_read_ack();
        if device_id == 3 {
            PACKET_SIZE.store(4, Ordering::Relaxed);
        } else {
            PACKET_SIZE.store(3, Ordering::Relaxed);
        }

        mouse_write(0xF4);
        let _ = mouse_read_ack();

        drain_output();
        PACKET_INDEX = 0;
    }
}

pub fn set_bounds(width: usize, height: usize) {
    crate::input::set_mouse_bounds(width, height);
}

pub fn set_position(x: i32, y: i32) {
    crate::input::set_mouse_position(x, y);
}

pub fn update() {
    unsafe {
        crate::hal::hal_disable_interrupts();
        while (inb(0x64) & 0x01) != 0 {
            let status = inb(0x64);
            let byte = inb(0x60);
            if (status & 0x20) == 0 {
                continue;
            }

            push_packet_byte(byte);
        }
        crate::hal::hal_enable_interrupts();
    }
}

pub fn push_packet_byte(byte: u8) {
    while PACKET_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    push_packet_byte_locked(byte);
    PACKET_LOCK.store(false, Ordering::Release);
}

fn push_packet_byte_locked(byte: u8) {
    unsafe {
        if PACKET_INDEX == 0 && (byte & 0x08) == 0 {
            return;
        }

        PACKET[PACKET_INDEX] = byte;
        PACKET_INDEX += 1;

        let packet_size = PACKET_SIZE.load(Ordering::Relaxed);
        if PACKET_INDEX == packet_size {
            let dx = PACKET[1] as i8 as i32;
            let dy = -(PACKET[2] as i8 as i32);
            let mut wheel = 0;
            if packet_size == 4 {
                let raw = PACKET[3] & 0x0F;
                wheel = if (raw & 0x08) != 0 {
                    (raw | 0xF0) as i8 as i32
                } else {
                    raw as i32
                };
            }
            crate::input::push_mouse_relative(dx, dy, PACKET[0] & 0x07, wheel);
            PACKET_INDEX = 0;
        }
    }
}

pub fn get_position() -> (i32, i32) {
    crate::input::mouse_position()
}

pub fn get_buttons() -> u8 {
    crate::input::mouse_buttons()
}

pub fn take_scroll_delta() -> i32 {
    crate::input::take_mouse_scroll_delta()
}

unsafe fn inb(port: u16) -> u8 {
    let result: u8;
    core::arch::asm!("in al, dx", out("al") result, in("dx") port, options(nomem, nostack));
    result
}

unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}

unsafe fn wait_read() {
    for _ in 0..100000 {
        if (inb(0x64) & 0x01) != 0 {
            break;
        }
        core::arch::asm!("pause");
    }
}

unsafe fn wait_write() {
    for _ in 0..100000 {
        if (inb(0x64) & 0x02) == 0 {
            break;
        }
        core::arch::asm!("pause");
    }
}

unsafe fn mouse_write(value: u8) {
    wait_write();
    outb(0x64, 0xD4);
    wait_write();
    outb(0x60, value);
}

unsafe fn mouse_set_sample_rate(rate: u8) {
    mouse_write(0xF3);
    let _ = mouse_read_ack();
    mouse_write(rate);
    let _ = mouse_read_ack();
}

unsafe fn mouse_read_ack() -> u8 {
    wait_read();
    inb(0x60)
}

unsafe fn drain_output() {
    for _ in 0..32 {
        if (inb(0x64) & 0x01) == 0 {
            break;
        }
        let _ = inb(0x60);
    }
}
