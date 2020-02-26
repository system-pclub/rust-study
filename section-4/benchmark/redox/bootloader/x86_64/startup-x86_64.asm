trampoline:
    .ready: dq 0
    .cpu_id: dq 0
    .page_table: dq 0
    .stack_start: dq 0
    .stack_end: dq 0
    .code: dq 0

    times 512 - ($ - trampoline) db 0

startup_ap:
    cli

    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax

    ; initialize stack
    mov sp, 0x7C00

    call initialize.fpu
    call initialize.sse

    ;cr3 holds pointer to PML4
    mov edi, 0x70000
    mov cr3, edi

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
    mov ebx, cr0
    or ebx, 1 << 31 | 1 << 16 | 1                ;Bit 31: Paging, Bit 16: write protect kernel, Bit 0: Protected Mode
    mov cr0, ebx

    ; far jump to enable Long Mode and load CS with 64 bit segment
    jmp gdt.kernel_code:long_mode_ap

%include "startup-common.asm"

startup_arch:
    cli
    ; setting up Page Tables
    ; Identity Mapping first GB
    mov ax, 0x7000
    mov es, ax

    xor edi, edi
    xor eax, eax
    mov ecx, 6 * 4096 / 4 ;PML4, PDP, 4 PD / moves 4 Bytes at once
    cld
    rep stosd

    xor edi, edi
    ;Link first PML4 and second to last PML4 to PDP
    mov DWORD [es:edi], 0x71000 | 1 << 1 | 1
    mov DWORD [es:edi + 510*8], 0x71000 | 1 << 1 | 1
    add edi, 0x1000
    ;Link last PML4 to PML4
    mov DWORD [es:edi - 8], 0x70000 | 1 << 1 | 1
    ;Link first four PDP to PD
    mov DWORD [es:edi], 0x72000 | 1 << 1 | 1
    mov DWORD [es:edi + 8], 0x73000 | 1 << 1 | 1
    mov DWORD [es:edi + 16], 0x74000 | 1 << 1 | 1
    mov DWORD [es:edi + 24], 0x75000 | 1 << 1 | 1
    add edi, 0x1000
    ;Link all PD's (512 per PDP, 2MB each)y
    mov ebx, 1 << 7 | 1 << 1 | 1
    mov ecx, 4*512
.setpd:
    mov [es:edi], ebx
    add ebx, 0x200000
    add edi, 8
    loop .setpd

    xor ax, ax
    mov es, ax

    ;cr3 holds pointer to PML4
    mov edi, 0x70000
    mov cr3, edi

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
    mov ebx, cr0
    or ebx, 1 << 31 | 1 << 16 | 1                ;Bit 31: Paging, Bit 16: write protect kernel, Bit 0: Protected Mode
    mov cr0, ebx

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

    ; stack_base
    mov rsi, 0xFFFFFF0000080000
    mov [args.stack_base], rsi
    ; stack_size
    mov rcx, 0x1F000
    mov [args.stack_size], rcx

    ; set stack pointer
    mov rsp, rsi
    add rsp, rcx

    ; copy env to stack
%ifdef KERNEL
    mov rsi, 0
    mov rcx, 0
%else
    mov rsi, redoxfs.env
    mov rcx, redoxfs.env.end - redoxfs.env
%endif
    mov [args.env_size], rcx
.copy_env:
    cmp rcx, 0
    je .no_env
    dec rcx
    mov al, [rsi + rcx]
    dec rsp
    mov [rsp], al
    jmp .copy_env
.no_env:
    mov [args.env_base], rsp

    ; align stack
    and rsp, 0xFFFFFFFFFFFFFFF0

    ; set args
    mov rdi, args

    ; entry point
    mov rax, [args.kernel_base]
    call [rax + 0x18]
.halt:
    cli
    hlt
    jmp .halt

long_mode_ap:
    mov rax, gdt.kernel_data
    mov ds, rax
    mov es, rax
    mov fs, rax
    mov gs, rax
    mov ss, rax

    mov rcx, [trampoline.stack_end]
    lea rsp, [rcx - 256]

    mov rdi, trampoline.cpu_id

    mov rax, [trampoline.code]
    mov qword [trampoline.ready], 1
    jmp rax

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
