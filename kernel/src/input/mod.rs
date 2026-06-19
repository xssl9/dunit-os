use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};

const MOUSE_EVENT_QUEUE_LEN: usize = 64;

static MOUSE_X: AtomicI32 = AtomicI32::new(512);
static MOUSE_Y: AtomicI32 = AtomicI32::new(384);
static MOUSE_MAX_X: AtomicI32 = AtomicI32::new(1023);
static MOUSE_MAX_Y: AtomicI32 = AtomicI32::new(767);
static MOUSE_BUTTONS: AtomicU8 = AtomicU8::new(0);
static MOUSE_WHEEL_DELTA: AtomicI32 = AtomicI32::new(0);
static MOUSE_EVENT_READ: AtomicUsize = AtomicUsize::new(0);
static MOUSE_EVENT_WRITE: AtomicUsize = AtomicUsize::new(0);
static MOUSE_EVENT_LOCK: AtomicBool = AtomicBool::new(false);

static mut MOUSE_EVENTS: [MouseEvent; MOUSE_EVENT_QUEUE_LEN] =
    [MouseEvent::empty(); MOUSE_EVENT_QUEUE_LEN];

#[derive(Clone, Copy)]
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
    pub buttons: u8,
    pub wheel: i32,
    pub absolute: bool,
}

impl MouseEvent {
    const fn empty() -> Self {
        Self {
            x: 0,
            y: 0,
            dx: 0,
            dy: 0,
            buttons: 0,
            wheel: 0,
            absolute: false,
        }
    }
}

pub fn set_mouse_bounds(width: usize, height: usize) {
    MOUSE_MAX_X.store(width.saturating_sub(1) as i32, Ordering::Relaxed);
    MOUSE_MAX_Y.store(height.saturating_sub(1) as i32, Ordering::Relaxed);
    clamp_mouse_position();
}

pub fn set_mouse_position(x: i32, y: i32) {
    let (x, y) = clamp_mouse_xy(x, y);
    MOUSE_X.store(x, Ordering::Relaxed);
    MOUSE_Y.store(y, Ordering::Relaxed);
}

pub fn mouse_position() -> (i32, i32) {
    (
        MOUSE_X.load(Ordering::Relaxed),
        MOUSE_Y.load(Ordering::Relaxed),
    )
}

pub fn mouse_buttons() -> u8 {
    MOUSE_BUTTONS.load(Ordering::Relaxed)
}

pub fn take_mouse_scroll_delta() -> i32 {
    MOUSE_WHEEL_DELTA.swap(0, Ordering::Relaxed)
}

pub fn push_mouse_relative(dx: i32, dy: i32, buttons: u8, wheel: i32) {
    let current_x = MOUSE_X.load(Ordering::Relaxed);
    let current_y = MOUSE_Y.load(Ordering::Relaxed);
    let (x, y) = clamp_mouse_xy(current_x.saturating_add(dx), current_y.saturating_add(dy));

    MOUSE_X.store(x, Ordering::Relaxed);
    MOUSE_Y.store(y, Ordering::Relaxed);
    MOUSE_BUTTONS.store(buttons, Ordering::Relaxed);
    if wheel != 0 {
        MOUSE_WHEEL_DELTA.fetch_add(wheel, Ordering::Relaxed);
    }

    push_mouse_event(MouseEvent {
        x,
        y,
        dx,
        dy,
        buttons,
        wheel,
        absolute: false,
    });
}

pub fn push_mouse_absolute(x: i32, y: i32, buttons: u8, wheel: i32) {
    let old_x = MOUSE_X.load(Ordering::Relaxed);
    let old_y = MOUSE_Y.load(Ordering::Relaxed);
    let (x, y) = clamp_mouse_xy(x, y);

    MOUSE_X.store(x, Ordering::Relaxed);
    MOUSE_Y.store(y, Ordering::Relaxed);
    MOUSE_BUTTONS.store(buttons, Ordering::Relaxed);
    if wheel != 0 {
        MOUSE_WHEEL_DELTA.fetch_add(wheel, Ordering::Relaxed);
    }

    push_mouse_event(MouseEvent {
        x,
        y,
        dx: x.saturating_sub(old_x),
        dy: y.saturating_sub(old_y),
        buttons,
        wheel,
        absolute: true,
    });
}

pub fn pop_mouse_event() -> Option<MouseEvent> {
    lock_mouse_events();

    let read = MOUSE_EVENT_READ.load(Ordering::Relaxed);
    let write = MOUSE_EVENT_WRITE.load(Ordering::Relaxed);
    if read == write {
        MOUSE_EVENT_LOCK.store(false, Ordering::Release);
        return None;
    }

    let event = unsafe { MOUSE_EVENTS[read] };
    MOUSE_EVENT_READ.store((read + 1) % MOUSE_EVENT_QUEUE_LEN, Ordering::Relaxed);
    MOUSE_EVENT_LOCK.store(false, Ordering::Release);
    Some(event)
}

fn push_mouse_event(event: MouseEvent) {
    lock_mouse_events();

    let write = MOUSE_EVENT_WRITE.load(Ordering::Relaxed);
    let next = (write + 1) % MOUSE_EVENT_QUEUE_LEN;
    if next == MOUSE_EVENT_READ.load(Ordering::Relaxed) {
        MOUSE_EVENT_READ.store(
            (MOUSE_EVENT_READ.load(Ordering::Relaxed) + 1) % MOUSE_EVENT_QUEUE_LEN,
            Ordering::Relaxed,
        );
    }
    unsafe {
        MOUSE_EVENTS[write] = event;
    }
    MOUSE_EVENT_WRITE.store(next, Ordering::Relaxed);
    MOUSE_EVENT_LOCK.store(false, Ordering::Release);
}

fn lock_mouse_events() {
    while MOUSE_EVENT_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn clamp_mouse_position() {
    let x = MOUSE_X.load(Ordering::Relaxed);
    let y = MOUSE_Y.load(Ordering::Relaxed);
    let (x, y) = clamp_mouse_xy(x, y);
    MOUSE_X.store(x, Ordering::Relaxed);
    MOUSE_Y.store(y, Ordering::Relaxed);
}

fn clamp_mouse_xy(x: i32, y: i32) -> (i32, i32) {
    let max_x = MOUSE_MAX_X.load(Ordering::Relaxed);
    let max_y = MOUSE_MAX_Y.load(Ordering::Relaxed);
    (x.max(0).min(max_x), y.max(0).min(max_y))
}
