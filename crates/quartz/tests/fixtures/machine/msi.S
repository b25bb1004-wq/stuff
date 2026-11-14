# Machine software interrupt through CLINT msip.
_start:
    la t0, trap
    csrw mtvec, t0
    li s0, 0
    li t1, 1 << 3
    csrs mie, t1
    csrsi mstatus, 8
    li t0, 0x02000000
    li t1, 1
    sw t1, 0(t0)
    li t2, 100
1:  bnez s0, 2f
    addi t2, t2, -1
    bnez t2, 1b
    FAIL 4
2:  li t0, 0x02000000
    lw t1, 0(t0)
    bnez t1, fail5
    PASS
trap:
    csrr t0, mcause
    li t1, 1
    slli t1, t1, 63
    addi t1, t1, 3
    bne t0, t1, fail6
    li t0, 0x02000000
    sw zero, 0(t0)
    li s0, 1
    mret
fail5: FAIL 5
fail6: FAIL 6
