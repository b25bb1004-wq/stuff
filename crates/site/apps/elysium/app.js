(() => {
  "use strict";

  window.DemoApp.mount({
    id: "elysium",
    name: "Elysium",
    source: "crates/elysium",
    about:
      "Bytecode VM with garbage collector. A sealed world inside a plain rock. Compiles Logos source to MBC1 bytecode, verifies safety invariants, disassembles, and executes under fuel and heap limits.",
    help:
      "elysium verify FILE.mbc\n" +
      "elysium dis FILE.mbc\n" +
      "elysium run FILE.mbc",
    examples: [
      {
        label: "Factorial (default)",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn fact(n: int) -> int {\n" +
          "  if n <= 1 { 1 } else { n * fact(n - 1) }\n" +
          "}\n\n" +
          "fn main() uses io {\n" +
          "  let r = fact(6)\n" +
          '  print("6! = " + str(r) + "\\n")\n' +
          "}"
      },
      {
        label: "Fibonacci loop",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn fib(n: int) -> int {\n" +
          "  var a = 0\n" +
          "  var b = 1\n" +
          "  for i in 0..n {\n" +
          "    let next = a + b\n" +
          "    a = b\n" +
          "    b = next\n" +
          "  }\n" +
          "  a\n" +
          "}\n\n" +
          "fn main() uses io {\n" +
          '  print("fib(10) = " + str(fib(10)) + "\\n")\n' +
          "}"
      },
      {
        label: "Type error",
        input: 'fn main() {\n  let bad: int = "wrong"\n}'
      }
    ]
  });
})();
