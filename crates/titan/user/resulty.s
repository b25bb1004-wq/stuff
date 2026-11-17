_start:
    la a0, json
    li a1, 11
    li a7, 16
    ecall
    li a0, 0
    li a7, 0
    ecall

.balign 4096
_data:
json:
    .ascii "{\"ok\":true}"
