# Five machine timer interrupts, 500 ticks apart; mtime must have advanced at least 2500.
_start:
    la t0, trap
    csrw mtvec, t0
    li s0, 0
    li s2, 0x02000000
    li t1, 0x4000
    add s3, s2, t1
    li t1, 0xBFF8
    add s4, s2, t1
    ld t0, 0(s4)
    addi t0, t0, 500
    sd t0, 0(s3)
    li t1, 1 << 7
    csrs mie, t1
    csrsi mstatus, 8
idle:
    wfi
    li t1, 5
    blt s0, t1, idle
    ld t0, 0(s4)
    li t1, 2500
    bltu t0, t1, fail2
    csrci mstatus, 8
    mv a0, s0
    addi a0, a0, '0'
    li t0, 0x10000000
    sb a0, 0(t0)
    PASS
trap:
    csrr t0, mcause
    li t1, 1
    slli t1, t1, 63
    addi t1, t1, 7
    bne t0, t1, fail3
    addi s0, s0, 1
    ld t0, 0(s3)
    addi t0, t0, 500
    sd t0, 0(s3)
    mret
fail2: FAIL 2
fail3: FAIL 3
