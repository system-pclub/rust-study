SECTION .text
USE16

align 512, db 0

config:
  .xres: dw 0
  .yres: dw 0

times 512 - ($ - config) db 0

save_config:
    mov eax, (config - boot) / 512
    mov bx, config
    mov cx, 1
    xor dx, dx
    call store
    ret

; store some sectors to disk from a buffer in memory
; buffer has to be below 1MiB
; IN
;   ax: start sector
;   bx: offset of buffer
;   cx: number of sectors (512 Bytes each)
;   dx: segment of buffer
; CLOBBER
;   ax, bx, cx, dx, si
; TODO rewrite to (eventually) move larger parts at once
; if that is done increase buffer_size_sectors in startup-common to that (max 0x80000 - startup_end)
store:
    cmp cx, 127
    jbe .good_size

    pusha
    mov cx, 127
    call store
    popa
    add ax, 127
    add dx, 127 * 512 / 16
    sub cx, 127

    jmp store
.good_size:
    mov [DAPACK.addr], eax
    mov [DAPACK.buf], bx
    mov [DAPACK.count], cx
    mov [DAPACK.seg], dx

    call print_dapack

    mov dl, [disk]
    mov si, DAPACK
    mov ah, 0x43
    int 0x13
    jc error
    ret
