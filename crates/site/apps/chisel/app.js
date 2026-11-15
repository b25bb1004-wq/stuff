(() => {
  "use strict";

  window.DemoApp.mount({
    id: "chisel",
    name: "Chisel",
    source: "crates/chisel",
    about:
      "RV64IMAC assembler and ELF emitter for the Silicon machine and Titan kernel. Assembles RISC-V source into machine code with an addressed hex dump and symbol table.",
    help:
      "silicon run ELF [--steps N] [--trace]\n" +
      "(Chisel hosts the assembler library and silicon CLI binary.)",
    examples: [
      {
        label: "Add & ebreak (default)",
        input:
          "start:\n" +
          "    addi a0, zero, 42\n" +
          "    addi a1, zero, 13\n" +
          "    add  a2, a0, a1\n" +
          "    ebreak"
      },
      {
        label: "Loop with data label",
        input:
          "entry:\n" +
          "    la t0, banner\n" +
          "    li t1, 0\n" +
          "spin:\n" +
          "    lbu t2, 0(t0)\n" +
          "    beqz t2, exit\n" +
          "    addi t0, t0, 1\n" +
          "    addi t1, t1, 1\n" +
          "    j spin\n" +
          "exit:\n" +
          "    ebreak\n" +
          "banner:\n" +
          '    .asciz "chisel"'
      },
      {
        label: "Invalid register (error)",
        input: "    addi x99, zero, 1"
      }
    ]
  });
})();
