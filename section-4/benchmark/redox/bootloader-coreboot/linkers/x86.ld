ENTRY(kstart)
OUTPUT_FORMAT(elf32-i386)

SECTIONS {
    . = 0x8000;

    . += SIZEOF_HEADERS;
    . = ALIGN(4096);

    .text : {
        __text_start = .;
        *(.text*)
		. = ALIGN(4096);
        __text_end = .;
    }

	.rodata : {
        __rodata_start = .;
        *(.rodata*)
		. = ALIGN(4096);
        __rodata_end = .;
    }

    .data : {
        __data_start = .;
        *(.data*)
		. = ALIGN(4096);
        __data_end = .;
        __bss_start = .;
        *(.bss*)
        . = ALIGN(4096);
        __bss_end = .;
    }

    .tdata : {
        __tdata_start = .;
        *(.tdata*)
        . = ALIGN(4096);
        __tdata_end = .;
        __tbss_start = .;
        *(.tbss*)
        . += 8;
        . = ALIGN(4096);
        __tbss_end = .;
    }

    __end = .;

    . = 0x100000;

    .kernel : {
        *(.kernel*)
    }

    /DISCARD/ : {
        *(.comment*)
        *(.eh_frame*)
        *(.gcc_except_table*)
        *(.note*)
        *(.rel.eh_frame*)
    }
}
