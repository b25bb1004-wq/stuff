(() => {
  "use strict";

  const STEPS_PER_FRAME = 2000000n;
  const MAX_CMD_HISTORY = 50;
  const EXAMPLES = [
    { label: "Help (default)", cmd: "help" },
    { label: "List root", cmd: "ls" },
    { label: "Unknown command (error)", cmd: "nonexistent" }
  ];

  const kit = window.Kit.mount({
    id: "titan",
    name: "Titan",
    source: "machine/titan",
    about:
      "Rust no_std kernel + userland on the Monolith CPU. The thick layer everything rides on. Boots in the browser on the Silicon RV64 emulator with an interactive UART console.",
    help:
      "titan boot [--disk IMG] [--script FILE]\n" +
      "Boots the Titan kernel on the Silicon RV64 emulator (binary in crates/neon)."
  });

  const createEl = window.Kit.createEl;
  const root = document.getElementById("app-root");
  const historyStore = kit.store("history");
  const transcriptStore = kit.store("transcript");

  let cmdHistory = historyStore.getJson([]);
  if (!Array.isArray(cmdHistory)) {
    cmdHistory = [];
  }
  let histCursor = cmdHistory.length;

  const shell = createEl("div", "titan-shell");
  const panel = createEl("section", "titan-panel");
  panel.setAttribute("aria-label", "Titan UART console");

  const bar = createEl("div", "demo-toolbar");
  const status = createEl("span", "demo-status", "loading kernel…");

  const exLabel = createEl("label", "sr-only", "Examples");
  exLabel.setAttribute("for", "titan-examples");
  const exSelect = document.createElement("select");
  exSelect.id = "titan-examples";
  const placeholderOpt = document.createElement("option");
  placeholderOpt.value = "";
  placeholderOpt.textContent = "Examples…";
  exSelect.appendChild(placeholderOpt);
  EXAMPLES.forEach((ex, idx) => {
    const opt = document.createElement("option");
    opt.value = String(idx);
    opt.textContent = ex.label;
    exSelect.appendChild(opt);
  });

  const rebootBtn = createEl("button", "", "Reboot");
  rebootBtn.type = "button";
  const clearBtn = createEl("button", "", "Clear");
  clearBtn.type = "button";
  const exportBtn = createEl("button", "", "Export");
  exportBtn.type = "button";
  const importBtn = createEl("button", "", "Import");
  importBtn.type = "button";

  bar.appendChild(status);
  bar.appendChild(exLabel);
  bar.appendChild(exSelect);
  bar.appendChild(createEl("span", "demo-toolbar-spacer"));
  bar.appendChild(rebootBtn);
  bar.appendChild(clearBtn);
  bar.appendChild(exportBtn);
  bar.appendChild(importBtn);

  const termOut = createEl("pre", "titan-out", "");
  termOut.id = "titan-out";
  termOut.setAttribute("aria-live", "polite");

  const form = createEl("form", "titan-form");
  const inputLabel = createEl("label", "sr-only", "Titan shell command");
  inputLabel.setAttribute("for", "titan-in");
  const input = document.createElement("input");
  input.type = "text";
  input.id = "titan-in";
  input.placeholder = "booting kernel…";
  input.disabled = true;

  const sendBtn = createEl("button", "kit-btn-accent", "Send");
  sendBtn.type = "submit";
  sendBtn.disabled = true;

  form.appendChild(inputLabel);
  form.appendChild(input);
  form.appendChild(sendBtn);

  panel.appendChild(bar);
  panel.appendChild(termOut);
  panel.appendChild(form);
  shell.appendChild(panel);
  root.innerHTML = "";
  root.appendChild(shell);

  let wasmRef = null;
  let pumping = false;
  let queuedLines = [];

  function saveTranscript() {
    const text = termOut.textContent || "";
    transcriptStore.set(text.slice(-16384));
  }

  function appendTerm(text) {
    if (!text) {
      return;
    }
    termOut.appendChild(document.createTextNode(text));
    termOut.scrollTop = termOut.scrollHeight;
    saveTranscript();
  }

  function setEnabled(enabled) {
    input.disabled = !enabled;
    sendBtn.disabled = !enabled;
    if (enabled) {
      input.placeholder = "type command (e.g. help, ls) and press Enter";
    }
  }

  function sendLine(line) {
    if (!wasmRef) {
      return;
    }
    const clean = String(line || "").replace(/\r?\n/g, "");
    appendTerm(`${clean}\n`);
    if (clean.trim()) {
      cmdHistory.push(clean);
      if (cmdHistory.length > MAX_CMD_HISTORY) {
        cmdHistory = cmdHistory.slice(-MAX_CMD_HISTORY);
      }
      historyStore.setJson(cmdHistory);
      histCursor = cmdHistory.length;
    }
    setEnabled(false);
    wasmRef.withGuestBytes(wasmRef.encoder.encode(clean), (ptr, len) =>
      wasmRef.exports.titan_send(ptr, len)
    );
    startPump();
  }

  function pumpFrame() {
    pumping = false;
    if (!wasmRef) {
      return;
    }
    const rc = wasmRef.exports.titan_pump(STEPS_PER_FRAME);
    const outLen = wasmRef.exports.titan_output();
    if (outLen > 0) {
      appendTerm(wasmRef.readNeonLast(outLen));
    }
    if (rc === 1) {
      status.className = "demo-status";
      status.textContent = "running";
      pumping = true;
      requestAnimationFrame(pumpFrame);
    } else if (rc === 0) {
      if (queuedLines.length > 0) {
        const nextLine = queuedLines.shift();
        sendLine(nextLine);
        return;
      }
      status.className = "demo-status is-ok";
      status.textContent = "waiting for input";
      setEnabled(true);
    } else if (rc >= 2) {
      queuedLines = [];
      status.className = "demo-status";
      status.textContent = `exited (${rc - 2})`;
      setEnabled(false);
    } else {
      queuedLines = [];
      status.className = "demo-status is-bad";
      status.textContent = "not booted";
      setEnabled(false);
    }
  }

  function startPump() {
    if (!pumping) {
      pumping = true;
      requestAnimationFrame(pumpFrame);
    }
  }

  async function bootKernel() {
    queuedLines = [];
    setEnabled(false);
    status.className = "demo-status";
    status.textContent = "loading kernel…";
    try {
      const wasm = await window.Kit.wasm();
      wasmRef = wasm;
      const elfResp = await fetch("../titan.elf");
      if (!elfResp.ok) {
        throw new Error(`failed to fetch titan.elf: ${elfResp.status}`);
      }
      const elfBytes = new Uint8Array(await elfResp.arrayBuffer());
      const rc = wasm.withGuestBytes(elfBytes, (ptr, len) => wasm.exports.titan_boot(ptr, len));
      if (rc !== 0) {
        const errLen = wasm.exports.neon_last_len();
        throw new Error(wasm.readNeonLast(errLen) || "titan_boot failed");
      }
      status.textContent = "booting…";
      startPump();
    } catch (err) {
      status.className = "demo-status is-bad";
      status.textContent = "failed";
      appendTerm(`\n[titan error] ${err && err.message ? err.message : String(err)}\n`);
    }
  }

  form.addEventListener("submit", (ev) => {
    ev.preventDefault();
    if (!wasmRef || input.disabled) {
      return;
    }
    const line = input.value;
    input.value = "";
    sendLine(line);
    input.focus();
  });

  input.addEventListener("keydown", (ev) => {
    if (ev.key === "ArrowUp") {
      if (histCursor > 0) {
        ev.preventDefault();
        histCursor -= 1;
        input.value = cmdHistory[histCursor] || "";
      }
    } else if (ev.key === "ArrowDown") {
      if (histCursor < cmdHistory.length) {
        ev.preventDefault();
        histCursor += 1;
        input.value = histCursor < cmdHistory.length ? cmdHistory[histCursor] : "";
      }
    }
  });

  exSelect.addEventListener("change", () => {
    if (exSelect.value === "") {
      return;
    }
    const idx = Number(exSelect.value);
    exSelect.value = "";
    if (!Number.isInteger(idx) || !EXAMPLES[idx]) {
      return;
    }
    const cmd = EXAMPLES[idx].cmd;
    if (!input.disabled) {
      sendLine(cmd);
    } else {
      input.value = cmd;
    }
  });

  rebootBtn.addEventListener("click", () => {
    termOut.textContent = "";
    bootKernel();
  });

  clearBtn.addEventListener("click", () => {
    termOut.textContent = "";
    transcriptStore.remove();
  });

  exportBtn.addEventListener("click", () => {
    const content = [
      "# Monolith Titan command history",
      cmdHistory.join("\n"),
      "",
      "# Transcript",
      termOut.textContent || ""
    ].join("\n");
    window.Kit.download("titan-session.txt", content);
  });

  importBtn.addEventListener("click", async () => {
    const picked = await window.Kit.pickFile();
    if (!picked || typeof picked.text !== "string") {
      return;
    }
    const lines = picked.text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#"));
    if (lines.length === 0) {
      return;
    }
    if (!input.disabled) {
      const first = lines.shift();
      queuedLines.push(...lines);
      sendLine(first);
    } else {
      queuedLines.push(...lines);
    }
  });

  const savedTranscript = transcriptStore.get("");
  if (savedTranscript) {
    termOut.textContent = savedTranscript + "\n--- reboot ---\n";
  }

  bootKernel();
})();
