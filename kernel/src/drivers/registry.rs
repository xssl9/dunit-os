use core::sync::atomic::{AtomicBool, Ordering};

const MAX_DEVICES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Framebuffer,
    Input,
    Bus,
    UsbController,
    Block,
    Network,
    Other,
}

#[derive(Clone, Copy)]
pub struct DeviceRegistration {
    pub name: &'static str,
    pub class: DeviceClass,
    pub driver: &'static str,
}

static REGISTRY_LOCK: AtomicBool = AtomicBool::new(false);
static mut DEVICES: [Option<DeviceRegistration>; MAX_DEVICES] = [None; MAX_DEVICES];
static mut DEVICE_COUNT: usize = 0;

pub fn register(name: &'static str, class: DeviceClass, driver: &'static str) {
    lock_registry();

    unsafe {
        let mut index = 0usize;
        while index < DEVICE_COUNT {
            if let Some(device) = DEVICES[index] {
                if device.name == name {
                    DEVICES[index] = Some(DeviceRegistration {
                        name,
                        class,
                        driver,
                    });
                    REGISTRY_LOCK.store(false, Ordering::Release);
                    publish_dev_node(name);
                    return;
                }
            }
            index += 1;
        }

        if DEVICE_COUNT < DEVICES.len() {
            DEVICES[DEVICE_COUNT] = Some(DeviceRegistration {
                name,
                class,
                driver,
            });
            DEVICE_COUNT += 1;
        }
    }

    REGISTRY_LOCK.store(false, Ordering::Release);
    publish_dev_node(name);
}

pub fn snapshot(out: &mut [Option<DeviceRegistration>]) -> usize {
    lock_registry();

    let count = unsafe { DEVICE_COUNT.min(out.len()) };
    let mut index = 0usize;
    while index < count {
        out[index] = unsafe { DEVICES[index] };
        index += 1;
    }

    REGISTRY_LOCK.store(false, Ordering::Release);
    count
}

pub fn class_name(class: DeviceClass) -> &'static str {
    match class {
        DeviceClass::Framebuffer => "framebuffer",
        DeviceClass::Input => "input",
        DeviceClass::Bus => "bus",
        DeviceClass::UsbController => "usb-controller",
        DeviceClass::Block => "block",
        DeviceClass::Network => "network",
        DeviceClass::Other => "other",
    }
}

fn publish_dev_node(name: &str) {
    let mut path = [0u8; 69];
    let prefix = b"/dev/";
    path[..prefix.len()].copy_from_slice(prefix);

    let name_bytes = name.as_bytes();
    let len = name_bytes.len().min(path.len() - prefix.len());
    path[prefix.len()..prefix.len() + len].copy_from_slice(&name_bytes[..len]);

    if let Ok(path) = core::str::from_utf8(&path[..prefix.len() + len]) {
        crate::fs::vfs::register_device_node(path);
    }
}

fn lock_registry() {
    while REGISTRY_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}
