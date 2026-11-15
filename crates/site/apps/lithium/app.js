(() => {
  "use strict";

  window.DemoApp.mount({
    id: "lithium",
    name: "Lithium",
    source: "machine/lithium",
    about:
      "A small kernel in pure RV64 assembly. Small, hard, makes the first spark. Boots the Lithium assembly kernel on Silicon for a bounded step budget and captures UART output.",
    help:
      "lithium run [--ticks N]\n" +
      "Runs the Lithium RV64 assembly kernel on Silicon.",
    examples: [
      {
        label: "2M steps (default)",
        input: "2000000"
      },
      {
        label: "Short 50k slice",
        input: "50000"
      },
      {
        label: "Zero steps (error)",
        input: "0"
      }
    ]
  });
})();
