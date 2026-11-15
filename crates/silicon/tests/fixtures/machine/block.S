# Block device: DMA read sectors 3-4 with completion IRQ (PLIC source 8), send their byte sum
# (4 bytes, little endian) to the UART, write them back +1 to sectors 9-10, then an
# out-of-range read must report status 1 and leave RAM alone.
.equ BLK, 0x10001000
.equ BUF, 0x80100000
_start:
    la t0, trap
    csrw mtvec, t0
    li s0, 0
    li t0, 0x0C000000
    li t1, 1
    sw t1, 32(t0)
    li t0, 0x0C002000
    li t1, 1 << 8
    sw t1, 0(t0)
    li t1, 1 << 11
    csrs mie, t1
    csrsi mstatus, 8
    li s5, BLK
    ld t1, 0x28(s5)
    li t2, 16
    bne t1, t2, fail6
    li t1, 1
    sw t1, 0x1C(s5)
    li t1, 3
    sd t1, 0(s5)
    li t1, BUF
    sd t1, 8(s5)
    li t1, 2
    sw t1, 0x10(s5)
    li t1, 1
    sw t1, 0x14(s5)
1:  beqz s0, 2f
    j 3f
2:  wfi
    j 1b
3:  lw t1, 0x18(s5)
    bnez t1, fail7
    li t2, BUF
    li t3, 1024
    li a0, 0
4:  lbu t4, 0(t2)
    add a0, a0, t4
    addi t4, t4, 1
    sb t4, 0(t2)
    addi t2, t2, 1
    addi t3, t3, -1
    bnez t3, 4b
    li t0, 0x10000000
    li t3, 4
5:  sb a0, 0(t0)
    srli a0, a0, 8
    addi t3, t3, -1
    bnez t3, 5b
    li s0, 0
    li t1, 9
    sd t1, 0(s5)
    li t1, 2
    sw t1, 0x14(s5)
6:  beqz s0, 7f
    j 8f
7:  wfi
    j 6b
8:  lw t1, 0x18(s5)
    bnez t1, fail8
    li t2, BUF
    sb zero, 0(t2)
    li t1, 15
    sd t1, 0(s5)
    li t1, 1
    sw t1, 0x14(s5)
    lw t1, 0x18(s5)
    li t2, 1
    bne t1, t2, fail9
    li t2, BUF
    lbu t1, 0(t2)
    bnez t1, fail10
    PASS
trap:
    li t0, 0x0C200004
    lw t1, 0(t0)
    li t2, 8
    bne t1, t2, fail11
    li t3, BLK
    sw zero, 0x20(t3)
    sw t1, 0(t0)
    li s0, 1
    mret
fail6: FAIL 6
fail7: FAIL 7
fail8: FAIL 8
fail9: FAIL 9
fail10: FAIL 10
fail11: FAIL 11
