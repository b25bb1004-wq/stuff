# Compressed execution: arithmetic, a 4-byte instruction at 2 mod 4, c.jalr link = pc+2,
# compressed branches, sp loads/stores, addi4spn/addi16sp, lui/srai/subw/addiw, a trap from
# c.ebreak (mepc = its address), a jump to a 2 mod 4 target, c.jr, misa has A and C, mepc clears only bit 0.
.option rvc
_start:
    la t0, trap
    csrw mtvec, t0
    li sp, 0x80100000
    c.li a0, 5
    c.addi a0, -3
    c.slli a0, 4
    c.mv a1, a0
    c.add a1, a0
    li t1, 64
    bne a1, t1, fail_1
    .balign 4
    c.nop
    addi a2, zero, 7
    c.addi a2, 1
    li t1, 8
    bne a2, t1, fail_2
    la t2, tgt
1:  c.jalr t2
    j fail_3
tgt:
    la t1, 1b
    addi t1, t1, 2
    bne ra, t1, fail_3
    c.li s0, 0
    c.beqz s0, 2f
    j fail_4
2:  c.bnez s0, 3f
    c.j 4f
3:  j fail_4
4:  c.li s1, 1
    c.bnez s1, 5f
    j fail_4
5:  li t1, 0x1122334455667788
    c.sdsp t1, 8(sp)
    c.ldsp a3, 8(sp)
    bne a3, t1, fail_5
    c.lwsp a4, 8(sp)
    li t1, 0x55667788
    bne a4, t1, fail_5
    li t1, -1
    c.swsp t1, 12(sp)
    c.ldsp a3, 8(sp)
    li t1, 0xffffffff55667788
    bne a3, t1, fail_5
    c.addi4spn a5, sp, 16
    addi t1, sp, 16
    bne a5, t1, fail_6
    mv s1, sp
    c.addi16sp sp, -32
    addi t1, s1, -32
    bne sp, t1, fail_6
    c.addi16sp sp, 32
    bne sp, s1, fail_6
    c.lui a0, 0xfffff
    li t1, -4096
    bne a0, t1, fail_7
    c.srai a0, 4
    li t1, -256
    bne a0, t1, fail_7
    li a1, 0x7fffffff
    li a2, -1
    c.subw a1, a2
    li t1, 0xffffffff80000000
    bne a1, t1, fail_8
    li a0, 0x7fffffff
    c.addiw a0, 1
    bne a0, t1, fail_8
    li s2, 0
    la s3, eb
eb: c.ebreak
    li t1, 1
    bne s2, t1, fail_9
    la t2, odd2
    jalr zero, 0(t2)
    j fail_10
    .balign 4
    c.nop
odd2:
    andi t1, t2, 3
    li t3, 2
    bne t1, t3, fail_10
    la t2, jr_ok
    c.jr t2
    j fail_11
jr_ok:
    csrr t1, misa
    andi t1, t1, 5
    li t3, 5
    bne t1, t3, fail_12
    li t1, 0x80000007
    csrw mepc, t1
    csrr t2, mepc
    li t3, 0x80000006
    bne t2, t3, fail_14
    PASS
trap:
    csrr t5, mcause
    li t6, 3
    bne t5, t6, fail_13
    csrr t5, mepc
    bne t5, s3, fail_13
    addi t5, t5, 2
    csrw mepc, t5
    li s2, 1
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
fail_13: FAIL 13
fail_14: FAIL 14
