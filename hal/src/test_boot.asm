[BITS 16]
[ORG 0x7C00]

start:
    mov ax, 0xB800
    mov es, ax
    mov si, msg
    xor di, di
    
.loop:
    lodsb
    test al, al
    jz .done
    mov ah, 0x0F
    stosw
    jmp .loop
    
.done:
    cli
    hlt
    jmp .done

msg db 'Kernel Boot Test!', 0

times 510-($-$$) db 0
dw 0xAA55
