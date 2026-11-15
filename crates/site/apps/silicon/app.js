(() => {
  "use strict";

  window.DemoApp.mount({
    id: "silicon",
    name: "Silicon",
    source: "crates/silicon",
    about:
      "RV64 emulator with MMU, traps, devices. The crystal that keeps time for every real processor. Assembles RV64 instructions, boots a 1 MiB virtual machine, and inspects UART and registers.",
    help:
      "silicon run ELF [--steps N] [--trace]\n" +
      "(Binary lives in crates/chisel.)",
    examples: [
      {
        label: "UART hello (default)",
        input:
          "    li t0, 0x10000000\n" +
          "    la t1, msg\n" +
          "loop:\n" +
          "    lbu t2, 0(t1)\n" +
          "    beqz t2, done\n" +
          "    sb t2, 0(t0)\n" +
          "    addi t1, t1, 1\n" +
          "    j loop\n" +
          "done:\n" +
          "    li a0, 42\n" +
          "    li a1, 100\n" +
          "    li a2, 0x1234\n" +
          "    li t0, 0x00100000\n" +
          "    li t1, 0x5555\n" +
          "    sw t1, 0(t0)\n" +
          "halt:\n" +
          "    j halt\n" +
          "msg:\n" +
          '    .asciz "hello from silicon\\n"'
      },
      {
        label: "Sum 1..10 loop",
        input:
          "    li a0, 0\n" +
          "    li t0, 1\n" +
          "    li t1, 11\n" +
          "sum_loop:\n" +
          "    add a0, a0, t0\n" +
          "    addi t0, t0, 1\n" +
          "    blt t0, t1, sum_loop\n" +
          "    li t2, 0x00100000\n" +
          "    li t3, 0x5555\n" +
          "    sw t3, 0(t2)\n" +
          "halt:\n" +
          "    j halt"
      },
      {
        label: "Unknown opcode (error)",
        input: "    bogus_op a0, a1, a2"
      }
    ]
  });
})();
