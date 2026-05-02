ENTRY(_start)

PHDRS {
    text     PT_LOAD FLAGS((1 << 0) | (1 << 2));
    rodata   PT_LOAD FLAGS((1 << 2));
    data     PT_LOAD FLAGS((1 << 1) | (1 << 2));
}

SECTIONS {
    . = 0xFFFFFFFF80000000;

    .text : ALIGN(4K) {
        KEEP(*(.limine_requests_start_marker))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests_end_marker))
        *(.text .text.*)
    } :text

    . = ALIGN(4K);

    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4K);

    .data : ALIGN(4K) {
        *(.data .data.*)
    } :data

    .bss : ALIGN(4K) {
        *(COMMON)
        *(.bss .bss.*)
    } :data
}
