OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x80000000;

    _stext = .;
    .text : ALIGN(4096) {
        *(.text.entry)
        *(.text .text.*)
    }
    . = ALIGN(4096);
    _etext = .;

    _srodata = .;
    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }
    . = ALIGN(4096);
    _erodata = .;

    _sdata = .;
    .data : ALIGN(4096) {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }
    . = ALIGN(4096);
    _edata = .;

    _sbss = .;
    .bss : ALIGN(4096) {
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
        . = ALIGN(16384);
        _ebss = .;
        _stack_bottom = .;
        . += 0x10000;
        _stack_top = .;
    }
    . = ALIGN(4096);
    _end = .;

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
        *(.comment)
    }
}
