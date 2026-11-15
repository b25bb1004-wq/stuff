# spy: isolation test guest (assembled at 0x8000_0000, RAM 2 MiB).
# Sets mtvec to a fault-counting handler that skips the faulting instruction,
# probes out-of-bounds GPAs and H/VS CSRs, prints "spy: faults <n>\n", and exits 0.

_start:
    li sp, 0x801f0000
    la t0, trap_handler
    csrw mtvec, t0

    li t0, 0x80200000
    ld t1, 0(t0)

    li t0, 0x81000000
    ld t1, 0(t0)

    li t0, 0x82000000
    ld t1, 0(t0)

    li t0, 0x80400000
    ld t1, 0(t0)

    li t0, 0
    ld t1, 0(t0)

    .word 0x68001073           # csrw hgatp, zero
    .word 0x28002573           # csrr a0, vsatp

    la a0, str_prefix
    call puts
    la t0, fault_count
    ld a0, 0(t0)
    call print_dec
    li a0, 10
    call uart_putc

    li t0, 0x00100000
    li t1, 0x5555
    sw t1, 0(t0)
spin:
    j spin

.balign 4
trap_handler:
    la t0, fault_count
    ld t1, 0(t0)
    addi t1, t1, 1
    sd t1, 0(t0)
    csrr t0, mepc
    lbu t1, 0(t0)
    andi t1, t1, 3
    li t2, 3
    beq t1, t2, th_wide
    addi t0, t0, 2
    j th_done
th_wide:
    addi t0, t0, 4
th_done:
    csrw mepc, t0
    mret

uart_putc:
    li t2, 0x10000005
uc_wait:
    lbu t3, 0(t2)
    andi t3, t3, 32
    beqz t3, uc_wait
    li t2, 0x10000000
    sb a0, 0(t2)
    ret

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

print_dec:
    mv s3, ra
    li t0, 10
    bltu a0, t0, pd_one
    addi a0, a0, 48
    j pd_out
pd_one:
    addi a0, a0, 48
pd_out:
    call uart_putc
    jr s3

.balign 8
fault_count:
    .quad 0
str_prefix:
    .asciz "spy: faults "
