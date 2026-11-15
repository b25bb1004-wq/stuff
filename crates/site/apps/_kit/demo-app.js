(() => {
  "use strict";

  const MAX_HISTORY = 20;

  function firstNonEmptyLine(text) {
    const lines = String(text || "").split(/\r?\n/);
    for (const line of lines) {
      const trimmed = line.trim();
      if (trimmed) {
        return trimmed;
      }
    }
    return "(empty)";
  }

  function formatTime(date) {
    const hh = String(date.getHours()).padStart(2, "0");
    const mm = String(date.getMinutes()).padStart(2, "0");
    const ss = String(date.getSeconds()).padStart(2, "0");
    return `${hh}:${mm}:${ss}`;
  }

  function mount(config) {
    const kit = window.Kit.mount(config);
    const root = document.getElementById("app-root");
    const createEl = window.Kit.createEl;
    const examples = Array.isArray(config.examples) ? config.examples : [];
    const defaultText = examples.length > 0 ? examples[0].input : "";

    const inputStore = kit.store("input");
    const historyStore = kit.store("history");

    let history = historyStore.getJson([]);
    if (!Array.isArray(history)) {
      history = [];
    }

    const tabsBar = createEl("div", "demo-tabs");
    tabsBar.setAttribute("role", "tablist");
    tabsBar.setAttribute("aria-label", "View switch");

    const tabInput = createEl("button", "demo-tab", "Input");
    tabInput.type = "button";
    tabInput.setAttribute("role", "tab");
    tabInput.setAttribute("aria-selected", "true");

    const tabOutput = createEl("button", "demo-tab", "Output");
    tabOutput.type = "button";
    tabOutput.setAttribute("role", "tab");
    tabOutput.setAttribute("aria-selected", "false");

    tabsBar.appendChild(tabInput);
    tabsBar.appendChild(tabOutput);

    const split = createEl("div", "demo-split");

    const leftPane = createEl("section", "demo-pane");
    leftPane.setAttribute("aria-label", "Input editor");

    const leftBar = createEl("div", "demo-toolbar");
    const editorLabel = createEl("label", "demo-toolbar-label", "Input");
    editorLabel.setAttribute("for", "demo-editor");

    const exLabel = createEl("label", "sr-only", "Examples");
    exLabel.setAttribute("for", "demo-examples");
    const exSelect = document.createElement("select");
    exSelect.id = "demo-examples";
    const placeholderOpt = document.createElement("option");
    placeholderOpt.value = "";
    placeholderOpt.textContent = "Examples…";
    exSelect.appendChild(placeholderOpt);
    examples.forEach((ex, idx) => {
      const opt = document.createElement("option");
      opt.value = String(idx);
      opt.textContent = ex.label || `Example ${idx + 1}`;
      exSelect.appendChild(opt);
    });

    const exportBtn = createEl("button", "", "Export");
    exportBtn.type = "button";
    const importBtn = createEl("button", "", "Import");
    importBtn.type = "button";

    leftBar.appendChild(editorLabel);
    leftBar.appendChild(exLabel);
    leftBar.appendChild(exSelect);
    leftBar.appendChild(createEl("span", "demo-toolbar-spacer"));
    leftBar.appendChild(exportBtn);
    leftBar.appendChild(importBtn);

    const textarea = document.createElement("textarea");
    textarea.id = "demo-editor";
    textarea.className = "demo-editor";
    textarea.spellcheck = false;
    const savedInput = inputStore.get(null);
    textarea.value = savedInput !== null ? savedInput : defaultText;

    leftPane.appendChild(leftBar);
    leftPane.appendChild(textarea);

    const rightPane = createEl("section", "demo-pane is-hidden-mobile");
    rightPane.setAttribute("aria-label", "Output and history");

    const rightBar = createEl("div", "demo-toolbar");
    const runBtn = createEl("button", "kit-btn-accent", "Run");
    runBtn.type = "button";
    const status = createEl("span", "demo-status", "ready");
    rightBar.appendChild(runBtn);
    rightBar.appendChild(status);

    const outputWrap = createEl("div", "demo-output-wrap");
    const outPre = createEl("pre", "demo-output", "Press Run or Ctrl+Enter to execute.");
    outPre.setAttribute("aria-live", "polite");
    const canvas = createEl("canvas", "demo-canvas");
    canvas.hidden = true;
    outputWrap.appendChild(outPre);
    outputWrap.appendChild(canvas);

    const historySection = createEl("div", "demo-history");
    const historyHead = createEl("div", "demo-history-head");
    historyHead.appendChild(createEl("span", "demo-toolbar-label", "History (last 20)"));
    const clearHistBtn = createEl("button", "", "Clear");
    clearHistBtn.type = "button";
    historyHead.appendChild(clearHistBtn);
    const historyList = createEl("div", "demo-history-list");
    historySection.appendChild(historyHead);
    historySection.appendChild(historyList);

    rightPane.appendChild(rightBar);
    rightPane.appendChild(outputWrap);
    rightPane.appendChild(historySection);

    split.appendChild(leftPane);
    split.appendChild(rightPane);

    root.innerHTML = "";
    root.appendChild(tabsBar);
    root.appendChild(split);

    function setActiveTab(tab) {
      const showInput = tab === "input";
      tabInput.setAttribute("aria-selected", showInput ? "true" : "false");
      tabOutput.setAttribute("aria-selected", showInput ? "false" : "true");
      leftPane.classList.toggle("is-hidden-mobile", !showInput);
      rightPane.classList.toggle("is-hidden-mobile", showInput);
    }

    tabInput.addEventListener("click", () => setActiveTab("input"));
    tabOutput.addEventListener("click", () => setActiveTab("output"));

    function saveInput() {
      inputStore.set(textarea.value);
    }

    function renderHistory() {
      historyList.innerHTML = "";
      if (history.length === 0) {
        historyList.appendChild(createEl("p", "demo-history-empty", "No runs recorded yet."));
        return;
      }
      history.forEach((entry) => {
        const item = createEl("button", "demo-history-item");
        item.type = "button";
        item.appendChild(createEl("span", "demo-history-time", entry.time || "--:--:--"));
        const badge = createEl(
          "span",
          entry.kind === "error" ? "demo-history-badge is-bad" : "demo-history-badge",
          entry.kind === "error" ? "err" : entry.kind === "pixels" ? "px" : "ok"
        );
        item.appendChild(badge);
        item.appendChild(createEl("span", "demo-history-line", entry.summary || firstNonEmptyLine(entry.input)));
        item.addEventListener("click", () => {
          textarea.value = entry.input || "";
          saveInput();
          if (entry.kind === "text" || entry.kind === "error") {
            canvas.hidden = true;
            outPre.hidden = false;
            outPre.className = entry.kind === "error" ? "demo-output is-error" : "demo-output";
            outPre.textContent = entry.output || "";
            status.className = entry.kind === "error" ? "demo-status is-bad" : "demo-status is-ok";
            status.textContent = entry.kind === "error" ? "error (restored)" : "ok (restored)";
          }
          setActiveTab("input");
          textarea.focus();
        });
        historyList.appendChild(item);
      });
    }

    function pushHistory(kind, inputText, outputText) {
      const entry = {
        time: formatTime(new Date()),
        kind,
        summary: firstNonEmptyLine(inputText),
        input: inputText,
        output: kind === "pixels" ? "" : String(outputText || "").slice(0, 4096)
      };
      history.unshift(entry);
      if (history.length > MAX_HISTORY) {
        history.length = MAX_HISTORY;
      }
      historyStore.setJson(history);
      renderHistory();
    }

    clearHistBtn.addEventListener("click", () => {
      history = [];
      historyStore.setJson(history);
      renderHistory();
    });

    exSelect.addEventListener("change", () => {
      if (exSelect.value === "") {
        return;
      }
      const idx = Number(exSelect.value);
      if (Number.isInteger(idx) && examples[idx]) {
        textarea.value = examples[idx].input;
        saveInput();
        setActiveTab("input");
      }
      exSelect.value = "";
    });

    exportBtn.addEventListener("click", () => {
      window.Kit.download(`${config.id}-input.txt`, textarea.value);
    });

    importBtn.addEventListener("click", async () => {
      const picked = await window.Kit.pickFile();
      if (picked && typeof picked.text === "string") {
        textarea.value = picked.text;
        saveInput();
        status.className = "demo-status";
        status.textContent = `imported ${picked.name}`;
      }
    });

    textarea.addEventListener("input", saveInput);

    textarea.addEventListener("keydown", (ev) => {
      if (ev.key === "Tab") {
        ev.preventDefault();
        const start = textarea.selectionStart;
        const end = textarea.selectionEnd;
        const val = textarea.value;
        textarea.value = val.slice(0, start) + "    " + val.slice(end);
        textarea.selectionStart = start + 4;
        textarea.selectionEnd = start + 4;
        saveInput();
      } else if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
        ev.preventDefault();
        runCurrent();
      }
    });

    async function runCurrent() {
      runBtn.disabled = true;
      status.className = "demo-status";
      status.textContent = "running…";
      setActiveTab("output");
      const currentInput = textarea.value;
      saveInput();

      try {
        const wasm = await window.Kit.wasm();
        const res =
          typeof config.run === "function"
            ? await config.run(wasm, currentInput)
            : wasm.runDemo(config.id, currentInput);

        if (res.kind === "pixels") {
          if (res.w > 0 && res.h > 0 && res.rgba && res.rgba.length === res.w * res.h * 4) {
            canvas.width = res.w;
            canvas.height = res.h;
            const ctx = canvas.getContext("2d");
            if (ctx) {
              ctx.putImageData(
                new ImageData(new Uint8ClampedArray(res.rgba.buffer), res.w, res.h),
                0,
                0
              );
            }
            canvas.hidden = false;
            outPre.hidden = true;
            status.className = "demo-status is-ok";
            status.textContent = `${res.w}x${res.h} frame`;
            pushHistory("pixels", currentInput, "");
          } else {
            canvas.hidden = true;
            outPre.hidden = false;
            outPre.className = "demo-output is-error";
            outPre.textContent = "invalid pixel buffer";
            status.className = "demo-status is-bad";
            status.textContent = "error";
            pushHistory("error", currentInput, "invalid pixel buffer");
          }
        } else if (res.kind === "text") {
          canvas.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output";
          outPre.textContent = res.text;
          status.className = "demo-status is-ok";
          status.textContent = "ok";
          pushHistory("text", currentInput, res.text);
        } else {
          canvas.hidden = true;
          outPre.hidden = false;
          outPre.className = "demo-output is-error";
          outPre.textContent = res.text || "demo failed";
          status.className = "demo-status is-bad";
          status.textContent = "error";
          pushHistory("error", currentInput, res.text);
        }
      } catch (err) {
        const msg = err && err.message ? err.message : String(err);
        canvas.hidden = true;
        outPre.hidden = false;
        outPre.className = "demo-output is-error";
        outPre.textContent = msg;
        status.className = "demo-status is-bad";
        status.textContent = "error";
        pushHistory("error", currentInput, msg);
      } finally {
        runBtn.disabled = false;
      }
    }

    runBtn.addEventListener("click", runCurrent);
    renderHistory();
  }

  window.DemoApp = { mount };
})();
