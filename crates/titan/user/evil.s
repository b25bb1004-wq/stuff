_start:
    # write(1, 0x8000_0000, 16) -> -1
    li a0, 1
    li a1, 0x80000000
    li a2, 16
    li a7, 1
    ecall
    li t0, -1
    bne a0, t0, fail

    # write(1, 0x3F_FFFF_FF00, 4096) -> -1
    li a0, 1
    li a1, 0x3FFFFFFF00
    li a2, 4096
    li a7, 1
    ecall
    li t0, -1
    bne a0, t0, fail

    # kill(1) from non-parent -> -1
    li a0, 1
    li a7, 7
    ecall
    li t0, -1
    bne a0, t0, fail

    li a0, 1
    la a1, msg
    li a2, 15
    li a7, 1
    ecall

    li a0, 3
    li a7, 0
    ecall

fail:
    li a0, 1
    li a7, 0
    ecall

.balign 4096
_data:
msg:
    .ascii "evil: rejected\n"
