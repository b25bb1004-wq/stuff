// Behaviour check for crates/site/web/feed.js (T130b). Node 18+, no npm packages.
// Loads the neon wasm through the kit loader (crates/neon/web/kit/wasm.js), boots
// target/athena-web/titan.elf, starts Feed, sends `ls`, pumps Titan and prints the emitted events.
// Exit 1 when there is no uart event, no block event, or an event names an id not in systems.js.
// Build the wasm first: CARGO_TARGET_DIR=<dir> cargo build -q --release -p neon --lib --target wasm32-unknown-unknown
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const targetDir = process.env.CARGO_TARGET_DIR || "C:/Users/ADMIN/strata-gate-target/h-130b";
const neonBytes = readFileSync(join(targetDir, "wasm32-unknown-unknown", "release", "neon.wasm"));
const elfBytes = new Uint8Array(readFileSync(join(root, "target", "athena-web", "titan.elf")));
const web = (name) => readFileSync(join(root, "crates", "site", "web", name), "utf8");
const kit = readFileSync(join(root, "crates", "neon", "web", "kit", "wasm.js"), "utf8");

// Motion stand-in: the harness runs the frame jobs itself, once after each pump step.
const frameJobs = [];
const events = [];
const sandbox = {
  console,
  TextEncoder,
  TextDecoder,
  WebAssembly,
  Response,
  setTimeout,
  clearTimeout,
  fetch: async () => new Response(neonBytes, { status: 200, headers: { "content-type": "application/wasm" } }),
  Motion: {
    reduced: () => false,
    tween(_length, _curve, onFrame) {
      const job = { onFrame, cancel: () => frameJobs.splice(frameJobs.indexOf(job), 1) };
      frameJobs.push(job);
      return job;
    },
  },
};
sandbox.window = sandbox;
const ctx = vm.createContext(sandbox);
vm.runInContext(kit, ctx, { filename: "wasm.js" });
vm.runInContext(web("systems.js"), ctx, { filename: "systems.js" });
vm.runInContext(web("feed.js"), ctx, { filename: "feed.js" });

const api = await ctx.Neon.load("neon.wasm");
const rc0 = api.withBytes(elfBytes, (ptr, len) => api.exports.titan_boot(ptr, len));
if (rc0 !== 0) throw new Error(`titan_boot failed: ${api.lastText()}`);

const stop = ctx.Feed.start(api, (kind, from, to, count) => events.push({ kind, from, to, count }));
api.withBytes(api.encode("ls"), (ptr, len) => api.exports.titan_send(ptr, len));

// Pump like the terminal does: 2000000 steps per frame, feed job after each.
let rc = 0;
let rounds = 0;
for (rounds = 1; rounds <= 400; rounds++) {
  rc = api.exports.titan_pump(2000000n);
  for (const job of frameJobs.slice()) job.onFrame(1);
  if (rounds >= 3 && (rc === 0 || rc >= 2)) break;
}
stop();

const ids = new Set(ctx.MONOLITH_SYSTEMS.map((s) => s.id));
const unknown = events.filter((e) => !ids.has(e.from) || !ids.has(e.to));
const uartEvents = events.filter((e) => e.kind === "uart").length;
const blockEvents = events.filter((e) => e.kind.startsWith("block")).length;
const probeLines = api.lastText(api.exports.titan_probe()).split("\n").filter(Boolean).length;

const byKey = new Map();
for (const e of events) {
  const key = `${e.kind} ${e.from}->${e.to}`;
  const row = byKey.get(key) || { n: 0, total: 0 };
  row.n += 1;
  row.total += e.count;
  byKey.set(key, row);
}

console.log(`feed-check: rounds=${rounds} last_rc=${rc} probe_lines=${probeLines} emits=${events.length}`);
for (const [key, row] of byKey) {
  console.log(`  ${key}: emits=${row.n} count=${row.total}`);
}
console.log(`  unknown ids: ${unknown.length}; uart events: ${uartEvents}; block events: ${blockEvents}`);
const pass = uartEvents >= 1 && blockEvents >= 1 && unknown.length === 0;
console.log(pass ? "PASS" : "FAIL");
process.exitCode = pass ? 0 : 1;
