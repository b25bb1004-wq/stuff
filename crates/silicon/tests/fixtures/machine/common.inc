.option norvc
.option arch, +m
.macro PASS
    li t0, 0x100000
    li t1, 0x5555
    sw t1, 0(t0)
99: j 99b
.endm
.macro FAIL code
    li t0, 0x100000
    li t1, (\code << 16) | 0x3333
    sw t1, 0(t0)
98: j 98b
.endm
