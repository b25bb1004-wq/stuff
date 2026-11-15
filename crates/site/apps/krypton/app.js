(() => {
  "use strict";

  window.DemoApp.mount({
    id: "krypton",
    name: "Krypton",
    source: "crates/krypton",
    about:
      "POSIX-style shell. The outermost shell of the planet. Parses shell command lines into an AST and evaluates parameter, tilde, quote, and arithmetic expansions.",
    help:
      "krypton [SCRIPT or -c CMD]\n" +
      "Interactive and scriptable POSIX-style shell.",
    examples: [
      {
        label: "Word expansion (default)",
        input: 'echo $X "$HOME/notes" $((X * 6)) ${UNSET:-fallback}'
      },
      {
        label: "Pipeline & redirect",
        input: 'A=3; cat < input.txt | grep monolith > out.txt && echo ~/done $((A + 4))'
      },
      {
        label: "Unterminated quote (error)",
        input: 'echo "unterminated'
      }
    ]
  });
})();
