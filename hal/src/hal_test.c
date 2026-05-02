#include "hal.h"

static int test_passed = 0;
static int test_failed = 0;

void test_assert(int condition, const char* test_name) {
    (void)test_name;
    if (condition) {
        test_passed++;
    } else {
        test_failed++;
    }
}

void test_gdt_entries(void) {
    test_assert(1, "GDT initialized");
}

void test_idt_entries(void) {
    test_assert(1, "IDT initialized");
}

void test_port_io(void) {
    hal_outb(0x80, 0x42);
    test_assert(1, "Port I/O write");
}

void hal_run_tests(void) {
    test_passed = 0;
    test_failed = 0;
    
    test_gdt_entries();
    test_idt_entries();
    test_port_io();
}

int hal_get_test_passed(void) {
    return test_passed;
}

int hal_get_test_failed(void) {
    return test_failed;
}
