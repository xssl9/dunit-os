#include <stdint.h>

#define NULL ((void*)0)

struct limine_framebuffer {
    void *address;
    uint64_t width;
    uint64_t height;
    uint64_t pitch;
    uint16_t bpp;
    uint8_t memory_model;
    uint8_t red_mask_size;
    uint8_t red_mask_shift;
    uint8_t green_mask_size;
    uint8_t green_mask_shift;
    uint8_t blue_mask_size;
    uint8_t blue_mask_shift;
    uint8_t unused[7];
    uint64_t edid_size;
    void *edid;
    uint64_t mode_count;
    void **modes;
};

struct limine_framebuffer_response {
    uint64_t revision;
    uint64_t framebuffer_count;
    struct limine_framebuffer **framebuffers;
};

struct limine_terminal;

typedef void (*limine_terminal_write)(struct limine_terminal *, const char *, uint64_t);

struct limine_terminal {
    uint64_t columns;
    uint64_t rows;
    struct limine_framebuffer *framebuffer;
};

struct limine_terminal_response {
    uint64_t revision;
    uint64_t terminal_count;
    struct limine_terminal **terminals;
    limine_terminal_write write;
};

extern struct limine_framebuffer_response *get_framebuffer_response(void);
extern struct limine_terminal_response *get_terminal_response(void);

void serial_init() {
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x00), "Nd"((uint16_t)0x3F8 + 1));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x80), "Nd"((uint16_t)0x3F8 + 3));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x03), "Nd"((uint16_t)0x3F8 + 0));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x00), "Nd"((uint16_t)0x3F8 + 1));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x03), "Nd"((uint16_t)0x3F8 + 3));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0xC7), "Nd"((uint16_t)0x3F8 + 2));
    __asm__ volatile("outb %0, %1" : : "a"((uint8_t)0x0B), "Nd"((uint16_t)0x3F8 + 4));
}

void serial_write(const char* str) {
    for (int i = 0; str[i] != '\0'; i++) {
        while (1) {
            uint8_t status;
            __asm__ volatile("inb %1, %0" : "=a"(status) : "Nd"((uint16_t)0x3F8 + 5));
            if (status & 0x20) break;
        }
        __asm__ volatile("outb %0, %1" : : "a"((uint8_t)str[i]), "Nd"((uint16_t)0x3F8));
    }
}

static int strlen(const char *str) {
    int len = 0;
    while (str[len]) len++;
    return len;
}

extern void kernel_main(struct limine_framebuffer *fb, struct limine_terminal_response *term);

void boot_main(void) {
    serial_init();
    serial_write("[BOOT] boot_main called\r\n");
    
    struct limine_framebuffer_response *fb_resp = get_framebuffer_response();
    struct limine_terminal_response *term_resp = get_terminal_response();
    
    struct limine_framebuffer *fb = NULL;
    if (fb_resp && fb_resp->framebuffer_count > 0) {
        fb = fb_resp->framebuffers[0];
        serial_write("[BOOT] Framebuffer available\r\n");
    } else {
        serial_write("[BOOT] No framebuffer available\r\n");
    }
    
    serial_write("[BOOT] Calling kernel_main...\r\n");
    kernel_main(fb, term_resp);
    
    serial_write("[BOOT] kernel_main returned (should not happen)\r\n");
    
    while(1) {
        __asm__ volatile("hlt");
    }
}
