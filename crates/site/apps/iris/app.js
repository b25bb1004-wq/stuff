(() => {
  "use strict";

  window.DemoApp.mount({
    id: "iris",
    name: "Iris",
    source: "crates/iris",
    about:
      "Compositing window manager. Each window is one face of the cut stone. Replays desktop commands across Terminal, Editor, and Files windows on a shared Hades filesystem.",
    help:
      "open terminal|editor|files [ARG] [W H]\n" +
      "tile [on|off] | float | move [ID] X Y | focus [ID|next|prev]\n" +
      "text STR | key NAME | wait MS | close [ID]",
    examples: [
      {
        label: "Tiled terminal + editor (default)",
        input:
          "open terminal 300 180\n" +
          "text echo hello iris\n" +
          "key enter\n" +
          "open editor /note.txt 280 180\n" +
          "tile on\n" +
          "wait 180"
      },
      {
        label: "Floating windows + file browser",
        input:
          "open files / 260 190\n" +
          "open terminal 300 180\n" +
          "move 2 290 70\n" +
          "text ls /\n" +
          "key enter\n" +
          "wait 200"
      },
      {
        label: "Unknown command (error)",
        input:
          "open terminal\n" +
          "warp 99"
      }
    ]
  });
})();
