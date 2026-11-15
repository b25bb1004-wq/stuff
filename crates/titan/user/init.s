_start:
    # Print "init: hello pid " + getpid() + "\n"
    la a0, s_hello
    li a1, 16
    call print_str
    li a7, 8
    ecall
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Spawn ping and pong
    la a0, n_ping
    li a1, 4
    li a2, 0
    li a3, 0
    li a7, 5
    ecall
    mv s1, a0

    la a0, n_pong
    li a1, 4
    li a2, 0
    li a3, 0
    li a7, 5
    ecall
    mv s2, a0

    # Wait ping
    mv a0, s1
    li a7, 6
    ecall
    mv s3, a0
    la a0, s_ping_ex
    li a1, 18
    call print_str
    mv a0, s3
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Wait pong
    mv a0, s2
    li a7, 6
    ecall
    mv s3, a0
    la a0, s_pong_ex
    li a1, 18
    call print_str
    mv a0, s3
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Spawn and wait hog
    la a0, n_hog
    li a1, 3
    li a2, 0
    li a3, 0
    li a7, 5
    ecall
    li a7, 6
    ecall
    mv s3, a0
    la a0, s_hog_ex
    li a1, 17
    call print_str
    mv a0, s3
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Spawn and wait fault
    la a0, n_fault
    li a1, 5
    li a2, 0
    li a3, 0
    li a7, 5
    ecall
    li a7, 6
    ecall
    mv s3, a0
    la a0, s_fault_ex
    li a1, 19
    call print_str
    mv a0, s3
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Spawn and wait evil
    la a0, n_evil
    li a1, 4
    li a2, 0
    li a3, 0
    li a7, 5
    ecall
    li a7, 6
    ecall
    mv s3, a0
    la a0, s_evil_ex
    li a1, 18
    call print_str
    mv a0, s3
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # Call ps and print buffer
    la a0, ps_buf
    li a1, 512
    li a7, 14
    ecall
    blez a0, skip_ps
    mv a1, a0
    la a0, ps_buf
    call print_str
skip_ps:

    # Check uptime > 0
    li a7, 15
    ecall
    blez a0, skip_uptime
    la a0, s_uptime
    li a1, 16
    call print_str
skip_uptime:

    li a0, 0
    li a7, 0
    ecall

# print_str(a0=ptr, a1=len)
print_str:
    mv a2, a1
    mv a1, a0
    li a0, 1
    li a7, 1
    ecall
    ret

# print_int(a0=signed 64-bit integer)
print_int:
    addi sp, sp, -48
    sd ra, 40(sp)
    mv t0, a0
    bgez t0, pos_num
    neg t0, t0
    sd t0, 32(sp)
    la a0, s_minus
    li a1, 1
    call print_str
    ld t0, 32(sp)
pos_num:
    addi t1, sp, 32
    li t2, 0
    li t3, 10
    bnez t0, digit_loop
    addi t1, t1, -1
    li t4, 48
    sb t4, 0(t1)
    li t2, 1
    j print_digits
digit_loop:
    beqz t0, print_digits
    remu t4, t0, t3
    divu t0, t0, t3
    addi t4, t4, 48
    addi t1, t1, -1
    sb t4, 0(t1)
    addi t2, t2, 1
    j digit_loop
print_digits:
    mv a0, t1
    mv a1, t2
    call print_str
    ld ra, 40(sp)
    addi sp, sp, 48
    ret

.balign 4096
_data:
s_hello:
    .ascii "init: hello pid "
s_ping_ex:
    .ascii "init: ping exited "
s_pong_ex:
    .ascii "init: pong exited "
s_hog_ex:
    .ascii "init: hog exited "
s_fault_ex:
    .ascii "init: fault exited "
s_evil_ex:
    .ascii "init: evil exited "
s_uptime:
    .ascii "init: uptime ok\n"
s_nl:
    .ascii "\n"
s_minus:
    .ascii "-"
n_ping:
    .ascii "ping"
n_pong:
    .ascii "pong"
n_hog:
    .ascii "hog"
n_fault:
    .ascii "fault"
n_evil:
    .ascii "evil"
ps_buf:
    .space 512
