(() => {
  "use strict";

  window.DemoApp.mount({
    id: "sphinx",
    name: "Sphinx",
    source: "crates/sphinx",
    about:
      "Sphinx: standard primitives proven on vectors. Medusa: our own experimental cipher, never the default. Computes SHA-256, SHA-512, HMAC-SHA256, ChaCha20 round-trips, and Ed25519 signatures.",
    help:
      "sphinx hash [FILE]\n" +
      "sphinx hmac KEY [FILE]\n" +
      "sphinx keygen\n" +
      "sphinx sign KEY [FILE]\n" +
      "sphinx verify PUB SIG [FILE]\n" +
      "sphinx seal KEY [FILE]\n" +
      "sphinx open KEY [FILE]",
    examples: [
      {
        label: "NIST vector abc (default)",
        input: "abc"
      },
      {
        label: "Structured message",
        input: "Monolith zero-dependency cryptography: SHA-256, ChaCha20, Ed25519."
      },
      {
        label: "Over 64 KiB cap (error)",
        input: "x".repeat(65537)
      }
    ]
  });
})();
