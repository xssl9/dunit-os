#ifndef HAL_H
#define HAL_H

#include <stdint.h>

void hal_init(void);
void hal_init_gdt(void);
void hal_init_idt(void);
void syscall_init(void);

void hal_enable_interrupts(void);
void hal_disable_interrupts(void);
void hal_set_vga_text_mode(void);

uint8_t hal_inb(uint16_t port);
void hal_outb(uint16_t port, uint8_t value);
uint16_t hal_inw(uint16_t port);
void hal_outw(uint16_t port, uint16_t value);
uint32_t hal_inl(uint16_t port);
void hal_outl(uint16_t port, uint32_t value);

void hal_run_tests(void);
int hal_get_test_passed(void);
int hal_get_test_failed(void);

#endif
