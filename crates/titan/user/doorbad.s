_start:
    li a0, 0
    la a1, msg
    li a2, 1
    li a7, 22
    ecall
    li t0, -1
    bne a0, t0, done
    la a1, refused
    li a0, 1
    li a2, 8
    li a7, 1
    ecall
done:
    li a0, 0
    li a7, 0
    ecall

.balign 4096
_data:
msg:
    .ascii "x"
refused:
    .ascii "refused\n"
