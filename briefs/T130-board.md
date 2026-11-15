# T130 Monolith site redesign: the live board (decided 2026-11-16)

Operator: current UI "feels like ai slop"; wants "modern, crazy, dynamic". Direction chosen: circuit board.

## Idea
The home page IS a printed circuit board, and it is alive. Each of the 25 systems is a chip on the board,
placed by layer (hardware at the bottom, kernel, services, apps at the top). Copper traces join chips that
really depend on each other (systems.js deps). Titan boots in WebAssembly in the background; every real
event it reports (titan_probe deltas: ecalls, block reads, uart bytes, traps) fires a light pulse along the
trace between the chips involved. Nothing on the board moves unless the machine did something.

## Tokens (CSS custom properties, in the site, not the shared kit)
- --mask #0B3D2E solder mask (page background, dark)
- --mask-deep #072A20 (chip bodies, pressed areas)
- --copper #C8803C traces and pads at rest
- --signal #FFC46B pulse head and focus ring
- --silk #EEF0E6 silkscreen text and outlines
- --silk-dim #9DB5A8 secondary text
- Light mode = bare FR4 board: --mask #D8CFA4, --mask-deep #C4B98A, --copper #9A5B22, --signal #D9480F, --silk #1F2A1F, --silk-dim #4B5A45
- Type: system stack only (no font URLs, the kit lint bans them).
  Display and labels: "Bahnschrift", "DIN Alternate", "Arial Narrow", sans-serif (condensed, like silkscreen).
  Terminal and code: ui-monospace, "Cascadia Mono", Consolas, monospace.
- Sentence case. No ALL-CAPS eyebrows, no 01-25 numbering, no middle-dot meta strings, no arrows on buttons.
- Corners: chips 3px, pads round, panels 0. No drop shadows; depth comes from mask-deep and copper edges.

## Layout
1. Hero, 100svh: the full board on one canvas. Center chip = Titan; its die is a live terminal
   (focus it and type). Headline in silkscreen at the board's top-left corner:
   "Monolith" / "25 systems written from scratch, running in your browser". One button: "Open the terminal".
2. Hover or focus a chip: its traces light, its name and one-line purpose appear as silkscreen beside it.
3. Click or Enter a chip: the camera zooms into it (one transform, firm spring from the kit); the chip's demo
   pane opens inside the die, runs its example on its own, and shows "Run again" and "Open app".
   Esc or "Back to board" zooms out. URL hash #<id> deep-links a chip.
4. Below the board: a short plain list of all 25 systems for keyboard and screen-reader use (also the no-JS view).

## Motion rules
- Only real events move things. Pulses: copper to --signal head, about 400 ms per trace, max 24 live pulses.
- Reduced motion: no pulses travel; a chip that had an event blinks its pad once. Zoom becomes a cut.
- One rAF loop owned by the kit motion engine (kit lint bans raw requestAnimationFrame in pages);
  the loop sleeps when no pulse is live and the tab is hidden.
- 360 px wide: board scales to fit, chips stay tappable (min 44 px hit area), list view below.

## Split
- T130a board.js: canvas renderer + layout + pulse engine + hit testing + zoom camera. Standalone demo page.
- T130b feed.js: boot Titan from the kit wasm loader, poll titan_probe, diff counters, emit board events.
- T130c index.html + style.css + site.js: tokens, hero, chip pane, list view; wire a and b; remove the cards.
