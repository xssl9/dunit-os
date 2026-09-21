#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop { core::hint::spin_loop(); } }

fn fail(reason: &str) -> ! {
    libdunit::print("vm_test: FAIL ");
    libdunit::println(reason);
    libdunit::exit(1)
}

fn wait_for(pid: u32, faulted: bool) {
    let mut status = libdunit::WaitStatus::empty();
    for _ in 0..50 {
        let result = libdunit::wait(pid, &mut status);
        if result == pid as isize {
            if (faulted && status.faulted()) || (!faulted && status.exited() && status.code == 0) {
                return;
            }
            fail("child status");
        }
        if result != libdunit::EAGAIN { fail("wait"); }
        libdunit::sleep_ms(10);
    }
    fail("wait timeout");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("vm_test: start");
    let rw = libdunit::VM_PROT_READ | libdunit::VM_PROT_WRITE;
    let flags = libdunit::VM_MAP_PRIVATE | libdunit::VM_MAP_ANONYMOUS;
    let base = libdunit::vm_map(0, 8192, rw, flags);
    if base < 0 { fail("private mmap"); }
    let base = base as usize;
    unsafe {
        if (base as *const u8).read_volatile() != 0 { fail("zero fill"); }
        (base as *mut u8).write_volatile(11);
        ((base + 4096) as *mut u8).write_volatile(12);
    }
    if libdunit::vm_protect(base + 4096, 4096, libdunit::VM_PROT_READ) != 0 {
        fail("protect read");
    }
    if libdunit::vm_protect(base + 4096, 4096, 0) != 0 { fail("protect none"); }
    if libdunit::vm_protect(base + 4096, 4096, rw) != 0 { fail("protect restore"); }
    if unsafe { ((base + 4096) as *const u8).read_volatile() } != 12 { fail("protect data"); }
    if libdunit::vm_protect(base, 4096, rw | libdunit::VM_PROT_EXEC) != libdunit::EINVAL {
        fail("W+X accepted");
    }
    if libdunit::vm_unmap(base + 4096, 4096) != 0 { fail("partial unmap"); }
    if libdunit::vm_map(base + 4096, 4096, rw, flags) != (base + 4096) as isize {
        fail("hole reuse");
    }
    if libdunit::vm_unmap(base, 8192) != 0 { fail("full unmap"); }
    if libdunit::vm_unmap(base, 4096) != libdunit::EINVAL { fail("double unmap"); }
    let none = libdunit::vm_map(0, 4096, 0, flags);
    if none < 0 || libdunit::vm_protect(none as usize, 4096, rw) != 0 {
        fail("PROT_NONE restore");
    }
    unsafe { (none as *mut u8).write_volatile(5); }
    if libdunit::vm_unmap(none as usize, 4096) != 0 { fail("PROT_NONE unmap"); }

    let guarded = libdunit::vm_map(0, 4096, rw, flags | libdunit::VM_MAP_GUARD);
    if guarded < 0 { fail("guarded mmap"); }
    let guarded = guarded as usize;
    if libdunit::vm_protect(guarded - 4096, 4096, rw) != libdunit::EINVAL {
        fail("guard protect");
    }
    if libdunit::vm_unmap(guarded, 4096) != 0 { fail("guard teardown"); }
    if libdunit::vm_map(guarded - 4096, 3 * 4096, rw, flags) != (guarded - 4096) as isize {
        fail("guard hole reuse");
    }
    if libdunit::vm_unmap(guarded - 4096, 3 * 4096) != 0 { fail("guard hole unmap"); }

    let id = libdunit::shared_vm_create(4096);
    if id <= 0 { fail("shared create"); }
    let id = id as u64;
    let mapped = libdunit::shared_vm_map(id, 0, rw);
    if mapped < 0 { fail("shared map"); }
    let mapped = mapped as usize;
    unsafe { (mapped as *mut u8).write_volatile(7); }
    let peer = libdunit::spawn("vm_peer");
    if peer <= 0 { fail("peer spawn"); }
    let mut msg = [0u8; 12];
    msg[..4].copy_from_slice(&libdunit::get_pid().to_le_bytes());
    msg[4..].copy_from_slice(&id.to_le_bytes());
    if libdunit::ipc_send(peer as u32, &msg) != 12 { fail("peer send"); }
    let mut ack = [0u8; 8];
    if libdunit::ipc_recv_blocking(&mut ack, 0) != 4 || &ack[..4] != b"done" {
        fail("peer ack");
    }
    wait_for(peer as u32, false);
    if unsafe { (mapped as *const u8).read_volatile() } != 9 { fail("shared data"); }
    let mut before = libdunit::SystemStats::default();
    if libdunit::get_system_stats(&mut before) != 0 { fail("stats before"); }
    if libdunit::vm_unmap(mapped, 4096) != 0 { fail("shared unmap"); }
    if libdunit::shared_vm_close(id) != 0 { fail("shared close"); }
    let mut after = libdunit::SystemStats::default();
    if libdunit::get_system_stats(&mut after) != 0
        || after.pmm_free_bytes < before.pmm_free_bytes + 4096 { fail("shared frame leak"); }
    if libdunit::shared_vm_map(id, 0, rw) >= 0 { fail("stale shared id"); }

    let second_id = libdunit::shared_vm_create(8192);
    if second_id <= 0 { fail("second shared create"); }
    let second_id = second_id as u64;
    let second = libdunit::shared_vm_map(second_id, 0, rw);
    if second < 0 { fail("second shared map"); }
    let second = second as usize;
    unsafe { ((second + 4096) as *mut u8).write_volatile(23); }
    if libdunit::vm_unmap(second, 4096) != 0 { fail("shared partial unmap"); }
    if libdunit::shared_vm_close(second_id) != 0 { fail("shared close with mapping"); }
    if unsafe { ((second + 4096) as *const u8).read_volatile() } != 23 {
        fail("shared live after close");
    }
    if libdunit::get_system_stats(&mut before) != 0 { fail("second stats before"); }
    if libdunit::vm_unmap(second + 4096, 4096) != 0 { fail("second shared unmap"); }
    if libdunit::get_system_stats(&mut after) != 0
        || after.pmm_free_bytes < before.pmm_free_bytes + 8192 {
        fail("second shared frame leak");
    }

    let fault = libdunit::spawn("vm_guard_fault");
    if fault <= 0 { fail("guard fault spawn"); }
    wait_for(fault as u32, true);
    let fault = libdunit::spawn("vm_protect_fault");
    if fault <= 0 { fail("protect fault spawn"); }
    wait_for(fault as u32, true);
    libdunit::println("vm_test: OK");
    libdunit::exit(0)
}
