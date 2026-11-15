(() => {
  "use strict";

  window.DemoApp.mount({
    id: "cobalt",
    name: "Cobalt",
    source: "crates/cobalt",
    about:
      "Logos to RV64 machine code and to bytecode. Sintering fuses loose powder into solid metal. Shows IR before and after optimization passes, encoded bytecode stats, and execution output.",
    help:
      "cobalt ir FILE.lg\n" +
      "cobalt opt FILE.lg\n" +
      "cobalt build FILE.lg -o OUT.mbc",
    examples: [
      {
        label: "Constant fold (default)",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn main() uses io {\n" +
          "  let a = 2 + 3 * 4\n" +
          "  let b = a\n" +
          "  let unused = 99 - 1\n" +
          "  let sum = b + 28\n" +
          '  print("answer = " + str(sum) + "\\n")\n' +
          "}"
      },
      {
        label: "Helper function & branch",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn clamp(v: int, lo: int, hi: int) -> int {\n" +
          "  if v < lo { lo } else if v > hi { hi } else { v }\n" +
          "}\n\n" +
          "fn main() uses io {\n" +
          '  print("clamped = " + str(clamp(42, 0, 10)) + "\\n")\n' +
          "}"
      },
      {
        label: "Syntax error",
        input: "fn main( {\n  let x = 1\n}"
      }
    ]
  });
})();
