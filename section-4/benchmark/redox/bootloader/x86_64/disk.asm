sectalign off

%include "bootsector.asm"

startup_start:
%ifdef ARCH_i386
    %include "startup-i386.asm"
%endif

%ifdef ARCH_x86_64
    %include "startup-x86_64.asm"
%endif
align 512, db 0
startup_end:

%ifdef KERNEL
    kernel_file:
      %defstr KERNEL_STR %[KERNEL]
      incbin KERNEL_STR
    .end:
    align 512, db 0
%else
    align BLOCK_SIZE, db 0
    %ifdef FILESYSTEM
        filesystem:
            %defstr FILESYSTEM_STR %[FILESYSTEM]
            incbin FILESYSTEM_STR
        .end:
        align BLOCK_SIZE, db 0
    %else
        filesystem:
    %endif
%endif
