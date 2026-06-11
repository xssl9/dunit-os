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

typedef struct {
    uint32_t reserved0;
    uint64_t rsp0;
    uint64_t rsp1;
    uint64_t rsp2;
    uint64_t reserved1;
    uint64_t ist1;
    uint64_t ist2;
    uint64_t ist3;
    uint64_t ist4;
    uint64_t ist5;
    uint64_t ist6;
    uint64_t ist7;
    uint64_t reserved2;
    uint16_t reserved3;
    uint16_t iomap_base;
} __attribute__((packed)) tss_t;

gdt_entry_t gdt[7];
gdt_ptr_t gdt_ptr;
static tss_t kernel_tss;
static uint8_t kernel_tss_stack[16384] __attribute__((aligned(16)));

extern void gdt_flush(uint64_t);
extern void gdt_load_tss(uint16_t);

void gdt_set_gate(int num, uint64_t base, uint64_t limit, uint8_t access, uint8_t gran) {
    gdt[num].base_low = (base & 0xFFFF);
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].base_high = (base >> 24) & 0xFF;
    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].granularity = (limit >> 16) & 0x0F;
    gdt[num].granularity |= gran & 0xF0;
    gdt[num].access = access;
}

static void gdt_set_tss_gate(int num, uint64_t base, uint32_t limit) {
    gdt[num].limit_low = limit & 0xFFFF;
    gdt[num].base_low = base & 0xFFFF;
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].access = 0x89;
    gdt[num].granularity = ((limit >> 16) & 0x0F);
    gdt[num].base_high = (base >> 24) & 0xFF;

    uint64_t *upper = (uint64_t *)&gdt[num + 1];
    *upper = (base >> 32) & 0xFFFFFFFF;
}

void hal_init_gdt(void) {
    gdt_ptr.limit = (sizeof(gdt_entry_t) * 7) - 1;
    gdt_ptr.base = (uint64_t)&gdt;
    
    gdt_set_gate(0, 0, 0, 0, 0);
    gdt_set_gate(1, 0, 0xFFFFFFFF, 0x9A, 0xA0);
    gdt_set_gate(2, 0, 0xFFFFFFFF, 0x92, 0xA0);
    gdt_set_gate(3, 0, 0xFFFFFFFF, 0xFA, 0xA0);
    gdt_set_gate(4, 0, 0xFFFFFFFF, 0xF2, 0xA0);

    kernel_tss.rsp0 = (uint64_t)&kernel_tss_stack[sizeof(kernel_tss_stack)];
    kernel_tss.iomap_base = sizeof(tss_t);
    gdt_set_tss_gate(5, (uint64_t)&kernel_tss, sizeof(tss_t) - 1);
    
    gdt_flush((uint64_t)&gdt_ptr);
    gdt_load_tss(0x28);
}
