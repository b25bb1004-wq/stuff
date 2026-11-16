(() => {
  "use strict";

  const make = window.Kit.createEl;
  const EXAMPLES = [
    {
      label: "Mixed sizes, one freed (default)",
      input:
        "arena 1048576\nalloc a 100\nalloc b 200\nalloc c 5000\nfree a\nalloc d 64\nfree c\nalloc e 4096"
    },
    {
      label: "Fragmentation: free the middle",
      input: "alloc a 1000\nalloc b 1000\nalloc c 1000\nfree b\nalloc d 1500\nfree a"
    },
    {
      label: "Refused: double free (error)",
      input: "alloc a 64\nfree a\nfree a"
    }
  ];

  window.Kit.mount({
    id: "midas",
    name: "Midas",
    source: "crates/midas",
    about:
      "Buddy page allocator with a size-class slab heap on top. Blocks are cut here, one script line at a time. The map is drawn from the real allocator's block list, not from a model of it.",
    help: "midas sim FILE\nPlanned CLI with the same script format. The tool is not built yet."
  });

  const root = document.getElementById("app-root");
  const wrap = make("div", "midas-wrap");

  const toolbar = make("div", "midas-toolbar");
  const scriptLabel = make("label", "demo-toolbar-label", "Script");
  scriptLabel.setAttribute("for", "midas-script");
  const exLabel = make("label", "sr-only", "Examples");
  exLabel.setAttribute("for", "midas-examples");
  const exSelect = document.createElement("select");
  exSelect.id = "midas-examples";
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = "Examples...";
  exSelect.appendChild(placeholder);
  EXAMPLES.forEach((ex, idx) => {
    const opt = document.createElement("option");
    opt.value = String(idx);
    opt.textContent = ex.label;
    exSelect.appendChild(opt);
  });
  const runBtn = make("button", "kit-btn-accent", "Run");
  runBtn.type = "button";
  const status = make("span", "demo-status", "ready");
  status.setAttribute("role", "status");
  toolbar.append(scriptLabel, exLabel, exSelect, runBtn, status);

  const editor = document.createElement("textarea");
  editor.id = "midas-script";
  editor.className = "demo-editor";
  editor.spellcheck = false;
  editor.value = EXAMPLES[0].input;
  editor.setAttribute("aria-describedby", "midas-hint");
  const hint = make(
    "p",
    "demo-hint",
    "One op per line: arena N (first line only), alloc NAME SIZE, free NAME. Ctrl+Enter runs it."
  );
  hint.id = "midas-hint";

  const barTitle = make("h2", "demo-toolbar-label", "Heap map");
  const bar = make("div", "midas-bar");
  bar.setAttribute("role", "img");
  bar.setAttribute("aria-label", "Heap map: live blocks drawn over the arena");
  const barNote = make("p", "demo-hint", "Amber spans are live blocks. The rest of the arena is free.");

  const statsDl = make("dl", "midas-stats");

  const tableWrap = make("div", "midas-table-wrap");
  const table = make("table", "midas-table");
  table.appendChild(make("caption", "sr-only", "Blocks handed out by the script"));
  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  ["Block", "Offset", "Size", "State"].forEach((label) => {
    const th = document.createElement("th");
    th.scope = "col";
    th.textContent = label;
    headRow.appendChild(th);
  });
  head.appendChild(headRow);
  table.appendChild(head);
  const tbody = document.createElement("tbody");
  table.appendChild(tbody);
  tableWrap.appendChild(table);

  const logLabel = make("span", "demo-toolbar-label", "Op log");
  const log = make("pre", "demo-output", "");
  log.setAttribute("aria-live", "polite");
  log.tabIndex = 0;

  wrap.append(
    toolbar,
    editor,
    hint,
    barTitle,
    bar,
    barNote,
    statsDl,
    tableWrap,
    logLabel,
    log
  );
  root.replaceChildren(wrap);

  function clearViews() {
    bar.replaceChildren();
    statsDl.replaceChildren();
    tbody.replaceChildren();
  }

  function showError(text) {
    clearViews();
    log.className = "demo-output is-error";
    log.textContent = text;
    status.className = "demo-status is-bad";
    status.textContent = "error";
  }

  // The wasm output has one fact per line: `arena N usable M`, `block OFF SIZE live|freed NAME`,
  // `stats used U live L freed F check ok`, plus the `op` lines shown in the log.
  function showResult(text) {
    let usable = 0;
    const blocks = [];
    let stats = [];
    for (const line of text.split("\n")) {
      const f = line.split(" ");
      if (f[0] === "arena") {
        usable = Number(f[3]);
      } else if (f[0] === "block") {
        blocks.push({ offset: Number(f[1]), size: Number(f[2]), state: f[3], name: f[4] });
      } else if (f[0] === "stats") {
        stats = f.slice(1);
      }
    }

    const scale = usable > 0 ? usable : 1;
    clearViews();
    blocks
      .filter((b) => b.state === "live")
      .forEach((b) => {
        const seg = make("div", "midas-used");
        seg.style.left = `${(b.offset / scale) * 100}%`;
        seg.style.width = `${(b.size / scale) * 100}%`;
        seg.title = `${b.name}: ${b.size} bytes at offset ${b.offset}`;
        bar.appendChild(seg);
      });

    for (let i = 0; i + 1 < stats.length; i += 2) {
      statsDl.appendChild(make("dt", "", stats[i]));
      statsDl.appendChild(make("dd", "", stats[i + 1]));
    }

    blocks.forEach((b) => {
      const tr = document.createElement("tr");
      [b.name, String(b.offset), `${b.size} B`, b.state].forEach((cell) => {
        const td = document.createElement("td");
        td.textContent = cell;
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });

    log.className = "demo-output";
    log.textContent = text;
    status.className = "demo-status is-ok";
    status.textContent = `${blocks.filter((b) => b.state === "live").length} live`;
  }

  async function run() {
    runBtn.disabled = true;
    status.className = "demo-status";
    status.textContent = "running...";
    try {
      const wasm = await window.Kit.wasm();
      const res = wasm.runDemo("midas", editor.value);
      if (res.kind === "text") {
        showResult(res.text);
      } else {
        showError(res.text || "demo failed");
      }
    } catch (err) {
      showError(err && err.message ? err.message : String(err));
    } finally {
      runBtn.disabled = false;
    }
  }

  exSelect.addEventListener("change", () => {
    const idx = Number(exSelect.value);
    if (exSelect.value !== "" && EXAMPLES[idx]) {
      editor.value = EXAMPLES[idx].input;
      run();
    }
    exSelect.value = "";
  });
  editor.addEventListener("keydown", (ev) => {
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
      ev.preventDefault();
      run();
    }
  });
  runBtn.addEventListener("click", run);
  run();
})();
