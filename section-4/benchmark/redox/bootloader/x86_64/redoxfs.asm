%define BLOCK_SHIFT 12
%define BLOCK_SIZE (1 << BLOCK_SHIFT)

struc Extent
    .block: resq 1,
    .length: resq 1
endstruc

struc Node
    .mode: resw 1
    .uid: resd 1
    .gid: resd 1
    .ctime: resq 1
    .ctime_nsec: resd 1
    .mtime: resq 1
    .mtime_nsec: resd 1
    .atime: resq 1
    .atime_nsec: resd 1
    .name: resb 226
    .parent: resq 1
    .next: resq 1
    .extents: resb (BLOCK_SIZE - 288)
endstruc

struc Header
    ; Signature, should be b"RedoxFS\0"
    .signature: resb 8
    ; Version, should be 4
    .version: resq 1,
    ; Disk ID, a 128-bit unique identifier
    .uuid: resb 16,
    ; Disk size, in BLOCK_SIZE-byte sectors
    .size: resq 1,
    ; Block of root node
    .root: resq 1,
    ; Block of free space node
    .free: resq 1
    ; Padding
    .padding: resb (BLOCK_SIZE - 56)
endstruc

; IN
; eax - the first sector of the filesystem
redoxfs:
        mov [.first_sector], eax
        call redoxfs.open
        test eax, eax
        jz .good_header
        ret

    .good_header:
        mov eax, [.header + Header.root]
        mov bx, .dir
        call .node

        jmp redoxfs.root

    ; node in eax, buffer in bx
    .node:
        shl eax, (BLOCK_SHIFT - 9)
        add eax, [redoxfs.first_sector]
        mov cx, (BLOCK_SIZE/512)
        mov dx, 0
        call load
        call print_line
        ret

        align BLOCK_SIZE, db 0

    .header:
        times BLOCK_SIZE db 0

    .dir:
        times BLOCK_SIZE db 0

    .file:
        times BLOCK_SIZE db 0

    .first_sector: dd 0

    .env:
        db "REDOXFS_BLOCK="
    .env.block:
        db "0000000000000000"
    .env.block_end:
        db `\n`
        db "REDOXFS_UUID="
    .env.uuid:
        db "00000000-0000-0000-0000-000000000000"
    .env.end:

redoxfs.open:
        mov eax, 0
        mov bx, redoxfs.header
        call redoxfs.node

        mov bx, 0
    .sig:
        mov al, [redoxfs.header + Header.signature + bx]
        mov ah, [.signature + bx]
        cmp al, ah
        jne .sig_err
        inc bx
        cmp bx, 8
        jl .sig

        mov bx, 0
    .ver:
        mov al, [redoxfs.header + Header.version + bx]
        mov ah, [.version + bx]
        cmp al, ah
        jne .ver_err
        inc bx
        jl .ver

        lea si, [redoxfs.header + Header.signature]
        call print
        mov al, ' '
        call print_char

        push eax
        push edx
        xor edx, edx
        mov eax, [redoxfs.first_sector]
        mov ebx, (BLOCK_SIZE / 512)
        div ebx ; EDX:EAX = EDX:EAX / EBX
        mov ebx, eax
        pop edx
        pop eax
        mov di, redoxfs.env.block_end - 1
    .block:
        mov al, bl
        and al, 0x0F
        cmp al, 0x0A
        jb .block.below_0xA
        add al, 'A' - 0xA - '0'
    .block.below_0xA:
        add al, '0'
        mov [di], al
        dec di
        shr ebx, 4
        test ebx, ebx
        jnz .block

        mov di, redoxfs.env.uuid
        xor si, si
    .uuid:
        cmp si, 4
        je .uuid.dash
        cmp si, 6
        je .uuid.dash
        cmp si, 8
        je .uuid.dash
        cmp si, 10
        je .uuid.dash
        jmp .uuid.no_dash
    .uuid.dash:
        mov al, '-'
        mov [di], al
        inc di
    .uuid.no_dash:
        mov bx, [redoxfs.header + Header.uuid + si]
        rol bx, 8

        mov cx, 4
    .uuid.char:
        mov al, bh
        shr al, 4

        cmp al, 0xA
        jb .uuid.below_0xA

        add al, 'a' - 0xA - '0'
    .uuid.below_0xA:
        add al, '0'

        mov [di], al
        inc di

        shl bx, 4
        loop .uuid.char

        add si, 2
        cmp si, 16
        jb .uuid

        mov si, redoxfs.env.uuid
        call print
        call print_line

        xor ax, ax
        ret

    .err_msg: db "Failed to open RedoxFS: ",0
    .sig_err_msg: db "Signature error",13,10,0
    .ver_err_msg: db "Version error",13,10,0

    .sig_err:
        mov si, .err_msg
        call print

        mov si, .sig_err_msg
        call print

        mov ax, 1
        ret

    .ver_err:
        mov si, .err_msg
        call print

        mov si, .ver_err_msg
        call print

        mov ax, 1
        ret

    .signature: db "RedoxFS",0
    .version: dq 4


redoxfs.root:
        lea si, [redoxfs.dir + Node.name]
        call print
        call print_line

    .lp:
        mov bx, 0
    .ext:
        mov eax, [redoxfs.dir + Node.extents + bx + Extent.block]
        test eax, eax
        jz .next

        mov ecx, [redoxfs.dir + Node.extents + bx + Extent.length]
        test ecx, ecx
        jz .next

        add ecx, BLOCK_SIZE
        dec ecx
        shr ecx, BLOCK_SHIFT

        push bx

    .ext_sec:
        push eax
        push ecx

        mov bx, redoxfs.file
        call redoxfs.node

        mov bx, 0
    .ext_sec_kernel:
        mov al, [redoxfs.file + Node.name + bx]
        mov ah, [.kernel_name + bx]

        cmp al, ah
        jne .ext_sec_kernel_break

        inc bx

        test ah, ah
        jnz .ext_sec_kernel

        pop ecx
        pop eax
        pop bx
        jmp redoxfs.kernel

    .ext_sec_kernel_break:
        pop ecx
        pop eax

        inc eax
        dec ecx
        jnz .ext_sec

        pop bx

        add bx, Extent_size
        cmp bx, (BLOCK_SIZE - 272)
        jb .ext

    .next:
        mov eax, [redoxfs.dir + Node.next]
        test eax, eax
        jz .no_kernel

        mov bx, redoxfs.dir
        call redoxfs.node
        jmp .lp

    .no_kernel:
        mov si, .no_kernel_msg
        call print

        mov si, .kernel_name
        call print

        call print_line

        mov eax, 1
        ret

    .kernel_name: db "kernel",0
    .no_kernel_msg: db "Did not find: ",0

redoxfs.kernel:
        lea si, [redoxfs.file + Node.name]
        call print
        call print_line

        mov edi, [args.kernel_base]
    .lp:
        mov bx, 0
    .ext:
        mov eax, [redoxfs.file + Node.extents + bx + Extent.block]
        test eax, eax
        jz .next

        mov ecx, [redoxfs.file + Node.extents + bx + Extent.length]
        test ecx, ecx
        jz .next

        push bx

        push eax
        push ecx
        push edi


        shl eax, (BLOCK_SHIFT - 9)
        add eax, [redoxfs.first_sector]
        add ecx, BLOCK_SIZE
        dec ecx
        shr ecx, 9
        call load_extent

        pop edi
        pop ecx
        pop eax

        add edi, ecx

        pop bx

        add bx, Extent_size
        cmp bx, Extent_size * 16
        jb .ext

    .next:
        mov eax, [redoxfs.file + Node.next]
        test eax, eax
        jz .done

        push edi

        mov bx, redoxfs.file
        call redoxfs.node

        pop edi
        jmp .lp

    .done:
        sub edi, [args.kernel_base]
        mov [args.kernel_size], edi

        xor eax, eax
        ret
