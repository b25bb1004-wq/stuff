_start:
    li a0, 1048576
    li a7, 10
    ecall
    blez a0, fail1
    mv s0, a0

    li t0, 0
    li t1, 256
    li t2, 4096
write_loop:
    bge t0, t1, check_start
    mul t3, t0, t2
    add t3, s0, t3
    andi t4, t0, 0xff
    xori t4, t4, 0x5a
    sb t4, 0(t3)
    addi t0, t0, 1
    j write_loop

check_start:
    li t0, 0
read_loop:
    bge t0, t1, ok_msg
    mul t3, t0, t2
    add t3, s0, t3
    lbu t5, 0(t3)
    andi t4, t0, 0xff
    xori t4, t4, 0x5a
    bne t5, t4, fail2
    addi t0, t0, 1
    j read_loop

ok_msg:
    li a0, 1
    la a1, msg
    li a2, 13
    li a7, 1
    ecall

    li a0, -1048576
    li a7, 10
    ecall
    bltz a0, fail3

    li a0, 0
    li a7, 0
    ecall

fail1:
    li a0, 1
    li a7, 0
    ecall
fail2:
    li a0, 2
    li a7, 0
    ecall
fail3:
    li a0, 3
    li a7, 0
    ecall

.balign 4096
_data:
msg:
    .ascii "hog: sbrk ok\n"
