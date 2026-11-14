_start:
    la a0, msg
    li t0, 0x10000000
1:  lbu t1, 0(a0)
    beqz t1, 3f
2:  lbu t2, 5(t0)
    andi t2, t2, 0x20
    beqz t2, 2b
    sb t1, 0(t0)
    addi a0, a0, 1
    j 1b
3:  PASS
msg: .asciz "Hello from quartz\n"
