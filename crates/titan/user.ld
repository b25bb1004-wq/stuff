OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x10000;

    .text : ALIGN(4096) {
        *(.text.entry)
        *(.text .text.*)
    }
    . = ALIGN(4096);

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }
    . = ALIGN(4096);

    .data : ALIGN(4096) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    . = ALIGN(4096);

    .bss : ALIGN(4096) {
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
    }
    . = ALIGN(4096);
    _end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}
