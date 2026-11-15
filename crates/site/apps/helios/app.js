(() => {
  "use strict";

  window.DemoApp.mount({
    id: "helios",
    name: "Helios",
    source: "crates/helios",
    about:
      "ECS, software renderer, physics, audio. The thing that moves worlds. Compiles Logos game scripts onto the Elysium VM and renders 320x240 frames.",
    help:
      "ticks N\n" +
      "<Logos script with fn update() and extern game.* calls>",
    examples: [
      {
        label: "Prism Drift (default)",
        input:
          "ticks 30\n" +
          "fn g_frame() -> int = extern \"game.frame\"\n" +
          "fn g_get(i: int) -> int = extern \"game.get\"\n" +
          "fn g_set(i: int, v: int) = extern \"game.set\"\n" +
          "fn g_rand(n: int) -> int = extern \"game.rand\"\n" +
          "fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern \"game.rect\"\n\n" +
          "fn init() {\n" +
          "    g_set(0, 152)\n" +
          "    g_set(1, 196)\n" +
          "    for i in 0..4 {\n" +
          "        g_set(10 + i * 2, 24 + i * 74)\n" +
          "        g_set(11 + i * 2, i * 32)\n" +
          "    }\n" +
          "}\n\n" +
          "fn update() {\n" +
          "    let f = g_frame()\n" +
          "    g_rect(0, 0, 320, 240, 0x090d1a)\n" +
          "    for s in 0..6 {\n" +
          "        let sx = (s * 53 + 17) % 316\n" +
          "        let sy = (s * 41 + f * (1 + (s % 2))) % 240\n" +
          "        g_rect(sx, sy, 2, 2, 0x334155)\n" +
          "    }\n" +
          "    for i in 0..4 {\n" +
          "        let mx = g_get(10 + i * 2)\n" +
          "        let my = (g_get(11 + i * 2) + 3) % 240\n" +
          "        g_set(11 + i * 2, my)\n" +
          "        g_rect(mx, my, 14, 14, 0xef4444)\n" +
          "    }\n" +
          "    let sx = 40 + (g_rand(1) + f * 3) % 240\n" +
          "    g_rect(sx, 110, 10, 10, 0x22c55e)\n" +
          "    let px = g_get(0) + ((f / 4) % 3) - 1\n" +
          "    let py = g_get(1)\n" +
          "    g_rect(px, py, 16, 12, 0x38bdf8)\n" +
          "    g_rect(px + 5, py - 3, 6, 4, 0xe0f2fe)\n" +
          "    g_rect(0, 0, 320, 14, 0x1e293b)\n" +
          "    g_rect(6, 4, min(180, f * 4), 6, 0x38bdf8)\n" +
          "}"
      },
      {
        label: "Bouncing block",
        input:
          "ticks 45\n" +
          "fn g_frame() -> int = extern \"game.frame\"\n" +
          "fn g_rect(x: int, y: int, w: int, h: int, c: int) = extern \"game.rect\"\n\n" +
          "fn update() {\n" +
          "    let f = g_frame()\n" +
          "    g_rect(0, 0, 320, 240, 0x111827)\n" +
          "    g_rect(20, 200, 280, 10, 0x475569)\n" +
          "    let bx = 24 + (f * 5) % 248\n" +
          "    let by = 176 - ((f * 7) % 120)\n" +
          "    g_rect(bx, by, 24, 24, 0xf59e0b)\n" +
          "    g_rect(bx + 4, by + 4, 8, 8, 0xfef3c7)\n" +
          "}"
      },
      {
        label: "Missing update() (error)",
        input:
          "ticks 10\n" +
          "fn main() -> int { 42 }"
      }
    ]
  });
})();
