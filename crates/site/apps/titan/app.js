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

  const termRoot = createEl("div", "titan-term");

  panel.appendChild(bar);
  panel.appendChild(termRoot);
  shell.appendChild(panel);
  root.innerHTML = "";
  root.appendChild(shell);

  let wasmRef = null;
  let queuedLines = [];

  function transcriptText() {
    const out = termRoot.querySelector(".term-out");
    return out ? out.textContent || "" : "";
  }

  function saveTranscript() {
    transcriptStore.set(transcriptText().slice(-16384));
  }

  function clearOutput() {
    const out = termRoot.querySelector(".term-out");
    if (out) {
      out.textContent = "";
    }
  }

  function inputReady() {
    const input = termRoot.querySelector(".term-in");
    return !!input && !input.disabled;
  }

  // Remember a command and hand it to the kernel. Term has already echoed a typed line.
  function sendToKernel(line) {
    if (line.trim()) {
      cmdHistory.push(line);
      if (cmdHistory.length > MAX_CMD_HISTORY) {
        cmdHistory = cmdHistory.slice(-MAX_CMD_HISTORY);
      }
      historyStore.setJson(cmdHistory);
    }
    wasmRef.withGuestBytes(wasmRef.encoder.encode(line), (ptr, len) =>
      wasmRef.exports.titan_send(ptr, len)
    );
  }

  // Programmatic lines (examples, imports): echo, send, then keep pumping.
  function sendLine(line) {
    if (!wasmRef) {
      return;
    }
    const clean = String(line || "").replace(/\r?\n/g, "");
    term.append(`${clean}\n`, "user-cmd");
    term.setEnabled(false);
    sendToKernel(clean);
    saveTranscript();
    term.start();
  }

  // Called once per frame by Term while the kernel is busy. Output goes straight into Term.
  function pump() {
    if (!wasmRef) {
      return { status: "done", text: "" };
    }
    const ex = wasmRef.exports;
    const rc = ex.titan_pump(STEPS_PER_FRAME);
    const outLen = ex.titan_output();
    if (outLen > 0) {
      term.append(wasmRef.readNeonLast(outLen));
      saveTranscript();
    }
    if (rc === 1) {
      status.className = "demo-status";
      status.textContent = "running";
      return { status: "busy", text: "" };
    }
    if (rc === 0 && queuedLines.length > 0) {
      const next = queuedLines.shift();
      term.append(`${next}\n`, "user-cmd");
      sendToKernel(next);
      saveTranscript();
      return { status: "busy", text: "" };
    }
    if (rc === 0) {
      status.className = "demo-status is-ok";
      status.textContent = "waiting for input";
      return { status: "waiting", text: "" };
    }
    queuedLines = [];
    if (rc >= 2) {
      status.className = "demo-status";
      status.textContent = `exited (${rc - 2})`;
    } else {
      status.className = "demo-status is-bad";
      status.textContent = "not booted";
    }
    return { status: "done", text: "" };
  }

  const term = window.Term.mount(termRoot, {
    send(line) {
      sendToKernel(line);
      saveTranscript();
    },
    pump
  });

  async function bootKernel() {
    queuedLines = [];
    term.setEnabled(false);
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
      term.start();
    } catch (err) {
      status.className = "demo-status is-bad";
      status.textContent = "failed";
      term.append(`\n[titan error] ${err && err.message ? err.message : String(err)}\n`);
    }
  }

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
    if (inputReady()) {
      sendLine(cmd);
    } else {
      termRoot.querySelector(".term-in").value = cmd;
    }
  });

  rebootBtn.addEventListener("click", () => {
    clearOutput();
    bootKernel();
  });

  clearBtn.addEventListener("click", () => {
    clearOutput();
    transcriptStore.remove();
  });

  exportBtn.addEventListener("click", () => {
    const content = [
      "# Monolith Titan command history",
      cmdHistory.join("\n"),
      "",
      "# Transcript",
      transcriptText()
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
    if (inputReady()) {
      const first = lines.shift();
      queuedLines.push(...lines);
      sendLine(first);
    } else {
      queuedLines.push(...lines);
    }
  });

  const savedTranscript = transcriptStore.get("");
  if (savedTranscript) {
    term.append(`${savedTranscript}\n--- reboot ---\n`);
  }

  bootKernel();
})();
