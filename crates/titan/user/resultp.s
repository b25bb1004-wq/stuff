_start:
    addi sp, sp, -256
    la a0, name
    li a1, 7
    li a2, 0
    li a3, 0
    li a4, 0
    li a5, 0
    li a7, 5
    ecall
    addi s0, a0, 0
    addi a0, s0, 0
    li a7, 6
    ecall
    addi a0, s0, 0
    addi a1, sp, 0
    li a2, 256
    li a7, 25
    ecall
    addi s1, a0, 0
    li t1, 0
    blt s1, t1, fail
    li a0, 1
    addi a1, sp, 0
    addi a2, s1, 0
    li a7, 1
    ecall
    li a0, 0
    li a7, 0
    ecall
fail:
    li a0, 1
    li a7, 0
    ecall

.balign 4096
_data:
name:
    .ascii "resulty"
