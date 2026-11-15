_start:
    li s0, 0
    li s1, 5
loop:
    bge s0, s1, done
    la t0, digit
    addi t1, s0, 48
    sb t1, 0(t0)
    li a0, 1
    la a1, msg
    li a2, 7
    li a7, 1
    ecall
    li t0, 120000
busy:
    addi t0, t0, -1
    bnez t0, busy
    addi s0, s0, 1
    j loop
done:
    li a0, 9
    li a7, 0
    ecall

.balign 4096
_data:
msg:
    .ascii "pong "
digit:
    .ascii "0\n"
