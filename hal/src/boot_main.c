#include <stdint.h>

#define NULL ((void*)0)

#define MEM_REGION_MAX 32

typedef struct {
    uint64_t base;
    uint64_t length;
    uint32_t type;
    uint32_t _pad;
} MemRegion;

MemRegion boot_mem_regions[MEM_REGION_MAX];
uint64_t boot_mem_region_count;

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

struct limine_kernel_file {
    uint64_t revision;
    void *address;
    uint64_t size;
    char *path;
    char *cmdline;
    uint32_t media_type;
    uint32_t unused;
    uint32_t tftp_ip;
    uint32_t tftp_port;
    uint32_t partition_index;
    uint32_t mbr_disk_id;
    void *gpt_disk_uuid;
    void *gpt_part_uuid;
    void *part_uuid;
};

struct limine_kernel_file_response {
    uint64_t revision;
    struct limine_kernel_file *kernel_file;
};

struct limine_hhdm_response {
    uint64_t revision;
    uint64_t offset;
};

struct limine_memmap_entry {
    uint64_t base;
    uint64_t length;
    uint64_t type;
};

struct limine_memmap_response {
    uint64_t revision;
    uint64_t entry_count;
    struct limine_memmap_entry **entries;
};

extern struct limine_framebuffer_response *get_framebuffer_response(void);
extern struct limine_terminal_response *get_terminal_response(void);
extern struct limine_kernel_file_response *get_kernel_file_response(void);
extern struct limine_hhdm_response *get_hhdm_response(void);
extern struct limine_memmap_response *get_memmap_response(void);

void serial_init() {
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x00), "d"((uint16_t)0x3F8 + 1));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x80), "d"((uint16_t)0x3F8 + 3));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x03), "d"((uint16_t)0x3F8 + 0));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x00), "d"((uint16_t)0x3F8 + 1));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x03), "d"((uint16_t)0x3F8 + 3));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0xC7), "d"((uint16_t)0x3F8 + 2));
    __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)0x0B), "d"((uint16_t)0x3F8 + 4));
}

void serial_write(const char* str) {
    for (int i = 0; str[i] != '\0'; i++) {
        while (1) {
            uint8_t status;
            __asm__ volatile("inb %%dx, %%al" : "=a"(status) : "d"((uint16_t)0x3F8 + 5));
            if (status & 0x20) break;
        }
        __asm__ volatile("outb %%al, %%dx" : : "a"((uint8_t)str[i]), "d"((uint16_t)0x3F8));
    }
}

static int strstr_contains(const char *haystack, const char *needle) {
    if (!haystack || !needle) return 0;

    while (*haystack) {
        const char *h = haystack;
        const char *n = needle;

        while (*h && *n && (*h == *n)) {
            h++;
            n++;
        }

        if (!*n) return 1;
        haystack++;
    }
    return 0;
}

extern void kernel_main(
    struct limine_framebuffer *fb,
    struct limine_terminal_response *term,
    int terminal_mode,
    uint64_t hhdm_offset
);

void boot_main(void) {
    boot_mem_region_count = 0;

    serial_init();
    serial_write("[BOOT] START\r\n");

    struct limine_framebuffer_response *fb_resp = get_framebuffer_response();
    struct limine_terminal_response *term_resp = get_terminal_response();
    struct limine_kernel_file_response *kf_resp = get_kernel_file_response();
    struct limine_hhdm_response *hhdm_resp = get_hhdm_response();
    struct limine_memmap_response *memmap_resp = get_memmap_response();

    uint64_t hhdm_offset = 0;
    if (hhdm_resp) {
        hhdm_offset = hhdm_resp->offset;
        serial_write("[BOOT] hhdm OK\r\n");
    } else {
        serial_write("[BOOT] hhdm FAIL\r\n");
    }

    serial_write("[BOOT] memmap START\r\n");
    if (memmap_resp && memmap_resp->entries) {
        uint64_t count = memmap_resp->entry_count;
        if (count > MEM_REGION_MAX) {
            count = MEM_REGION_MAX;
        }
        for (uint64_t i = 0; i < count; i++) {
            struct limine_memmap_entry *entry = memmap_resp->entries[i];
            if (!entry) {
                continue;
            }
            boot_mem_regions[boot_mem_region_count].base = entry->base;
            boot_mem_regions[boot_mem_region_count].length = entry->length;
            boot_mem_regions[boot_mem_region_count].type = (uint32_t)entry->type;
            boot_mem_regions[boot_mem_region_count]._pad = 0;
            boot_mem_region_count++;
        }
        serial_write("[BOOT] memmap OK\r\n");
    } else {
        serial_write("[BOOT] memmap FAIL\r\n");
    }

    int terminal_mode = 0;
    if (kf_resp && kf_resp->kernel_file && kf_resp->kernel_file->cmdline) {
        if (strstr_contains(kf_resp->kernel_file->cmdline, "mode=terminal")) {
            terminal_mode = 1;
            serial_write("[BOOT] terminal mode OK\r\n");
        }
    }

    struct limine_framebuffer *fb = NULL;
    if (fb_resp && fb_resp->framebuffer_count > 0) {
        fb = fb_resp->framebuffers[0];
        serial_write("[BOOT] framebuffer OK\r\n");
    } else {
        serial_write("[BOOT] framebuffer FAIL\r\n");
    }

    serial_write("[BOOT] kernel handoff START\r\n");
    kernel_main(fb, term_resp, terminal_mode, hhdm_offset);
    serial_write("[BOOT] kernel handoff FAIL\r\n");

    while (1) {
        __asm__ volatile("hlt");
    }
}
