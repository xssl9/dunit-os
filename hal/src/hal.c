#include "hal.h"

extern void hal_init_gdt(void);
extern void hal_init_idt(void);
extern void syscall_init(void);

void hal_enable_interrupts(void) {
    __asm__ volatile("sti");
}

void hal_disable_interrupts(void) {
    __asm__ volatile("cli");
}

void hal_init(void) {
    hal_init_gdt();
    hal_init_idt();
    syscall_init();
    
    hal_outb(0x20, 0x11);
    hal_outb(0xA0, 0x11);
    hal_outb(0x21, 0x20);
    hal_outb(0xA1, 0x28);
    hal_outb(0x21, 0x04);
    hal_outb(0xA1, 0x02);
    hal_outb(0x21, 0x01);
    hal_outb(0xA1, 0x01);
    hal_outb(0x21, 0x0);
    hal_outb(0xA1, 0x0);
    
    hal_enable_interrupts();
}

void hal_set_vga_text_mode(void) {
    __asm__ volatile(
        "mov $0x0003, %%ax\n"
        "int $0x10\n"
        :
        :
        : "ax"
    );
}
