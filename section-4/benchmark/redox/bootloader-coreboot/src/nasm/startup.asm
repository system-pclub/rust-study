SECTION .text
USE32

GLOBAL startup
startup:
    ;disable paging
    mov eax, cr0
    and eax, 0x7FFFFFFF
    mov cr0, eax

    ;cr3 holds pointer to PML4
    mov eax, 0x70000
    mov cr3, eax

    ;enable OSXSAVE, FXSAVE/FXRSTOR, Page Global, Page Address Extension, and Page Size Extension
    mov eax, cr4
    or eax, 1 << 18 | 1 << 9 | 1 << 7 | 1 << 5 | 1 << 4
    mov cr4, eax

    ; load protected mode GDT
    lgdt [gdtr]

    mov ecx, 0xC0000080               ; Read from the EFER MSR.
    rdmsr
    or eax, 1 << 11 | 1 << 8          ; Set the Long-Mode-Enable and NXE bit.
    wrmsr

    ;enabling paging and protection simultaneously
    mov eax, cr0
    or eax, 1 << 31 | 1 << 16 | 1                ;Bit 31: Paging, Bit 16: write protect kernel, Bit 0: Protected Mode
    mov cr0, eax

    ; far jump to enable Long Mode and load CS with 64 bit segment
    jmp gdt.kernel_code:long_mode

USE64
long_mode:
    ; load all the other segments with 64 bit data segments
    mov rax, gdt.kernel_data
    mov ds, rax
    mov es, rax
    mov fs, rax
    mov gs, rax
    mov ss, rax

%ifdef KERNEL
    ; set kernel size
    mov rax, __kernel_end
    sub rax, __kernel
    mov [args.kernel_size], rax

    ; set stack pointer
    mov rsp, [args.stack_base]
    add rsp, [args.stack_size]

    ; align stack
    and rsp, 0xFFFFFFFFFFFFFFF0

    ; set args
    mov rdi, args

    ; entry point
    mov rax, [args.kernel_base]
    call [rax + 0x18]
%endif

.halt:
    cli
    hlt
    jmp .halt

SECTION .data

args:
    .kernel_base dq 0x100000
    .kernel_size dq 0
    .stack_base dq 0xFFFFFF0000080000
    .stack_size dq 0x1F000
    .env_base dq 0
    .env_size dq 0

%include "descriptor_flags.inc"
%include "gdt_entry.inc"

gdtr:
    dw gdt.end + 1  ; size
    dq gdt          ; offset

gdt:
.null equ $ - gdt
    dq 0

.kernel_code equ $ - gdt
istruc GDTEntry
    at GDTEntry.limitl, dw 0
    at GDTEntry.basel, dw 0
    at GDTEntry.basem, db 0
    at GDTEntry.attribute, db attrib.present | attrib.user | attrib.code
    at GDTEntry.flags__limith, db flags.long_mode
    at GDTEntry.baseh, db 0
iend

.kernel_data equ $ - gdt
istruc GDTEntry
    at GDTEntry.limitl, dw 0
    at GDTEntry.basel, dw 0
    at GDTEntry.basem, db 0
; AMD System Programming Manual states that the writeable bit is ignored in long mode, but ss can not be set to this descriptor without it
    at GDTEntry.attribute, db attrib.present | attrib.user | attrib.writable
    at GDTEntry.flags__limith, db 0
    at GDTEntry.baseh, db 0
iend

.end equ $ - gdt

SECTION .kernel
%ifdef KERNEL
__kernel:
      %defstr KERNEL_STR %[KERNEL]
      INCBIN KERNEL_STR
__kernel_end:
%endif
