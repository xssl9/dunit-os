#![no_std]
#![no_main]

use core::panic::PanicInfo;

const STRESS_PATH: &str = "/tmp/runtime_stress.txt";
const STRESS_DATA: &[u8] = b"runtime stress vfs payload";
const READ_BUF_LEN: usize = 64;

static mut READ_BUF: [u8; READ_BUF_LEN] = [0; READ_BUF_LEN];

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
    }
}

fn fail(message: &str, code: i32) -> ! {
    libdunit::print("runtime_stress: FAIL ");
    libdunit::println(message);
    libdunit::exit(code);
}

fn append_u32_decimal(out: &mut [u8], mut len: usize, value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    let mut n = value;
    if n == 0 {
        digits[0] = b'0';
        count = 1;
    } else {
        while n > 0 {
            digits[count] = b'0' + (n % 10) as u8;
            count += 1;
            n /= 10;
        }
    }
    while count > 0 {
        count -= 1;
        out[len] = digits[count];
        len += 1;
    }
    len
}

fn wait_exited(pid: u32, code: i32, label: &str) {
    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid, &mut status);
    if waited != pid as isize {
        fail(label, 20);
    }
    if !status.exited() || status.code != code {
        fail(label, 21);
    }
}

fn wait_faulted(pid: u32, label: &str) {
    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid, &mut status);
    if waited != pid as isize {
        fail(label, 22);
    }
    if !status.faulted() || status.code >= 0 {
        fail(label, 23);
    }
}

fn expect_wait_would_block(pid: u32, label: &str) {
    let mut status = libdunit::WaitStatus::empty();
    if libdunit::wait(pid, &mut status) != libdunit::EAGAIN {
        fail(label, 24);
    }
    if status.kind != libdunit::WAIT_KIND_EMPTY || status.code != 0 {
        fail(label, 25);
    }
}

fn wait_exited_or_blocked(pid: u32, code: i32, label: &str) -> bool {
    let mut status = libdunit::WaitStatus::empty();
    let waited = libdunit::wait(pid, &mut status);
    if waited == libdunit::EAGAIN {
        if status.kind != libdunit::WAIT_KIND_EMPTY || status.code != 0 {
            fail(label, 25);
        }
        return false;
    }
    if waited != pid as isize {
        fail(label, 20);
    }
    if !status.exited() || status.code != code {
        fail(label, 21);
    }
    true
}

fn drive_until_exited(pid: u32, code: i32, label: &str) {
    let mut attempts = 0usize;
    while attempts < 8 {
        if wait_exited_or_blocked(pid, code, label) {
            return;
        }
        let yielded = libdunit::yield_now();
        if yielded < 0 && yielded != libdunit::EAGAIN {
            fail(label, 26);
        }
        attempts += 1;
    }
    wait_exited(pid, code, label);
}

fn exercise_vfs() {
    libdunit::println("runtime_stress: vfs start");

    let fd = libdunit::open(
        STRESS_PATH,
        libdunit::OPEN_CREATE | libdunit::OPEN_WRITE | libdunit::OPEN_TRUNC,
    );
    if fd < 0 {
        fail("vfs open write", 30);
    }
    if libdunit::write(fd as usize, STRESS_DATA) != STRESS_DATA.len() as isize {
        libdunit::close(fd as usize);
        fail("vfs write", 31);
    }
    if libdunit::close(fd as usize) != 0 {
        fail("vfs close write", 32);
    }

    let fd = libdunit::open(STRESS_PATH, libdunit::OPEN_READ);
    if fd < 0 {
        fail("vfs open read", 33);
    }
    let buf = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(READ_BUF) as *mut u8, READ_BUF_LEN)
    };
    let read = libdunit::read(fd as usize, buf);
    if read != STRESS_DATA.len() as isize {
        libdunit::close(fd as usize);
        fail("vfs read", 34);
    }
    let mut index = 0usize;
    while index < STRESS_DATA.len() {
        if buf[index] != STRESS_DATA[index] {
            libdunit::close(fd as usize);
            fail("vfs data mismatch", 35);
        }
        index += 1;
    }
    if libdunit::close(fd as usize) != 0 {
        fail("vfs close read", 36);
    }

    libdunit::println("runtime_stress: vfs OK");
}

fn exercise_resumable_roundtrip() {
    libdunit::println("runtime_stress: resumable start");
    let child = libdunit::spawn("resumable_child");
    if child < 0 {
        fail("spawn resumable_child", 40);
    }
    let child = child as u32;

    expect_wait_would_block(child, "resumable early wait");
    if libdunit::yield_now() != 0 {
        fail("yield to resumable child A", 41);
    }
    libdunit::println("runtime_stress: parent after child A");
    if !wait_exited_or_blocked(child, 7, "resumable mid wait") {
        drive_until_exited(child, 7, "wait resumable child");
    }
    libdunit::println("runtime_stress: resumable OK");
}

fn exercise_ipc_roundtrip() {
    libdunit::println("runtime_stress: ipc start");
    let mut empty = [0u8; 4];
    if libdunit::ipc_recv(&mut empty) != libdunit::EAGAIN {
        fail("ipc empty recv", 50);
    }

    let child = libdunit::spawn("ipc_child");
    if child < 0 {
        fail("spawn ipc_child", 51);
    }
    let child = child as u32;

    let mut ping = [0u8; 32];
    ping[..5].copy_from_slice(b"ping:");
    let len = append_u32_decimal(&mut ping, 5, libdunit::get_pid());
    if libdunit::ipc_send(child, &ping[..len]) != len as isize {
        fail("ipc send ping", 52);
    }
    if libdunit::yield_now() != 0 {
        fail("yield to ipc_child", 53);
    }

    let mut pong = [0u8; 16];
    let pong_len = libdunit::ipc_recv(&mut pong);
    if pong_len != 4 || &pong[..4] != b"pong" {
        fail("ipc recv pong", 54);
    }
    wait_exited(child, 0, "wait ipc_child");
    if libdunit::ipc_recv(&mut empty) != libdunit::EAGAIN {
        fail("ipc queue not empty", 55);
    }
    libdunit::println("runtime_stress: ipc OK");
}

fn exercise_repeated_spawn_wait() {
    libdunit::println("runtime_stress: repeated spawn start");
    let mut round = 0usize;
    while round < 3 {
        let child = libdunit::spawn("elf_demo");
        if child < 0 {
            fail("spawn elf_demo", 60);
        }
        let child = child as u32;
        expect_wait_would_block(child, "elf_demo early wait");
        if libdunit::yield_now() != 0 {
            fail("yield elf_demo", 61);
        }
        wait_exited(child, 0, "wait elf_demo");
        round += 1;
    }
    libdunit::println("runtime_stress: repeated spawn OK");
}

fn exercise_fault_after_normal_apps() {
    libdunit::println("runtime_stress: fault start");
    let child = libdunit::spawn("fault_pf");
    if child < 0 {
        fail("spawn fault_pf", 70);
    }
    let child = child as u32;
    if libdunit::yield_now() != 0 {
        fail("yield fault_pf", 71);
    }
    wait_faulted(child, "wait fault_pf");
    libdunit::println("runtime_stress: fault OK");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    libdunit::println("runtime_stress: start");
    exercise_vfs();
    exercise_resumable_roundtrip();
    exercise_ipc_roundtrip();
    exercise_repeated_spawn_wait();
    exercise_fault_after_normal_apps();
    libdunit::println("runtime_stress: OK");
    libdunit::exit(0);
}
