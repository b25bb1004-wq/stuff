import { showEl, leave } from "./athena-motion.js";

const PALETTE_COMMANDS = [
  { id: "run", label: "Run", shortcut: "Ctrl+Enter" },
  { id: "test", label: "Test", shortcut: "Alt+T" },
  { id: "format", label: "Format", shortcut: "Shift+Alt+F" },
  { id: "new-file", label: "New File", shortcut: "Alt+N" },
  { id: "save", label: "Save", shortcut: "Ctrl+S" },
  { id: "close-tab", label: "Close Tab", shortcut: "Ctrl+W" },
  { id: "toggle-panel", label: "Toggle Panel", shortcut: "Ctrl+`" },
  { id: "toggle-sidebar", label: "Toggle Sidebar", shortcut: "Ctrl+B" },
  { id: "toggle-theme", label: "Toggle Theme", shortcut: "Alt+M" },
  { id: "goto-definition", label: "Go to Definition", shortcut: "F12" },
  { id: "show-hover", label: "Show Hover", shortcut: "Ctrl+I" },
  { id: "focus-terminal", label: "Focus Terminal", shortcut: "Alt+J" },
  { id: "install-app", label: "Install App", shortcut: "Alt+I" },
  { id: "debug-continue", label: "Debug: Continue", shortcut: "F5" },
  { id: "debug-step-over", label: "Debug: Step Over", shortcut: "F10" },
  { id: "debug-step-into", label: "Debug: Step Into", shortcut: "F11" },
  { id: "debug-step-out", label: "Debug: Step Out", shortcut: "Shift+F11" },
  { id: "debug-restart", label: "Debug: Restart", shortcut: "Ctrl+Shift+F5" },
  { id: "debug-stop", label: "Debug: Stop", shortcut: "Shift+F5" }
];

const $ = (id) => document.getElementById(id);

function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

function mkBtn(cls, text, onClick, ariaLabel) {
  const b = el("button", cls, text);
  b.type = "button";
  if (ariaLabel) b.setAttribute("aria-label", ariaLabel);
  if (onClick) b.addEventListener("click", onClick);
  return b;
}

function subsequencePositions(query, text) {
  if (!query) return [];
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const positions = [];
  let qi = 0;
  for (let ti = 0; ti < t.length && qi < q.length; ti += 1) {
    if (t.charCodeAt(ti) === q.charCodeAt(qi)) {
      positions.push(ti);
      qi += 1;
    }
  }
  return qi === q.length ? positions : null;
}

function matchScore(positions) {
  if (!positions || positions.length === 0) return 0;
  let gaps = 0;
  for (let i = 1; i < positions.length; i += 1) gaps += positions[i] - positions[i - 1] - 1;
  return positions[0] * 4 + gaps * 10;
}

function appendHighlightedText(parent, text, positions) {
  if (!positions || positions.length === 0) {
    parent.appendChild(document.createTextNode(text));
    return;
  }
  const set = new Set(positions);
  let buf = "";
  let inMark = false;
  const flush = () => {
    if (!buf) return;
    parent.appendChild(inMark ? el("mark", "search-mark", buf) : document.createTextNode(buf));
    buf = "";
  };
  for (let i = 0; i < text.length; i += 1) {
    const m = set.has(i);
    if (m !== inMark) {
      flush();
      inMark = m;
    }
    buf += text[i];
  }
  flush();
}

export function createWorkbench(app) {
  const [
    workbench, titlebarTabs, editorTabbarHost, editorTabbar,
    btnRun, btnTest, btnFormat, btnPalette, btnTheme, btnInstall,
    btnNewFile, newFileForm, newFileInput, btnNewCancel, newFileError, fileList,
    searchInput, searchSummary, searchResults, btnRunAllTests, testingSidebarSummary, testingSidebarList,
    tabStrip, unsavedBar, unsavedMsg, btnUnsavedSave, btnUnsavedDiscard, btnUnsavedCancel,
    resizerLeft, resizerPanel, btnPanelToggle, problemsBadge, problemsEmpty, diagList,
    wasmStatus, statusFile, saveStatus, statusCursor, statusProblems, statusLang,
    paletteBackdrop, paletteDialog, paletteTitle, paletteClose, paletteInput, paletteList
  ] = [
    "workbench", "titlebar-tabs", "editor-tabbar-host", "editor-tabbar",
    "btn-run", "btn-test", "btn-format", "btn-palette", "btn-theme", "btn-install",
    "btn-new-file", "new-file-form", "new-file-input", "btn-new-cancel", "new-file-error", "file-list",
    "search-input", "search-summary", "search-results", "btn-run-all-tests", "testing-sidebar-summary", "testing-sidebar-list",
    "tab-strip", "unsaved-bar", "unsaved-msg", "btn-unsaved-save", "btn-unsaved-discard", "btn-unsaved-cancel",
    "resizer-left", "resizer-panel", "btn-panel-toggle", "problems-badge", "problems-empty", "diag-list",
    "wasm-status", "status-file", "save-status", "status-cursor", "status-problems", "status-lang",
    "palette-backdrop", "palette-dialog", "palette-title", "palette-close", "palette-input", "palette-list"
  ].map($);

  const actButtons = Array.from(document.querySelectorAll(".act-btn[data-view]"));
  const mobileActButtons = Array.from(document.querySelectorAll(".act-btn[data-mobile]"));
  const sidebarViews = { explorer: $("sidebar-explorer"), search: $("sidebar-search"), testing: $("sidebar-testing"), debug: $("sidebar-debug") };
  const panelTabButtons = Array.from(document.querySelectorAll(".tab-btn[data-panel-tab]"));
  const panelBodies = {
    problems: $("panel-problems"),
    output: $("panel-output"),
    tests: $("panel-tests"),
    terminal: $("panel-terminal")
  };

  let sidebarView = "explorer";
  let sidebarOpen = true;
  let panelOpen = true;
  const bottomPanel = $("bottom-panel");
  let activePanelTab = "problems";
  let editingState = null;
  let pendingCloseTab = null;
  let deferredInstallPrompt = null;
  let testStatusByFile = {};
  const paletteState = { open: false, mode: "command", items: [], selectedIndex: 0, lastFocus: null };

  const isMobile = () => window.innerWidth <= 720;

  function currentTheme() {
    const explicit = document.documentElement.getAttribute("data-theme");
    if (explicit === "light" || explicit === "dark") return explicit;
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }

  function updateThemeButton() {
    const t = currentTheme();
    btnTheme.textContent = t === "light" ? "Dark" : "Light";
    btnTheme.setAttribute("aria-label", `Switch to ${t === "light" ? "dark" : "light"} theme`);
  }

  function toggleTheme() {
    document.documentElement.setAttribute("data-theme", currentTheme() === "light" ? "dark" : "light");
    updateThemeButton();
  }

  btnTheme.addEventListener("click", toggleTheme);
  updateThemeButton();

  const isStandaloneMode = () =>
    (window.matchMedia && window.matchMedia("(display-mode: standalone)").matches) ||
    window.navigator.standalone === true;

  function updateInstallVisibility() {
    btnInstall.hidden = !(Boolean(deferredInstallPrompt) && !isStandaloneMode());
  }

  async function triggerInstall() {
    if (!deferredInstallPrompt) {
      app.setTransientStatus("Install prompt is not available right now.", false);
      return;
    }
    const promptEv = deferredInstallPrompt;
    deferredInstallPrompt = null;
    updateInstallVisibility();
    try {
      await promptEv["prompt"]();
      await promptEv.userChoice;
    } catch (_err) {
      // Ignore canceled prompt errors.
    }
  }

  window.addEventListener("beforeinstallprompt", (ev) => {
    ev.preventDefault();
    deferredInstallPrompt = ev;
    updateInstallVisibility();
  });
  window.addEventListener("appinstalled", () => {
    deferredInstallPrompt = null;
    updateInstallVisibility();
  });

  if (window.matchMedia) {
    const standaloneMedia = window.matchMedia("(display-mode: standalone)");
    if (typeof standaloneMedia.addEventListener === "function") {
      standaloneMedia.addEventListener("change", updateInstallVisibility);
    }
    const wcoMedia = window.matchMedia("(display-mode: window-controls-overlay)");
    const syncWcoPlacement = () => {
      const target = wcoMedia.matches ? titlebarTabs : editorTabbarHost;
      if (editorTabbar.parentElement !== target) target.appendChild(editorTabbar);
    };
    syncWcoPlacement();
    if (typeof wcoMedia.addEventListener === "function") wcoMedia.addEventListener("change", syncWcoPlacement);
  }

  btnInstall.addEventListener("click", triggerInstall);
  updateInstallVisibility();

  function setMobileView(view) {
    workbench.setAttribute("data-mobile-view", view);
    for (const b of actButtons) {
      const v = b.dataset.view;
      const active = (view === "files" && v === "explorer") || (view === "search" && v === "search") || (view === "debug" && v === "debug");
      if (isMobile()) {
        b.classList.toggle("is-active", active);
        b.setAttribute("aria-pressed", active ? "true" : "false");
      }
    }
    for (const b of mobileActButtons) {
      const active = b.dataset.mobile === view;
      b.classList.toggle("is-active", active);
      b.setAttribute("aria-pressed", active ? "true" : "false");
    }
    if (view === "files" || view === "search" || view === "debug") {
      sidebarView = view === "files" ? "explorer" : view;
      syncSidebarViews();
    } else if (view === "panel" && activePanelTab === "terminal") {
      app.onTerminalTabOpened();
    }
  }

  function syncSidebarViews() {
    workbench.setAttribute("data-sidebar-open", sidebarOpen ? "true" : "false");
    for (const [key, node] of Object.entries(sidebarViews)) showEl(node, key === sidebarView);
    if (!isMobile()) {
      for (const b of actButtons) {
        const active = sidebarOpen && b.dataset.view === sidebarView;
        b.classList.toggle("is-active", active);
        b.setAttribute("aria-pressed", active ? "true" : "false");
      }
    }
    if (sidebarView === "search") renderSearchResults();
    else if (sidebarView === "testing") renderTestingSidebar();
  }

  function toggleSidebar(forceOpen) {
    sidebarOpen = typeof forceOpen === "boolean" ? forceOpen : !sidebarOpen;
    syncSidebarViews();
  }

  function activateSidebarView(view) {
    if (isMobile()) {
      setMobileView(view === "explorer" ? "files" : view);
      if (view === "search") searchInput.focus();
      return;
    }
    if (sidebarOpen && sidebarView === view) {
      sidebarOpen = false;
    } else {
      sidebarView = view;
      sidebarOpen = true;
    }
    syncSidebarViews();
    if (sidebarOpen && view === "search") searchInput.focus();
  }

  for (const b of actButtons) b.addEventListener("click", () => activateSidebarView(b.dataset.view));
  for (const b of mobileActButtons) b.addEventListener("click", () => setMobileView(b.dataset.mobile));

  function setPanelOpen(open) {
    const was = panelOpen;
    panelOpen = open;
    btnPanelToggle.textContent = panelOpen ? "Hide" : "Show";
    if (open === was) return;
    if (open) {
      workbench.setAttribute("data-panel-open", "true");
      Motion.enter(bottomPanel, { curve: "firm", dist: 1 });
    } else {
      leave(bottomPanel, () => panelOpen, () => workbench.setAttribute("data-panel-open", "false"));
    }
  }

  function togglePanel() {
    if (isMobile()) {
      const cur = workbench.getAttribute("data-mobile-view");
      setMobileView(cur === "panel" ? "editor" : "panel");
      return;
    }
    setPanelOpen(!panelOpen);
  }

  function setPanelTab(tab) {
    activePanelTab = tab;
    setPanelOpen(true);
    for (const b of panelTabButtons) {
      const active = b.dataset.panelTab === tab;
      b.classList.toggle("is-active", active);
      b.setAttribute("aria-selected", active ? "true" : "false");
    }
    for (const [key, body] of Object.entries(panelBodies)) showEl(body, key === tab);
    if (tab === "terminal") app.onTerminalTabOpened();
    if (isMobile()) setMobileView("panel");
  }

  for (const b of panelTabButtons) b.addEventListener("click", () => setPanelTab(b.dataset.panelTab));
  btnPanelToggle.addEventListener("click", () => setPanelOpen(false));
  statusProblems.addEventListener("click", () => setPanelTab("problems"));

  function renderTabs() {
    const active = app.getActiveFile();
    const frag = document.createDocumentFragment();
    for (const name of app.getOpenTabs()) {
      const isActive = name === active;
      const tab = el("div", isActive ? "editor-tab is-active" : "editor-tab");
      const openBtn = mkBtn("editor-tab-open", name, () => app.setActiveFile(name));
      openBtn.setAttribute("role", "tab");
      openBtn.setAttribute("aria-selected", isActive ? "true" : "false");
      if (app.isDirty(name)) {
        const mark = el("span", "tab-dirty", "*");
        mark.setAttribute("aria-label", "unsaved");
        openBtn.appendChild(mark);
      }
      const closeBtn = mkBtn(
        "editor-tab-close",
        "×",
        (ev) => {
          ev.stopPropagation();
          requestCloseTab(name);
        },
        `Close ${name}`
      );
      tab.addEventListener("mousedown", (ev) => {
        if (ev.button === 1) ev.preventDefault();
      });
      tab.addEventListener("auxclick", (ev) => {
        if (ev.button === 1) {
          ev.preventDefault();
          requestCloseTab(name);
        }
      });
      tab.append(openBtn, closeBtn);
      frag.appendChild(tab);
    }
    tabStrip.replaceChildren(frag);
  }

  function requestCloseTab(name) {
    if (!name) return;
    if (app.isDirty(name)) {
      pendingCloseTab = name;
      unsavedMsg.textContent = `Save changes to "${name}" before closing?`;
      unsavedBar.hidden = false;
      btnUnsavedSave.focus();
      return;
    }
    app.closeTab(name);
  }

  function dismissUnsavedBar() {
    pendingCloseTab = null;
    unsavedBar.hidden = true;
  }

  btnUnsavedSave.addEventListener("click", () => {
    const name = pendingCloseTab;
    dismissUnsavedBar();
    if (name) {
      app.saveFile(name);
      app.closeTab(name);
    }
  });
  btnUnsavedDiscard.addEventListener("click", () => {
    const name = pendingCloseTab;
    dismissUnsavedBar();
    if (name) {
      app.discardDraft(name);
      app.closeTab(name);
    }
  });
  btnUnsavedCancel.addEventListener("click", () => {
    dismissUnsavedBar();
    app.focusEditor();
  });

  function renderFileTree() {
    const active = app.getActiveFile();
    const frag = document.createDocumentFragment();
    for (const row of app.getTreeRows()) {
      const name = row.path;
      const li = el("li", name === active ? "file-row is-active" : "file-row");
      if (row.depth > 0) li.style.marginLeft = `${row.depth * 12}px`;
      if (editingState && editingState.mode === "rename" && editingState.name === name) {
        const form = el("form", "inline-form");
        const inp = el("input", "inline-input");
        inp.type = "text";
        inp.value = name;
        inp.setAttribute("aria-label", `Rename ${name}`);
        const err = el("div", "inline-error");
        const actions = el("div", "inline-form-actions");
        const saveBtn = el("button", "btn btn-sm btn-primary", "Save");
        saveBtn.type = "submit";
        actions.append(saveBtn, mkBtn("btn btn-sm", "Cancel", () => {
          editingState = null;
          renderFileTree();
        }));
        form.addEventListener("submit", async (ev) => {
          ev.preventDefault();
          const res = await app.renameFile(name, inp.value);
          if (!res.ok) {
            err.textContent = res.error;
            return;
          }
          editingState = null;
          renderAll();
        });
        form.append(inp, actions, err);
        li.appendChild(form);
        frag.appendChild(li);
        setTimeout(() => {
          inp.focus();
          inp.select();
        }, 0);
        continue;
      }
      const mainRow = el("div", "file-row-main");
      const label = row.dir
        ? `${row.open ? "▾" : "▸"} ${row.name}`
        : app.isDirty(name) ? `${row.name} *` : row.name;
      mainRow.appendChild(
        mkBtn("file-open-btn", label, () => {
          editingState = null;
          if (row.dir) {
            app.toggleDir(name);
            return;
          }
          app.setActiveFile(name);
          if (isMobile()) setMobileView("editor");
        })
      );
      const actions = el("div", "file-row-actions");
      if (editingState && editingState.mode === "delete" && editingState.name === name) {
        actions.append(
          mkBtn("btn btn-sm btn-danger", "Confirm", async () => {
            editingState = null;
            if (pendingCloseTab === name) dismissUnsavedBar();
            await app.deleteFile(name);
            renderAll();
          }),
          mkBtn("btn btn-sm", "Cancel", () => {
            editingState = null;
            renderFileTree();
          })
        );
      } else {
        actions.append(
          mkBtn("btn btn-sm", "Rename", () => {
            editingState = { mode: "rename", name };
            renderFileTree();
          }, `Rename ${name}`),
          mkBtn("btn btn-sm btn-danger", "Delete", () => {
            editingState = { mode: "delete", name };
            renderFileTree();
          }, `Delete ${name}`)
        );
      }
      li.append(mainRow, actions);
      frag.appendChild(li);
    }
    fileList.replaceChildren(frag);
  }

  function startNewFile() {
    if (isMobile()) setMobileView("files");
    else {
      sidebarView = "explorer";
      sidebarOpen = true;
      syncSidebarViews();
    }
    newFileError.textContent = "";
    newFileInput.value = "";
    newFileForm.hidden = false;
    newFileInput.focus();
  }

  btnNewFile.addEventListener("click", startNewFile);
  btnNewCancel.addEventListener("click", () => {
    newFileForm.hidden = true;
    newFileError.textContent = "";
  });
  newFileForm.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const res = await app.createFile(newFileInput.value);
    if (!res.ok) {
      newFileError.textContent = res.error;
      return;
    }
    newFileForm.hidden = true;
    newFileError.textContent = "";
    renderAll();
    if (isMobile()) setMobileView("editor");
    app.focusEditor();
  });

  function renderSearchResults() {
    const query = searchInput.value;
    if (!query) {
      searchSummary.textContent = "Type to search workspace files.";
      searchResults.replaceChildren();
      return;
    }
    const lowerQ = query.toLowerCase();
    const frag = document.createDocumentFragment();
    let totalMatches = 0;
    let matchedFiles = 0;

    for (const name of app.getFileNames()) {
      const lines = app.getFileContent(name).split("\n");
      const hits = [];
      for (let i = 0; i < lines.length; i += 1) {
        const idx = lines[i].toLowerCase().indexOf(lowerQ);
        if (idx !== -1) hits.push({ line: i + 1, col: idx + 1, idx, text: lines[i] });
      }
      if (hits.length === 0) continue;
      matchedFiles += 1;
      totalMatches += hits.length;

      const group = el("div", "search-group");
      const head = el("div", "search-file-head");
      head.append(el("span", "", name), el("span", "", String(hits.length)));
      group.appendChild(head);

      for (const hit of hits) {
        const b = mkBtn("search-hit", undefined, () => {
          app.openFileAt(name, hit.line, hit.col, query.length);
          if (isMobile()) setMobileView("editor");
        });
        b.append(
          el("span", "search-ln", `Ln ${hit.line}:`),
          document.createTextNode(hit.text.slice(0, hit.idx)),
          el("mark", "search-mark", hit.text.slice(hit.idx, hit.idx + query.length)),
          document.createTextNode(hit.text.slice(hit.idx + query.length))
        );
        group.appendChild(b);
      }
      frag.appendChild(group);
    }

    searchSummary.textContent = `${totalMatches} result${totalMatches === 1 ? "" : "s"} in ${matchedFiles} file${matchedFiles === 1 ? "" : "s"}`;
    searchResults.replaceChildren(frag);
  }

  searchInput.addEventListener("input", renderSearchResults);

  function renderTestingSidebar() {
    const names = app.getFileNames().filter((n) => n.endsWith(".logos"));
    const frag = document.createDocumentFragment();
    let totalTests = 0;

    for (const name of names) {
      const lines = app.getFileContent(name).split("\n");
      const items = [];
      for (let i = 0; i < lines.length; i += 1) {
        const m = /^\s*(test|prop)\s+"([^"]+)"/.exec(lines[i]);
        if (m) items.push({ kind: m[1], label: m[2], line: i + 1 });
      }
      totalTests += items.length;

      const li = el("li", "testing-sidebar-item");
      const badge = testStatusByFile[name] ? ` (${testStatusByFile[name]})` : "";
      li.appendChild(mkBtn("", `${name} [${items.length}]${badge}`, () => {
        app.setActiveFile(name);
        app.testActiveFile();
      }));
      frag.appendChild(li);

      for (const item of items) {
        const sub = el("li", "testing-sidebar-item");
        sub.appendChild(mkBtn("", `  ${item.kind} "${item.label}"`, () => app.openFileAt(name, item.line, 1, 0)));
        frag.appendChild(sub);
      }
    }

    testingSidebarSummary.textContent =
      totalTests === 0
        ? "No test or prop blocks found."
        : `${totalTests} test/prop block${totalTests === 1 ? "" : "s"} across ${names.length} file${names.length === 1 ? "" : "s"}`;
    testingSidebarList.replaceChildren(frag);
  }

  btnRunAllTests.addEventListener("click", () => {
    const summary = app.runAllTests();
    if (summary) {
      testStatusByFile = summary.byFile;
      testingSidebarSummary.textContent = `${summary.passed} passed, ${summary.failed} failed`;
      renderTestingSidebar();
    }
  });

  function updateProblems(entries) {
    const count = entries.length;
    problemsBadge.textContent = String(count);
    problemsBadge.classList.toggle("has-errors", count > 0);
    statusProblems.textContent = count === 0 ? "0 problems" : `${count} error${count === 1 ? "" : "s"}`;
    statusProblems.classList.toggle("is-error", count > 0);
    problemsEmpty.hidden = count > 0;
    const frag = document.createDocumentFragment();
    for (const entry of entries) {
      frag.appendChild(
        mkBtn("diag-item", `${entry.file}:${entry.line}:${entry.col}: error: ${entry.message}`, () => {
          app.openFileAt(entry.file, entry.line, entry.col, 0);
          if (isMobile()) setMobileView("editor");
        })
      );
    }
    diagList.replaceChildren(frag);
  }

  function updateStatusBar({ activeFile, dirty, saveText, line, col, wasmReady }) {
    const hasFile = Boolean(activeFile);
    btnRun.disabled = !hasFile || !wasmReady;
    btnTest.disabled = !hasFile || !wasmReady;
    btnFormat.disabled = !hasFile || !wasmReady;
    statusFile.textContent = hasFile ? (dirty ? `${activeFile} *` : activeFile) : "no file";
    saveStatus.textContent = dirty ? "unsaved" : saveText || "saved";
    statusCursor.textContent = hasFile ? `Ln ${line}, Col ${col}` : "Ln -, Col -";
    statusLang.textContent = !hasFile ? "—" : activeFile.endsWith(".logos") ? "Logos" : "Plain Text";
  }

  const setWasmStatus = (text) => {
    wasmStatus.textContent = text;
  };

  const COMMAND_HANDLERS = {
    run: () => app.runActiveFile(),
    test: () => app.testActiveFile(),
    format: () => app.formatActiveFile(),
    "new-file": () => startNewFile(),
    save: () => {
      const active = app.getActiveFile();
      if (active) app.saveFile(active);
    },
    "close-tab": () => {
      const active = app.getActiveFile();
      if (active) requestCloseTab(active);
    },
    "toggle-panel": () => togglePanel(),
    "toggle-sidebar": () => {
      if (isMobile()) {
        const cur = workbench.getAttribute("data-mobile-view");
        setMobileView(cur === "files" || cur === "search" ? "editor" : "files");
      } else {
        toggleSidebar();
      }
    },
    "toggle-theme": () => toggleTheme(),
    "goto-definition": () => app.goToDefinitionAtCaret(),
    "show-hover": () => app.showHoverAtCaret(),
    "focus-terminal": () => {
      setPanelTab("terminal");
      app.focusTerminalInput();
    },
    "install-app": () => triggerInstall(),
    "debug-continue": () => app.debugCommand("debug-continue"),
    "debug-step-over": () => app.debugCommand("debug-step-over"),
    "debug-step-into": () => app.debugCommand("debug-step-into"),
    "debug-step-out": () => app.debugCommand("debug-step-out"),
    "debug-restart": () => app.debugCommand("debug-restart"),
    "debug-stop": () => app.debugCommand("debug-stop")
  };

  function executeCommand(id) {
    const handler = COMMAND_HANDLERS[id];
    if (handler) handler();
  }

  function buildPaletteItems(mode, query) {
    const out = [];
    const source =
      mode === "file"
        ? app.getFileNames().map((name) => ({
            kind: "file",
            id: name,
            label: name,
            shortcut: app.isDirty(name) ? "unsaved" : ""
          }))
        : PALETTE_COMMANDS.map((cmd) => ({ kind: "command", ...cmd }));
    for (const item of source) {
      const positions = subsequencePositions(query, item.label);
      if (positions !== null) out.push({ ...item, positions, score: matchScore(positions) });
    }
    if (query) out.sort((a, b) => a.score - b.score || a.label.localeCompare(b.label));
    return out;
  }

  function renderPaletteList() {
    paletteState.items = buildPaletteItems(paletteState.mode, paletteInput.value);
    if (paletteState.selectedIndex >= paletteState.items.length) {
      paletteState.selectedIndex = Math.max(0, paletteState.items.length - 1);
    }
    const frag = document.createDocumentFragment();
    if (paletteState.items.length === 0) {
      frag.appendChild(el("li", "palette-empty", paletteState.mode === "file" ? "No matching files." : "No matching commands."));
    } else {
      paletteState.items.forEach((item, idx) => {
        const li = el("li", idx === paletteState.selectedIndex ? "palette-item is-selected" : "palette-item");
        li.setAttribute("role", "option");
        li.setAttribute("aria-selected", idx === paletteState.selectedIndex ? "true" : "false");
        const labelSpan = el("span", "palette-item-label");
        appendHighlightedText(labelSpan, item.label, item.positions);
        li.appendChild(labelSpan);
        if (item.shortcut) li.appendChild(el("span", "palette-shortcut", item.shortcut));
        li.addEventListener("click", () => {
          paletteState.selectedIndex = idx;
          confirmPaletteSelection();
        });
        frag.appendChild(li);
      });
    }
    paletteList.replaceChildren(frag);
    const activeEl = paletteList.children[paletteState.selectedIndex];
    if (activeEl && typeof activeEl.scrollIntoView === "function") activeEl.scrollIntoView({ block: "nearest" });
  }

  function openPalette(mode) {
    if (!paletteState.open) {
      paletteState.lastFocus = document.activeElement;
      Motion.enter($("palette-dialog"), { curve: "firm", dist: 1 });
    }
    paletteState.open = true;
    paletteState.mode = mode;
    paletteState.selectedIndex = 0;
    paletteTitle.textContent = mode === "file" ? "Quick Open" : "Command Palette";
    paletteInput.placeholder = mode === "file" ? "Type a file name to open…" : "Type a command…";
    paletteInput.value = "";
    paletteBackdrop.hidden = false;
    renderPaletteList();
    paletteInput.focus();
  }

  function closePalette(restoreFocus) {
    if (!paletteState.open) return;
    paletteState.open = false;
    leave(paletteDialog, () => paletteState.open, () => { paletteBackdrop.hidden = true; });
    const prev = paletteState.lastFocus;
    paletteState.lastFocus = null;
    if (restoreFocus && prev && typeof prev.focus === "function") prev.focus();
  }

  function confirmPaletteSelection() {
    const item = paletteState.items[paletteState.selectedIndex];
    if (!item) return;
    if (item.kind === "file") {
      closePalette(false);
      app.setActiveFile(item.id);
      if (isMobile()) setMobileView("editor");
      app.focusEditor();
      return;
    }
    const selfFocus = ["new-file", "focus-terminal", "goto-definition", "show-hover"].includes(item.id);
    closePalette(!selfFocus);
    executeCommand(item.id);
  }

  btnPalette.addEventListener("click", () => openPalette("command"));
  paletteClose.addEventListener("click", () => closePalette(true));
  paletteBackdrop.addEventListener("click", (ev) => {
    if (ev.target === paletteBackdrop) closePalette(true);
  });
  paletteInput.addEventListener("input", () => {
    paletteState.selectedIndex = 0;
    renderPaletteList();
  });

  paletteDialog.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      ev.preventDefault();
      ev.stopPropagation();
      closePalette(true);
    } else if (ev.key === "Tab") {
      ev.preventDefault();
      (document.activeElement === paletteInput ? paletteClose : paletteInput).focus();
    } else if (ev.key === "ArrowDown" && paletteState.items.length > 0) {
      ev.preventDefault();
      paletteState.selectedIndex = (paletteState.selectedIndex + 1) % paletteState.items.length;
      renderPaletteList();
    } else if (ev.key === "ArrowUp" && paletteState.items.length > 0) {
      ev.preventDefault();
      paletteState.selectedIndex = (paletteState.selectedIndex - 1 + paletteState.items.length) % paletteState.items.length;
      renderPaletteList();
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      confirmPaletteSelection();
    }
  });

  function setupResizer(handle, cssVar, minPx, maxPx, axis, sign) {
    const currentSize = () =>
      Number.parseInt(getComputedStyle(document.documentElement).getPropertyValue(cssVar), 10) || minPx;
    const applySize = (px) =>
      document.documentElement.style.setProperty(cssVar, `${Math.max(minPx, Math.min(maxPx, px))}px`);

    handle.addEventListener("pointerdown", (ev) => {
      ev.preventDefault();
      handle.setPointerCapture(ev.pointerId);
      const startPos = axis === "x" ? ev.clientX : ev.clientY;
      const startSize = currentSize();
      const onMove = (moveEv) =>
        applySize(startSize + ((axis === "x" ? moveEv.clientX : moveEv.clientY) - startPos) * sign);
      const onUp = () => {
        handle.removeEventListener("pointermove", onMove);
        handle.removeEventListener("pointerup", onUp);
      };
      handle.addEventListener("pointermove", onMove);
      handle.addEventListener("pointerup", onUp);
    });

    handle.addEventListener("keydown", (ev) => {
      if (axis === "x" && (ev.key === "ArrowLeft" || ev.key === "ArrowRight")) {
        ev.preventDefault();
        applySize(currentSize() + (ev.key === "ArrowRight" ? 16 : -16) * sign);
      } else if (axis === "y" && (ev.key === "ArrowUp" || ev.key === "ArrowDown")) {
        ev.preventDefault();
        applySize(currentSize() + (ev.key === "ArrowDown" ? 16 : -16) * sign);
      }
    });
  }

  setupResizer(resizerLeft, "--left-w", 150, 440, "x", 1);
  setupResizer(resizerPanel, "--panel-h", 100, 480, "y", -1);

  document.addEventListener("keydown", (ev) => {
    const mod = ev.ctrlKey || ev.metaKey;
    if (paletteState.open) {
      if (ev.key === "Escape") {
        ev.preventDefault();
        closePalette(true);
      }
      return;
    }
    if (ev.key === "F1" && !mod && !ev.altKey && !ev.shiftKey) {
      ev.preventDefault();
      openPalette("command");
    } else if (mod && ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "p") {
      ev.preventDefault();
      openPalette("command");
    } else if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "p") {
      ev.preventDefault();
      openPalette("file");
    } else if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "b") {
      ev.preventDefault();
      executeCommand("toggle-sidebar");
    } else if (ev.ctrlKey && !ev.shiftKey && !ev.altKey && (ev.key === "`" || ev.code === "Backquote")) {
      ev.preventDefault();
      togglePanel();
    } else if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "w") {
      ev.preventDefault();
      executeCommand("close-tab");
    } else if (ev.ctrlKey && !ev.altKey && !ev.metaKey && ev.key === "Tab") {
      ev.preventDefault();
      app.cycleTab(ev.shiftKey ? -1 : 1);
    } else if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "s") {
      ev.preventDefault();
      executeCommand("save");
    } else if (ev.altKey && !mod && !ev.shiftKey) {
      const altMap = { t: "test", n: "new-file", m: "toggle-theme", j: "focus-terminal", i: "install-app" };
      const cmd = altMap[ev.key.toLowerCase()];
      if (cmd) {
        ev.preventDefault();
        executeCommand(cmd);
      }
    }
  });

  function renderAll() {
    renderTabs();
    renderFileTree();
    if (sidebarView === "search") renderSearchResults();
    else if (sidebarView === "testing") renderTestingSidebar();
  }

  return {
    renderAll,
    renderTabs,
    renderFileTree,
    setPanelTab,
    setMobileView,
    updateProblems,
    updateStatusBar,
    setWasmStatus,
    isMobile
  };
}
