(() => {
  "use strict";

  window.DemoApp.mount({
    id: "odyssey",
    name: "Odyssey",
    source: "crates/odyssey",
    about:
      "HTML + CSS parser, layout, paint. Spelunking is exploring caves. Parses HTML5 + inline <style>, computes CSS cascade, lays out at 480 px, and software-paints pixels.",
    help:
      "<style>...</style>\n" +
      "<h1>Title</h1><p>Body</p>",
    examples: [
      {
        label: "Flex cards (default)",
        input:
          "<style>\n" +
          "  body { background: #f4f6f9; color: #1e293b; padding: 14px; font-size: 16px; }\n" +
          "  h1 { color: #0f172a; font-size: 24px; margin: 0 0 8px 0; }\n" +
          "  p { margin: 0 0 10px 0; }\n" +
          "  .row { display: flex; }\n" +
          "  .card { background: #ffffff; border: 2px solid #2563eb; color: #1d4ed8; padding: 8px; margin: 0 8px 0 0; }\n" +
          "</style>\n" +
          "<h1>Odyssey Browser</h1>\n" +
          "<p>HTML5 tokenizer, CSS cascade, block + flex layout, software paint.</p>\n" +
          "<div class=\"row\">\n" +
          "  <div class=\"card\">480px viewport</div>\n" +
          "  <div class=\"card\">Zero crates</div>\n" +
          "</div>"
      },
      {
        label: "Dark banner",
        input:
          "<style>\n" +
          "  body { background: #0f172a; color: #e2e8f0; padding: 16px; }\n" +
          "  .box { background: #1e293b; border: 3px solid #38bdf8; color: #f8fafc; padding: 12px; }\n" +
          "</style>\n" +
          "<div class=\"box\">\n" +
          "  <h2>Strata Render</h2>\n" +
          "  <p>Cascade, block boxes, and bitmap text rendered without a GPU.</p>\n" +
          "</div>"
      },
      {
        label: "Empty document (error)",
        input: ""
      }
    ]
  });
})();
