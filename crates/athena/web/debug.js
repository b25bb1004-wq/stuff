// Debugger UI: gutter breakpoints, the paused line, the floating toolbar and the Run and Debug view.
// It drives the wasm exports athena_debug_start/step/frame/stop. Each reply is text, one record per
// line (status, line, col, selected, message, output, frame, var); see reports/T74b.md.

const BP_KEY = "athena.breakpoints.v1";
const LINE_PX = 20;
const PAD_PX = 12;
const IDLE_TEXT = "Not running. Click the gutter to set a breakpoint, then press F5.";
const encoder = new TextEncoder();
const MODES = { "debug-continue": 0, "debug-step-over": 1, "debug-step-into": 2, "debug-step-out": 3 };

const $ = (id) => document.getElementById(id);
const unesc = (s) => s.replace(/\\(.)/g, (_m, c) => (c === "n" ? "\n" : c === "r" ? "\r" : c));

function parseState(text) {
  const st = { status: "error", line: 0, col: 0, selected: 0, message: "no reply from the debugger", output: "", frames: [], vars: [] };
  for (const raw of text.split("\n")) {
    const sp = raw.indexOf(" ");
    const key = sp < 0 ? raw : raw.slice(0, sp);
    const rest = sp < 0 ? "" : raw.slice(sp + 1);
    if (key === "status") st.status = rest;
    else if (key === "line" || key === "col" || key === "selected") st[key] = Number.parseInt(rest, 10) || 0;
    else if (key === "message" || key === "output") st[key] = unesc(rest);
    else if (key === "frame") {
      const parts = rest.split(" ");
      st.frames.push({ name: unesc(parts.slice(1, -1).join(" ")), line: Number.parseInt(parts[parts.length - 1], 10) || 0 });
    } else if (key === "var") {
      const j = rest.indexOf(" ");
      st.vars.push({ name: j < 0 ? rest : rest.slice(0, j), value: j < 0 ? "" : unesc(rest.slice(j + 1)) });
    }
  }
  return st;
}

function loadBreakpoints() {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(BP_KEY) || "null");
    const out = {};
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      for (const [file, lines] of Object.entries(parsed)) {
        if (!Array.isArray(lines)) continue;
        const clean = [...new Set(lines.filter((n) => Number.isInteger(n) && n > 0))].sort((a, b) => a - b);
        if (clean.length > 0) out[file] = clean;
      }
    }
    return out;
  } catch (_e) {
    return {};
  }
}

function mark(cls, y) {
  const node = document.createElement("div");
  node.className = cls;
  node.style.top = `${y}px`;
  return node;
}

function listItem(cls, text) {
  const li = document.createElement("li");
  li.className = cls;
  li.textContent = text;
  return li;
}

/**
 * Wires the debugger into the page. `deps` supplies: wasm() (the exports or null), withBytes(bytes, fn),
 * readAthenaLast(len), activeFile() (name or null) and setStatus(msg, isError).
 */
export function createDebugUi(deps) {
  const codeInput = $("code-input");
  const codeStack = codeInput.parentElement;
  const gutter = $("editor-gutter");
  const editorWrap = $("editor-wrap");
  const runOut = $("run-out");
  const toolbar = $("debug-toolbar");
  const varsList = $("debug-vars");
  const framesList = $("debug-frames");
  const summary = $("debug-summary");

  const band = document.createElement("div");
  band.className = "dbg-band";
  band.hidden = true;
  codeStack.appendChild(band);
  const marks = document.createElement("div");
  marks.className = "dbg-marks";
  editorWrap.appendChild(marks);

  let breakpoints = loadBreakpoints();
  let session = null; // { file, paused, running, timer, mode } while a program is being debugged
  let view = null; // the last paused state (its line is highlighted, its vars and frames are listed)

  function saveBreakpoints() {
    try {
      window.localStorage.setItem(BP_KEY, JSON.stringify(breakpoints));
    } catch (_e) {
      // Storage is blocked: the breakpoints last until the page reloads.
    }
  }

  function refresh() {
    const file = deps.activeFile();
    const top = (n) => (n - 1) * LINE_PX + PAD_PX;
    const live = session && session.paused && view && file === session.file ? view.line : 0;
    const nodes = (breakpoints[file] || []).map((n) => mark("dbg-dot", top(n) + 6 - gutter.scrollTop));
    if (live > 0) nodes.push(mark("dbg-arrow", top(live) + 5 - gutter.scrollTop));
    marks.style.width = `${gutter.offsetWidth}px`;
    marks.replaceChildren(...nodes);
    const y = live > 0 ? top(live) - codeInput.scrollTop : 0;
    const show = live > 0 && y + LINE_PX > 0 && y < codeInput.clientHeight;
    // A new line slides the bar (Motion.flip); scrolling alone just moves it.
    const slide = show && !band.hidden && band.dataset.line !== String(live);
    band.dataset.line = show ? String(live) : "";
    if (slide) Motion.flip(band, () => { band.style.top = `${y}px`; });
    else band.style.top = `${y}px`;
    band.hidden = !show;
    toolbar.hidden = !session;
  }

  function showOutput(st) {
    let text = st.output;
    if (st.status === "trapped") text += `${text ? "\n" : ""}trap: ${st.message}`;
    if (st.status === "error") text = `error: ${st.message}`;
    runOut.classList.toggle("is-error", st.status === "trapped" || st.status === "error");
    runOut.textContent = text || "(no output)";
  }

  function renderPanels(st) {
    const items = st.vars.map((v) => {
      const li = document.createElement("li");
      li.className = "debug-item";
      const name = document.createElement("span");
      name.className = "debug-name";
      name.textContent = v.name;
      const value = document.createElement("span");
      value.className = "debug-value";
      value.textContent = v.value;
      li.append(name, document.createTextNode(" = "), value);
      return li;
    });
    varsList.replaceChildren(...(items.length > 0 ? items : [listItem("debug-empty", session ? "No variables in this frame." : "Not running.")]));
    framesList.replaceChildren(
      ...st.frames.map((f, i) => {
        const li = document.createElement("li");
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "debug-frame";
        btn.setAttribute("aria-pressed", i === st.selected ? "true" : "false");
        btn.textContent = `${f.name}:${f.line}`;
        btn.addEventListener("click", () => selectFrame(i));
        li.appendChild(btn);
        return li;
      })
    );
  }

  function endSession(msg, isError) {
    const s = session;
    if (s) {
      if (s.timer) cancelAnimationFrame(s.timer);
      session = null;
      view = null;
      const w = deps.wasm();
      if (w) w.athena_debug_stop();
    }
    renderPanels({ vars: [], frames: [], selected: 0 });
    summary.textContent = IDLE_TEXT;
    refresh();
    if (msg) deps.setStatus(msg, Boolean(isError));
  }

  function apply(st) {
    showOutput(st);
    if (st.status === "paused") {
      session.paused = true;
      view = st;
      summary.textContent = `Paused at ${session.file}:${st.line}`;
      renderPanels(st);
      refresh();
      return;
    }
    if (st.status === "done") endSession("Program finished.", false);
    else if (st.status === "trapped") endSession(`Trapped: ${st.message}`, true);
    else endSession(st.message || "The debugger reported an error.", true);
  }

  // One bounded wasm step. A Budget reply means the program is still running: go on from the next frame,
  // so a long loop never blocks the page. The wasm side keeps the stepping anchor between calls.
  function drive(mode) {
    const s = session;
    const w = deps.wasm();
    if (!s || !w) return;
    s.paused = false;
    s.mode = mode;
    const bps = (breakpoints[s.file] || []).join("\n");
    const st = parseState(deps.readAthenaLast(deps.withBytes(encoder.encode(bps), (p, n) => w.athena_debug_step(mode, p, n))));
    if (st.status === "budget") {
      s.running = true;
      showOutput(st);
      summary.textContent = `Running ${s.file}…`;
      refresh();
      s.timer = requestAnimationFrame(() => {
        if (session === s) drive(s.mode);
      });
      return;
    }
    s.running = false;
    s.timer = 0;
    apply(st);
  }

  function startSession() {
    const w = deps.wasm();
    const file = deps.activeFile();
    if (!w) {
      deps.setStatus("The debugger starts when the wasm module is ready.", true);
      return false;
    }
    if (!file || !file.endsWith(".logos")) {
      deps.setStatus("Open a .logos file to debug.", true);
      return false;
    }
    endSession("", false);
    const text = deps.readAthenaLast(deps.withBytes(encoder.encode(codeInput.value), (p, n) => w.athena_debug_start(p, n)));
    const st = parseState(text);
    if (st.status === "error") {
      showOutput(st);
      deps.setStatus(st.message, true);
      return false;
    }
    session = { file, paused: false, running: false, timer: 0, mode: 0 };
    $("tab-output").click();
    apply(st);
    return session !== null;
  }

  function selectFrame(i) {
    const w = deps.wasm();
    if (!w || !session || !session.paused) return;
    const st = parseState(deps.readAthenaLast(w.athena_debug_frame(i)));
    if (st.status !== "paused") return;
    view = st;
    renderPanels(st);
  }

  /** Runs a palette command id (debug-continue, debug-step-over, debug-step-into, debug-step-out, debug-restart, debug-stop). */
  function command(id) {
    if (id === "debug-stop") {
      if (session) endSession("Debug session stopped.", false);
      return;
    }
    if (id === "debug-restart") {
      if (startSession()) drive(0);
      return;
    }
    if (!(id in MODES)) return;
    const fresh = !session;
    if (fresh && !startSession()) return;
    if (session.running) {
      deps.setStatus("The program is running. Press Shift+F5 to stop it.", false);
      return;
    }
    // A session that was just started is already paused before main: only Continue runs on from there.
    if (fresh && id !== "debug-continue") return;
    drive(MODES[id]);
  }

  // Breakpoints: a click on the gutter toggles the line of the active file.
  gutter.addEventListener("click", (ev) => {
    const file = deps.activeFile();
    if (!file) return;
    const y = ev.clientY - gutter.getBoundingClientRect().top + gutter.scrollTop - PAD_PX;
    if (y < 0) return;
    const line = Math.floor(y / LINE_PX) + 1;
    if (line > codeInput.value.split("\n").length) return;
    const set = new Set(breakpoints[file] || []);
    if (set.has(line)) set.delete(line);
    else set.add(line);
    if (set.size > 0) breakpoints[file] = [...set].sort((a, b) => a - b);
    else delete breakpoints[file];
    saveBreakpoints();
    refresh();
  });

  // Editing a paused (or running) program stops it: its source no longer matches the lines it runs.
  codeInput.addEventListener("input", () => {
    if (session) endSession("Debug session stopped: the file was edited while debugging.", true);
  });
  codeInput.addEventListener("scroll", refresh);

  $("dbg-continue").addEventListener("click", () => command("debug-continue"));
  $("dbg-step-over").addEventListener("click", () => command("debug-step-over"));
  $("dbg-step-into").addEventListener("click", () => command("debug-step-into"));
  $("dbg-step-out").addEventListener("click", () => command("debug-step-out"));
  $("dbg-restart").addEventListener("click", () => command("debug-restart"));
  $("dbg-stop").addEventListener("click", () => command("debug-stop"));
  $("debug-start").addEventListener("click", () => command("debug-continue"));

  // F-keys work from anywhere in the page except while the command palette is open.
  document.addEventListener("keydown", (ev) => {
    if (!$("palette-backdrop").hidden || ev.altKey) return;
    const mod = ev.ctrlKey || ev.metaKey;
    let id = null;
    if (ev.key === "F5") id = mod && ev.shiftKey ? "debug-restart" : ev.shiftKey ? "debug-stop" : mod ? null : "debug-continue";
    else if (ev.key === "F10" && !ev.shiftKey && !mod) id = "debug-step-over";
    else if (ev.key === "F11" && !mod) id = ev.shiftKey ? "debug-step-out" : "debug-step-into";
    if (!id) return;
    ev.preventDefault();
    command(id);
  });

  renderPanels({ vars: [], frames: [], selected: 0 });
  refresh();
  return { command, refresh };
}
