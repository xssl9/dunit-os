#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut message = [0u8; 12];
    if libdunit::ipc_recv_blocking(&mut message, 0) != 12 { libdunit::exit(1); }
    let owner = u32::from_le_bytes(message[..4].try_into().unwrap());
    let id = u64::from_le_bytes(message[4..12].try_into().unwrap());
    let mapped = libdunit::shared_vm_map(id, 0, libdunit::VM_PROT_READ | libdunit::VM_PROT_WRITE);
    if mapped < 0 { libdunit::exit(2); }
    let byte = mapped as *mut u8;
    if unsafe { byte.read_volatile() } != 7 { libdunit::exit(3); }
    unsafe { byte.write_volatile(9); }
    if libdunit::ipc_send(owner, b"done") != 4 { libdunit::exit(4); }
    // Leave the mapping live; process teardown must release its reference.
    libdunit::exit(0)
}
