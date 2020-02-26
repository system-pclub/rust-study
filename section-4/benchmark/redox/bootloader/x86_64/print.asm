SECTION .text
USE16
; provide function for printing in x86 real mode

; print a string and a newline
; CLOBBER
;   ax
print_line:
    mov al, 13
    call print_char
    mov al, 10
    jmp print_char

; print a string
; IN
;   si: points at zero-terminated String
; CLOBBER
;   si, ax
print:
    pushf
    cld
.loop:
    lodsb
    test al, al
    jz .done
    call print_char
    jmp .loop
.done:
    popf
    ret

; print a character
; IN
;   al: character to print
print_char:
    pusha
    mov bx, 7
    mov ah, 0x0e
    int 0x10
    popa
    ret

; print a number in hex
; IN
;   bx: the number
; CLOBBER
;   al, cx
print_hex:
    mov cx, 4
.lp:
    mov al, bh
    shr al, 4

    cmp al, 0xA
    jb .below_0xA

    add al, 'A' - 0xA - '0'
.below_0xA:
    add al, '0'

    call print_char

    shl bx, 4
    loop .lp

    ret
