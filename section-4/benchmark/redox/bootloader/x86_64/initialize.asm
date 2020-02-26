SECTION .text
USE16

initialize:
.fpu: ;enable fpu
    mov eax, cr0
    and al, 11110011b
    or al, 00100010b
    mov cr0, eax
    mov eax, cr4
    or eax, 0x200
    mov cr4, eax
    fninit
    ret

.sse: ;enable sse
    mov eax, cr4
    or ax, 0000011000000000b
    mov cr4, eax
    ret
