SECTION .text
USE16

args:
    .kernel_base dq 0x100000
    .kernel_size dq 0
    .stack_base dq 0
    .stack_size dq 0
    .env_base dq 0
    .env_size dq 0

startup:
    ; enable A20-Line via IO-Port 92, might not work on all motherboards
    in al, 0x92
    or al, 2
    out 0x92, al

    %ifdef KERNEL
        mov edi, [args.kernel_base]
        mov ecx, (kernel_file.end - kernel_file)
        mov [args.kernel_size], ecx

        mov eax, (kernel_file - boot)/512
        add ecx, 511
        shr ecx, 9
        call load_extent
    %else

        %ifdef FILESYSTEM
            mov eax, (filesystem - boot) / 512
        %else
            call find_redoxfs_partition
        %endif

        call redoxfs
        test eax, eax
        jnz error
    %endif

    jmp .loaded_kernel

.loaded_kernel:
    call memory_map

    call vesa

    mov si, init_fpu_msg
    call print
    call initialize.fpu

    mov si, init_sse_msg
    call print
    call initialize.sse

    mov si, startup_arch_msg
    call print

    jmp startup_arch

; load a disk extent into high memory
; eax - sector address
; ecx - sector count
; edi - destination
load_extent:
    ; loading kernel to 1MiB
    ; move part of kernel to startup_end via bootsector#load and then copy it up
    ; repeat until all of the kernel is loaded
    buffer_size_sectors equ 127

.lp:
    cmp ecx, buffer_size_sectors
    jb .break

    ; saving counter
    push eax
    push ecx

    push edi

    ; populating buffer
    mov ecx, buffer_size_sectors
    mov bx, startup_end
    mov dx, 0x0

    ; load sectors
    call load

    ; set up unreal mode
    call unreal

    pop edi

    ; move data
    mov esi, startup_end
    mov ecx, buffer_size_sectors * 512 / 4
    cld
    a32 rep movsd

    pop ecx
    pop eax

    add eax, buffer_size_sectors
    sub ecx, buffer_size_sectors
    jmp .lp

.break:
    ; load the part of the kernel that does not fill the buffer completely
    test ecx, ecx
    jz .finish ; if cx = 0 => skip

    push ecx
    push edi

    mov bx, startup_end
    mov dx, 0x0
    call load

    ; moving remnants of kernel
    call unreal

    pop edi
    pop ecx

    mov esi, startup_end
    shl ecx, 7 ; * 512 / 4
    cld
    a32 rep movsd

.finish:
    call print_line
    ret

%include "config.asm"
%include "descriptor_flags.inc"
%include "gdt_entry.inc"
%include "unreal.asm"
%include "memory_map.asm"
%include "vesa.asm"
%include "initialize.asm"
%ifndef KERNEL
    %include "redoxfs.asm"
    %ifndef FILESYSTEM
        %include "partitions.asm"
    %endif
%endif

init_fpu_msg: db "Init FPU",13,10,0
init_sse_msg: db "Init SSE",13,10,0
init_pit_msg: db "Init PIT",13,10,0
init_pic_msg: db "Init PIC",13,10,0
startup_arch_msg: db "Startup Arch",13,10,0
