# M-mode delegates SEI to S-mode and drops to S; S-mode takes key interrupts (PLIC source 11,
# context 1, priority 2 over threshold 1), sends each key's low byte to the UART, stops at ESC.
_start:
    la t0, mtrap
    csrw mtvec, t0
    li t0, 1 << 9
    csrw mideleg, t0
    li t0, 0x0C000000
    li t1, 2
    sw t1, 44(t0)
    li t0, 0x0C002080
    li t1, 1 << 11
    sw t1, 0(t0)
    li t0, 0x0C201000
    li t1, 1
    sw t1, 0(t0)
    li t0, 0x10002000
    li t1, 1
    sw t1, 8(t0)
    li t0, 3 << 11
    csrc mstatus, t0
    li t0, 1 << 11
    csrs mstatus, t0
    la t0, s_entry
    csrw mepc, t0
    mret
s_entry:
    la t0, strap
    csrw stvec, t0
    li t0, 1 << 9
    csrs sie, t0
    csrsi sstatus, 2
1:  wfi
    j 1b
strap:
    csrr t0, scause
    li t1, 1
    slli t1, t1, 63
    addi t1, t1, 9
    bne t0, t1, fail12
    li s1, 0x0C201004
    lw s2, 0(s1)
    li t1, 11
    bne s2, t1, fail13
    li s3, 0x10002000
    li s4, 0x10000000
2:  lw t1, 0(s3)
    beqz t1, 3f
    lw t2, 4(s3)
    sb t2, 0(s4)
    li t3, 0x1B
    beq t2, t3, fin
    j 2b
3:  sw s2, 0(s1)
    sret
fin: PASS
mtrap: FAIL 14
fail12: FAIL 12
fail13: FAIL 13
