# lithium: two-task RV64 kernel for silicon (M-mode kernel, U-mode tasks).
# Layout: kernel stack 0x80100000, task stacks 0x80200000 / 0x80300000, frames in the image.
# Trap entry: mscratch holds the current task's frame base while tasks run.

_start:
    li sp, 0x80100000            # kernel stack top
    la t0, trap_entry
    csrw mtvec, t0               # direct mode
    la a0, str_boot
    call puts
    la t0, frames                # task A frame: pc, sp
    la t1, task_a_body
    sd t1, 0(t0)
    li t1, 0x80200000
    sd t1, 16(t0)
    addi t0, t0, 256             # task B frame
    la t1, task_b_body
    sd t1, 0(t0)
    li t1, 0x80300000
    sd t1, 16(t0)
    li t0, 0x02004000            # mtimecmp = mtime + 5000
    li t1, 0x0200BFF8
    ld t2, 0(t1)
    li t3, 5000
    add t2, t2, t3
    sd t2, 0(t0)
    li t0, 0x80                  # mie.MTIE
    csrs mie, t0
    li t0, 0x1800                # mstatus.MPP = U
    csrc mstatus, t0
    j restore

# Restore the current task (cur_task) from its frame and mret into it.
restore:
    la t6, cur_task
    ld t6, 0(t6)
    slli t6, t6, 8
    la t5, frames
    add t6, t6, t5
    csrw mscratch, t6
    ld t0, 0(t6)
    csrw mepc, t0
    ld ra, 8(t6)
    ld sp, 16(t6)
    ld gp, 24(t6)
    ld tp, 32(t6)
    ld t0, 40(t6)
    ld t1, 48(t6)
    ld t2, 56(t6)
    ld s0, 64(t6)
    ld s1, 72(t6)
    ld a0, 80(t6)
    ld a1, 88(t6)
    ld a2, 96(t6)
    ld a3, 104(t6)
    ld a4, 112(t6)
    ld a5, 120(t6)
    ld a6, 128(t6)
    ld a7, 136(t6)
    ld s2, 144(t6)
    ld s3, 152(t6)
    ld s4, 160(t6)
    ld s5, 168(t6)
    ld s6, 176(t6)
    ld s7, 184(t6)
    ld s8, 192(t6)
    ld s9, 200(t6)
    ld s10, 208(t6)
    ld s11, 216(t6)
    ld t3, 224(t6)
    ld t4, 232(t6)
    ld t5, 240(t6)
    ld t6, 248(t6)
    mret

.balign 4
trap_entry:
    csrrw t0, mscratch, t0       # t0 = frame base, mscratch = user t0
    sd t1, 48(t0)
    csrr t1, mscratch
    sd t1, 40(t0)
    sd ra, 8(t0)
    sd sp, 16(t0)
    sd gp, 24(t0)
    sd tp, 32(t0)
    sd t2, 56(t0)
    sd s0, 64(t0)
    sd s1, 72(t0)
    sd a0, 80(t0)
    sd a1, 88(t0)
    sd a2, 96(t0)
    sd a3, 104(t0)
    sd a4, 112(t0)
    sd a5, 120(t0)
    sd a6, 128(t0)
    sd a7, 136(t0)
    sd s2, 144(t0)
    sd s3, 152(t0)
    sd s4, 160(t0)
    sd s5, 168(t0)
    sd s6, 176(t0)
    sd s7, 184(t0)
    sd s8, 192(t0)
    sd s9, 200(t0)
    sd s10, 208(t0)
    sd s11, 216(t0)
    sd t3, 224(t0)
    sd t4, 232(t0)
    sd t5, 240(t0)
    sd t6, 248(t0)
    csrr t1, mepc
    sd t1, 0(t0)
    csrw mscratch, t0
    li sp, 0x80100000            # kernel stack
    csrr t1, mcause
    bltz t1, timer_irq
    li t2, 8                     # ecall from U-mode
    beq t1, t2, syscall
    j panic

timer_irq:
    li t2, 0x0200BFF8            # re-arm: mtimecmp = mtime + 5000
    ld t3, 0(t2)
    li t4, 5000
    add t3, t3, t4
    li t2, 0x02004000
    sd t3, 0(t2)
    call pick_other
    j restore

syscall:
    csrr t0, mscratch
    ld t1, 0(t0)                 # return past the ecall
    addi t1, t1, 4
    sd t1, 0(t0)
    ld t1, 136(t0)               # a7 = syscall number
    li t2, 1
    beq t1, t2, sys_write
    li t2, 2
    beq t1, t2, sys_yield
    li t2, 3
    beq t1, t2, sys_exit
    li t2, -1
    sd t2, 80(t0)
    j restore

sys_write:
    ld s5, 80(t0)                # ptr
    ld s6, 88(t0)                # len
    mv s9, s6
    call uart_write
    csrr t0, mscratch
    sd s9, 80(t0)                # return len
    j restore

sys_yield:
    sd zero, 80(t0)
    call pick_other
    j restore

sys_exit:
    la t1, cur_task
    ld t2, 0(t1)
    la t3, alive
    slli t4, t2, 3
    add t3, t3, t4
    sd zero, 0(t3)
    la t3, alive
    ld t4, 0(t3)
    ld t5, 8(t3)
    or t4, t4, t5
    beqz t4, halt
    call pick_other
    j restore

# Switch cur_task to the other task if it is alive; otherwise keep the current one.
pick_other:
    la t2, cur_task
    ld t3, 0(t2)
    xori t4, t3, 1
    la t5, alive
    slli t6, t4, 3
    add t5, t5, t6
    ld t5, 0(t5)
    beqz t5, po_ret
    sd t4, 0(t2)
po_ret:
    ret

halt:
    la a0, str_halt
    call puts
    li t1, 0x00100000
    li t2, 0x5555
    sw t2, 0(t1)
spin:
    j spin

panic:
    la a0, str_panic
    call puts
    csrr a0, mcause
    call print_hex
    li a0, 10
    call uart_putc
    li t1, 0x00100000
    li t2, 0x13333
    sw t2, 0(t1)
spin2:
    j spin2

# Wait for THR empty, then send the byte in a0. Clobbers t2, t3.
uart_putc:
    li t2, 0x10000005
uc_wait:
    lbu t3, 0(t2)
    andi t3, t3, 32
    beqz t3, uc_wait
    li t2, 0x10000000
    sb a0, 0(t2)
    ret

# Print the NUL-terminated string at a0. Clobbers s4, s5.
puts:
    mv s4, ra
    mv s5, a0
puts_loop:
    lbu a0, 0(s5)
    beqz a0, puts_done
    call uart_putc
    addi s5, s5, 1
    j puts_loop
puts_done:
    jr s4

# Print s5 (ptr) for s6 (len) bytes. Clobbers s5, s6, s8.
uart_write:
    mv s8, ra
uw_loop:
    beqz s6, uw_done
    lbu a0, 0(s5)
    call uart_putc
    addi s5, s5, 1
    addi s6, s6, -1
    j uw_loop
uw_done:
    jr s8

# Print a0 as 0x followed by minimal lowercase hex. Clobbers s0..s3.
print_hex:
    mv s3, ra
    mv s0, a0
    li a0, 48
    call uart_putc
    li a0, 120
    call uart_putc
    li s1, 60
    li s2, 0
ph_loop:
    srl t0, s0, s1
    andi t0, t0, 15
    bnez t0, ph_digit
    bnez s2, ph_digit
    bnez s1, ph_next
ph_digit:
    li s2, 1
    addi a0, t0, 48
    li t1, 10
    bltu t0, t1, ph_out
    addi a0, t0, 87
ph_out:
    call uart_putc
ph_next:
    addi s1, s1, -4
    bgez s1, ph_loop
    jr s3

# Task A enters here: letter 'A', then the shared loop.
task_a_body:
    addi s0, zero, 65
    j task_loop

task_b_body:
    addi s0, zero, 66

# Five lines "<letter><digit>\n", each after ~2000 busy instructions, then exit(0).
task_loop:
    li s1, 0
tl_line:
    li s2, 1000
tl_busy:
    addi s2, s2, -1
    bnez s2, tl_busy
    addi sp, sp, -16
    sb s0, 0(sp)
    addi t0, s1, 48
    sb t0, 1(sp)
    addi t0, zero, 10
    sb t0, 2(sp)
    mv a0, sp
    addi a1, zero, 3
    addi a7, zero, 1
    ecall
    addi sp, sp, 16
    addi s1, s1, 1
    addi t0, zero, 5
    bne s1, t0, tl_line
    addi a7, zero, 3
    addi a0, zero, 0
    ecall
    j spin2

.balign 8
frames:
    .space 512
cur_task:
    .quad 0
alive:
    .quad 1
    .quad 1
str_boot:
    .asciz "lithium: boot\n"
str_halt:
    .asciz "lithium: halt\n"
str_panic:
    .asciz "lithium: panic "
