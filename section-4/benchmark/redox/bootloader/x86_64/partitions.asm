struc mbr_partition_rec
.sys: resb 1
.chs_start: resb 3
.ty: resb 1
.chs_end: resb 3
.lba_start: resd 1
.sector_count: resd 1
endstruc

; Find a partition to load RedoxFS from.
; The partition has to be one of the primary MBR partitions.
; OUT
;   eax - start_lba
; CLOBBER
;   ebx
find_redoxfs_partition:
    xor ebx, ebx
.loop:
    mov al, byte [partitions + mbr_partition_rec + mbr_partition_rec.ty]
    cmp al, 0x83
    je .found
    add ebx, 1
    cmp ebx, 4
    jb .loop
    jmp .notfound
.found:
    mov eax, [partitions + mbr_partition_rec + mbr_partition_rec.lba_start]
    ret
.notfound:
    mov si, .no_partition_found_msg
    call print
    mov eax, (filesystem - boot) / 512
    ret
.no_partition_found_msg: db "No MBR partition with type 0x83 found", 0xA, 0xD, 0
