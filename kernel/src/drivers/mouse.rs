use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};

static MOUSE_X: AtomicI32 = AtomicI32::new(512);
static MOUSE_Y: AtomicI32 = AtomicI32::new(384);
static MOUSE_BUTTONS: AtomicU8 = AtomicU8::new(0);
static MOUSE_MAX_X: AtomicI32 = AtomicI32::new(1023);
static MOUSE_MAX_Y: AtomicI32 = AtomicI32::new(767);
static mut PACKET: [u8; 3] = [0; 3];
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
        mouse_write(0xF4);
        let _ = mouse_read_ack();

        drain_output();
        PACKET_INDEX = 0;
    }
}

pub fn set_bounds(width: usize, height: usize) {
    let max_x = width.saturating_sub(1) as i32;
    let max_y = height.saturating_sub(1) as i32;
    MOUSE_MAX_X.store(max_x, Ordering::Relaxed);
    MOUSE_MAX_Y.store(max_y, Ordering::Relaxed);
    clamp_position();
}

pub fn set_position(x: i32, y: i32) {
    let max_x = MOUSE_MAX_X.load(Ordering::Relaxed);
    let max_y = MOUSE_MAX_Y.load(Ordering::Relaxed);
    MOUSE_X.store(x.max(0).min(max_x), Ordering::Relaxed);
    MOUSE_Y.store(y.max(0).min(max_y), Ordering::Relaxed);
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

        if PACKET_INDEX == 3 {
            MOUSE_BUTTONS.store(PACKET[0] & 0x07, Ordering::Relaxed);
            let dx = PACKET[1] as i8 as i32;
            let dy = -(PACKET[2] as i8 as i32);

            let max_x = MOUSE_MAX_X.load(Ordering::Relaxed);
            let max_y = MOUSE_MAX_Y.load(Ordering::Relaxed);
            let x = (MOUSE_X.load(Ordering::Relaxed) + dx).max(0).min(max_x);
            let y = (MOUSE_Y.load(Ordering::Relaxed) + dy).max(0).min(max_y);
            MOUSE_X.store(x, Ordering::Relaxed);
            MOUSE_Y.store(y, Ordering::Relaxed);
            PACKET_INDEX = 0;
        }
    }
}

pub fn get_position() -> (i32, i32) {
    (
        MOUSE_X.load(Ordering::Relaxed),
        MOUSE_Y.load(Ordering::Relaxed),
    )
}

pub fn get_buttons() -> u8 {
    MOUSE_BUTTONS.load(Ordering::Relaxed)
}

fn clamp_position() {
    let max_x = MOUSE_MAX_X.load(Ordering::Relaxed);
    let max_y = MOUSE_MAX_Y.load(Ordering::Relaxed);
    let x = MOUSE_X.load(Ordering::Relaxed).max(0).min(max_x);
    let y = MOUSE_Y.load(Ordering::Relaxed).max(0).min(max_y);
    MOUSE_X.store(x, Ordering::Relaxed);
    MOUSE_Y.store(y, Ordering::Relaxed);
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
