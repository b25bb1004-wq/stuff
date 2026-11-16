(() => {
  "use strict";

  const STEPS_PER_FRAME = 2000000n;
  const LOGOS_FUEL = 1000000n;
  /* Titan's boot session: typed in order once the shell prompt is up, mirrored in the die log. */
  const TITAN_SCRIPT = ["ls /", "cat /hello.txt", "echo hello from the board"];
  const DIE_LINES = 8;
  /* The pane never draws smaller than a phone-sized console; on desktop it is the die rect exactly. */
  const PANE_MIN_W = 280;
  const PANE_MIN_H = 260;

  const LOGOS_EXAMPLES = {
    hello:
      'fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  print("hello from logos\\n")\n}\n',
    loop:
      'fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  var sum = 0\n  for i in 1..11 {\n    sum = sum + i * i\n  }\n  print("sum of squares 1..10 = " + str(sum) + "\\n")\n}\n',
    error:
      'fn main() {\n  let x: int = "not an int"\n}\n'
  };

  document.documentElement.classList.remove("no-js");

  const systems = Array.isArray(window.MONOLITH_SYSTEMS) ? window.MONOLITH_SYSTEMS : [];
  const byId = new Map(systems.map((sys) => [sys.id, sys]));
  const hero = document.getElementById("hero");
  const canvas = document.getElementById("board");
  const chipLabel = document.getElementById("chip-label");
  const pane = document.getElementById("demo-pane");
  const dieLog = document.getElementById("die-log");
  const dieText = document.getElementById("die-text");
  const openTerminal = document.getElementById("open-terminal");
  const holder = document.getElementById("term-holder");
  const wasmStatus = document.getElementById("wasm-status");
  const themeToggle = document.getElementById("theme-toggle");

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  let wasmPromise = null;
  let wasmApi = null;
  let wasmExports = null;
  let board = null;
  let selectedId = null;
  let seq = 0;
  let titanBox = null;
  let titanTerm = null;
  let titanBoot = null;
  let titanBooted = false;
  let scriptQueue = [];
  let dieLines = [];
  let dieTail = "";
  let feedStop = null;

  function reduced() {
    return window.Motion ? Motion.reduced() : false;
  }

  function applyTheme(theme) {
    if (theme === "light" || theme === "dark") {
      document.documentElement.setAttribute("data-theme", theme);
      themeToggle.textContent = theme === "light" ? "Theme: Light" : "Theme: Dark";
    } else {
      document.documentElement.removeAttribute("data-theme");
      themeToggle.textContent = "Theme: Auto";
    }
  }

  themeToggle.addEventListener("click", () => {
    const cur = document.documentElement.getAttribute("data-theme");
    const prefersLight =
      window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches;
    if (!cur) {
      applyTheme(prefersLight ? "dark" : "light");
    } else if (cur === "light") {
      applyTheme("dark");
    } else {
      applyTheme("light");
    }
    remountBoard();
  });

  function ensureWasm() {
    if (!wasmPromise) {
      wasmStatus.textContent = "loading wasm…";
      wasmPromise = Neon.load("site.wasm")
        .then((api) => {
          wasmApi = api;
          wasmExports = api.exports;
          wasmStatus.textContent = "wasm ready";
          return wasmExports;
        })
        .catch((err) => {
          wasmStatus.textContent = "wasm error";
          wasmPromise = null;
          throw err;
        });
    }
    return wasmPromise;
  }

  function withGuestBytes(wasm, bytes, fn) {
    const len = bytes.length;
    const ptr = wasm.neon_alloc(len);
    try {
      if (len > 0) {
        new Uint8Array(wasm.memory.buffer, ptr, len).set(bytes);
      }
      return fn(ptr, len);
    } finally {
      wasm.neon_free(ptr, len);
    }
  }

  function readNeonLast(wasm, len) {
    if (len <= 0) {
      return "";
    }
    const ptr = wasm.neon_last_ptr();
    return decoder.decode(new Uint8Array(wasm.memory.buffer, ptr, len));
  }

  function readSiteLastBytes(wasm) {
    const len = wasm.site_last_len();
    if (len <= 0) {
      return new Uint8Array(0);
    }
    const ptr = wasm.site_last_ptr();
    return new Uint8Array(wasm.memory.buffer, ptr, len).slice();
  }

  function createEl(tag, cls, text) {
    const el = document.createElement(tag);
    if (cls) {
      el.className = cls;
    }
    if (text !== undefined) {
      el.textContent = text;
    }
    return el;
  }

  function renderRegistryDemo(demoId, labelText, defaultInput, isSub) {
    const box = createEl("div", isSub ? "demo-box demo-subbox" : "demo-box");
    const bar = createEl("div", "demo-bar");
    const inputId = `demo-in-${demoId}`;
    const label = createEl("label", "demo-label", labelText);
    label.setAttribute("for", inputId);
    const status = createEl("span", "demo-status", "ready");
    bar.appendChild(label);
    if (isSub) {
      const subApp = createEl("a", "btn-link btn-app", "Open app");
      subApp.href = `${demoId}/`;
      bar.appendChild(subApp);
    }
    bar.appendChild(status);

    const textarea = document.createElement("textarea");
    textarea.id = inputId;
    textarea.value = defaultInput || "";
    textarea.spellcheck = false;

    const controls = createEl("div", "demo-controls");
    const runBtn = createEl("button", "", "Run");
    runBtn.type = "button";
    const resetBtn = createEl("button", "btn-ghost", "Reset");
    resetBtn.type = "button";
    controls.appendChild(runBtn);
    controls.appendChild(resetBtn);

    const outPre = createEl("pre", "demo-output", "Click Run to execute in WebAssembly.");
    outPre.setAttribute("aria-live", "polite");
    outPre.tabIndex = 0;

    const canvasEl = createEl("canvas", "demo-canvas");
    canvasEl.hidden = true;
    canvasEl.setAttribute("role", "img");
    canvasEl.setAttribute("aria-label", `Rendered frame from ${labelText}`);

    resetBtn.addEventListener("click", () => {
      textarea.value = defaultInput || "";
    });

    textarea.addEventListener("keydown", (ev) => {
      if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
        ev.preventDefault();
        runBtn.click();
      }
    });

    runBtn.addEventListener("click", async () => {
      runBtn.disabled = true;
      status.className = "demo-status";
      status.textContent = "running…";
      try {
        const wasm = await ensureWasm();
        const idBytes = encoder.encode(demoId);
        const inBytes = encoder.encode(textarea.value);
        const rc = withGuestBytes(wasm, idBytes, (idPtr, idLen) =>
          withGuestBytes(wasm, inBytes, (inPtr, inLen) =>
            wasm.site_demo(idPtr, idLen, inPtr, inLen)
          )
        );
        const bytes = readSiteLastBytes(wasm);
        if (rc === 1) {
          const w = wasm.site_pixels_w();
          const h = wasm.site_pixels_h();
          if (w > 0 && h > 0 && bytes.length === w * h * 4) {
            canvasEl.width = w;
            canvasEl.height = h;
            const ctx = canvasEl.getContext("2d");
            if (ctx) {
              ctx.putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer), w, h), 0, 0);
            }
            canvasEl.hidden = false;
            outPre.hidden = true;
            status.className = "demo-status is-ok";
            status.textContent = `${w}x${h} frame`;
          } else {
            canvasEl.hidden = true;
            outPre.hidden = false;
            outPre.className = "demo-output is-error";
            outPre.textContent = "invalid pixel buffer";
            status.className = "demo-status is-bad";
            status.textContent = "error";
          }
        } else if (rc === 0) {
          canvasEl.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output";
          outPre.textContent = decoder.decode(bytes);
          status.className = "demo-status is-ok";
          status.textContent = "ok";
        } else {
          canvasEl.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output is-error";
          outPre.textContent = decoder.decode(bytes) || "demo failed";
          status.className = "demo-status is-bad";
          status.textContent = "error";
        }
      } catch (err) {
        canvasEl.hidden = true;
        outPre.hidden = false;
        outPre.className = "demo-output is-error";
        outPre.textContent = err && err.message ? err.message : String(err);
        status.className = "demo-status is-bad";
        status.textContent = "error";
      } finally {
        runBtn.disabled = false;
      }
    });

    box.appendChild(bar);
    box.appendChild(textarea);
    box.appendChild(controls);
    const stage = createEl("div", "demo-stage");
    stage.appendChild(outPre);
    stage.appendChild(canvasEl);
    box.appendChild(stage);
    return box;
  }

  /* Titan's terminal lives in a hidden holder from page load, so the machine keeps running and
  the board's events keep flowing; the pane moves it in while Titan is open. */
  function renderTitanDemo() {
    const box = createEl("div", "demo-box titan-box");

    /* Term.mount owns the one status line (.term-status); the pump only reports busy, waiting or done. */
    const termRoot = createEl("div", "titan-term");
    titanTerm = Term.mount(termRoot, {
      send(line) {
        withGuestBytes(wasmExports, encoder.encode(line), (ptr, len) =>
          wasmExports.titan_send(ptr, len)
        );
      },
      pump() {
        if (!wasmExports) {
          return { status: "done", text: "" };
        }
        const rc = wasmExports.titan_pump(STEPS_PER_FRAME);
        const outLen = wasmExports.titan_output();
        const text = outLen > 0 ? readNeonLast(wasmExports, outLen) : "";
        noteOutput(text);
        if (rc === 1) {
          return { status: "busy", text };
        }
        if (rc === 0 && scriptQueue.length > 0) {
          const cmd = scriptQueue.shift();
          titanTerm.append(`${cmd}\n`, "user-cmd");
          noteOutput(`${cmd}\n`);
          withGuestBytes(wasmExports, encoder.encode(cmd), (ptr, len) =>
            wasmExports.titan_send(ptr, len)
          );
          return { status: "busy", text };
        }
        if (rc === 0) {
          return { status: "waiting", text };
        }
        return { status: "done", text };
      }
    });

    box.appendChild(termRoot);

    let booting = false;
    titanBoot = async () => {
      if (booting) {
        return;
      }
      booting = true;
      try {
        const wasm = await ensureWasm();
        const elfResp = await fetch("titan.elf");
        if (!elfResp.ok) {
          throw new Error(`failed to fetch titan.elf: ${elfResp.status}`);
        }
        const elfBytes = new Uint8Array(await elfResp.arrayBuffer());
        const rc = withGuestBytes(wasm, elfBytes, (ptr, len) => wasm.titan_boot(ptr, len));
        if (rc !== 0) {
          const errLen = wasm.neon_last_len();
          throw new Error(readNeonLast(wasm, errLen) || "titan_boot failed");
        }
        titanBooted = true;
        runTitanScript();
        if (window.Feed && !feedStop) {
          feedStop = window.Feed.start(wasmApi, emitBoard);
        }
      } catch (err) {
        booting = false;
        titanTerm.append(`
[titan error] ${err && err.message ? err.message : String(err)}
`);
      }
    };
    return box;
  }

  /* Keeps the last DIE_LINES lines of Titan's output (a partial line counts as the last one). */
  function noteOutput(text) {
    if (!text) {
      return;
    }
    const parts = (dieTail + text).split("\n");
    dieTail = parts.pop();
    dieLines = dieLines.concat(parts).slice(-DIE_LINES);
    dieText.textContent = dieLines.concat(dieTail ? [dieTail] : []).slice(-DIE_LINES).join("\n");
  }

  /* The boot session, typed through the same path as a user's line; run again by the pane's Run again. */
  function runTitanScript() {
    scriptQueue = scriptQueue.concat(TITAN_SCRIPT);
    if (titanTerm) {
      titanTerm.start();
    }
  }

  /* The die log sits over Titan's chip on the board, and only while no chip is open. */
  function placeDie() {
    const r = board && !selectedId ? rectOf("titan") : null;
    if (!r) {
      dieLog.hidden = true;
      return;
    }
    dieLog.style.left = `${r.x}px`;
    dieLog.style.top = `${r.y}px`;
    dieLog.style.width = `${r.w}px`;
    dieLog.style.height = `${r.h}px`;
    dieLog.hidden = false;
  }

  function renderLogosDemo() {
    const box = createEl("div", "demo-box");
    const bar = createEl("div", "demo-bar");
    const label = createEl("label", "demo-label", "Logos playground");
    label.setAttribute("for", "logos-src");
    const status = createEl("span", "demo-status", "ready");
    bar.appendChild(label);
    bar.appendChild(status);

    const controls = createEl("div", "demo-controls");
    const exLabel = createEl("label", "demo-label", "Example");
    exLabel.setAttribute("for", "logos-examples");
    const select = document.createElement("select");
    select.id = "logos-examples";
    for (const [key, title] of [
      ["hello", "Hello"],
      ["loop", "Sum of squares"],
      ["error", "Type error"]
    ]) {
      const opt = document.createElement("option");
      opt.value = key;
      opt.textContent = title;
      select.appendChild(opt);
    }
    const runBtn = createEl("button", "", "Run");
    runBtn.type = "button";
    controls.appendChild(exLabel);
    controls.appendChild(select);
    controls.appendChild(runBtn);

    const textarea = document.createElement("textarea");
    textarea.id = "logos-src";
    textarea.value = LOGOS_EXAMPLES.hello;
    textarea.spellcheck = false;
    textarea.addEventListener("keydown", (ev) => {
      if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
        ev.preventDefault();
        runBtn.click();
      }
    });

    const outPre = createEl("pre", "demo-output", "Select an example and click Run.");
    outPre.setAttribute("aria-live", "polite");
    outPre.tabIndex = 0;

    select.addEventListener("change", () => {
      if (Object.prototype.hasOwnProperty.call(LOGOS_EXAMPLES, select.value)) {
        textarea.value = LOGOS_EXAMPLES[select.value];
      }
    });

    runBtn.addEventListener("click", async () => {
      runBtn.disabled = true;
      status.className = "demo-status";
      status.textContent = "running…";
      try {
        const wasm = await ensureWasm();
        /* A task break lets the running status paint before the run. */
        setTimeout(() => {
          try {
            const bytes = encoder.encode(textarea.value);
            const len = withGuestBytes(wasm, bytes, (ptr, bLen) =>
              wasm.logos_run(ptr, bLen, LOGOS_FUEL)
            );
            outPre.className = "demo-output";
            outPre.textContent = readNeonLast(wasm, len);
            status.className = "demo-status is-ok";
            status.textContent = "ok";
          } finally {
            runBtn.disabled = false;
          }
        });
      } catch (err) {
        outPre.className = "demo-output is-error";
        outPre.textContent = err && err.message ? err.message : String(err);
        status.className = "demo-status is-bad";
        status.textContent = "error";
        runBtn.disabled = false;
      }
    });

    box.appendChild(bar);
    box.appendChild(controls);
    box.appendChild(textarea);
    box.appendChild(outPre);
    return box;
  }

  function renderAthenaDemo() {
    const box = createEl("div", "demo-box");
    const row = createEl("div", "athena-row");
    const desc = createEl(
      "p",
      "demo-label",
      "Open the full multi-pane Athena workshop with Calliope syntax highlighting, Logos runner, and Titan terminal."
    );
    const link = createEl("a", "btn-link", "Open Athena IDE");
    link.href = "athena/";
    row.appendChild(desc);
    row.appendChild(link);
    box.appendChild(row);
    return box;
  }

  /* One demo pane: the system's own demo(s), built from the render functions above. */
  function buildDemos(sys) {
    const boxes = [];
    if (sys.demo === "titan") {
      boxes.push(titanBox);
    } else if (sys.demo === "logos") {
      boxes.push(renderLogosDemo());
    } else if (sys.demo === "athena") {
      boxes.push(renderAthenaDemo());
    } else if (typeof sys.demo === "string") {
      boxes.push(renderRegistryDemo(sys.demo, `${sys.name} input`, sys.defaultInput, false));
    } else {
      boxes.push(createEl("p", "demo-placeholder", "Live demo coming in the next build"));
    }
    if (sys.extraDemo) {
      boxes.push(
        renderRegistryDemo(
          sys.extraDemo.id,
          `${sys.extraDemo.title} (${sys.extraDemo.source})`,
          sys.extraDemo.defaultInput,
          true
        )
      );
    }
    return boxes.map((el) => ({ el, run: runnerFor(el) }));
  }

  function runnerFor(el) {
    if (el === titanBox) {
      return () => {
        if (titanBooted) {
          runTitanScript();
        } else {
          titanBoot();
        }
        titanTerm.focus();
      };
    }
    const first = el.querySelector("button");
    return first ? () => first.click() : null;
  }

  /* Board events: a real machine event pulses its trace (or blinks the pad under reduced motion). */
  function emitBoard(kind, fromId, toId) {
    if (board) {
      board.pulse(fromId, toId, kind);
    }
  }

  function rectOf(id) {
    return board ? board.chipRect(id) : null;
  }

  function placePane() {
    if (!selectedId || pane.hidden) {
      return;
    }
    const r = rectOf(selectedId);
    if (!r) {
      pane.classList.add("is-static");
      pane.removeAttribute("style");
      return;
    }
    pane.classList.remove("is-static");
    const wrap = canvas.parentNode;
    const w = Math.max(r.w, PANE_MIN_W);
    const h = Math.max(r.h, PANE_MIN_H);
    const x = Math.min(Math.max(r.x, 0), Math.max(0, wrap.clientWidth - w));
    const y = Math.min(Math.max(r.y, 0), Math.max(0, wrap.clientHeight - h));
    pane.style.left = `${x}px`;
    pane.style.top = `${y}px`;
    pane.style.width = `${w}px`;
    pane.style.height = `${h}px`;
  }

  function placeOverlays() {
    placePane();
    placeDie();
  }

  function showLabel(id) {
    const sys = byId.get(id);
    const r = rectOf(id);
    if (!sys || !r) {
      chipLabel.hidden = true;
      return;
    }
    chipLabel.replaceChildren(createEl("strong", "", sys.name), document.createTextNode(sys.line));
    chipLabel.style.left = `${r.x}px`;
    chipLabel.style.top = `${r.y + r.h + 6}px`;
    chipLabel.hidden = false;
  }

  function closePane() {
    if (titanBox && titanBox.parentNode === pane) {
      holder.appendChild(titanBox);
    }
    pane.replaceChildren();
    pane.classList.remove("is-titan");
    pane.hidden = true;
  }

  function openPane(sys) {
    const demos = buildDemos(sys);
    const title = createEl("h2", "pane-title", sys.name);
    title.tabIndex = -1;
    const actions = createEl("div", "pane-actions");
    const again = createEl("button", "", "Run again");
    again.type = "button";
    again.disabled = demos.every((d) => !d.run);
    again.addEventListener("click", () => demos.forEach((d) => d.run && d.run()));
    const app = createEl("a", "btn-link", "Open app");
    app.href = `${sys.id}/`;
    const back = createEl("button", "btn-ghost", "Back to board");
    back.type = "button";
    back.addEventListener("click", goBack);
    actions.append(again, app, back);

    /* One header row (name and actions) stays at the top; the body below is the only scroll area. */
    const head = createEl("div", "pane-head");
    head.append(title, actions);
    const isTitan = sys.demo === "titan";
    pane.classList.toggle("is-titan", isTitan);
    const body = [head];
    if (!isTitan) {
      body.push(createEl("p", "pane-what", sys.line));
    }
    pane.replaceChildren(...body, ...demos.map((d) => d.el));
    pane.hidden = false;
    placeDie();
    placePane();
    if (window.Motion) {
      Motion.enter(pane, { curve: "firm", dist: 1 });
    }
    title.focus();
    demos.forEach((d) => d.run && d.run());
  }

  /* One pane at a time. Resolves once the pane is open (after the zoom, if there is a board). */
  function select(id) {
    const sys = byId.get(id);
    if (!sys) {
      return Promise.resolve();
    }
    if (id === selectedId) {
      return Promise.resolve();
    }
    seq += 1;
    const mine = seq;
    closePane();
    selectedId = id;
    setHash(id);
    chipLabel.hidden = true;
    if (!board) {
      openPane(sys);
      return Promise.resolve();
    }
    return Promise.resolve()
      .then(() => board.zoomTo(id))
      .catch(() => {})
      .then(() => {
        if (mine === seq && selectedId === id) {
          openPane(sys);
        }
      });
  }

  async function goBack() {
    if (!selectedId) {
      return;
    }
    const id = selectedId;
    seq += 1;
    selectedId = null;
    closePane();
    setHash("");
    if (!board) {
      openTerminal.focus();
      return;
    }
    try {
      await board.zoomOut();
    } catch (err) {
      /* The board stays where it is; the list below still works. */
    }
    placeDie();
    board.focus(id);
  }

  function setHash(id) {
    try {
      history.replaceState(null, "", id ? `#${id}` : `${location.pathname}${location.search}`);
    } catch (err) {
      /* Some embedded frames refuse history changes; the hash is then only a convenience. */
    }
  }

  function mountBoard() {
    if (board) {
      board.destroy();
      board = null;
    }
    if (window.Board && typeof window.Board.mount === "function") {
      try {
        board = window.Board.mount(canvas, systems);
        board.on("hover", (id) => {
          if (id && !selectedId) {
            showLabel(id);
          } else {
            chipLabel.hidden = true;
          }
        });
        board.on("select", (id) => {
          select(id);
        });
      } catch (err) {
        console.warn("board unavailable, list view only:", err);
        board = null;
      }
    }
    hero.classList.toggle("no-board", board === null);
    placeDie();
  }

  /* Colours are read at mount, so a theme change remounts the board and keeps the open chip. */
  function remountBoard() {
    const id = selectedId;
    mountBoard();
    if (board && id) {
      Promise.resolve()
        .then(() => board.zoomTo(id))
        .then(placeOverlays, () => {});
    }
  }

  function buildPage() {
    mountBoard();

    if (window.Motion) {
      Motion.hero("site", (ctl) => {
        const handle = Motion.enter(document.getElementById("board-wrap"), { curve: "heavy", dist: 2 });
        ctl.onSkip(() => handle.finish());
        return handle.done;
      });
    }

    openTerminal.addEventListener("click", () => {
      select("titan").then(() => titanTerm.focus());
    });

    document.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape" && selectedId) {
        goBack();
      }
    });

    window.addEventListener("hashchange", () => {
      const id = location.hash.slice(1);
      if (byId.has(id)) {
        select(id);
      } else if (selectedId) {
        goBack();
      }
    });

    window.addEventListener("resize", placeOverlays);
    /* Chip rects are read before the first layout pass on some loads; the wrap's size settling re-places them. */
    if (window.ResizeObserver) {
      new ResizeObserver(placeOverlays).observe(canvas.parentNode);
    }

    const scheme = window.matchMedia ? window.matchMedia("(prefers-color-scheme: light)") : null;
    if (scheme && scheme.addEventListener) {
      scheme.addEventListener("change", remountBoard);
    }

    if (titanBoot) {
      titanBoot();
    }
    const initial = location.hash.slice(1);
    if (byId.has(initial)) {
      select(initial);
    }
  }

  titanBox = renderTitanDemo();
  holder.appendChild(titanBox);
  buildPage();
})();
