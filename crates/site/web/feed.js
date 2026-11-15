// Feed: turns titan_probe counter deltas into board events. Global `Feed` (window.Feed).
//
// start(titanApi, emit) -> stop
//   titanApi: the Neon.load() result for the neon wasm (has .exports and .lastText).
//   emit(kind, fromId, toId, count): called with ids from systems.js.
//   Feed does not pump Titan; the site's terminal pump does that. Feed only reads counters.
//
// Mapping (titan_probe name -> kind, from, to). Ids are the systems.js ids.
//   ecall.count            ecall        titan   -> silicon   Titan services ECALLs run by the CPU
//   exception.NN           trap         silicon -> titan     traps raised by the CPU (summed)
//   interrupt.NN           trap         silicon -> titan     interrupts raised by the CPU (summed)
//   uart.out               uart         titan   -> iris      console output to the terminal
//   uart.in                uart-in      iris    -> titan     keys typed into the terminal
//   block.reads            block-read   titan   -> hades     block reads for the file system
//   block.writes           block-write  titan   -> hades     block writes for the file system
//   anything else          ignored
//
// Each tick: parse "name value" lines, diff against the last tick, sum deltas per (kind, from, to).
// A counter that goes down (a seek rewinds device counters) is a reset, not an event.
// The first tick after start only takes a baseline. At most 8 emits per tick; groups past the
// eighth are added to the count of the eighth.
// Timing: one Motion.tween(Infinity) job on the kit frame loop. With reduced motion there is no
// frame loop, so it polls on a setTimeout chain instead (no setInterval).
(function () {
  "use strict";

  const FIXED = {
    "ecall.count": ["ecall", "titan", "silicon"],
    "uart.out": ["uart", "titan", "iris"],
    "uart.in": ["uart-in", "iris", "titan"],
    "block.reads": ["block-read", "titan", "hades"],
    "block.writes": ["block-write", "titan", "hades"],
  };
  const TRAP = ["trap", "silicon", "titan"];
  const MAX_EMITS = 8;
  const FALLBACK_MS = 100;

  function routeOf(name) {
    if (Object.prototype.hasOwnProperty.call(FIXED, name)) return FIXED[name];
    if (/^(exception|interrupt)\.\d+$/.test(name)) return TRAP;
    return null;
  }

  function parse(text) {
    const counters = new Map();
    for (const line of text.split("\n")) {
      const parts = line.split(" ");
      if (parts.length !== 2) continue;
      const value = Number(parts[1]);
      if (Number.isFinite(value)) counters.set(parts[0], value);
    }
    return counters;
  }

  function tick(titanApi, state, emit) {
    const len = titanApi.exports.titan_probe();
    const now = parse(len > 0 ? titanApi.lastText(len) : "");
    if (state.prev === null) {
      state.prev = now;
      return;
    }
    const groups = new Map();
    for (const [name, value] of now) {
      const route = routeOf(name);
      if (route === null) continue;
      const delta = value - (state.prev.get(name) || 0);
      if (delta <= 0) continue;
      const key = route.join("|");
      const g = groups.get(key) || { kind: route[0], from: route[1], to: route[2], count: 0 };
      g.count += delta;
      groups.set(key, g);
    }
    state.prev = now;
    const list = Array.from(groups.values());
    const shown = list.slice(0, MAX_EMITS);
    for (const extra of list.slice(MAX_EMITS)) {
      shown[MAX_EMITS - 1].count += extra.count;
    }
    for (const g of shown) {
      emit(g.kind, g.from, g.to, g.count);
    }
  }

  function start(titanApi, emit) {
    const state = { prev: null };
    let stopped = false;
    let timer = 0;
    let job = null;
    const step = () => {
      if (!stopped) tick(titanApi, state, emit);
    };
    if (window.Motion && !window.Motion.reduced()) {
      job = window.Motion.tween(Infinity, "out", step);
    } else {
      const loop = () => {
        step();
        if (!stopped) timer = setTimeout(loop, FALLBACK_MS);
      };
      loop();
    }
    return function stop() {
      stopped = true;
      if (job) job.cancel();
      clearTimeout(timer);
    };
  }

  window.Feed = { start };
})();
