# T131 Monolith site v3: an Apple-style product page per product (decided 2026-11-16)

Operator, 2026-11-16: "these arent interconnected, these are all different products"; picked "one stage, one
product"; then: the UI "is so old and ai slop ... create a new better version, something like apple, or meta".
The 25 items of the build-your-own-X list (docs/SPEC.md) are 25 separate products, each its own app and CLI
(docs/APPS.md). Never draw wires between products, never say "one machine". The PCB board look is retired.

Vault design rules applied: ui-ux-pro-max pre-delivery checklist (palette rejected as generic), agency-agents
design-ui-designer.md and design-ui-finish-gate-reviewer.md (C:\Users\ADMIN\agent-tools\agency-agents\design\).

## Tokens
- Light (default): --page #FBFBFD, --ink #1D1D1F, --ink-2 #6E6E73, --line #D2D2D7, --surface #FFFFFF
- Dark chapters and dark mode: --page #000000, --ink #F5F5F7, --ink-2 #86868B, --line #333336, --surface #1D1D1F
- Each product owns one hue (products.js `hue`, HSL, chosen so all 25 are distinct and pass 4.5:1 on both
  pages). The hue appears only in that product's chapter: its device-frame glow, its primary button, its icon.
- Type: system only (no font URLs): -apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI Variable Display",
  "Segoe UI", system-ui, sans-serif. Mono: ui-monospace, "SF Mono", "Cascadia Mono", Consolas, monospace.
  Scale: hero 96/88 px (clamp to 48 on phones), weight 600, tracking -0.02em; chapter title 64; lead 28/400;
  body 19/1.5; captions 14. Sentence case. No ALL-CAPS labels, no numbering, no arrows on buttons, no emoji.
- Radius: device frame 22px, buttons pill (980px), nothing else rounded. No drop shadows except the
  device-frame glow (product hue, 0 30px 80px, 25% alpha).

## Page
1. Global nav, 44 px, sticky, translucent (backdrop-filter blur 20px, saturate 180%): "Monolith" left; center the
   four groups (Systems, Languages and tools, Data and network, Graphics), each opening a flyout of its products
   with icon + name; theme toggle right.
2. Hero, 100svh, light: "Monolith" 96 px; lead "25 products, each built from scratch. Every one runs right here in
   your browser and ships as its own command-line tool."; two pill buttons "Explore the products" and "Open
   Titan". Below, a strip of the 25 product icons (pigment icons from the pack) that scrolls slowly sideways
   (paused on hover and under reduced motion).
3. One chapter per product, in SPEC order, alternating light and dark pages. Each chapter is 160svh with a sticky
   inner scene: left column name (64 px), what it is ("Database"), one-line pitch, three short feature lines from
   APPS.md, the CLI line in mono (`<cli> --help`), buttons "Open app" (/<id>/) and "Run demo". Right: a device
   frame (window chrome with three dots and the product name) holding that product's LIVE demo (reuse site.js
   per-system renderers), which auto-runs when the chapter is 50% in view, once.
   Scroll motion (the one orchestrated moment, via the kit motion engine, scroll-linked): the frame scales from
   0.9 to 1 and its glow fades in as the chapter enters; text lines rise 24 px in order. Reduced motion: static.
4. Footer: plain list of all 25 with links, the repo link, "Built with zero external crates".
- Phones (375 px): nav collapses to a menu button opening a full-screen sheet of groups; chapters stack text then
  frame; no sticky scenes; no horizontal scroll.
- Deep link #<id> scrolls to that chapter. Titan's chapter frame is its live terminal.

## Data contract: crates/site/web/products.js
window.PRODUCTS = [{ id, name, kind ("Database"), group ("systems"|"tools"|"data"|"graphics"), hue (0-359),
  pitch (one line, plain, from the user's view), features: [3 short lines], cli ("iridium sql DB.idb"), app: true|false }]
in SPEC table order, 25 entries. Groups: systems = titan, lithium, zeus, kairos, midas, hades, silicon, pandora;
tools = logos, cobalt, elysium, athena, calliope, chronos, argo, krypton; data = iridium, mercury, xenon, aurum,
sphinx, odyssey; graphics = helios, iris, indium.
