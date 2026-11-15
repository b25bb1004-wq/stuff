_start:
    # 1. Read /hello.txt
    la a0, p_hello
    li a1, 10
    li a2, 1
    li a7, 3
    ecall
    bltz a0, fail
    mv s0, a0

    mv a0, s0
    la a1, io_buf
    li a2, 64
    li a7, 2
    ecall
    blez a0, fail
    mv s1, a0

    mv a0, s0
    li a7, 4
    ecall
    bltz a0, fail

    la a0, s_read_pre
    li a1, 13
    call print_str
    la a0, io_buf
    mv a1, s1
    call print_str
    la a0, s_nl
    li a1, 1
    call print_str

    # 2. Create /a.txt with "world"
    la a0, p_atxt
    li a1, 6
    li a2, 14
    li a7, 3
    ecall
    bltz a0, fail
    mv s0, a0

    mv a0, s0
    la a1, s_world
    li a2, 5
    li a7, 1
    ecall
    li t0, 5
    bne a0, t0, fail
    mv s1, a0

    mv a0, s0
    li a7, 4
    ecall
    bltz a0, fail

    la a0, s_wrote_pre
    li a1, 14
    call print_str
    mv a0, s1
    call print_int
    la a0, s_nl
    li a1, 1
    call print_str

    # 3. mkdir /dir (allow 0 on first boot or -5 on persistent reboot)
    la a0, p_dir
    li a1, 4
    li a7, 11
    ecall
    beqz a0, mkdir_done
    li t0, -5
    bne a0, t0, fail
mkdir_done:

    # 4. List / with readdir
    la a0, s_ls_pre
    li a1, 10
    call print_str

    la a0, p_root
    li a1, 1
    li a2, 1
    li a7, 3
    ecall
    bltz a0, fail
    mv s0, a0

ls_loop:
    mv a0, s0
    la a1, ls_buf
    li a2, 512
    li a7, 13
    ecall
    bltz a0, fail
    beqz a0, ls_done

    la s1, ls_buf
    add s2, s1, a0
    mv s3, s1
ls_scan:
    bge s1, s2, ls_loop
    lbu t0, 0(s1)
    li t1, 10
    bne t0, t1, ls_next
    la a0, s_space
    li a1, 1
    call print_str
    mv a0, s3
    sub a1, s1, s3
    call print_str
    addi s3, s1, 1
ls_next:
    addi s1, s1, 1
    j ls_scan

ls_done:
    mv a0, s0
    li a7, 4
    ecall
    bltz a0, fail

    la a0, s_nl
    li a1, 1
    call print_str

    # 5. Error checks: open("/nope", 1) == -2, mkdir("/dir") == -5, read(99, ..) == -4
    la a0, p_nope
    li a1, 5
    li a2, 1
    li a7, 3
    ecall
    li t0, -2
    bne a0, t0, fail

    la a0, p_dir
    li a1, 4
    li a7, 11
    ecall
    li t0, -5
    bne a0, t0, fail

    li a0, 99
    la a1, io_buf
    li a2, 16
    li a7, 2
    ecall
    li t0, -4
    bne a0, t0, fail

    la a0, s_err_ok
    li a1, 18
    call print_str

    # 6. unlink /a.txt
    la a0, p_atxt
    li a1, 6
    li a7, 12
    ecall
    bnez a0, fail

    la a0, s_unlink_ok
    li a1, 18
    call print_str

    li a0, 0
    li a7, 0
    ecall

fail:
    li a0, 1
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
p_hello:
    .ascii "/hello.txt"
p_atxt:
    .ascii "/a.txt"
p_dir:
    .ascii "/dir"
p_root:
    .ascii "/"
p_nope:
    .ascii "/nope"
s_world:
    .ascii "world"
s_read_pre:
    .ascii "fsdemo: read "
s_wrote_pre:
    .ascii "fsdemo: wrote "
s_ls_pre:
    .ascii "fsdemo: ls"
s_err_ok:
    .ascii "fsdemo: errors ok\n"
s_unlink_ok:
    .ascii "fsdemo: unlink ok\n"
s_space:
    .ascii " "
s_nl:
    .ascii "\n"
s_minus:
    .ascii "-"
io_buf:
    .space 64
ls_buf:
    .space 512
