ENTRY(_start)

SECTIONS {
    . = 0x7C00;

    .boot : {
        *(.boot)
    }

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(.bss .bss.*)
    }

    . = 0x7DFE;
    .signature : {
        SHORT(0xAA55)
    }
}
