# A extension semantics: W results sign-extended, signed vs unsigned min/max, .w leaves the
# upper word alone, D forms, LR/SC success and failure, SC to another address fails and clears
# the reservation, misaligned AMO/SC -> cause 6 and LR -> cause 4 with mtval = the address.
.option arch, +a, +m
_start:
    la t0, trap
    csrw mtvec, t0
    li s0, 0x80100000
    li t1, 0x7fffffff
    sw t1, 0(s0)
    li t2, 1
    amoadd.w a0, t2, (s0)
    li t3, 0x7fffffff
    bne a0, t3, fail_1
    lw a1, 0(s0)
    li t3, 0xffffffff80000000
    bne a1, t3, fail_1
    amoadd.w a0, zero, (s0)
    bne a0, t3, fail_1
    li t2, 1
    amomin.w a0, t2, (s0)
    lw a1, 0(s0)
    bne a1, t3, fail_2
    amominu.w a0, t2, (s0)
    lw a1, 0(s0)
    li t4, 1
    bne a1, t4, fail_2
    li t2, -5
    amomax.w a0, t2, (s0)
    lw a1, 0(s0)
    bne a1, t4, fail_2
    amomaxu.w a0, t2, (s0)
    lw a1, 0(s0)
    bne a1, t2, fail_2
    li t1, 0x0123456789abcdef
    sd t1, 8(s0)
    addi s1, s0, 8
    li t2, 0xffffffff
    amoxor.w a0, t2, (s1)
    li t3, 0xffffffff89abcdef
    bne a0, t3, fail_3
    ld a1, 8(s0)
    li t3, 0x0123456776543210
    bne a1, t3, fail_3
    li t1, -1
    sd t1, 16(s0)
    addi s1, s0, 16
    li t2, 2
    amoadd.d a0, t2, (s1)
    li t3, -1
    bne a0, t3, fail_4
    ld a1, 0(s1)
    li t3, 1
    bne a1, t3, fail_4
    li t2, 0x8000000000000000
    amomin.d a0, t2, (s1)
    ld a1, 0(s1)
    bne a1, t2, fail_4
    amomaxu.d a0, zero, (s1)
    ld a1, 0(s1)
    bne a1, t2, fail_4
    amoswap.d a0, zero, (s1)
    bne a0, t2, fail_4
    ld a1, 0(s1)
    bnez a1, fail_4
    li t2, 0xf0
    amoor.d a0, t2, (s1)
    li t2, 0x3c
    amoand.d a0, t2, (s1)
    ld a1, 0(s1)
    li t3, 0x30
    bne a1, t3, fail_4
    addi s1, s0, 24
    li t1, 10
    sd t1, 0(s1)
    lr.d a0, (s1)
    addi a0, a0, 1
    sc.d a1, a0, (s1)
    bnez a1, fail_5
    ld a2, 0(s1)
    li t3, 11
    bne a2, t3, fail_5
    sc.d a1, a0, (s1)
    beqz a1, fail_6
    sd zero, 0(s1)
    li t1, 11
    sd t1, 0(s1)
    lr.w a0, (s1)
    addi s2, s0, 32
    sc.w a1, a0, (s2)
    beqz a1, fail_7
    sc.w a1, a0, (s1)
    beqz a1, fail_7
    li t1, 0x80000000
    sw t1, 0(s2)
    lr.w a0, (s2)
    li t3, 0xffffffff80000000
    bne a0, t3, fail_8
    sc.w a1, zero, (s2)
    bnez a1, fail_8
    lw a2, 0(s2)
    bnez a2, fail_8
    li s3, 0
    addi s4, s0, 2
    amoadd.w a0, t1, (s4)
    li t3, 1
    bne s3, t3, fail_9
    li t3, 6
    bne s5, t3, fail_9
    bne s6, s4, fail_9
    addi s4, s0, 4
    amoadd.d a0, t1, (s4)
    li t3, 2
    bne s3, t3, fail_10
    li t3, 6
    bne s5, t3, fail_10
    bne s6, s4, fail_10
    addi s4, s0, 1
    lr.w a0, (s4)
    li t3, 3
    bne s3, t3, fail_11
    li t3, 4
    bne s5, t3, fail_11
    bne s6, s4, fail_11
    addi s4, s0, 2
    sc.w a0, t1, (s4)
    li t3, 4
    bne s3, t3, fail_12
    li t3, 6
    bne s5, t3, fail_12
    PASS
trap:
    csrr s5, mcause
    csrr s6, mtval
    addi s3, s3, 1
    csrr t5, mepc
    addi t5, t5, 4
    csrw mepc, t5
    mret
fail_1: FAIL 1
fail_2: FAIL 2
fail_3: FAIL 3
fail_4: FAIL 4
fail_5: FAIL 5
fail_6: FAIL 6
fail_7: FAIL 7
fail_8: FAIL 8
fail_9: FAIL 9
fail_10: FAIL 10
fail_11: FAIL 11
fail_12: FAIL 12
