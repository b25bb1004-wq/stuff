_start:
    addi sp, sp, -1040
    addi s1, sp, 1024
    li s0, 0
loop:
    addi a0, sp, 0
    li a1, 1024
    addi a2, s1, 0
    li a7, 23
    ecall
    li t1, 0
    blt a0, t1, busy
    addi a2, a0, 0
    addi a1, sp, 0
    lbu a0, 0(s1)
    li a7, 22
    ecall
    li t1, 0
    bne a0, t1, fail
    addi s0, s0, 1
    li t0, 2
    bge s0, t0, finish
    j loop
busy:
    li t1, -7
    bne a0, t1, fail
    li a7, 9
    ecall
    j loop
finish:
    li a0, 0
    li a7, 0
    ecall
fail:
    li a0, 1
    li a7, 0
    ecall
