# chisel corpus: every word must equal LLVM 19 (rustc), base 0x80000000

.equ K0, 12
.equ K1, K0 * 3 + 1
.set K2, -7
.equ K3, 0x7fffe000
sw sp, -2048(t6)
bgt t3, x8, L107
.option rvc
c.jr x27
c.andi a0, -10
.option norvc
jr ra
addi a4, t3, -719  # trailing comment
L5:
.option rvc
c.srai a4, 20
c.bnez s1, L11
c.slli t2, 30
.option norvc
jal x18, L867
7: bgez s1, L5
.option push
.option rvc
c.addi t0, -9
c.ldsp a6, 264(sp)
c.addi s2, 10
.option pop
snez a1, s7
lw t5, (s5)
L11: bltz x28, L191
addi s5, a6, -2048  # trailing comment
beqz s8, L805
jalr x28
mret
srli x18, gp, 7
6: mulw s0, t5, t2
lr.w a6, (gp)
9: sc.w x9, t6, (gp)
slli a7, x2, 27
nop
addi t6, x9, 892  # trailing comment
addw x7, a6, x31
sh t5, 2047(x19)
auipc x5, 340739
.option push
.option rvc
c.beqz a0, 7b
c.bnez a3, L11
c.j L290
.option pop
csrrc s1, mtvec, t1
L28: sltiu a2, a0, 2047
ecall
divuw s6, zero, s2
rdtime t1
L32: addiw t6, tp, 0x56c
auipc s7, 335268
L34: xori s0, a0, -1131
ori s6, s5, -2048
mv x19, x10
lui gp, %hi(K0)
bge a5, tp, L381
sltiu t5, s5, -1228
li tp, -88854528
sc.d.rl x24, t3, (a0)
bgt x12, t0, L416
xori t2, t1, 18
5: lw t1, 2047(s4)
.option rvc
c.j L370
c.sd s1, 176(a5)
c.jr s5
.option norvc
tail L1467
bleu x7, zero, L624
beq a2, s11, L74
addi tp, sp, %lo(0x12345678)
xori tp, x19, -1285
.option rvc
c.ld s1, 104(a5)
.option norvc
.option rvc
c.sw s0, 20(s0)
.option norvc
addi x14, s6, 0  # trailing comment
L54: csrrs t4, cycle, zero
andi tp, t3, ~0xf
addi s0, t5, -497  # trailing comment
bgtu a0, s9, L357
L58: .Lpc1: auipc t3, %pcrel_hi(L1576)
addi t3, t3, %pcrel_lo(.Lpc1)
csrr a0, time
addi s7, t2, 0  # trailing comment
slti a2, s5, 0x7ff
L62: addi x24, x23, -2048  # trailing comment
jr x2
bgtz t3, 1f
L65: ble s2, sp, L58
divuw s9, a5, s11
jal s1, L970
beqz x20, 3f
sraiw x14, x2, 20
slliw s9, s1, 18
call L1065
addi s5, s7, -1761  # trailing comment
not x13, s3
L74: auipc s5, 0xfffff
L75: sraiw s8, x14, 15
lbu x3, 0(zero)
L77: addi x3, x10, %lo(0x12345678)
negw x13, s7
mulh x4, a3, s8
amoxor.w.aqrl s1, x9, (tp)
sub ra, tp, zero
auipc s3, 468552
ble s5, a4, L516
sc.d.aqrl t5, a6, (a3)
call L2012
mulhsu gp, gp, s10
L87: addi s7, s10, 0  # trailing comment
j L1122
andi t3, x9, 0
sltu s10, x29, t6
addi s6, ra, K2
jal L433
.word L1932 - L882
.balign 8
li s10, (1 << 9) | 5
L95: lbu s3, -1994(s0)
addi a3, ra, %lo(0x12345678)
1: addi s7, x9, 0x7ff  # trailing comment
rem x9, s5, s4
srliw x7, x2, 10
lui s8, %hi(K2)
addi a7, x17, -2048  # trailing comment
rdcycle a0
jal x15, L5
addiw s4, t2, 0x7ff
beq a5, s3, L282
csrrc s1, mscratch, s6
L107: li t0, 2147483647
L108: auipc x5, 0x77a64
beq s1, a4, 5f
L110:
.option rvc
c.bnez a1, L54
c.addi4spn a3, sp, 764
.option norvc
sret
amoswap.w t0, a5, (x24)
sc.d.rl s11, s3, (s5)
lhu s0, 2047(x3)
jalr a6, 0(x8)
auipc t6, 0
srai x7, x18, 20
L118: lui x2, 1048575
5:
jal s3, 6f
L120: lr.w a1, (s4)
.option rvc
c.bnez s0, L65
c.srli a0, 50
.option norvc
sw t5, 0(x9)
jal L182
L124: fence
addi x5, tp, 0  # trailing comment
j L11
.byte 1, 255, -1, 0x4
.balign 8
lr.w.aqrl t1, (t3)
seqz a0, zero
andi x20, t6, 2047
srl s9, a3, x19
lui x18, 587416
sd s9, -2048(x0)
.zero 8
mul t1, s10, a3
wfi
rdinstret s2
bgez s7, L411
ble t2, t0, L200
bleu a1, s5, L433
2: csrw scause, t5
addi s0, t3, 1669  # trailing comment
addi x10, gp, 55  # trailing comment
L144:
andi s2, s5, ~0xf
ble s6, gp, L1006
ble a1, a7, L58
sc.d t6, s8, (t3)
remuw t2, s9, x4
.option rvc
c.bnez a1, L110
c.ld a0, 152(s0)
c.ebreak
.option norvc
addi s5, a4, 0  # trailing comment
sraw x8, s3, zero
jal a6, L223
lr.w.aq t3, (a2)
sfence.vma t1
auipc s7, 1001751
bge s8, sp, 1f
xori x12, s10, -231
csrrs s9, scounteren, zero
call L1038
.option rvc
c.j L614
c.srli s0, 15
.option norvc
ble a6, s6, L649
call L992
ld t4, -2048(t5)
beq a7, t6, L405
csrrs t4, mhartid, zero
wfi
addi s4, x1, -2023  # trailing comment
L168: addi a6, a2, 1405  # trailing comment
sltiu t5, t1, 943
L170:
auipc a5, 350950
bgeu t2, a1, L894
addi s6, s2, -1938  # trailing comment
addi x21, t3, -2048  # trailing comment
bnez t6, L482
bge x1, a0, 6b
addi t3, zero, -1791  # trailing comment
L177: andi t3, x12, -1696
lw x14, -2048(x5)
li s8, -2049
sfence.vma
blt x25, ra, L770
L182: beq t3, zero, L303
.option push
.option rvc
c.lw s1, 84(a0)
c.ld a3, 16(s1)
c.lui t2, 1048569
.option pop
bleu x31, fp, L770
9:
addi s4, x1, 0x0  # trailing comment
2: lh s11, 0(t6)
L187:
sc.d x10, s3, (t5)
not tp, x23
andi tp, t3, -2048
nop
L191:
mulw s7, tp, s8
slti x26, t6, -1561
lwu x18, 23(t4)
fence
L195: li s7, 0x487
L196: ret
call L416
L198:
mul t5, a5, t3
j L1352
L200:
xori a1, s11, -1421
.byte 1, 255, -1, 0x12
amoand.w t5, t5, (t2)
bgez x9, L1020
addi ra, zero, 201  # trailing comment
addi sp, s5, 0x7ff  # trailing comment
bltu zero, t1, L584
sc.w.aqrl t2, s8, (x21)
3:
sfence.vma x2
ori s5, s4, -532
L210: bgt s1, x12, 6f
L211:
slti a5, t5, 0x471
jal a3, L797
7:
subw ra, gp, zero
sext.w x17, s9
L215:
addi x10, t3, 2047  # trailing comment
sraiw a7, ra, 25
divu t6, x6, a1
L218: sret
L219: slt a1, t3, s6
sret
sret
ecall
L223:
ecall
jalr x24, 2047(x19)
amoand.d.aqrl a1, x18, (t1)
bgt x3, t5, 2b
sltz gp, a1
bge a4, t0, L802
ecall
L230: addi s7, t0, 0x7ff  # trailing comment
addi s4, x4, -2048  # trailing comment
remuw a4, a7, s4
L233: subw t2, a3, x10
L234: sw t6, 0x3d8(x20)
.option rvc
c.sw s1, 24(a0)
c.addi16sp sp, 496
.option norvc
sc.w a2, gp, (t3)
addi s5, x1, 0  # trailing comment
lhu t5, -1830(x23)
sext.w a6, t4
ori x2, s11, -164
ori x3, x26, -2048
amoadd.w.aqrl x20, x1, (t3)
beqz s11, 3b
lhu x20, 0(x20)
bltz x8, 1f
bgeu a0, t2, 3f
L247:
sd a7, 0(s5)
L248:
bltz a6, L1017
jr s0
3: csrrw x15, misa, a4
addi sp, x23, -2048  # trailing comment
amoxor.d.aqrl x28, t5, (a5)
.option push
.option rvc
c.lwsp a7, 56(sp)
c.lui tp, 30
.option pop
L254: addi t4, s2, 0x21d  # trailing comment
rdtime t1
bleu x17, tp, 9f
jalr x31
.option rvc
c.addi16sp sp, 416
c.ldsp a3, 200(sp)
c.ld a4, 40(a4)
.option norvc
neg x10, t1
sc.w.aq x5, gp, (x29)
lui s10, 0
andi a6, a1, ~0xf
sc.w.aqrl x11, s1, (x20)
L264: lr.w a6, (x24)
L265:
andi x2, s11, ~0xf
.option rvc
c.ldsp s9, 72(sp)
.option norvc
L267:
li x13, 0
addi a6, s2, 0x4f2  # trailing comment
sw s2, 0(a2)
csrrs sp, medeleg, zero
addi x11, x20, -1989  # trailing comment
bge a0, t1, 7f
xori x13, a7, 0
sd t2, -2048(t1)
addi t0, t1, K2
addi zero, zero, K1
L277: lui tp, 1048575
mret
addi a2, s2, 2047  # trailing comment
lla s7, L746
li x15, 1388601344
L282: j L288
li a4, 1620
bgtu s11, a1, L459
li x23, -1225554442
L286: csrrc fp, 1984, x30
bleu s5, t3, 1b
L288: ori t6, a0, 561
blt s6, a7, L882
L290:
addi x2, s4, -2048  # trailing comment
slliw x31, x4, 1
csrs medeleg, zero
lhu t2, (t2)
lwu s9, 2047(s5)
.Lpc2: auipc a7, %pcrel_hi(L1773)
addi a7, a7, %pcrel_lo(.Lpc2)
bleu x25, x23, 7f
addi x20, a6, -119  # trailing comment
addi s2, s6, 74  # trailing comment
.Lpc3: auipc a4, %pcrel_hi(L1020)
lw a4, %pcrel_lo(.Lpc3)(a4)
jalr s1
jalr ra, 0(ra)
amomaxu.w t2, s6, (s11)
L303:
sb s8, 0(x0)
L304: blt x5, a2, 2b
bgtu t5, tp, L288
andi zero, s8, ~0xf
.asciz "hey"
.ascii "a\tb\n"
lr.w.aq a2, (s0)
slti x27, x27, -2048
remw t2, fp, x13
bne t2, tp, 5f
addi s5, x28, -2048
sc.d.aqrl x10, s6, (s7)
lbu s9, 0x7ff(a2)
sraiw s4, a4, 2
.byte 1, 255, -1, 0x4
L317: lr.d.rl a0, (a6)
sc.w ra, sp, (t3)
sb t1, 2047(s10)
li a7, 0x7a1
addi s1, t3, 578  # trailing comment
addi x11, s9, 0x0  # trailing comment
lr.d x11, (x8)
or s9, a5, x18
L325:
addi x26, s4, 0  # trailing comment
seqz x24, x22
sh s10, 2047(gp)
5: sc.w.rl a3, tp, (s8)
lla t3, L196
addi t6, t3, %lo(0x12345678)
mulhu zero, s3, x24
andi ra, s8, -2048
blez x20, L1063
jalr s3, -1651(x12)
addiw s8, a1, 0
bleu t6, a2, L585
.option push
.option rvc
c.j 3b
c.li sp, -32
c.srli a4, 14
.option pop
xori t5, t0, -1263
call L723
L340: divuw a0, x1, a2
tail L1886
2: li t3, 1743048704
tail L956
sraiw a4, s6, 12
auipc t0, 810624
addi gp, tp, 1671  # trailing comment
sc.d x23, s0, (x26)
.option rvc
c.and a5, s1
c.mv x27, a4
.option norvc
tail L2187
.option rvc
c.addiw sp, 10
c.addiw s10, -3
.option norvc
sltiu x22, a2, 1743
L352:
amoxor.w.aqrl a7, sp, (x31)
wfi
addi zero, a4, 0x0  # trailing comment
li a2, -710524157
divu s10, s6, s5
L357: addi x15, x28, 0x7ff  # trailing comment
addi fp, x3, -746  # trailing comment
8: slt a4, s8, a1
.option rvc
c.slli t1, 6
c.addi s5, 15
.option norvc
L361: sra s8, t1, s4
addi ra, t1, 0  # trailing comment
L363: or a0, s0, s8
csrrwi a5, stval, 18
snez t4, zero
sw t3, 2047(t6)
lui tp, 0x0
sltz sp, t4
sd a0, 0x122(t1)
L370:
sret
L371:
divuw x9, s4, t1
L372: csrw sepc, x29
and x0, t4, zero
sd x0, 1793(x4)
mulh s1, x8, s4
bnez s5, L234
csrrs s2, mimpid, zero
beqz ra, L522
addi s1, gp, -1724  # trailing comment
subw s3, x6, x9
L381: fence iow, i
bne x7, t3, L361
jr x29
6: addi s10, fp, 462  # trailing comment
j L325
addi sp, x3, 0
csrrwi a4, sie, 15
sltiu t1, s7, -2048
sc.w s8, x22, (s5)
.option rvc
c.nop
c.jr x15
c.addi4spn a1, sp, 1012
.option norvc
.option push
.option rvc
c.add s9, x6
c.sw s0, 116(a3)
.option pop
fence.i
li s4, 0x7fffffff
sw x4, 2047(t4)
L395: amoand.w.aqrl x24, x22, (s6)
.option push
.option rvc
c.add x17, a1
c.addi16sp sp, -80
c.srai s1, 59
.option pop
auipc s8, 0x0
li s2, -1465292702
ret
mulh x24, a4, t3
8: lr.d x21, (x16)
beq x7, t1, L1085
bleu a4, t3, L144
L404: bge s1, x19, 5f
L405: slliw x7, a0, 23
lh x16, (x2)
L407: sub x19, s9, t2
addi t0, s6, K0
csrw scounteren, x30
csrrwi s9, scause, 31
L411:
sfence.vma
ret
j 6b
bleu t2, t1, L74
.option rvc
c.jr s3
c.addiw t4, 23
.option norvc
L416: bnez s2, L1145
remuw s8, s10, a5
addi x6, sp, 0  # trailing comment
addi a4, s7, 2047  # trailing comment
srliw a6, x24, 9
amoxor.d.rl x27, t6, (a0)
lhu s3, -1235(s7)
csrrc t2, mcause, t2
.Lpc4: auipc x26, %pcrel_hi(L1694)
ld x26, %pcrel_lo(.Lpc4)(x26)
.option rvc
c.sw a5, 120(s1)
.option norvc
jalr t1, 0x7ff(x17)
7: ori a5, x15, 0
rem ra, s10, s11
lui a6, %hi(0x12345678)
sret
L431: addiw ra, t4, -2048
sc.w.aqrl t1, s5, (x0)
L433: slti a2, t5, 0x584
jalr ra, 0(x9)
lr.w.rl a1, (s11)
L436:
auipc gp, 0x0
bnez x4, 2f
li s3, 2047
addi s1, a3, -1032  # trailing comment
sd fp, -1105(x22)
blez a2, L1090
csrrc x24, mepc, s9
sb x18, 2047(s0)
auipc t5, 0
L445: li fp, -2006216704
ble sp, s3, L1056
slt sp, ra, a0
ori t2, s11, -1348
addi x23, s1, -2048  # trailing comment
j L1883
bne s11, t0, L11
L452:
andi a5, s10, -1722
sllw s2, s2, t5
5: addi a2, x23, K0
addi t5, x7, 550  # trailing comment
srai s5, t0, 39
or s3, a2, s5
bgtz t3, L694
L459: bge a1, x17, L802
lr.w t5, (s9)
.option push
.option rvc
c.addi16sp sp, 272
c.addiw t6, -24
.option pop
7: xori t6, s8, -1795
.option push
.option rvc
c.jalr tp
c.andi a3, 3
c.ldsp s8, 336(sp)
.option pop
addi x13, tp, 2047  # trailing comment
lui zero, 164294
L466: ld x0, 2047(t0)
bgtu s3, s6, L753
csrr s0, scause
sb x9, 0x7ff(t3)
lw x26, 2047(s4)
nop
3:
li ra, 0x413249ab
L473: addi s9, x16, (K1 - 3) * 2
ble s4, s4, L753
L475: li x13, 1672
negw a2, tp
sfence.vma a2
li x12, -2049
sllw a0, x24, x15
5: .Lpc5: auipc a2, %pcrel_hi(L526)
ld a2, %pcrel_lo(.Lpc5)(a2)
ret
L482:
remw a5, x30, t0
L483: ble t4, s0, 3f
addi s5, a3, 0  # trailing comment
.option push
.option rvc
c.addi4spn s1, sp, 128
c.bnez a1, L505
c.addi a4, 4
.option pop
.option rvc
c.j 2b
.option norvc
5: srlw t0, a6, a0
addi x30, x9, -1069  # trailing comment
ble zero, x27, L760
jalr t3, 0(ra)
.option rvc
c.add a4, a3
.option norvc
addi a1, t1, 631  # trailing comment
.option rvc
c.li x29, -17
c.lwsp t6, 140(sp)
.option norvc
amoand.d.aq t3, t3, (sp)
.zero 8
.balign 8
mulw t5, sp, x6
sc.d s0, x17, (a1)
bltu t2, x12, L1218
rdinstret s2
bltz x5, L1069
addi s3, s2, 0x6b7  # trailing comment
addi a3, x16, -2048  # trailing comment
auipc s11, 1048575
lhu t4, 2047(x18)
L505: mulhsu a1, a6, x13
jr x27
addi s0, a1, 0  # trailing comment
slti s9, gp, 0x7ff
li t1, 2014420992
5:
bgtz s11, L372
j 8f
srli s9, s4, 60
jalr ra, 0(a4)
mulhu t1, t6, t0
addi a6, fp, K0
L516: xor t2, a1, a3
L517: addi x2, t3, 0
bltu x17, t2, L352
li x26, -1183440896
srlw a6, s5, x23
sd x31, 0(x4)
L522: subw ra, s10, x2
ble t3, a6, L911
lui a7, %hi(K2)
L525: amoor.w.aqrl x29, x3, (sp)
L526: sb zero, 0(x28)
li t2, -117667879
lla s1, L1765
addi ra, x16, 2047  # trailing comment
.option rvc
c.ldsp t4, 264(sp)
c.mv s1, x17
.option norvc
csrrw gp, 832, s9
slti sp, ra, 0
tail L1183
jalr a5
addi t5, s6, 2047  # trailing comment
csrrs x11, medeleg, zero
sc.w a5, x1, (x9)
amoor.w.aq x5, a0, (s8)
addi s10, gp, 0x0  # trailing comment
lb a4, 0(s3)
L541: ret
.word 0x700867cb, -1, L77
srai s10, gp, 44
srai s5, s5, 41
sc.d s3, t6, (a0)
sfence.vma a3
.zero 8
.balign 8
.option rvc
c.ld a5, 248(s0)
.option norvc
or t3, s4, s6
addi a2, a2, (K1 - 3) * 2
addi t3, s7, 1480  # trailing comment
L552: addi s9, s7, 0  # trailing comment
L553: ld s10, -1945(x18)
auipc x21, 0
csrrwi a1, mtval, 12
li a1, 2048
srai fp, a4, 3
sc.w.rl t2, s8, (a6)
addiw a6, s9, 1268
.option rvc
c.srli a3, 60
c.jr a7
.option norvc
bltu sp, s11, L1343
L562: jal L906
addi x0, a1, 0x25  # trailing comment
addi x20, x23, -356  # trailing comment
L565: lr.d t4, (x2)
addi s10, s7, -1807  # trailing comment
bltu x10, x11, L198
csrrc s8, mcounteren, zero
lr.w.rl a1, (t0)
bgeu x30, a6, L1313
csrsi sscratch, 20
L572: jal L797
L573:
rdinstret s2
lui s10, 0x0
jal a6, L573
addi x1, s7, K1
ori t5, sp, 0
lui t1, 0
beqz a5, L505
L580: li gp, 0x7ffff7ff
addi s1, t6, -349  # trailing comment
bleu s1, tp, L639
.Lpc6: auipc ra, %pcrel_hi(L2177)
ld ra, %pcrel_lo(.Lpc6)(ra)
L584: addi x5, a4, -1486  # trailing comment
L585: divu zero, t0, a6
sub x16, ra, ra
amomaxu.w x22, s5, (a6)
bge a5, x14, L120
srliw x15, a7, 17
sltu t4, x4, a5
6: lr.d s2, (x9)
sltiu s8, s11, -184
addw x6, s0, x5
addi t0, ra, -1479  # trailing comment
bgt zero, t2, 8f
sc.w.rl t6, x24, (s7)
L597: auipc x9, 0x19936
lui x0, 0x78b0d
rdinstret s2
7: jalr ra, 0(s9)
lwu s9, 0(s11)
mv t4, s8
L603: sltiu s7, s6, 0x0
csrrwi t6, stvec, 20
addi x7, x6, -432  # trailing comment
bltu x5, ra, L974
srliw x17, s9, 17
lr.d t5, (a3)
add s7, s8, s3
.option rvc
c.j 2b
c.add a2, sp
.option norvc
ret
addi s6, x3, -2048  # trailing comment
andi s5, s4, ~0xf
L614: .Lpc7: auipc x7, %pcrel_hi(L282)
lw x7, %pcrel_lo(.Lpc7)(x7)
sret
addi x30, x28, 333  # trailing comment
sub x13, s1, t4
csrrw x28, mideleg, a1
sll tp, x26, s1
csrrc gp, mscratch, x30
srlw s5, gp, zero
rdcycle a0
slliw t6, zero, 9
L624: or s5, tp, x25
remuw s11, s0, s8
4: sret
.option rvc
c.ldsp t3, 160(sp)
c.slli s3, 21
c.jalr s9
.option norvc
and x27, x23, s2
lh x18, -2048(tp)
sltu t2, a7, s10
9: mret
bltz x8, 5f
sc.w.rl s1, a3, (a6)
L634: sd a4, 2047(x28)
2:
sext.w a5, tp
addi s8, x29, 0  # trailing comment
addiw s3, s1, 2047
wfi
L639:
addi s7, gp, -940  # trailing comment
slli tp, t1, 5
li x7, 404029440
slliw a6, x12, 7
sltu s8, x0, sp
.option rvc
c.mv t3, a4
c.sd a3, 208(a4)
c.slli x6, 57
.option norvc
ebreak
jal sp, L1343
csrr t0, marchid
bge s5, t1, L264
L649: bgt zero, x20, L211
.option rvc
c.add t0, a3
.option norvc
addi t5, a7, -1702  # trailing comment
jal L1871
sc.d s1, x6, (s4)
sc.d s2, s9, (t4)
L655:
bltu x5, s0, 8b
slti s8, a3, 0x7ff
bltu t1, x18, 3f
bnez s5, L760
.asciz "hey"
.ascii "a\tb\n"
blt s11, a6, L867
addi s1, ra, 0x0  # trailing comment
L662: sc.w x16, a7, (gp)
jalr a7
sc.d s6, s9, (s1)
divu s5, x13, t1
li gp, (1 << 9) | 5
sltiu x30, s5, -2048
srli s7, s6, 24
jalr s10, 0x2cb(s2)
jr ra
sgtz x31, t0
bge s8, a5, 4f
andi x28, a0, ~0xf
lwu t4, -1071(a7)
lr.d.aqrl s8, (s9)
sh x21, 0x7ff(x27)
slti s1, x21, -1622
jal s0, L325
addi x6, t3, 0x7ff  # trailing comment
csrrs s6, sepc, zero
ret
li x11, -365961216
lr.w s3, (s1)
jalr x18, 0(a5)
addi x30, t5, -2048  # trailing comment
ret
remw a0, a3, t0
lr.d.aqrl gp, (s3)
sb a4, 0(tp)
slt x8, t3, a0
lhu a4, 0x0(a5)
auipc s3, 0
seqz s9, s11
L694: bltu x18, s11, L1414
snez s9, a7
bge s11, a2, L1343
bgeu x26, x22, L416
csrc misa, s6
neg x12, a4
sraiw x14, a1, 13
addi x29, a0, 0x0  # trailing comment
sc.w.aq x12, s0, (a2)
L703: csrw scause, ra
8:
addi a6, zero, -641  # trailing comment
srliw s7, s2, 17
amomax.d t1, fp, (x22)
bgez s2, 7b
.option push
.option rvc
c.addi16sp sp, 384
c.addiw t1, 17
.option pop
addi x22, s0, -2048  # trailing comment
sret
beqz s6, 9f
csrrw s10, 1984, s6
.Lpc8: auipc s3, %pcrel_hi(L1722)
addi s3, s3, %pcrel_lo(.Lpc8)
lui a6, %hi(K3)
slti tp, a1, 0x601
sb s3, -2048(x2)
sltu t3, s11, x2
mv t3, s10
8:
ble x16, a2, 4f
bltu x31, a4, 3f
sh t4, -275(s11)
lr.w.aqrl t1, (t1)
L723:
xor x28, x6, t6
sc.d x17, x22, (a7)
lwu t1, 0(x14)
jalr ra, 0(a3)
beqz a5, 4f
addiw x25, a5, -2048
addi t1, x27, 0  # trailing comment
addi zero, s6, -1904  # trailing comment
sw a2, -2048(a7)
6: addi a6, a3, -2048  # trailing comment
bgeu t0, t0, L1307
.option push
.option rvc
c.beqz a2, L745
c.andi a0, 6
.option pop
or s4, gp, x8
blt t1, x2, L267
addi a2, tp, K0
xor a6, s8, t6
lw t3, -2048(x21)
beqz x16, L1270
li a7, 0x744
5: .asciz "hey"
.ascii "a\tb\n"
sb a2, 0x7ff(s10)
.zero 8
L745: lh s6, 0x7ff(x22)
L746: auipc x25, 905585
sc.d.aq s9, t6, (s6)
.dword L1411, 0x482fa62d762f22ff
7: mulhsu t3, x23, s11
bgeu x27, tp, L118
call L553
addi x25, s4, 0
L753: fence
li s9, 2048
L755:
.option push
.option rvc
c.andi s0, -22
c.add t3, t1
.option pop
csrrw a6, sstatus, a0
csrr t1, satp
csrw mepc, s5
.option rvc
c.j L1017
.option norvc
L760: addi x23, x7, %lo(0x12345678)
csrrc x17, 1984, s9
sc.w.aqrl s6, a7, (s2)
sltu t4, t6, x28
slti t6, x30, 2047
sraiw s9, t5, 1
csrrw zero, mepc, s7
sgtz t3, x20
fence.i
bne s2, x22, L1430
L770:
.option push
.option rvc
c.addi t0, -29
c.lui s9, 1048551
c.lw s0, 60(a2)
.option pop
addi s2, s8, 0x0  # trailing comment
bgeu s6, a1, 4b
subw s2, a7, a4
mulh s5, s2, x15
sltu t4, s10, t3
li x16, 0x1
sb zero, 692(x26)
remu x18, x26, x27
ori s7, a4, -1202
mulw a2, t6, sp
lhu a4, (a5)
fence.i
auipc gp, 0
li s0, -587
.option rvc
c.bnez s1, L810
c.beqz a3, L753
.option norvc
.word L1692, L1053, 7
sltiu a4, x3, 0x0
li x13, (1 << 9) | 5
addi t2, s4, 0x0  # trailing comment
srlw t6, x15, x11
remu a1, x11, s7
bne t1, x10, 4b
.asciz "hey"
.ascii "a\tb\n"
sraiw x1, x24, 3
li sp, 2147479552
L796: .asciz "hey"
.ascii "a\tb\n"
L797:
ld s4, -1509(t4)
bltu x18, t2, 1b
addi x10, x2, K2
addi a3, x7, %lo(0x12345678)
and x21, x28, x28
L802:
lwu t4, -1924(t6)
3: lr.d.aq x18, (s3)
auipc gp, 0xbda65
L805: addi s6, a2, 0x7ff  # trailing comment
addi a1, x9, -472  # trailing comment
jr s10
ebreak
amominu.d a1, x28, (x20)
L810: addi s6, gp, 1757  # trailing comment
addi x0, x31, 0  # trailing comment
ret
mulhu x6, a6, s3
bleu s5, a1, L1017
fence ir, i
csrr x24, scounteren
lh t4, (x12)
srl s10, a1, a6
lhu s5, (a2)
tail L1274
addi x0, tp, 0x0  # trailing comment
lbu tp, (a3)
amominu.w x18, s3, (s7)
sltiu x5, s1, -1832
snez a6, s8
beq a6, a7, L168
addi t5, x19, 0  # trailing comment
L828: addi tp, sp, 0  # trailing comment
8: or a5, s11, sp
.asciz "hey"
.ascii "a\tb\n"
lla x23, L340
bleu s8, a7, L1182
bltu s8, a6, L1765
lr.w s11, (a0)
mulhu x6, sp, t0
amoswap.w.aq a2, x8, (s3)
negw x14, s6
csrrc t5, misa, tp
bne a7, t5, L1444
sret
bgt a1, t4, L1063
addi zero, s11, -2048  # trailing comment
.option push
.option rvc
c.lui gp, 22
c.xor a1, a1
c.beqz a5, L802
.option pop
li t0, -1602949304
addi x17, t4, -440  # trailing comment
amoadd.d s8, x28, (x17)
jal L755
3: addi x20, a7, 0  # trailing comment
sh a2, 2047(x20)
ble x12, s10, L927
L851: csrrc s3, sstatus, fp
wfi
addi t2, a2, 0x535  # trailing comment
xor x3, sp, x9
addi ra, a0, 2047  # trailing comment
sd s10, 2047(gp)
slliw ra, gp, 6
bltu s1, x22, L983
5: sfence.vma x10, x23
li t3, -2049
L861: csrrwi a2, misa, 0
mret
sc.w x14, s6, (x1)
srliw x8, t3, 30
lw x24, (a2)
L866:
jalr x8, -1762(s3)
L867: addi s3, x5, 0x2a2  # trailing comment
addi t6, s10, 0  # trailing comment
sltu x6, t5, t5
addi x4, x28, -87  # trailing comment
sltu x5, t6, t1
sll s4, a7, ra
jal t3, L2012
mv s4, a4
sfence.vma
jr x10
addiw t4, s1, 0x0
srlw s5, x30, t1
bltz x26, 7b
lui x22, 0x0
fence w, w
L882: amoxor.d.aqrl t4, ra, (x14)
csrsi satp, 4
lh s3, 2047(fp)
sd x4, -2048(t6)
ori s4, s10, 0
srai s11, t4, 6
sh s9, 939(t2)
csrrw a4, mideleg, s10
.option push
.option rvc
c.bnez s1, 3f
c.addi16sp sp, 176
c.slli x7, 18
.option pop
lui a7, 0xee95d
.option rvc
c.or a2, a3
.option norvc
bgtz a4, L797
L894: amoadd.w.rl s2, s4, (tp)
addi fp, s6, -2048  # trailing comment
mul t5, s8, x30
subw s4, t3, x19
ld x22, 2047(ra)
slliw t6, x13, 24
sub tp, t6, x20
tail L2185
L902: bgt x23, a5, 8f
lui x24, 0x0
li x16, (1 << 9) | 5
lui a1, 0x555d6
L906: lui t4, 1048575
neg s8, s0
sh sp, -2048(s9)
snez x23, a6
L910: bgt x26, s6, L1254
L911: csrrw x20, mtvec, s11
sb s2, -2048(tp)
addi x9, s7, -1120  # trailing comment
lh a3, -2048(s6)
sret
csrrc a1, 2496, x26
sc.w.aq x19, t5, (s3)
amomin.d s11, x4, (a4)
addw a7, ra, s1
bgtu a6, a2, L1006
addi x30, x20, -2048  # trailing comment
mulhsu ra, t1, a7
addi x7, x10, -2048  # trailing comment
bleu s8, a6, L1159
lb t4, (x30)
ld a2, 0(x21)
L927:
jalr ra, 0(x13)
L928: .Lpc9: auipc x3, %pcrel_hi(L1182)
ld x3, %pcrel_lo(.Lpc9)(x3)
li x29, 0x726
addi t0, x2, 0x7ff  # trailing comment
addi x24, a5, 2047
bnez a1, L404
6: addi s9, t4, 0  # trailing comment
3: ld x1, -894(sp)
L935:
bltu s6, t2, L1342
lwu t0, 0(x12)
addi s5, x21, 0x3d  # trailing comment
addi x30, s7, -8  # trailing comment
li s3, 1778335744
blez x27, L1352
xori a7, s10, 0x103
lla s1, L1164
sb x16, 2047(x28)
bgeu a6, t3, L1727
li a2, (1 << 9) | 5
addi s4, t2, -2048  # trailing comment
L947: lr.w.aqrl s7, (a5)
addi a1, ra, -431  # trailing comment
blt t6, a1, L1257
addi t1, tp, 0  # trailing comment
L951: div s3, s4, x6
lr.w.aqrl zero, (t6)
addi x24, x14, -1982
jalr ra, 0(x16)
sc.w.aq a2, t5, (a3)
L956: lhu x8, 0(x15)
li tp, -1987010560
jal 1b
sd a1, 0x0(a1)
not sp, tp
slliw x5, s10, 0
.option rvc
c.srai a1, 53
.option norvc
sltiu t3, tp, 78
2: beq t1, fp, L124
.word L277 - L1547
amoor.w.aq a2, t0, (s10)
subw s4, s3, a3
.Lpc10: auipc s0, %pcrel_hi(L983)
ld s0, %pcrel_lo(.Lpc10)(s0)
lla s3, L1500
L970: .word L565 - L1060
amoxor.d.aqrl a6, t4, (t2)
snez x13, t5
amoor.d tp, a4, (t5)
L974:
.option push
.option rvc
c.sw a4, 100(a3)
.option pop
L975:
ori t5, gp, 0x65d
L976:
divuw t4, x3, t5
jalr s2, 1242(t6)
L978: auipc x20, 1048575
auipc a6, 0
bgtu x17, a7, L1694
addi t5, sp, 2047
slti sp, t2, 2047
L983:
addi a0, x30, -1414
.option rvc
c.nop
c.slli t6, 35
.option norvc
addi s9, s2, 0x7ff  # trailing comment
bgtu s11, t1, L1821
addi x1, t2, -2048  # trailing comment
bgt a0, t6, L110
lui x4, 0x0
remw x31, ra, a1
jalr ra, 0(x30)
L992: auipc a2, 0
lwu s8, 0x7ff(t1)
.option rvc
c.addi t3, 3
.option norvc
L995: slli s9, a5, 63
addi t4, t0, 0  # trailing comment
li s6, -728
L998:
sra a4, s1, x17
addi s9, x22, -2048  # trailing comment
addi a6, s5, 0  # trailing comment
auipc x3, 629035
lui s3, %hi(K1)
lr.w.rl t2, (x18)
li x22, (1 << 9) | 5
negw tp, a3
L1006:
addi x29, t1, -430  # trailing comment
blt x2, t3, L947
addi s1, s3, %lo(0x12345678)
li s1, (1 << 9) | 5
mulhsu s2, a2, s0
beq s6, s3, L1231
li a1, 0x55f77000
amominu.d t1, s5, (x8)
sw s5, -2048(zero)
jalr ra
not s5, s0
L1017:
sra s1, x13, t4
lb a4, 0(a4)
lw x22, -1062(t6)
L1020:
bltz a4, L1932
L1021:
.option push
.option rvc
c.srli a4, 58
.option pop
addi s0, t5, 0  # trailing comment
divw s0, tp, a5
li a5, 991
addi t0, t4, -1341  # trailing comment
sraw a1, x14, t2
addi a5, s8, -2048  # trailing comment
blt a2, x9, L1922
3:
mret
L1030:
.option rvc
c.addiw t1, 15
c.jr sp
.option norvc
addi a2, tp, 0x7ff  # trailing comment
L1032: lla a1, L219
addi t2, s10, -2048  # trailing comment
wfi
L1035: slti a1, s10, 2047
slti t2, sp, 0
j 1f
L1038: slliw t0, x10, 23
addw a5, s10, x11
lr.d.aqrl x4, (t5)
L1041: sraw s3, a6, ra
8:
.option rvc
c.srai s1, 7
c.nop
.option norvc
not s0, s0
.option rvc
c.add s1, x19
c.beqz a3, L1021
c.srli s1, 17
.option norvc
addi s2, a4, 2047  # trailing comment
addi a6, x21, -2048  # trailing comment
lui x6, %hi(K2)
addi a5, x23, 571  # trailing comment
L1049:
.option rvc
c.srli a3, 19
c.add a4, t4
.option norvc
lui s8, 4286
fence.i
lhu s4, 0x7ff(x8)
L1053: jr x16
bgtu tp, fp, L1152
csrrs x4, mstatus, zero
L1056:
.half -14696, 0xf22d
.balign 8
addi s2, s11, 983  # trailing comment
jal x13, L522
sltiu gp, x22, -290
L1060: beqz t0, L191
addi s1, fp, 2047  # trailing comment
csrr x14, medeleg
L1063: .Lpc11: auipc a4, %pcrel_hi(L1751)
ld a4, %pcrel_lo(.Lpc11)(a4)
blt s8, a5, L1063
L1065:
addi x18, x3, -2048  # trailing comment
L1066: addi x31, s7, -1993  # trailing comment
sll x21, a7, s0
bleu t0, s1, L522
L1069: auipc s7, 0
8:
lui a7, 0x37238
addi t5, x13, 480  # trailing comment
bleu x5, a3, L395
divw x15, x0, x1
L1074:
jal t6, L1651
mulh s11, x28, a6
lb x10, (a2)
addi t0, t1, K0
addi a3, a1, -1351  # trailing comment
blt s11, x30, L1804
addi a7, a3, K2
sltz ra, x19
.word L317 - L2020
.balign 8
bgez t5, L473
li t1, 708980736
L1085: li s7, -160
j L1790
.word L1183 - L1550
blez s7, L1355
1:
lr.d.rl gp, (a3)
L1090: .Lpc12: auipc s0, %pcrel_hi(L1563)
ld s0, %pcrel_lo(.Lpc12)(s0)
addi a3, s10, 1830  # trailing comment
L1092: ret
lr.d.aq x23, (s2)
bge s9, x23, L433
bltz a4, L796
.half 16602, 0x4b57
bleu sp, s8, 8b
lr.w.aqrl x12, (s9)
lla x17, L2043
li s3, 4095
lb x19, 0(s9)
sw t0, -2048(t5)
blt s4, t2, 2b
.asciz "hey"
.ascii "a\tb\n"
.balign 8
amoor.w.rl s7, s3, (x21)
sraiw t1, ra, 17
snez gp, t4
ble ra, a0, L947
.option rvc
c.add a2, ra
c.ebreak
.option norvc
sw x11, -2048(a7)
addi t2, s9, K2
.Lpc13: auipc s11, %pcrel_hi(L573)
lw s11, %pcrel_lo(.Lpc13)(s11)
.option push
.option rvc
c.jalr fp
c.srli a2, 22
.option pop
.asciz "hey"
.ascii "a\tb\n"
.balign 8
9: lr.d s6, (a3)
bgt s10, s0, L1133
sltiu a6, a4, -1833
bgt x2, x16, L1480
rdinstret s2
5:
lui x26, %hi(K2)
bne s4, s4, L1655
L1122: bgeu x26, s4, L1883
ret
addi s6, s7, 0x7ff  # trailing comment
lr.w s10, (ra)
li s2, 2048
ret
2: lr.d t1, (t0)
addi s7, x9, K1
li x1, 0x7ffff000
csrw mstatus, s9
.word L1765 - L662
.balign 8
L1133: sll tp, t3, t3
j 4b
j 8f
addi ra, s0, 2047  # trailing comment
bgez zero, 7f
wfi
srli t1, x3, 58
rem gp, sp, x30
tail L1041
sw s9, 0(tp)
sltiu s8, s0, 1770
fence orw, rw
L1145:
lw x31, -1078(t2)
addi zero, t0, -668  # trailing comment
auipc s3, 1048575
lui sp, 0x0
ori x26, s1, -1338
L1150: addi t2, t4, 0x4c8  # trailing comment
beq a2, s5, L1662
L1152: li t0, 1840871938
addi x20, s11, 0x44e  # trailing comment
sltiu a0, ra, 0
9: lui x8, %hi(0x12345678)
csrr zero, mhartid
divu x21, a3, s9
sret
L1159: sraw s6, s7, x13
amomax.w.aqrl s5, s4, (a2)
.option push
.option rvc
c.jr s7
.option pop
slti x11, t3, 0
.option rvc
c.sd a0, 232(a3)
c.nop
.option norvc
L1164: addi sp, gp, -2048  # trailing comment
L1165: ld s7, 568(sp)
L1166: addi s4, sp, %lo(0x12345678)
j L2184
sh s2, -2048(s6)
9:
addi s4, sp, -2048
amoor.d.aqrl s11, t0, (x3)
ori s10, s5, -2048
7:
lui a3, 218646
L1173: srai fp, x6, 7
1: bgez s4, 6f
sc.d.rl x0, x27, (ra)
xori x15, a3, 0
rdinstret s2
L1178: xori s0, x13, 1403
andi t1, s4, -1919
addiw t2, s7, -2048
1: sgtz x30, s6
L1182:
addi s2, t4, 618  # trailing comment
L1183: csrr s6, time
jalr sp
jalr s4, 0(x22)
addi x20, a0, 2047  # trailing comment
slti ra, x16, 0x377
bltz s9, L277
ori a1, a6, -2048
bltz s4, L805
bnez x19, L1035
4:
bnez sp, L1444
slti tp, a1, -2048
lwu s5, -509(s1)
sltiu ra, tp, 2047
bne s0, t4, 7b
sh a7, 2047(a2)
li s1, -1419062548
L1199: bgt x17, a4, L1492
amomaxu.w x30, t0, (x25)
.option rvc
c.xor a3, a2
c.nop
.option norvc
j L580
addi a5, x22, 222  # trailing comment
.option rvc
c.ebreak
c.addi4spn a5, sp, 28
c.addiw s0, -13
.option norvc
ecall
lui s0, %hi(K3)
lw zero, -52(s3)
beq ra, gp, L584
bgt x16, x20, L1347
addi s4, x31, 1418  # trailing comment
1: sd t0, 498(x28)
lw t3, (x13)
lui s11, 0x0
lr.w.aq x24, (t6)
ori t6, t2, 2047
L1216:
bge t1, t0, L1074
sc.d.aqrl s1, t4, (a2)
L1218:
.option rvc
c.li gp, 15
.option norvc
seqz x4, x9
lr.d x8, (a7)
L1221: lh s3, -1963(x28)
bgt zero, a5, L1390
jalr ra
L1224: addi a7, a3, 379  # trailing comment
.option push
.option rvc
c.srai a3, 50
.option pop
negw t4, s7
addi gp, sp, K2
bgeu a6, ra, L1444
L1229: beq s4, sp, 5f
.option rvc
c.srai s1, 30
.option norvc
L1231: lh sp, 2047(s9)
blez tp, L1049
li x24, -2048
.option push
.option rvc
c.addiw x6, -11
c.andi a4, -27
.option pop
.option rvc
c.srli a2, 35
.option norvc
.byte 1, 255, -1, 0x27
L1237: lui x23, %hi(0x12345678)
add s0, fp, s8
bltz s11, 5b
tail L976
li zero, 2147483647
amoor.d.aqrl t4, sp, (a7)
addi a3, tp, 0  # trailing comment
L1244: neg s5, a3
li x2, 0x4aba5aaa
lui x26, %hi(0x12345678)
sraw x17, t6, a1
li s1, -1914535936
jal t1, L866
csrrs s11, sstatus, zero
srli t1, s4, 56
addi x22, t3, 0  # trailing comment
3:
lui x17, %hi(0x12345678)
L1254: lb a3, (a1)
fence
beqz t5, 6b
L1257:
lr.w.rl s5, (x17)
sfence.vma s9
bnez t4, L1006
sfence.vma s9, x12
addi a1, s8, 0x473  # trailing comment
ebreak
L1263:
jal L1274
sraw x2, a3, x1
beq a0, s2, L459
bgtz x15, L1752
sfence.vma
3:
.option rvc
c.beqz a1, L1298
c.sd a3, 48(a1)
.option norvc
.option rvc
c.add x10, x2
.option norvc
L1270: amoxor.d.aq s6, x31, (a1)
remuw a3, s4, x15
jalr ra, 0(x3)
addi a3, x12, 0x278  # trailing comment
L1274:
ret
csrsi mstatus, 3
addi x10, x4, 0x5b4  # trailing comment
addi s5, tp, 0  # trailing comment
sll t2, s4, x11
sh t4, 983(s4)
sext.w s8, s7
csrrwi s7, mcause, 3
divw a3, s2, s5
andi x13, gp, ~0xf
csrwi mepc, 1
mret
rem x29, s5, gp
div t5, a6, s5
lbu s6, (tp)
sc.w a7, s8, (a6)
6:
addi a6, x23, 0x329  # trailing comment
sh a5, -2048(x1)
addi x27, s11, 0x0  # trailing comment
ebreak
lr.d.aqrl t0, (s0)
srli s5, ra, 0
srli t0, t6, 25
addiw x15, a6, 2047
L1298: sb x28, 0x7ff(x16)
5: lui t2, 0
bge s0, s3, 7b
bne t3, s5, L552
csrsi 832, 27
addi s2, x13, K2
sc.d s1, s3, (x8)
fence
sc.w.aq x12, x12, (s2)
L1307: addi s11, x31, 2047  # trailing comment
L1308: sd t5, 2047(a5)
not x18, a6
li x28, 1
li a0, -2048
sra t4, x31, ra
L1313: ret
.Lpc14: auipc a4, %pcrel_hi(L195)
lw a4, %pcrel_lo(.Lpc14)(a4)
L1315: addiw t2, s9, -1992
xori t5, x25, 2047
div t4, t0, s6
lui a5, %hi(K3)
amoand.d x23, s9, (s9)
srai x6, s0, 12
L1321: bgt s6, a5, L1888
sd x14, 0x0(t1)
ori x21, a3, 726
lui s6, 0
.option rvc
c.j 1f
c.andi a0, -9
c.ebreak
.option norvc
li x5, (1 << 9) | 5
ori a3, x5, 0
csrsi medeleg, 24
addi s8, s11, 1429  # trailing comment
5: addi t6, x30, (K1 - 3) * 2
bgeu x12, t3, L975
L1332: beqz tp, L1840
jalr ra, 0(a3)
3: addi a7, s8, 2047  # trailing comment
.half 20496, 0x614
rem a7, zero, s0
addi s2, a4, 948  # trailing comment
csrw mie, a0
bltz a5, L1548
addi s11, x10, 2047  # trailing comment
lr.w x21, (a5)
L1342: li x9, 1610973184
L1343: div s7, a0, x17
L1344: jal x2, L1173
blez x27, L1904
ori x5, a7, -2048
L1347: .Lpc15: auipc x11, %pcrel_hi(L1049)
ld x11, %pcrel_lo(.Lpc15)(x11)
csrrc a5, mstatus, x29
lw t4, -424(t0)
addi s6, tp, 0x63
.option rvc
c.bnez s0, L1409
.option norvc
L1352: lui x31, 1048575
addi s8, x25, %lo(0x12345678)
addiw x31, x17, 0x0
L1355: sb s1, -2048(s10)
lui x31, 1048575
addi x22, s6, 995  # trailing comment
csrr t6, cycle
L1359: sb s4, -2048(a4)
sra zero, x23, x24
.option rvc
c.swsp sp, 96(sp)
c.srai s1, 15
.option norvc
.option rvc
c.jr x15
.option norvc
addi sp, s11, 1090  # trailing comment
addi s3, a1, 0x0  # trailing comment
beq x29, t4, L995
addi s1, a6, 1075  # trailing comment
sc.w.rl x2, x21, (zero)
slliw zero, gp, 0
bne tp, x22, L867
add s1, s10, s0
jal s3, L1715
.option rvc
c.srai a3, 56
.option norvc
slliw a6, s11, 7
sd ra, 0x32d(a2)
auipc t3, 0x39cd1
addi x3, a4, -2048  # trailing comment
ret
6: addi a1, a1, 0  # trailing comment
addi x22, s3, 926  # trailing comment
jal L1932
.option rvc
c.ebreak
c.beqz s0, L1343
.option norvc
slt a2, s4, a1
L1383:
jal 5f
5: xori t0, s11, -403
fence
addi a3, s2, -2048  # trailing comment
and s1, t1, x30
blt a4, t2, L1790
L1389:
.option rvc
c.beqz s0, L1389
c.ebreak
.option norvc
L1390: beqz sp, 6f
slli t1, s7, 62
amoswap.w.aqrl fp, t4, (a3)
beq s7, x5, L797
sllw s4, t0, a5
bge s5, a1, 9f
sltu fp, sp, t6
lw a7, -1925(x9)
srl x23, a5, a1
.option push
.option rvc
c.addw a1, a3
c.sd a3, 72(a5)
c.andi a3, 2
.option pop
bgez t3, 6b
call L5
snez a7, x4
5: sfence.vma
li a7, 0
2: jr x20
bgeu a1, t5, L975
jal sp, L1263
8:
.option rvc
c.ebreak
c.addi16sp sp, -240
c.srli s0, 41
.option norvc
L1409:
jal s10, 8f
2:
bgeu s2, x31, 6b
L1411: fence r, o
sw tp, 0(ra)
jal t4, L1090
L1414: mul gp, x18, a6
L1415: snez s9, a3
addi x16, a4, -2048  # trailing comment
.option push
.option rvc
c.srai a1, 62
c.add x18, s9
.option pop
.option rvc
c.add x7, t1
c.ebreak
.option norvc
sll x25, x17, s3
bne t1, x29, L2140
jalr ra, 0(a2)
add s9, a6, x30
.option rvc
c.ldsp fp, 416(sp)
.option norvc
j 7f
bgt x30, x21, L1383
6: sc.d s4, s0, (t4)
li s2, 46792704
bge s4, x29, L1777
bnez a6, L2187
L1430:
sltiu s6, zero, -1319
addi t3, tp, (K1 - 3) * 2
L1432: .asciz "hey"
.ascii "a\tb\n"
sll s11, x29, t5
7:
addi a6, a3, 0  # trailing comment
fence.i
sw x11, 0x32e(a2)
j L1548
bgt s0, a2, 8b
.Lpc16: auipc s6, %pcrel_hi(L2113)
lw s6, %pcrel_lo(.Lpc16)(s6)
6:
lr.w s10, (x18)
beq t0, x20, L634
addi a6, x30, -2048  # trailing comment
addiw s2, t2, -2048
L1444:
addi x30, zero, 0  # trailing comment
li zero, 0x4c560dc7
addiw s1, x9, 0
L1447:
ori tp, t0, 0
addi x3, x0, 1363  # trailing comment
mulhu a2, t2, a2
jal 2b
addi x24, s1, K1
amoor.w.aqrl t0, t6, (t4)
li t0, 603233079
andi s6, x4, 0x7ff
bltz t0, L1850
csrrwi x30, mcause, 27
jal L1656
addiw x2, tp, -2048
addi t6, s1, -1157  # trailing comment
addi a3, s6, 0  # trailing comment
sltu s2, t4, x25
bgtu x8, t2, 8f
srai x12, a2, 43
addi t0, a3, 0x0  # trailing comment
sh x26, -2048(a2)
L1466: slliw x25, a7, 18
L1467: lui x25, 0xfffff
li sp, (1 << 9) | 5
li t5, -4096
addw s7, a7, x16
add s11, s4, t1
addi s1, t3, 2047  # trailing comment
.option rvc
c.lwsp s7, 4(sp)
.option norvc
ori a6, s7, 2047
addi s3, a7, 0
L1476:
addi a2, t2, 2047  # trailing comment
addi t1, a0, 2047
1: tail L1030
lui a0, %hi(K2)
L1480: .Lpc17: auipc a7, %pcrel_hi(L1133)
ld a7, %pcrel_lo(.Lpc17)(a7)
.option push
.option rvc
c.beqz a2, 6b
.option pop
div x31, s3, x21
lr.w.aq sp, (x27)
amoswap.d.rl a1, t4, (a6)
slti x10, s6, 0x7ff
or x21, x29, a2
amoadd.d t3, x22, (x1)
sh tp, -2048(x12)
bltz s0, L1715
srai x22, s1, 8
sc.d a3, gp, (s11)
L1492:
lbu x20, 0x0(a2)
addi s6, a0, 0  # trailing comment
csrsi scause, 15
lui s8, %hi(0x12345678)
slli x31, x15, 47
csrr s3, scounteren
mulhsu a6, s3, a7
L1499: addi a2, a6, -2048  # trailing comment
L1500: li s11, -454
wfi
bnez sp, L1216
L1503:
subw a4, s6, tp
.option push
.option rvc
c.ld a2, 40(a1)
c.j L1547
.option pop
lui x29, %hi(0x12345678)
sh a3, 0(t3)
addi tp, ra, %lo(0x12345678)
ebreak
auipc tp, 1048575
tail L200
bltu s1, a1, L974
.word L1581 - L1085
addi x27, x25, 2047  # trailing comment
auipc x13, 0
sc.w.aqrl s2, s8, (x8)
jalr ra, 0(s9)
sb t6, -952(a6)
addi gp, ra, -2048  # trailing comment
jal t4, L1575
lr.d.aq s2, (x18)
L1521: jal 5b
xori a0, x24, 858
sraiw t3, sp, 15
bgeu s0, x20, L1411
andi ra, x31, ~0xf
3: srai s3, gp, 60
9: jal t2, 3b
jalr t3, 1737(a2)
sext.w a3, t3
.zero 8
rem t3, a7, gp
sd gp, -2048(s1)
.option rvc
c.swsp s10, 216(sp)
.option norvc
L1534: sc.w.rl x0, x4, (s2)
bgtz a4, 3f
addiw s1, zero, 935
L1537:
mulw x19, s11, s0
csrrc x21, mideleg, s6
lui t1, %hi(K3)
sgtz s6, t1
mulhsu x25, x14, t5
lb s11, 2047(x7)
srliw a0, x30, 14
lui tp, 1048575
amominu.d.rl x13, t6, (s10)
.option rvc
c.beqz a2, L1500
.option norvc
L1547:
ld t6, -2048(x16)
L1548:
addi t5, x9, -1277  # trailing comment
xori x29, ra, -2048
L1550: sc.d.aqrl t6, s6, (x13)
blt s6, s10, 3b
amomin.d.rl a5, x17, (t4)
j L2012
addi a4, x31, K1
jal 9f
blez a5, L1599
slli a7, x0, 14
bgtu x8, a7, L1352
.option rvc
c.bnez s1, L1500
.option norvc
li x13, -23079662
L1561: addw s8, s9, s9
addi s9, a0, 0  # trailing comment
L1563: blez x9, L1237
bgtu s4, x1, 4b
addi t6, s5, 0  # trailing comment
bgtu a7, s4, 1b
sb s3, -1173(s2)
remw x31, a7, a3
addi a4, x7, 1362  # trailing comment
lh tp, 2047(gp)
sll a7, zero, zero
li t4, 0x18d14000
L1573: jalr s11, 0x7ff(a1)
.option push
.option rvc
c.ldsp a0, 144(sp)
c.add x26, s3
c.mv t2, x12
.option pop
L1575: remuw fp, s7, s2
L1576:
bgeu s10, a5, L1901
L1577: auipc s10, 0x0
bltz a1, L1467
call L1480
srai x28, a2, 0
L1581: jal s6, L1801
lr.d a6, (x29)
sraw s10, x11, x11
fence ow, iw
rem s6, a5, t3
.option rvc
c.bnez a1, L1561
c.addi a0, 31
.option norvc
srai s4, a0, 57
addi s11, ra, -2048
remu s1, s4, ra
bltu s0, s7, L1263
L1591: sfence.vma tp
bltu s2, zero, 1b
jal L1030
mul a1, s8, a0
jal L1771
L1596: slliw x16, gp, 7
sc.w.aq tp, gp, (x19)
sltu a1, a2, s6
L1599: csrrs s11, mscratch, zero
L1600: bltu x21, s7, L1063
L1601: sh s1, -2048(t6)
amoand.d s8, x23, (s9)
sll a4, t2, s7
sltu fp, s2, t3
add fp, t0, x13
bltu t0, s8, 5f
xori gp, x18, 0x6ac
.option rvc
c.lwsp fp, 132(sp)
.option norvc
addiw x8, s6, -763
L1610:
lui a1, 0
mul t5, t0, x24
sb x22, 2047(a7)
bleu s7, s5, L2125
addi s4, a0, 0x77a  # trailing comment
addi s8, x21, 0  # trailing comment
li t0, 0x42f
sraiw a5, x29, 11
andi t1, t3, ~0xf
amoswap.w t3, x29, (x31)
li x7, -1034
auipc s10, 0
L1622: auipc t5, 856450
jalr ra, 0(a5)
addi gp, sp, 0x755  # trailing comment
sltz x15, x14
mul a1, x16, t4
ble x14, a5, L1389
addi x2, s8, 1208  # trailing comment
ret
addi x24, s8, -964  # trailing comment
3: addiw s11, t3, 577
L1632: not s6, x24
3: amoor.d.aq x13, gp, (s6)
xori s2, s10, 1603
srai a1, a4, 61
L1636:
slt x27, a7, s7
bltu s8, t6, 4b
.option rvc
c.beqz a1, L1596
c.addiw x19, 1
.option norvc
bne t6, x20, 6f
xori x24, zero, 0
beqz gp, L2150
3: remw x8, s3, x0
addi a1, a3, K1
ori s9, s10, 142
fence w, rw
bltu s0, sp, L1383
ble a4, a1, L2199
addi s2, s3, -2048  # trailing comment
addi x8, t1, 0x0  # trailing comment
lui s5, 1048575
L1651: lr.d.aq x7, (s10)
2: bne sp, x16, L867
amomaxu.w a1, s8, (a7)
L1654:
jalr a5, 2047(ra)
L1655: ori zero, t0, 2047
L1656:
mv s3, s7
addi x24, t6, 2047  # trailing comment
lwu sp, -1410(a6)
blez s2, 8f
.option rvc
c.addi16sp sp, 496
c.ld a4, 224(a2)
c.j L2116
.option norvc
sc.w.rl tp, a1, (ra)
L1662: lb t1, (s1)
beq s4, s2, L1840
amoor.d.aqrl x28, a0, (a1)
fence.i
lr.w.rl tp, (x1)
addi a3, gp, 0x724  # trailing comment
bleu t6, t0, L1414
lr.d t5, (x10)
addiw t0, x11, -2048
addi a7, s1, 0x132
j L1321
li a5, -2049
addi a0, s2, -2048
tail L1017
lr.w.aqrl s7, (x5)
.option push
.option rvc
c.ld a2, 80(a2)
c.ldsp s9, 96(sp)
c.srai a2, 16
.option pop
ret
jalr ra, 0(gp)
addi a2, zero, 0  # trailing comment
1: addi zero, a7, (K1 - 3) * 2
remuw s0, s2, x2
ori x10, t5, -2048
negw a1, a5
lr.w.aq t0, (x22)
L1686: andi t4, a4, -199
6:
.option push
.option rvc
c.beqz a2, L1656
.option pop
L1688:
jalr a3
csrrc x26, mtvec, t3
ebreak
ori x10, s8, -2048
L1692: srliw tp, s11, 24
bge a7, gp, L2043
L1694: .Lpc18: auipc s1, %pcrel_hi(L1715)
ld s1, %pcrel_lo(.Lpc18)(s1)
L1695: sub a3, t0, s0
csrr x8, sepc
ori t5, s2, 1220
div a1, s2, s8
.asciz "hey"
.ascii "a\tb\n"
.balign 8
L1700: rem x13, a3, s6
remu s6, t4, s1
sc.d.aq t3, sp, (x14)
mulh a6, a0, x21
addi x12, ra, -620  # trailing comment
jr x11
4: auipc s6, 979770
ble tp, s1, 9b
addi t0, s6, %lo(0x12345678)
li x31, 0x388
lbu a5, (x27)
csrrc x17, satp, t1
auipc s6, 1048575
slliw t5, a5, 2
addi a2, a4, 2047  # trailing comment
L1715: sc.d.rl x9, t3, (s7)
xori x27, x27, -1008
xori t2, x28, 0
sext.w a4, tp
bne x27, x24, L927
bge a4, s0, L1700
bleu x25, a7, L1476
L1722:
.option push
.option rvc
c.addi16sp sp, 96
c.ld a2, 248(a5)
c.lwsp x20, 200(sp)
.option pop
lla tp, L1632
addi s1, t4, 0  # trailing comment
subw gp, s11, x16
2:
sc.w t0, x21, (x27)
L1727: mret
addi t6, s4, 0  # trailing comment
j L2199
L1730: mul t0, a3, gp
sw a4, 1371(t5)
lhu x9, 2047(x2)
auipc tp, 0
lla x27, L1694
srai t6, s6, 26
.option rvc
c.add s3, t0
c.jr t3
.option norvc
sfence.vma x22, x11
bgeu a6, x13, L1444
amomaxu.w.rl gp, s3, (s10)
sraiw a7, s6, 14
addi x8, s2, 40  # trailing comment
lui s10, 1048575
jalr ra, 0(x14)
.option rvc
c.beqz a2, L1715
.option norvc
ld x7, 2047(t1)
lui a0, %hi(K3)
addi x20, a0, 2047
1: fence o, iow
li t3, 493791384
L1750: blez a2, L1231
L1751: .asciz "hey"
.ascii "a\tb\n"
L1752: csrrc s0, mepc, a2
lui x2, 0x0
csrrwi ra, mcause, 7
bltu x4, s10, 8f
lr.d t3, (t5)
amoand.d.aq t4, a0, (t3)
xori s11, t5, 1385
ori s10, gp, 0
addi t4, s5, K2
blt a4, sp, L927
seqz a6, x16
subw a4, x2, zero
divuw s5, s3, gp
L1765:
ld x23, 0x781(sp)
call L1688
lb a4, 2047(a4)
sw x21, -2048(s5)
sltu a5, x1, x19
sc.w.rl zero, x6, (x12)
L1771: sgtz t6, sp
bltu x20, t1, L1883
L1773:
sret
4: bgeu a6, x12, L1229
sw t4, -2048(s3)
csrw mie, a1
L1777:
addi t1, s6, 0  # trailing comment
j 1f
ble a4, s11, 1f
mret
bne t5, t5, 3f
jr t4
addi a1, s8, 2047
xori s1, zero, -1618
ret
addi t5, s5, -2048  # trailing comment
li s2, -2147483648
addi s10, x10, 1168  # trailing comment
addi s1, gp, 0  # trailing comment
L1790: srli t3, s6, 53
csrw sie, t4
L1792: jr s11
jalr x20
addi s0, s6, 0  # trailing comment
ori t0, tp, -1036
jr ra
.option rvc
c.li fp, -20
c.addi4spn s0, sp, 184
.option norvc
blt t5, x13, L1492
csrrc s7, 2496, t1
nop
L1801: bgtz ra, L1021
lbu x26, (x1)
addiw x27, a3, 2047
L1804:
li zero, 0x800
L1805: xori x13, t6, -1493
beqz s8, L1622
negw s8, s5
bgez tp, 3b
bltz t5, L2079
1: jalr a0
slti s7, s4, 1480
call L95
.byte 1, 255, -1, 0x13
.balign 8
sh x15, -1757(a4)
csrrwi s9, 1472, 1
lwu s2, -1969(s10)
amoswap.w.rl x22, s11, (s0)
bgeu t1, s11, L1056
addi sp, x20, 0x7ff  # trailing comment
L1820: ori x13, a0, 0
L1821: addi ra, s6, 1969  # trailing comment
jalr sp
addi t3, x2, 2047  # trailing comment
lhu s6, 170(x9)
sret
bgez x0, 2b
jalr ra, 0(s10)
bleu a0, x26, L1801
jal s5, 3b
mulh t5, x16, a7
not a7, s5
sw s8, -2048(s7)
L1833: bne a4, ra, L2184
ret
fence
sw s10, -1574(a4)
bgeu a7, x11, L1344
lla a1, L995
bgt x9, t0, L1480
L1840: sc.d.rl x6, zero, (a7)
csrrwi tp, 832, 9
xori t1, t3, 0x7ff
rem t2, s1, x5
L1844:
.option rvc
c.sub s1, s1
c.beqz a4, L1860
.option norvc
L1845: addi a3, a3, (K1 - 3) * 2
addi s3, s1, -1056  # trailing comment
jalr ra, 0(s0)
xori x22, x7, 655
sub a0, x19, zero
L1850:
ebreak
remw gp, s0, x19
csrci 2496, 7
li a2, 209
sw s5, 0x7c5(a3)
.dword L290, 0x7b6b8f4ca2d69171
.balign 8
lbu a6, 0(tp)
lui x24, 0x63a6e
L1858: and a3, ra, t5
addi s7, t4, -2048  # trailing comment
L1860: mulhu a2, a6, tp
li s3, (1 << 9) | 5
.option push
.option rvc
c.ebreak
c.srli s0, 8
c.lwsp a3, 152(sp)
.option pop
remuw t4, sp, s3
andi x5, a0, ~0xf
addi t0, s8, -862  # trailing comment
srli x4, s7, 4
addi s4, s3, 2047  # trailing comment
addi s9, t1, 1961  # trailing comment
amoor.d.aq zero, gp, (s11)
sext.w s8, x28
L1871:
addi t5, gp, 0x2c1  # trailing comment
mulw t1, x30, x18
sgtz ra, x25
sub ra, a3, x21
2: jal L970
lr.d x12, (s11)
sret
srli t6, s11, 18
5: sw s5, -2048(t0)
sc.d.rl s10, x16, (s9)
mulw s8, x22, s9
not s3, sp
L1883: jalr gp
lui sp, 0
sltiu a2, x27, 0x763
L1886:
lr.w s10, (x14)
addi x29, t5, 0x7ff  # trailing comment
L1888: ret
bgez x26, L2020
bleu x1, x11, L1896
addi s0, zero, -2048  # trailing comment
divu x1, s0, x2
sd s3, -2048(t2)
sltiu x8, a5, 258
sc.w.rl x23, t5, (s6)
L1896: sraiw a7, s5, 7
.option rvc
c.mv s4, s11
c.mv tp, x20
c.ldsp s8, 384(sp)
.option norvc
.byte 1, 255, -1, 0xb
8: bleu s8, a6, 6f
and a7, x3, gp
L1901: mret
sc.w.aq s2, s7, (t3)
jalr ra, 0(x21)
L1904: amominu.d.aq ra, s7, (s3)
jal L74
nop
srli a6, x1, 6
jalr s11, 2047(s5)
jal 1b
slti zero, s9, 27
amomax.d.aqrl x29, tp, (a6)
sc.d.aq x31, a5, (s7)
.zero 8
.balign 8
addi t0, s0, 2047  # trailing comment
subw s8, t2, sp
tail L1860
lui t1, 1048575
jr t2
addw s3, a2, x17
addi t1, x14, (K1 - 3) * 2
srliw s9, x10, 31
L1922: addi t0, x18, -1013  # trailing comment
amoand.d a4, t1, (s8)
beq t4, a6, L976
sh t4, -475(x17)
.word L1956, L1777, 7
j L2079
jalr a2, 2047(ra)
sc.d.aqrl gp, x27, (t4)
lbu t3, 2047(a3)
addi t0, s4, 0  # trailing comment
L1932: sraiw s4, a0, 7
5: lui a3, 0xfffff
div a1, s4, a2
srliw x25, t1, 10
mulhsu s5, s9, s2
ebreak
jalr s11, -95(tp)
L1939: jal s10, L1270
addi s11, s3, 2047  # trailing comment
li x13, -1711
.byte 1, 255, -1, 0x28
.option push
.option rvc
c.srli a1, 57
.option pop
addi s8, s9, 1723
ori t6, t1, 0
addi s2, s1, 0x7ff  # trailing comment
8: bgt gp, a7, L1922
jalr s6, 2047(x30)
tail L1409
lwu gp, (s8)
amoor.w.aq x9, x21, (tp)
andi tp, s9, ~0xf
srliw s2, t3, 11
5: lh sp, 2047(s11)
auipc s7, 0x14058
L1956: .Lpc19: auipc t4, %pcrel_hi(L1347)
ld t4, %pcrel_lo(.Lpc19)(t4)
addi x24, a7, -32  # trailing comment
remw x15, x5, fp
sraiw sp, s8, 10
mulw t2, x0, a6
mulhsu a2, s2, s9
xori s3, s7, -2048
fence
jr s8
csrs mcounteren, s1
addi x9, x29, -2048  # trailing comment
li s8, -1388928549
addi a3, t4, -1367  # trailing comment
lr.d s6, (t2)
tail L1801
csrsi scause, 2
li s5, 4096
lbu x1, 0(x19)
slt x4, x9, s1
.option rvc
c.sd a5, 184(a5)
c.sub a0, a4
.option norvc
bleu x8, s7, L1173
auipc x14, 884727
9: auipc s8, 0xfffff
sc.d x28, t3, (x19)
L1980: jalr a7, 1350(t2)
add t3, a2, x26
remu sp, x30, t3
addi a3, a4, -2048  # trailing comment
.option rvc
c.ebreak
c.jr gp
.option norvc
.option rvc
c.lwsp t1, 32(sp)
c.addiw s1, 31
.option norvc
9:
srli a6, x19, 17
sb t3, 0x0(s4)
addw x23, tp, s11
sltiu ra, a3, 0
addi t0, s5, -2044  # trailing comment
addi s5, a1, -2048
slti x3, x1, 1900
div tp, a4, a3
li s7, -928
li a5, -775876491
bleu t5, t1, L1622
auipc x26, 0
sc.w sp, s11, (a4)
csrr a2, mtvec
bgeu x27, a7, L2102
sub s9, t6, s4
bltz x3, L1622
jalr t1, 611(x29)
sllw a4, x3, s3
lwu a0, 0(t4)
slti t6, t6, 1488
andi t2, x8, -504
.option rvc
c.beqz a2, 9b
c.srai a4, 62
c.li sp, 1
.option norvc
sraiw s0, a7, 0
sh tp, -2048(s5)
ld gp, -2048(s8)
L2012:
divw t1, x1, tp
.half -13633, 0x791d
addi x8, tp, -2048  # trailing comment
mret
addi s7, x30, -698  # trailing comment
sgtz a6, s5
jalr x3
li sp, -1676042240
L2020:
.option rvc
c.j L1792
.option norvc
L2021:
addi x16, t1, -86  # trailing comment
snez a1, x9
lwu t0, (s10)
lla s0, L796
sh x20, 0(a7)
lui s6, 403946
1: li t5, -1816
rem a2, s7, s7
andi x6, x7, 0x4d4
addi x30, a3, 1462  # trailing comment
ble a4, x5, 2b
.option push
.option rvc
c.swsp a2, 148(sp)
c.beqz a0, L2081
.option pop
lb t0, 2047(x9)
.option rvc
c.add s8, a0
.option norvc
srai a5, a4, 58
sltu x1, s9, s10
ebreak
call L2177
andi gp, a1, -1495
ori t2, t2, 1494
bgt t5, t2, L1224
lui ra, %hi(0x12345678)
L2043:
srai s1, ra, 16
jal x21, L1182
L2045: lwu t0, -2048(a3)
bgeu x23, a7, L2021
sraw t4, t1, a3
addi a6, x1, -1838  # trailing comment
divu a4, t5, s6
3:
.option rvc
c.jalr a4
.option norvc
add x9, a7, s8
blt s1, x23, L1347
ble t4, a1, L1216
bgtu gp, x26, L2020
.option push
.option rvc
c.lw a3, 76(a4)
c.bnez a2, L2012
c.ebreak
.option pop
sltu t3, x10, t2
jalr a4, -478(t2)
sgtz a5, x27
addi s7, t5, -2048  # trailing comment
sub a2, s1, x30
andi s7, s3, -2048
li x7, (1 << 9) | 5
lla a7, L1308
bltz s6, L1563
slliw x29, a4, 9
bgt sp, zero, L1308
L2067: ble t6, s8, L1922
ori x26, a7, 0x518
neg s3, a3
sc.w.aqrl s5, t6, (s9)
addiw a0, t3, -1425
sfence.vma t3, s10
blt s7, t1, L1500
bgeu a3, sp, L1904
addi x9, ra, 2047  # trailing comment
9: bnez s3, 5f
addi a7, t1, (K1 - 3) * 2
xori a5, a5, -2048
L2079: addi ra, s6, 0x55d  # trailing comment
lui a3, 122751
L2081: addi s10, x28, 2047  # trailing comment
srl a2, tp, x22
csrsi sstatus, 11
rem t1, t6, x1
.word L614 - L303
csrw mideleg, s0
jr s6
andi a2, s11, 0
amoor.d.rl zero, t0, (t3)
lr.d s4, (ra)
bgtu t1, s8, L1344
bltz tp, L1563
.Lpc20: auipc x30, %pcrel_hi(L2113)
addi x30, x30, %pcrel_lo(.Lpc20)
jr x25
or s7, x2, s9
lw a3, (x21)
bge x30, t2, L2177
addi a7, t2, 0
ori x23, x12, 2047
jal L248
addi t6, zero, -757  # trailing comment
L2102:
remw sp, t3, s5
mulw t1, t1, x7
ld fp, (a2)
remw a3, t2, s10
li x13, -609560571
addi s11, sp, K2
lui t6, 504154
sd a5, 0(x2)
auipc x17, 1048575
.option rvc
c.ebreak
c.bnez a0, L2140
.option norvc
sh t5, -2048(gp)
L2113: csrsi mip, 7
ori x22, x9, 0x0
divu s11, x19, ra
L2116: blt s10, s5, L1654
addi x10, s10, 0x4ba  # trailing comment
5: srl a0, a0, a7
bne t5, a3, 1f
bltz a3, L1342
.word L1165, L541, 7
jalr ra, 0(t3)
negw s5, ra
slt x24, a2, s10
L2125:
blt t4, x24, L1344
li t3, -2049
lui t0, 973883
auipc s1, 0x0
.option rvc
c.ld a4, 224(a4)
.option norvc
addi s6, a3, %lo(0x12345678)
lla sp, L1065
sltiu s9, s9, -2048
ori sp, x3, 695
addi x21, s10, -543  # trailing comment
xori zero, s10, -2048
j 8b
fence
slti x23, a1, -1146
nop
L2140: addi s8, t2, -348  # trailing comment
mulhu a5, t6, s7
addi s5, s2, -2048  # trailing comment
bgeu t0, gp, 5b
addi s7, t2, -123  # trailing comment
L2145:
tail L2145
sext.w x15, s0
sh x18, -2048(a0)
beq s9, a0, L1550
6: sltz t6, x20
L2150:
.option rvc
c.lui tp, 1048560
.option norvc
csrrc s9, 832, t4
bgeu s11, a4, L1575
andi a7, a3, -2048
lui x11, 0
beq s7, s7, L1480
ori sp, s3, 1454
call L2177
addi x19, a7, 0  # trailing comment
li x1, -2147483648
.option rvc
c.slli x10, 37
.option norvc
and a5, s9, s8
div a3, s6, sp
ori t4, a4, -2048
addi gp, s4, -2048  # trailing comment
sfence.vma t5, s5
bgt s11, s1, 6b
srliw a7, a0, 7
.option push
.option rvc
c.ebreak
c.j L2020
c.beqz a3, L2150
.option pop
amoswap.d x19, x3, (s11)
csrwi mepc, 1
3:
addi a1, a4, K0
lr.d.aq tp, (t3)
.option push
.option rvc
c.addw a3, a3
c.beqz a4, 6b
c.nop
.option pop
lw s11, 0x0(t3)
csrsi mideleg, 21
amomax.d x20, s4, (a2)
L2177:
csrsi mip, 27
ble x20, a3, L1651
addi s2, a5, -2048  # trailing comment
andi s7, a2, ~0xf
mulhu x5, s10, t3
lb x6, 0(s5)
lui a4, 122404
L2184: bgtu s6, s0, 7b
L2185: li ra, -153452544
j 6b
L2187: jalr s7, 0x7ff(a1)
jal L1069
lui t0, 1048575
jalr s2
lbu x30, (a5)
addi fp, t1, 2047  # trailing comment
sll t1, x10, x0
8:
li x23, 2147479552
.option rvc
c.addi4spn a3, sp, 80
c.lw a5, 32(s0)
c.j L2067
.option norvc
xor t3, s4, t3
1:
xori x13, s6, -2048
bgt t6, s11, L1804
L2199: jal t3, 5b
