(() => {
  "use strict";

  const LOGOS_FUEL = 1000000n;

  window.DemoApp.mount({
    id: "logos",
    name: "Logos",
    source: "crates/logos",
    about:
      "Lexer, parser, type checker, IR. Logos splits into perfect thin layers, like its effect system. Interactive playground for writing, type-checking, and running Logos programs in WebAssembly.",
    help:
      "logos check FILE.lg\n" +
      "logos fmt FILE.lg\n" +
      "logos run FILE.lg\n" +
      "logos test FILE.lg\n" +
      "(Binary lives in crates/cobalt.)",
    examples: [
      {
        label: "Hello (default)",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn main() uses io {\n" +
          '  print("hello from logos\\n")\n' +
          "}\n"
      },
      {
        label: "Sum of squares",
        input:
          'fn print(s: str) uses io = extern "io.print"\n\n' +
          "fn main() uses io {\n" +
          "  var sum = 0\n" +
          "  for i in 1..11 {\n" +
          "    sum = sum + i * i\n" +
          "  }\n" +
          '  print("sum of squares 1..10 = " + str(sum) + "\\n")\n' +
          "}\n"
      },
      {
        label: "Type error",
        input: 'fn main() {\n  let x: int = "not an int"\n}\n'
      }
    ],
    run(wasm, source) {
      return new Promise((resolve) => {
        requestAnimationFrame(() => {
          const bytes = wasm.encoder.encode(source);
          const len = wasm.withGuestBytes(bytes, (ptr, bLen) =>
            wasm.exports.logos_run(ptr, bLen, LOGOS_FUEL)
          );
          const text = wasm.readNeonLast(len);
          if (text.startsWith("parse error") || text.startsWith("check error") || text.startsWith("error:")) {
            resolve({ kind: "error", text });
          } else {
            resolve({ kind: "text", text });
          }
        });
      });
    }
  });
})();
