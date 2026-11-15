(() => {
  "use strict";

  window.DemoApp.mount({
    id: "mercury",
    name: "Mercury",
    source: "crates/mercury",
    about:
      "HTTP/1.1 server on our own thread pool. Where the rock breaks the surface and the world can see it. Parses raw HTTP/1.1 requests with strict limits and serializes canonical responses.",
    help:
      "mercury parse FILE\n" +
      "mercury serve DIR [--listen ADDR]\n" +
      "mercury get URL",
    examples: [
      {
        label: "POST JSON/text (default)",
        input:
          "POST /api/layers HTTP/1.1\n" +
          "Host: monolith.local\n" +
          "Content-Type: text/plain\n" +
          "Content-Length: 13\n\n" +
          "hello mercury"
      },
      {
        label: "GET HTTP/1.0 close",
        input:
          "GET /status?verbose=1 HTTP/1.0\n" +
          "Host: monolith.local\n" +
          "User-Agent: Monolith/0.1\n\n"
      },
      {
        label: "Malformed request (error)",
        input:
          "BAD_REQUEST_LINE\n" +
          "Host: monolith.local\n\n"
      }
    ]
  });
})();
