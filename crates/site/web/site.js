(() => {
  "use strict";

  const STEPS_PER_FRAME = 2000000n;
  const LOGOS_FUEL = 1000000n;

  const LOGOS_EXAMPLES = {
    hello:
      'fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  print("hello from logos\\n")\n}\n',
    loop:
      'fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  var sum = 0\n  for i in 1..11 {\n    sum = sum + i * i\n  }\n  print("sum of squares 1..10 = " + str(sum) + "\\n")\n}\n',
    error:
      'fn main() {\n  let x: int = "not an int"\n}\n'
  };

  const systems = Array.isArray(window.MONOLITH_SYSTEMS) ? window.MONOLITH_SYSTEMS : [];
  const layerDiagram = document.getElementById("layer-diagram");
  const systemsList = document.getElementById("systems");
  const activeLayerLabel = document.getElementById("active-layer-label");
  const wasmStatus = document.getElementById("wasm-status");
  const themeToggle = document.getElementById("theme-toggle");

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  let wasmPromise = null;
  let wasmExports = null;
  let titanStarted = false;
  let titanHooks = null;

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
  });

  function ensureWasm() {
    if (!wasmPromise) {
      wasmStatus.textContent = "loading wasm…";
      wasmPromise = Neon.load("site.wasm")
        .then((api) => {
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

  function formatNum(n) {
    return String(n).padStart(2, "0");
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

    const canvas = createEl("canvas", "demo-canvas");
    canvas.hidden = true;
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", `Rendered frame from ${labelText}`);

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
            canvas.width = w;
            canvas.height = h;
            const ctx = canvas.getContext("2d");
            if (ctx) {
              ctx.putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer), w, h), 0, 0);
            }
            canvas.hidden = false;
            outPre.hidden = true;
            status.className = "demo-status is-ok";
            status.textContent = `${w}x${h} frame`;
          } else {
            canvas.hidden = true;
            outPre.hidden = false;
            outPre.className = "demo-output is-error";
            outPre.textContent = "invalid pixel buffer";
            status.className = "demo-status is-bad";
            status.textContent = "error";
          }
        } else if (rc === 0) {
          canvas.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output";
          outPre.textContent = decoder.decode(bytes);
          status.className = "demo-status is-ok";
          status.textContent = "ok";
        } else {
          canvas.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output is-error";
          outPre.textContent = decoder.decode(bytes) || "demo failed";
          status.className = "demo-status is-bad";
          status.textContent = "error";
        }
      } catch (err) {
        canvas.hidden = true;
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
    stage.appendChild(canvas);
    box.appendChild(stage);
    return box;
  }

  function renderTitanDemo() {
    const box = createEl("div", "demo-box");
    const bar = createEl("div", "demo-bar");
    const label = createEl("span", "demo-label", "Titan UART console");
    const status = createEl("span", "demo-status", "waiting for viewport");
    bar.appendChild(label);
    bar.appendChild(status);

    const termRoot = createEl("div", "titan-term");
    const term = Term.mount(termRoot, {
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
        if (rc === 1) {
          status.className = "demo-status";
          status.textContent = "running";
          return { status: "busy", text };
        }
        if (rc === 0) {
          status.className = "demo-status is-ok";
          status.textContent = "waiting for input";
          return { status: "waiting", text };
        }
        if (rc >= 2) {
          status.className = "demo-status";
          status.textContent = `exited (${rc - 2})`;
        } else {
          status.className = "demo-status is-bad";
          status.textContent = "not booted";
        }
        return { status: "done", text };
      }
    });

    box.appendChild(bar);
    box.appendChild(termRoot);

    titanHooks = {
      boot: async () => {
        if (titanStarted) {
          return;
        }
        titanStarted = true;
        status.className = "demo-status";
        status.textContent = "loading kernel…";
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
          status.textContent = "booting…";
          term.start();
        } catch (err) {
          status.className = "demo-status is-bad";
          status.textContent = "failed";
          term.append(`
[titan error] ${err && err.message ? err.message : String(err)}
`);
        }
      }
    };
    return box;
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
        /* A task break lets the running status paint before the run; rAF is reserved for the kit. */
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

  /* Core sample hero (4.5): the layer chips stack up from below on the first load of a session.
  A key, click, wheel or touch finishes it at once; reduced motion shows the end picture. */
  function playCoreSample() {
    if (!window.Motion) {
      return;
    }
    Motion.hero("site", (ctl) => {
      const handles = Motion.stagger(layerDiagram, { curve: "heavy", dist: 3, fill: "backwards" });
      ctl.onSkip(() => handles.forEach((h) => h.finish()));
      return Promise.all(handles.map((h) => h.done));
    });
  }

  function buildPage() {
    const chips = new Map();
    const sectionToNum = new Map();

    for (const sys of systems) {
      const chip = createEl("a", "layer-chip");
      chip.href = `#sys-${sys.id}`;
      chip.appendChild(createEl("span", "layer-num", formatNum(sys.num)));
      chip.appendChild(createEl("span", "layer-name", sys.name));
      layerDiagram.appendChild(chip);
      chips.set(sys.num, chip);

      const section = createEl("section", "system-card reveal");
      section.id = `sys-${sys.id}`;
      section.dataset.num = String(sys.num);
      if (sys.demo && sys.demo !== "athena") {
        section.dataset.demoKind = sys.demo;
      }

      const head = createEl("div", "system-head");
      head.appendChild(createEl("span", "system-num", formatNum(sys.num)));
      head.appendChild(createEl("h2", "system-title", sys.name));
      head.appendChild(createEl("span", "system-what", sys.what));
      head.appendChild(createEl("code", "system-src", sys.source));
      if (sys.demo) {
        const appLink = createEl("a", "btn-link btn-app", "Open app");
        appLink.href = `${sys.id}/`;
        head.appendChild(appLink);
      }

      const line = createEl("p", "system-line", sys.line);

      section.appendChild(head);
      section.appendChild(line);

      if (sys.demo === "titan") {
        section.appendChild(renderTitanDemo());
      } else if (sys.demo === "logos") {
        section.appendChild(renderLogosDemo());
      } else if (sys.demo === "athena") {
        section.appendChild(renderAthenaDemo());
      } else if (typeof sys.demo === "string") {
        section.appendChild(renderRegistryDemo(sys.demo, `${sys.name} input`, sys.defaultInput, false));
      } else {
        section.appendChild(createEl("p", "demo-placeholder", "Live demo coming in the next build"));
      }

      if (sys.extraDemo) {
        section.appendChild(
          renderRegistryDemo(
            sys.extraDemo.id,
            `${sys.extraDemo.title} (${sys.extraDemo.source})`,
            sys.extraDemo.defaultInput,
            true
          )
        );
      }

      systemsList.appendChild(section);
      sectionToNum.set(section, sys.num);
    }

    function setActiveLayer(num) {
      for (const [k, chip] of chips.entries()) {
        chip.classList.toggle("is-active", k === num);
      }
      activeLayerLabel.textContent = `${formatNum(num)} / ${formatNum(systems.length)}`;
    }

    if (systems.length > 0) {
      setActiveLayer(systems[0].num);
    }

    playCoreSample();

    /* Kit motion when it loaded; the CSS end state (is-visible) shows the section either way. */
    const motion = window.Motion || null;
    const staggerMs = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--stagger")) || 0;
    let revealed = 0;

    if ("IntersectionObserver" in window) {
      const revealObserver = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (entry.isIntersecting) {
              if (!entry.target.classList.contains("is-visible")) {
                entry.target.classList.add("is-visible");
                if (motion) {
                  /* Sections that arrive together enter one stagger step apart, at most eight steps. */
                  motion.enter(entry.target, { dist: 2, dur: "slow", delay: Math.min(revealed, 7) * staggerMs });
                  revealed += 1;
                }
              }
              const num = sectionToNum.get(entry.target);
              if (num !== undefined) {
                setActiveLayer(num);
              }
            }
          }
          revealed = 0;
        },
        { threshold: 0.12 }
      );

      const lazyObserver = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (!entry.isIntersecting) {
              continue;
            }
            const kind = entry.target.dataset.demoKind;
            if (kind) {
              ensureWasm().catch(() => {});
            }
            if (kind === "titan" && titanHooks) {
              titanHooks.boot();
            }
            lazyObserver.unobserve(entry.target);
          }
        },
        { rootMargin: "240px 0px" }
      );

      for (const section of sectionToNum.keys()) {
        revealObserver.observe(section);
        if (section.dataset.demoKind) {
          lazyObserver.observe(section);
        }
      }
    } else {
      for (const section of sectionToNum.keys()) {
        section.classList.add("is-visible");
      }
    }
  }

  buildPage();
})();
