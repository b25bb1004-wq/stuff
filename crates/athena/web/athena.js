(() => {
  "use strict";

  const STORAGE_KEY = "athena.files.v1";
  const ACTIVE_KEY = "athena.active.v1";
  const STEPS_PER_FRAME = 2000000n;
  const LOGOS_FUEL = 1000000n;
  const DEBOUNCE_MS = 300;
  const LINE_HEIGHT_PX = 20;

  const SEED_FILES = {
    "main.logos": [
      'fn print(s: str) uses io = extern "io.print"',
      "",
      "fn main() uses io {",
      "  var sum = 0",
      "  for i in 1..6 {",
      "    sum = sum + i",
      '    print("step " + str(i) + ": sum=" + str(sum) + "\\n")',
      "  }",
      "}",
      ""
    ].join("\n"),
    "math.logos": [
      "fn add(a: int, b: int) -> int {",
      "  a + b",
      "}",
      "",
      "fn max2(a: int, b: int) -> int {",
      "  if a > b { a } else { b }",
      "}",
      "",
      'test "add works" {',
      "  assert(add(2, 3) == 5)",
      "}",
      "",
      'test "max2 works" {',
      "  assert(max2(7, 3) == 7)",
      "  assert(max2(-2, 4) == 4)",
      "}",
      "",
      'prop "max2 >= both" (a: int, b: int) {',
      "  let m = max2(a, b)",
      "  assert(m >= a)",
      "  assert(m >= b)",
      "}",
      ""
    ].join("\n"),
    "broken.logos": [
      "fn main() {",
      '  let x: int = "not an int"',
      "}",
      ""
    ].join("\n")
  };

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  const workspace = document.getElementById("workspace");
  const activeFileName = document.getElementById("active-file-name");
  const saveStatus = document.getElementById("save-status");
  const wasmStatus = document.getElementById("wasm-status");
  const btnRun = document.getElementById("btn-run");
  const btnTest = document.getElementById("btn-test");
  const btnTheme = document.getElementById("btn-theme");

  const btnNewFile = document.getElementById("btn-new-file");
  const newFileForm = document.getElementById("new-file-form");
  const newFileInput = document.getElementById("new-file-input");
  const btnNewCancel = document.getElementById("btn-new-cancel");
  const newFileError = document.getElementById("new-file-error");
  const fileList = document.getElementById("file-list");

  const editorMeta = document.getElementById("editor-meta");
  const editorEmpty = document.getElementById("editor-empty");
  const editorWrap = document.getElementById("editor-wrap");
  const gutter = document.getElementById("editor-gutter");
  const codeHighlight = document.getElementById("code-highlight");
  const codeHighlightInner = document.getElementById("code-highlight-inner");
  const codeInput = document.getElementById("code-input");

  const diagList = document.getElementById("diag-list");
  const runOut = document.getElementById("run-out");
  const testSummary = document.getElementById("test-summary");
  const testList = document.getElementById("test-list");
  const titanStatus = document.getElementById("titan-status");
  const termOut = document.getElementById("term-out");
  const termForm = document.getElementById("term-form");
  const termIn = document.getElementById("term-in");
  const termSend = document.getElementById("term-send");

  const tabButtons = Array.from(document.querySelectorAll(".tab-btn"));
  const tabPanels = {
    output: document.getElementById("panel-output"),
    tests: document.getElementById("panel-tests"),
    terminal: document.getElementById("panel-terminal")
  };
  const mobileTabs = Array.from(document.querySelectorAll(".mobile-tab"));
  const resizerLeft = document.getElementById("resizer-left");
  const resizerRight = document.getElementById("resizer-right");

  let files = loadFiles();
  let activeFile = loadActiveFile(files);
  let editingState = null;
  let wasm = null;
  let debounceTimer = null;
  let pumping = false;
  let titanBootStarted = false;

  function storageGet(key) {
    try {
      return window.localStorage.getItem(key);
    } catch (_err) {
      return null;
    }
  }

  function storageSet(key, val) {
    try {
      window.localStorage.setItem(key, val);
      return true;
    } catch (_err) {
      return false;
    }
  }

  function loadFiles() {
    const raw = storageGet(STORAGE_KEY);
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          const clean = {};
          for (const [k, v] of Object.entries(parsed)) {
            if (typeof k === "string" && k.trim() !== "" && typeof v === "string") {
              clean[k] = v;
            }
          }
          if (Object.keys(clean).length > 0) {
            return clean;
          }
        }
      } catch (_err) {
        // Fall back to seed files.
      }
    }
    const seeded = { ...SEED_FILES };
    storageSet(STORAGE_KEY, JSON.stringify(seeded));
    return seeded;
  }

  function saveFiles() {
    const ok = storageSet(STORAGE_KEY, JSON.stringify(files));
    saveStatus.textContent = ok ? "saved" : "memory only";
  }

  function loadActiveFile(fileMap) {
    const keys = Object.keys(fileMap);
    if (keys.length === 0) {
      return null;
    }
    const saved = storageGet(ACTIVE_KEY);
    if (saved && Object.prototype.hasOwnProperty.call(fileMap, saved)) {
      return saved;
    }
    return Object.prototype.hasOwnProperty.call(fileMap, "main.logos")
      ? "main.logos"
      : keys[0];
  }

  function setActiveFile(name) {
    activeFile = name;
    if (name) {
      storageSet(ACTIVE_KEY, name);
    }
    syncEditorFromState();
    renderFileTree();
  }

  function currentTheme() {
    const explicit = document.documentElement.getAttribute("data-theme");
    if (explicit === "light" || explicit === "dark") {
      return explicit;
    }
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches
      ? "light"
      : "dark";
  }

  function updateThemeButton() {
    const t = currentTheme();
    btnTheme.textContent = t === "light" ? "Dark" : "Light";
    btnTheme.setAttribute("aria-label", `Switch to ${t === "light" ? "dark" : "light"} theme`);
  }

  btnTheme.addEventListener("click", () => {
    const next = currentTheme() === "light" ? "dark" : "light";
    document.documentElement.setAttribute("data-theme", next);
    updateThemeButton();
  });
  updateThemeButton();

  function setMobileView(view) {
    workspace.setAttribute("data-mobile-view", view);
    for (const btn of mobileTabs) {
      const active = btn.dataset.mobileView === view;
      btn.classList.toggle("is-active", active);
      btn.setAttribute("aria-selected", active ? "true" : "false");
    }
    if (view === "terminal") {
      setRightTab("terminal");
    } else if (view === "output") {
      const currentTab = tabButtons.find((b) => b.classList.contains("is-active"));
      if (!currentTab || currentTab.dataset.tab === "terminal") {
        setRightTab("output");
      }
    }
  }

  for (const btn of mobileTabs) {
    btn.addEventListener("click", () => {
      setMobileView(btn.dataset.mobileView);
    });
  }

  function setRightTab(tab) {
    for (const btn of tabButtons) {
      const active = btn.dataset.tab === tab;
      btn.classList.toggle("is-active", active);
      btn.setAttribute("aria-selected", active ? "true" : "false");
    }
    for (const [key, panel] of Object.entries(tabPanels)) {
      panel.hidden = key !== tab;
    }
    if (tab === "terminal") {
      bootTitanIfNeeded();
    }
  }

  for (const btn of tabButtons) {
    btn.addEventListener("click", () => {
      setRightTab(btn.dataset.tab);
    });
  }

  function withBytes(bytes, fn) {
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

  function readNeonLast(len) {
    if (len <= 0) {
      return "";
    }
    const ptr = wasm.neon_last_ptr();
    return decoder.decode(new Uint8Array(wasm.memory.buffer, ptr, len));
  }

  function readAthenaLast(len) {
    if (len <= 0) {
      return "";
    }
    const ptr = wasm.athena_last_ptr();
    return decoder.decode(new Uint8Array(wasm.memory.buffer, ptr, len));
  }

  function parseDiagnostics(raw) {
    if (!raw) {
      return [];
    }
    const out = [];
    for (const line of raw.split("\n")) {
      if (!line) {
        continue;
      }
      const parts = line.split(":");
      if (parts.length < 5) {
        continue;
      }
      out.push({
        line: Number.parseInt(parts[0], 10) || 1,
        col: Number.parseInt(parts[1], 10) || 1,
        endLine: Number.parseInt(parts[2], 10) || 1,
        endCol: Number.parseInt(parts[3], 10) || 1,
        message: parts.slice(4).join(":")
      });
    }
    return out;
  }

  function parseHighlight(raw) {
    if (!raw) {
      return [];
    }
    const runs = [];
    for (const line of raw.split("\n")) {
      if (!line) {
        continue;
      }
      const parts = line.split(" ");
      if (parts.length < 3) {
        continue;
      }
      const start = Number.parseInt(parts[0], 10);
      const len = Number.parseInt(parts[1], 10);
      const cls = parts[2];
      if (!Number.isNaN(start) && !Number.isNaN(len) && len > 0) {
        runs.push({ start, end: start + len, cls });
      }
    }
    return runs;
  }

  function lineColToCharOffset(src, targetLine, targetCol) {
    let line = 1;
    let idx = 0;
    while (line < targetLine && idx < src.length) {
      if (src.charCodeAt(idx) === 10) {
        line += 1;
      }
      idx += 1;
    }
    let col = 1;
    while (col < targetCol && idx < src.length && src.charCodeAt(idx) !== 10) {
      idx += 1;
      col += 1;
    }
    return idx;
  }

  function buildDiagByteRanges(src, totalBytes, diags) {
    const ranges = [];
    for (const d of diags) {
      const startChar = lineColToCharOffset(src, d.line, d.col);
      let endChar = lineColToCharOffset(src, d.endLine, d.endCol);
      if (endChar <= startChar) {
        if (startChar < src.length && src.charCodeAt(startChar) !== 10) {
          endChar = startChar + 1;
        } else {
          endChar = startChar;
        }
      }
      const startByte = encoder.encode(src.slice(0, startChar)).length;
      let endByte = encoder.encode(src.slice(0, endChar)).length;
      if (endByte <= startByte && startByte < totalBytes) {
        endByte = Math.min(totalBytes, startByte + 1);
      }
      if (startByte < endByte) {
        ranges.push([startByte, endByte]);
      }
    }
    return ranges;
  }

  function hasDiagOverlap(byteStart, byteEnd, diagRanges) {
    for (const [ds, de] of diagRanges) {
      if (byteStart < de && byteEnd > ds) {
        return true;
      }
    }
    return false;
  }

  function renderHighlighted(src, bytes, runs, diags) {
    const diagRanges = buildDiagByteRanges(src, bytes.length, diags);
    const cuts = new Set();
    for (const r of runs) {
      cuts.add(r.start);
      cuts.add(r.end);
    }
    for (const [ds, de] of diagRanges) {
      cuts.add(ds);
      cuts.add(de);
    }
    const sortedCuts = Array.from(cuts).sort((a, b) => a - b);

    const frag = document.createDocumentFragment();
    if (runs.length === 0) {
      codeHighlightInner.textContent = src;
      return;
    }

    let runIdx = 0;
    for (let i = 0; i + 1 < sortedCuts.length; i += 1) {
      const segStart = sortedCuts[i];
      const segEnd = sortedCuts[i + 1];
      if (segStart >= segEnd) {
        continue;
      }
      while (runIdx < runs.length && runs[runIdx].end <= segStart) {
        runIdx += 1;
      }
      if (runIdx >= runs.length) {
        break;
      }
      const run = runs[runIdx];
      const text = decoder.decode(bytes.subarray(segStart, segEnd));
      const isDiag = hasDiagOverlap(segStart, segEnd, diagRanges);
      if (run.cls === "plain" && !isDiag) {
        frag.appendChild(document.createTextNode(text));
      } else {
        const span = document.createElement("span");
        span.className = isDiag ? `hl-${run.cls} diag-err` : `hl-${run.cls}`;
        span.textContent = text;
        frag.appendChild(span);
      }
    }

    if (src.endsWith("\n")) {
      frag.appendChild(document.createTextNode("\n"));
    }
    codeHighlightInner.replaceChildren(frag);
  }

  function renderDiagnostics(diags) {
    const frag = document.createDocumentFragment();
    for (const d of diags) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "diag-item";
      btn.textContent = `error: ${d.line}:${d.col}: ${d.message}`;
      btn.addEventListener("click", () => {
        jumpTo(d.line, d.col);
      });
      frag.appendChild(btn);
    }
    diagList.replaceChildren(frag);
  }

  function jumpTo(line, col) {
    if (!activeFile) {
      return;
    }
    if (window.innerWidth <= 760) {
      setMobileView("editor");
    }
    const offset = lineColToCharOffset(codeInput.value, line, col);
    codeInput.focus();
    codeInput.setSelectionRange(offset, offset);
    const targetScroll = Math.max(0, (line - 3) * LINE_HEIGHT_PX);
    codeInput.scrollTop = targetScroll;
    syncScroll();
    updateCursorMeta();
  }

  function refreshAnalysis() {
    if (!activeFile) {
      codeHighlightInner.textContent = "";
      diagList.replaceChildren();
      return;
    }
    const src = codeInput.value;
    if (!wasm) {
      codeHighlightInner.textContent = src.endsWith("\n") ? `${src}\n` : src;
      diagList.replaceChildren();
      return;
    }
    const bytes = encoder.encode(src);
    const hlLen = withBytes(bytes, (ptr, len) => wasm.athena_highlight(ptr, len));
    const runs = parseHighlight(readAthenaLast(hlLen));

    const chkLen = withBytes(bytes, (ptr, len) => wasm.athena_check(ptr, len));
    const diags = parseDiagnostics(readAthenaLast(chkLen));

    renderHighlighted(src, bytes, runs, diags);
    renderDiagnostics(diags);
  }

  function scheduleAnalysis() {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      refreshAnalysis();
      saveStatus.textContent = "saved";
    }, DEBOUNCE_MS);
  }

  function updateGutter(src) {
    const lines = src.split("\n").length;
    const nums = [];
    for (let i = 1; i <= lines; i += 1) {
      nums.push(String(i));
    }
    gutter.textContent = nums.join("\n");
  }

  function updateCursorMeta() {
    const pos = codeInput.selectionStart || 0;
    const before = codeInput.value.slice(0, pos);
    const parts = before.split("\n");
    const line = parts.length;
    const col = parts[parts.length - 1].length + 1;
    editorMeta.textContent = `Ln ${line}, Col ${col}`;
  }

  function syncScroll() {
    codeHighlight.scrollTop = codeInput.scrollTop;
    codeHighlight.scrollLeft = codeInput.scrollLeft;
    gutter.scrollTop = codeInput.scrollTop;
  }

  function syncEditorFromState() {
    const hasFile = activeFile !== null && Object.prototype.hasOwnProperty.call(files, activeFile);
    activeFileName.textContent = hasFile ? activeFile : "none";
    editorEmpty.hidden = hasFile;
    editorWrap.hidden = !hasFile;
    btnRun.disabled = !hasFile || !wasm;
    btnTest.disabled = !hasFile || !wasm;
    if (!hasFile) {
      codeInput.value = "";
      codeHighlightInner.textContent = "";
      gutter.textContent = "";
      diagList.replaceChildren();
      editorMeta.textContent = "No file";
      return;
    }
    const src = files[activeFile];
    codeInput.value = src;
    updateGutter(src);
    updateCursorMeta();
    refreshAnalysis();
    syncScroll();
  }

  codeInput.addEventListener("input", () => {
    if (!activeFile) {
      return;
    }
    files[activeFile] = codeInput.value;
    saveStatus.textContent = "saving…";
    storageSet(STORAGE_KEY, JSON.stringify(files));
    updateGutter(codeInput.value);
    updateCursorMeta();
    scheduleAnalysis();
  });

  codeInput.addEventListener("scroll", syncScroll);
  codeInput.addEventListener("click", updateCursorMeta);
  codeInput.addEventListener("keyup", updateCursorMeta);

  codeInput.addEventListener("keydown", (ev) => {
    if (ev.key === "Tab") {
      ev.preventDefault();
      const start = codeInput.selectionStart;
      const end = codeInput.selectionEnd;
      const val = codeInput.value;
      codeInput.value = `${val.slice(0, start)}    ${val.slice(end)}`;
      codeInput.setSelectionRange(start + 4, start + 4);
      codeInput.dispatchEvent(new Event("input", { bubbles: true }));
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
      ev.preventDefault();
      runActiveFile();
      return;
    }
    if ((ev.ctrlKey || ev.metaKey) && (ev.key === "s" || ev.key === "S")) {
      ev.preventDefault();
      if (debounceTimer !== null) {
        clearTimeout(debounceTimer);
        debounceTimer = null;
      }
      if (activeFile) {
        files[activeFile] = codeInput.value;
      }
      saveFiles();
      refreshAnalysis();
    }
  });

  function sortedFileNames() {
    return Object.keys(files).sort((a, b) => a.localeCompare(b));
  }

  function validateFileName(name, currentName) {
    const trimmed = name.trim();
    if (!trimmed) {
      return { ok: false, error: "Name cannot be empty." };
    }
    if (trimmed.includes("/") || trimmed.includes("\\")) {
      return { ok: false, error: "Slashes are not allowed." };
    }
    if (
      trimmed !== currentName &&
      Object.prototype.hasOwnProperty.call(files, trimmed)
    ) {
      return { ok: false, error: `"${trimmed}" already exists.` };
    }
    return { ok: true, name: trimmed };
  }

  function renderFileTree() {
    const frag = document.createDocumentFragment();
    for (const name of sortedFileNames()) {
      const li = document.createElement("li");
      li.className = name === activeFile ? "file-row is-active" : "file-row";

      if (editingState && editingState.mode === "rename" && editingState.name === name) {
        const form = document.createElement("form");
        form.className = "inline-form";
        const inp = document.createElement("input");
        inp.type = "text";
        inp.className = "inline-input";
        inp.value = name;
        inp.setAttribute("aria-label", `Rename ${name}`);

        const err = document.createElement("div");
        err.className = "inline-error";

        const actions = document.createElement("div");
        actions.className = "inline-form-actions";

        const saveBtn = document.createElement("button");
        saveBtn.type = "submit";
        saveBtn.className = "btn btn-sm btn-primary";
        saveBtn.textContent = "Save";

        const cancelBtn = document.createElement("button");
        cancelBtn.type = "button";
        cancelBtn.className = "btn btn-sm";
        cancelBtn.textContent = "Cancel";
        cancelBtn.addEventListener("click", () => {
          editingState = null;
          renderFileTree();
        });

        form.addEventListener("submit", (ev) => {
          ev.preventDefault();
          const check = validateFileName(inp.value, name);
          if (!check.ok) {
            err.textContent = check.error;
            return;
          }
          const nextName = check.name;
          if (nextName !== name) {
            const content = files[name];
            delete files[name];
            files[nextName] = content;
            if (activeFile === name) {
              activeFile = nextName;
              storageSet(ACTIVE_KEY, nextName);
            }
            saveFiles();
          }
          editingState = null;
          syncEditorFromState();
          renderFileTree();
        });

        actions.append(saveBtn, cancelBtn);
        form.append(inp, actions, err);
        li.appendChild(form);
        frag.appendChild(li);
        setTimeout(() => {
          inp.focus();
          inp.select();
        }, 0);
        continue;
      }

      const mainRow = document.createElement("div");
      mainRow.className = "file-row-main";

      const openBtn = document.createElement("button");
      openBtn.type = "button";
      openBtn.className = "file-open-btn";
      openBtn.textContent = name;
      openBtn.addEventListener("click", () => {
        editingState = null;
        setActiveFile(name);
        if (window.innerWidth <= 760) {
          setMobileView("editor");
        }
      });
      mainRow.appendChild(openBtn);

      const actions = document.createElement("div");
      actions.className = "file-row-actions";

      if (editingState && editingState.mode === "delete" && editingState.name === name) {
        const confirmBtn = document.createElement("button");
        confirmBtn.type = "button";
        confirmBtn.className = "btn btn-sm btn-danger";
        confirmBtn.textContent = "Confirm";
        confirmBtn.addEventListener("click", () => {
          delete files[name];
          editingState = null;
          saveFiles();
          if (activeFile === name) {
            const remaining = sortedFileNames();
            setActiveFile(remaining.length > 0 ? remaining[0] : null);
          } else {
            renderFileTree();
          }
        });

        const cancelBtn = document.createElement("button");
        cancelBtn.type = "button";
        cancelBtn.className = "btn btn-sm";
        cancelBtn.textContent = "Cancel";
        cancelBtn.addEventListener("click", () => {
          editingState = null;
          renderFileTree();
        });

        actions.append(confirmBtn, cancelBtn);
      } else {
        const renameBtn = document.createElement("button");
        renameBtn.type = "button";
        renameBtn.className = "btn btn-sm";
        renameBtn.textContent = "Rename";
        renameBtn.setAttribute("aria-label", `Rename ${name}`);
        renameBtn.addEventListener("click", () => {
          editingState = { mode: "rename", name };
          renderFileTree();
        });

        const delBtn = document.createElement("button");
        delBtn.type = "button";
        delBtn.className = "btn btn-sm btn-danger";
        delBtn.textContent = "Delete";
        delBtn.setAttribute("aria-label", `Delete ${name}`);
        delBtn.addEventListener("click", () => {
          editingState = { mode: "delete", name };
          renderFileTree();
        });

        actions.append(renameBtn, delBtn);
      }

      li.append(mainRow, actions);
      frag.appendChild(li);
    }
    fileList.replaceChildren(frag);
  }

  btnNewFile.addEventListener("click", () => {
    newFileError.textContent = "";
    newFileInput.value = "";
    newFileForm.hidden = false;
    newFileInput.focus();
  });

  btnNewCancel.addEventListener("click", () => {
    newFileForm.hidden = true;
    newFileError.textContent = "";
  });

  newFileForm.addEventListener("submit", (ev) => {
    ev.preventDefault();
    const check = validateFileName(newFileInput.value, null);
    if (!check.ok) {
      newFileError.textContent = check.error;
      return;
    }
    const name = check.name;
    files[name] = name.endsWith(".logos") ? "fn main() {\n  \n}\n" : "";
    newFileForm.hidden = true;
    newFileError.textContent = "";
    saveFiles();
    setActiveFile(name);
    if (window.innerWidth <= 760) {
      setMobileView("editor");
    }
  });

  function runActiveFile() {
    if (!wasm || !activeFile) {
      return;
    }
    setRightTab("output");
    if (window.innerWidth <= 760) {
      setMobileView("output");
    }
    runOut.classList.remove("is-error");
    runOut.textContent = "running…";
    const src = codeInput.value;
    requestAnimationFrame(() => {
      const bytes = encoder.encode(src);
      const outLen = withBytes(bytes, (ptr, len) => wasm.logos_run(ptr, len, LOGOS_FUEL));
      const text = readNeonLast(outLen);
      runOut.textContent = text || "(no output)";
      if (text.startsWith("error: ") || text.startsWith("trap: ")) {
        runOut.classList.add("is-error");
      }
      refreshAnalysis();
    });
  }

  function renderTestResults(raw) {
    if (!raw) {
      testSummary.className = "test-summary";
      testSummary.textContent = "No unit tests or properties found in this file.";
      testList.replaceChildren();
      return;
    }
    const lines = raw.split("\n").filter(Boolean);
    let passed = 0;
    let failed = 0;
    const frag = document.createDocumentFragment();

    for (const line of lines) {
      const li = document.createElement("li");
      if (line.startsWith("PASS ")) {
        passed += 1;
        li.className = "test-item is-pass";
        const badge = document.createElement("span");
        badge.className = "test-badge-pass";
        badge.textContent = "PASS ";
        li.append(badge, document.createTextNode(line.slice(5)));
      } else if (line.startsWith("FAIL ")) {
        failed += 1;
        li.className = "test-item is-fail";
        const badge = document.createElement("span");
        badge.className = "test-badge-fail";
        badge.textContent = "FAIL ";
        li.append(badge, document.createTextNode(line.slice(5)));
      } else {
        failed += 1;
        li.className = "test-item is-fail";
        li.textContent = line;
      }
      frag.appendChild(li);
    }

    testSummary.className = failed === 0 ? "test-summary is-ok" : "test-summary is-bad";
    testSummary.textContent = `${passed} passed, ${failed} failed`;
    testList.replaceChildren(frag);
  }

  function testActiveFile() {
    if (!wasm || !activeFile) {
      return;
    }
    setRightTab("tests");
    if (window.innerWidth <= 760) {
      setMobileView("output");
    }
    testSummary.className = "test-summary";
    testSummary.textContent = "running tests…";
    const src = codeInput.value;
    requestAnimationFrame(() => {
      const bytes = encoder.encode(src);
      const outLen = withBytes(bytes, (ptr, len) => wasm.athena_test(ptr, len, LOGOS_FUEL));
      renderTestResults(readAthenaLast(outLen));
    });
  }

  btnRun.addEventListener("click", runActiveFile);
  btnTest.addEventListener("click", testActiveFile);

  function appendTerm(text, cls) {
    if (!text) {
      return;
    }
    if (cls) {
      const span = document.createElement("span");
      span.className = cls;
      span.textContent = text;
      termOut.appendChild(span);
    } else {
      termOut.appendChild(document.createTextNode(text));
    }
    termOut.scrollTop = termOut.scrollHeight;
  }

  function setTermInputEnabled(enabled) {
    termIn.disabled = !enabled;
    termSend.disabled = !enabled;
    if (enabled) {
      termIn.placeholder = "type command and press Enter";
    }
  }

  function pumpFrame() {
    pumping = false;
    if (!wasm) {
      return;
    }
    const rc = wasm.titan_pump(STEPS_PER_FRAME);
    const outLen = wasm.titan_output();
    if (outLen > 0) {
      appendTerm(readNeonLast(outLen));
    }
    if (rc === 1) {
      titanStatus.textContent = "running";
      pumping = true;
      requestAnimationFrame(pumpFrame);
    } else if (rc === 0) {
      titanStatus.textContent = "waiting for input";
      setTermInputEnabled(true);
    } else if (rc >= 2) {
      titanStatus.textContent = `exited (${rc - 2})`;
      setTermInputEnabled(false);
    } else {
      titanStatus.textContent = "not booted";
      setTermInputEnabled(false);
    }
  }

  function startPump() {
    if (!pumping) {
      pumping = true;
      requestAnimationFrame(pumpFrame);
    }
  }

  async function bootTitanIfNeeded() {
    if (titanBootStarted || !wasm) {
      return;
    }
    titanBootStarted = true;
    titanStatus.textContent = "fetching kernel…";
    try {
      const elfResp = await fetch("titan.elf");
      if (!elfResp.ok) {
        throw new Error(`failed to fetch titan.elf: ${elfResp.status}`);
      }
      const elfBytes = new Uint8Array(await elfResp.arrayBuffer());
      const rc = withBytes(elfBytes, (ptr, len) => wasm.titan_boot(ptr, len));
      if (rc !== 0) {
        const errLen = wasm.neon_last_len();
        throw new Error(readNeonLast(errLen) || "titan_boot failed");
      }
      titanStatus.textContent = "booting…";
      startPump();
    } catch (err) {
      titanStatus.textContent = "failed";
      appendTerm(`\n[titan error] ${err && err.message ? err.message : String(err)}\n`);
    }
  }

  termForm.addEventListener("submit", (ev) => {
    ev.preventDefault();
    if (!wasm || termIn.disabled) {
      return;
    }
    const line = termIn.value;
    termIn.value = "";
    appendTerm(`${line}\n`, "user-cmd");
    setTermInputEnabled(false);
    const bytes = encoder.encode(line);
    withBytes(bytes, (ptr, len) => wasm.titan_send(ptr, len));
    startPump();
    termIn.focus();
  });

  function setupResizer(handle, cssVar, minPx, maxPx, sign) {
    function currentWidth() {
      const val = getComputedStyle(document.documentElement).getPropertyValue(cssVar);
      return Number.parseInt(val, 10) || minPx;
    }

    function applyWidth(px) {
      const clamped = Math.max(minPx, Math.min(maxPx, px));
      document.documentElement.style.setProperty(cssVar, `${clamped}px`);
    }

    handle.addEventListener("pointerdown", (ev) => {
      ev.preventDefault();
      handle.setPointerCapture(ev.pointerId);
      const startX = ev.clientX;
      const startW = currentWidth();

      function onMove(moveEv) {
        applyWidth(startW + (moveEv.clientX - startX) * sign);
      }

      function onUp() {
        handle.removeEventListener("pointermove", onMove);
        handle.removeEventListener("pointerup", onUp);
      }

      handle.addEventListener("pointermove", onMove);
      handle.addEventListener("pointerup", onUp);
    });

    handle.addEventListener("keydown", (ev) => {
      if (ev.key === "ArrowLeft") {
        ev.preventDefault();
        applyWidth(currentWidth() - 16 * sign);
      } else if (ev.key === "ArrowRight") {
        ev.preventDefault();
        applyWidth(currentWidth() + 16 * sign);
      }
    });
  }

  setupResizer(resizerLeft, "--left-w", 140, 420, 1);
  setupResizer(resizerRight, "--right-w", 220, 640, -1);

  async function loadWasm() {
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        const res = await WebAssembly.instantiateStreaming(fetch("athena.wasm"), {});
        return res.instance.exports;
      } catch (_err) {
        // Fallback when the server does not serve application/wasm MIME type.
      }
    }
    const resp = await fetch("athena.wasm");
    if (!resp.ok) {
      throw new Error(`failed to fetch athena.wasm: ${resp.status}`);
    }
    const buf = await resp.arrayBuffer();
    const res = await WebAssembly.instantiate(buf, {});
    return res.instance.exports;
  }

  async function init() {
    renderFileTree();
    syncEditorFromState();
    try {
      wasm = await loadWasm();
      wasmStatus.textContent = "wasm ready";
      syncEditorFromState();
      if (!tabPanels.terminal.hidden) {
        bootTitanIfNeeded();
      }
    } catch (err) {
      wasmStatus.textContent = "wasm error";
      runOut.classList.add("is-error");
      runOut.textContent = err && err.message ? err.message : String(err);
    }
  }

  init();
})();
