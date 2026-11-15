// Athena's motion (docs/PLAN-NEXT.md 4.5), on the kit's global `Motion`. Only opacity and transform move.
// Panels, tabs and the palette use the `firm` spring; the Run line sweeps the editor's top edge and lands a tick per result.

/** A duration token (for example `--t-press`) as milliseconds. */
export function tokenMs(name) {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

/** Shows `el` (fading in on `firm` when it was hidden) or hides it at once. */
export function showEl(el, show) {
  if (!show) {
    el.hidden = true;
    return;
  }
  if (!el.hidden) return;
  el.hidden = false;
  Motion.enter(el, { curve: "firm", dist: 1 });
}

/** Plays the exit on `el`, then calls `hide` unless `keep()` says it was reopened meanwhile. */
export function leave(el, keep, hide) {
  const h = Motion.exit(el, { curve: "firm", dist: 1 });
  h.done.then(() => {
    // The exit's held end state (or reduced motion's inline opacity) would hide the next enter.
    if (h.anim) h.anim.cancel();
    el.style.opacity = "";
    if (!keep()) hide();
  });
}

/** Runs `fn` after the browser has painted the state set just before. */
export function afterPaint(fn) {
  Motion.tween(tokenMs("--t-press"), "out", (e) => {
    if (e >= 1) fn();
  });
}

/**
 * The Run line for the editor area `host`: `start()` sweeps a hairline across the top edge,
 * `land(text)` drops one tick for the compile result and one for the run result of `logos_run`.
 */
export function createRunLine(host) {
  const hair = host.querySelector(".run-hair");
  const ticks = host.querySelector(".run-ticks");
  let runId = 0;
  let sweep = null;
  let landed = 0;

  return {
    start() {
      const id = ++runId;
      if (sweep) sweep.cancel();
      ticks.replaceChildren();
      ticks.hidden = false;
      hair.hidden = false;
      hair.style.transform = "scaleX(0)";
      landed = 0;
      sweep = Motion.tween(tokenMs("--t-large"), "firm", (k) => {
        hair.style.transform = `scaleX(${k})`;
      });
      sweep.done.then(() => {
        if (id !== runId) return;
        leave(hair, () => id !== runId, () => { hair.hidden = true; });
        leave(ticks, () => id !== runId, () => { ticks.hidden = true; });
      });
    },
    land(text) {
      const compiled = !text.startsWith("error: ");
      tick(!compiled);
      if (compiled) tick(text.startsWith("trap: "));
    },
  };

  function tick(bad) {
    landed += 1;
    const t = document.createElement("span");
    t.className = bad ? "run-tick is-bad" : "run-tick";
    t.style.left = `${Math.min(100, landed * 50)}%`;
    ticks.appendChild(t);
    Motion.enter(t, { curve: "firm", dist: 1 });
  }
}
