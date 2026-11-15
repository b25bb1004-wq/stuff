(() => {
  "use strict";

  window.DemoApp.mount({
    id: "indium",
    name: "Indium",
    source: "crates/indium",
    about:
      "Widgets, layout, text, animation. What a polished surface gives back. Parses an indented widget tree, runs flex layout, advances focus, and paints the focus ring.",
    help:
      "column | row | text \"...\" | button \"...\" | list N\n" +
      "focus N  (0-based focusable widget index)",
    examples: [
      {
        label: "Toolbar + list with focus ring (default)",
        input:
          "column\n" +
          "  text \"Indium GUI Toolkit\"\n" +
          "  row\n" +
          "    button \"Open\"\n" +
          "    button \"Save\"\n" +
          "    button \"Run\"\n" +
          "  list 3\n" +
          "focus 1"
      },
      {
        label: "Nested split columns",
        input:
          "row\n" +
          "  column\n" +
          "    text \"Sidebar\"\n" +
          "    list 4\n" +
          "  column\n" +
          "    text \"Inspector\"\n" +
          "    button \"Apply\"\n" +
          "    button \"Reset\"\n" +
          "focus 4"
      },
      {
        label: "Focus out of range (error)",
        input:
          "column\n" +
          "  button \"Only\"\n" +
          "focus 5"
      }
    ]
  });
})();
