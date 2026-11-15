# M-mode: UART RX interrupt via PLIC context 0, echo uppercased, stop at newline.
_start:
    la t0, trap
    csrw mtvec, t0
    li t0, 0x0C000000
    li t1, 1
    sw t1, 40(t0)
    li t0, 0x0C002000
    li t1, 1 << 10
    sw t1, 0(t0)
    li t0, 0x0C200000
    sw zero, 0(t0)
    li t0, 0x10000000
    li t1, 1
    sb t1, 1(t0)
    li t1, 1 << 11
    csrs mie, t1
    csrsi mstatus, 8
idle:
    wfi
    j idle
trap:
    csrr t0, mcause
    li t1, 1
    slli t1, t1, 63
    addi t1, t1, 11
    bne t0, t1, fail2
    li t0, 0x0C200004
    lw s1, 0(t0)
    li t5, 10
    bne s1, t5, fail1
    li t2, 0x10000000
rx: lbu t3, 5(t2)
    andi t3, t3, 1
    beqz t3, done
    lbu t4, 0(t2)
    li t5, 'a'
    blt t4, t5, put
    li t5, 'z'
    bgt t4, t5, put
    addi t4, t4, -32
put:
    sb t4, 0(t2)
    li t5, 10
    beq t4, t5, fin
    j rx
done:
    sw s1, 0(t0)
    mret
fin: PASS
fail1: FAIL 1
fail2: FAIL 2
