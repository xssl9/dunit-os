#![no_std]
#![no_main]

//! Dunit Userspace ABI v0 capability-query smoke. Exercises `sys_abi_query`
//! (syscall 74) through libdunit and checks every selector: version, syscall
//! count, has-syscall (present + absent), and the ENOSYS path for an unknown
//! selector. Prints `[ABI-QUERY-TEST] OK` and exits 0 only on a full pass.

extern crate alloc;

use alloc::format;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    libdunit::println("[ABI-QUERY-TEST] FAIL panic");
    libdunit::exit(101)
}

fn fail(why: &str) -> ! {
    libdunit::println(why);
    libdunit::println("[ABI-QUERY-TEST] FAIL");
    libdunit::exit(1)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let version = libdunit::abi_version();
    let count = libdunit::abi_query(libdunit::ABI_QUERY_SYSCALL_COUNT, 0);
    let has_self = libdunit::abi_query(libdunit::ABI_QUERY_HAS_SYSCALL, libdunit::SYSCALL_ABI_QUERY);
    let has_absent = libdunit::abi_query(libdunit::ABI_QUERY_HAS_SYSCALL, 9999);
    let unknown = libdunit::abi_query(99, 0);
    libdunit::println(&format!(
        "abi_test: version={} count={} has_self={} has_absent={} unknown={} (abiq#={})",
        version, count, has_self, has_absent, unknown, libdunit::SYSCALL_ABI_QUERY
    ));

    if version != 0 {
        fail("abi_test: unexpected ABI version");
    }
    if count <= libdunit::SYSCALL_ABI_QUERY as isize {
        fail("abi_test: syscall count does not cover AbiQuery");
    }
    if has_self != 1 {
        fail("abi_test: HAS_SYSCALL(AbiQuery) != 1");
    }
    if has_absent != 0 {
        fail("abi_test: HAS_SYSCALL(9999) != 0");
    }
    if unknown != libdunit::ENOSYS {
        fail("abi_test: unknown selector did not return ENOSYS");
    }
    libdunit::println("[ABI-QUERY-TEST] OK");
    libdunit::exit(0)
}
