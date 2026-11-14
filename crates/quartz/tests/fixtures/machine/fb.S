# Framebuffer: pixel (x, y) = R x, G y, B x^y, A 255, using the size registers.
_start:
    li t0, 0x10003000
    lw s0, 0(t0)
    lw s1, 4(t0)
    li s2, 0x30000000
    li t1, 0
1:  bge t1, s1, 4f
    li t2, 0
2:  bge t2, s0, 3f
    xor t3, t1, t2
    andi t3, t3, 0xff
    slli t3, t3, 16
    andi t4, t1, 0xff
    slli t4, t4, 8
    or t3, t3, t4
    andi t4, t2, 0xff
    or t3, t3, t4
    li t4, 0xff
    slli t4, t4, 24
    or t3, t3, t4
    mul t4, t1, s0
    add t4, t4, t2
    slli t4, t4, 2
    add t4, t4, s2
    sw t3, 0(t4)
    addi t2, t2, 1
    j 2b
3:  addi t1, t1, 1
    j 1b
4:  PASS
