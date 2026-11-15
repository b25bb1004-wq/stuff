# Front v1: look and motion of the Monolith site and the Athena IDE

v1, written by the leader on 2026-11-16 (D6). Covers T72 (Athena), T75 (site), T76 (pass). Short on
purpose: workers follow it exactly, the leader changes it.

## Rules for every front page
- Plain HTML, CSS and JS. No frameworks, no npm, no CDN, no web fonts, no request to any other host:
  every file the page loads is served next to it. The wasm comes from `neon` (and Athena's own crate).
- Works at 360 px wide with no horizontal scroll, and at 1440 px. Keyboard reachable, visible focus ring,
  labels on every control, contrast at least 4.5:1 for text.
- `prefers-reduced-motion: reduce` turns every animation into an instant change. `prefers-color-scheme`
  picks dark (default) or light; a `data-theme` attribute on `<html>` overrides it.
- Long work never blocks input: Titan and Logos run in `requestAnimationFrame` slices (as `neon.js` does).

## Tokens (the only colours, sizes and timings allowed; CSS custom properties on `:root`)
| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg` | `#0e1013` | `#f4f2ee` | page (basalt / limestone) |
| `--panel` | `#161a1f` | `#ffffff` | panes, cards |
| `--line` | `#262c34` | `#d9d4ca` | borders, dividers |
| `--text` | `#e6e3dc` | `#1b1d21` | body text |
| `--muted` | `#9aa1ab` | `#5c6168` | secondary text |
| `--accent` | `#e0a458` | `#9a5b12` | one accent (amber, like a cut mineral); links, focus, running state |
| `--ok` / `--bad` | `#7cc49a` / `#ef7b7b` | `#1f7a47` / `#b3261e` | pass / fail only |

- Type: `--font-ui: system-ui, sans-serif`; `--font-code: ui-monospace, "Cascadia Code", Consolas, monospace`.
  Scale 13 / 15 / 18 / 24 / 36 / 56 px. Code is 13 px on mobile, 14 px on desktop.
- Space: multiples of 4 px. Radius 6 px on panes, 4 px on controls. No shadows except focus.
- Motion: `--t-fast: 120ms` (hover, press), `--t-med: 240ms` (panels, tabs), `--t-slow: 600ms` (site
  reveals). Easing `cubic-bezier(.2,.7,.2,1)`. Animate only `opacity` and `transform`.

## Athena (T72): the workshop (layout replaced by `briefs/T85.md`: VS Code-style workbench, PWA)
- Layout (desktop): top bar (name, Run, Test, theme toggle) / left file tree 220 px / editor / right or
  bottom output pane with tabs `Output`, `Tests`, `Terminal`. Panes resize with a drag handle (keyboard:
  focus the handle, arrows). Mobile: one column, a tab strip switches Files / Editor / Output / Terminal.
- Editor: a `<textarea>` over a highlighted `<pre>` that mirrors it (same font metrics, scroll synced),
  line numbers in a gutter, Tab inserts 4 spaces, Ctrl/Cmd+Enter runs, Ctrl/Cmd+S saves. Highlight colours
  come from calliope's `Style` classes mapped to tokens (keyword `--accent`, string `--ok`, comment `--muted`).
- Diagnostics: an `error: line:col: message` underline (wavy, `--bad`) on that span and the line in Output;
  clicking it moves the caret there.

## Site (T75): one page per idea, scroll tells the story
- A single long page: hero ("Monolith: 25 systems, one machine, zero dependencies"), then one section per
  system in build order (the 25 in `docs/SPEC.md`), each with name, one line, a small live demo or a
  recorded transcript when a live one is too heavy, and a link to its source folder.
- Scroll motion: a section's title and demo fade and rise 16 px as they enter (IntersectionObserver adds a
  class; CSS does the motion). One pinned element only: the layer diagram in the hero, whose layers light up
  as you scroll past their sections. No scroll-jacking, no parallax on text.
- Live demos reuse `neon.wasm` (Titan terminal, Logos playground) and Athena; nothing loads until its
  section is near the viewport.
