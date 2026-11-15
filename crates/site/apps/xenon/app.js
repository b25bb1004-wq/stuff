(() => {
  "use strict";

  window.DemoApp.mount({
    id: "xenon",
    name: "Xenon",
    source: "crates/xenon",
    about:
      "Encrypted reliable transport over UDP. Signals that travel through the earth. Runs sans-I/O client and server state machines over a simulated lossy, delayed link.",
    help:
      "loss <0..100>\n" +
      "delay <ms>\n" +
      "send <text>",
    examples: [
      {
        label: "20% loss, 3 streams (default)",
        input:
          "loss 20\n" +
          "delay 20\n" +
          "send hello xenon\n" +
          "send reliable over udp\n" +
          "send encrypted frames\n" +
          "send out of order recovery\n" +
          "send verified stream"
      },
      {
        label: "Heavy 45% loss, high latency",
        input:
          "loss 45\n" +
          "delay 80\n" +
          "send packet one\n" +
          "send packet two"
      },
      {
        label: "100% packet loss (error)",
        input:
          "loss 100\n" +
          "delay 10\n" +
          "send unreachable"
      }
    ]
  });
})();
