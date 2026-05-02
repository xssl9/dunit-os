#include <stdint.h>
#include "hal.h"

typedef struct {
    uint16_t limit_low;
    uint16_t base_low;
    uint8_t base_middle;
    uint8_t access;
    uint8_t granularity;
    uint8_t base_high;
} __attribute__((packed)) gdt_entry_t;

typedef struct {
    uint16_t limit;
    uint64_t base;
} __attribute__((packed)) gdt_ptr_t;

gdt_entry_t gdt[5];
gdt_ptr_t gdt_ptr;

extern void gdt_flush(uint64_t);

void gdt_set_gate(int num, uint64_t base, uint64_t limit, uint8_t access, uint8_t gran) {
    gdt[num].base_low = (base & 0xFFFF);
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].base_high = (base >> 24) & 0xFF;
    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].granularity = (limit >> 16) & 0x0F;
    gdt[num].granularity |= gran & 0xF0;
    gdt[num].access = access;
}

void hal_init_gdt(void) {
    gdt_ptr.limit = (sizeof(gdt_entry_t) * 5) - 1;
    gdt_ptr.base = (uint64_t)&gdt;
    
    gdt_set_gate(0, 0, 0, 0, 0);
    gdt_set_gate(1, 0, 0xFFFFFFFF, 0x9A, 0xA0);
    gdt_set_gate(2, 0, 0xFFFFFFFF, 0x92, 0xA0);
    gdt_set_gate(3, 0, 0xFFFFFFFF, 0xFA, 0xA0);
    gdt_set_gate(4, 0, 0xFFFFFFFF, 0xF2, 0xA0);
    
    gdt_flush((uint64_t)&gdt_ptr);
}
