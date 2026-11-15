(() => {
  "use strict";

  window.DemoApp.mount({
    id: "aurum",
    name: "Aurum",
    source: "crates/aurum",
    about:
      "Proof-of-stake chain, contracts on Elysium, stored in Iridium. One unbroken vein of ore. Signs Ed25519 transfers, proposes blocks, and verifies RFC 6962 Merkle inclusion proofs.",
    help:
      "aurum keygen\n" +
      "aurum tx ...\n" +
      "aurum verify CHAIN\n" +
      "aurum merkle ...\n" +
      "aurum node",
    examples: [
      {
        label: "4 transfers (default)",
        input:
          "alice bob 100\n" +
          "alice carol 25\n" +
          "bob carol 40\n" +
          "carol alice 15"
      },
      {
        label: "Round-robin settlement",
        input:
          "bob alice 200\n" +
          "carol bob 150\n" +
          "alice carol 50"
      },
      {
        label: "Unknown account (error)",
        input: "alice mallory 50"
      }
    ]
  });
})();
