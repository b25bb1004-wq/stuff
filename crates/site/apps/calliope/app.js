(() => {
  "use strict";

  window.DemoApp.mount({
    id: "calliope",
    name: "Calliope",
    source: "crates/calliope",
    about:
      "Rope-based modal editor. The first thing people wrote on. Compiles linear-time regular expressions and reports all matched byte/line/column spans.",
    help:
      "calliope grep REGEX FILE...\n" +
      "calliope edit SCRIPT FILE",
    examples: [
      {
        label: "Function calls (default)",
        input:
          "[A-Za-z_][A-Za-z0-9_]*\\([0-9]+\\)\n" +
          "fn main() {\n" +
          "  let a = calc(42)\n" +
          "  let b = step(7) + calc(108)\n" +
          "}"
      },
      {
        label: "Hex literals",
        input:
          "0x[0-9a-fA-F]+\n" +
          "const UART = 0x10000000\n" +
          "const MAGIC = 0xdeadBEEF\n" +
          "let bad = 1234"
      },
      {
        label: "Unclosed class (error)",
        input: "[A-Z\nsample text"
      }
    ]
  });
})();
