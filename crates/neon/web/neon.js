(() => {
  "use strict";

  const STEPS_PER_FRAME = 2000000n;
  const LOGOS_FUEL = 1000000n;

  const EXAMPLES = {
    hello: `fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  print("hello from logos\\n")\n}\n`,
    loop: `fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  var sum = 0\n  for i in 1..11 {\n    sum = sum + i * i\n  }\n  print("sum of squares 1..10 = " + str(sum) + "\\n")\n}\n`,
    error: `fn main() {\n  let x: int = "not an int"\n}\n`
  };

  const wasmStatus = document.getElementById("wasm-status");
  const titanStatus = document.getElementById("titan-status");
  const termOut = document.getElementById("term-out");
  const termForm = document.getElementById("term-form");
  const termIn = document.getElementById("term-in");
  const termSend = document.getElementById("term-send");
  const logosExamples = document.getElementById("logos-examples");
  const logosSrc = document.getElementById("logos-src");
  const logosRun = document.getElementById("logos-run");
  const logosOut = document.getElementById("logos-out");

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  let wasm = null;
  let pumping = false;

  logosSrc.value = EXAMPLES.hello;
  logosExamples.addEventListener("change", () => {
    const key = logosExamples.value;
    if (Object.prototype.hasOwnProperty.call(EXAMPLES, key)) {
      logosSrc.value = EXAMPLES[key];
    }
  });

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

  function readLast(len) {
    if (len <= 0) {
      return "";
    }
    const ptr = wasm.neon_last_ptr();
    const view = new Uint8Array(wasm.memory.buffer, ptr, len);
    return decoder.decode(view);
  }

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

  function setInputEnabled(enabled) {
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
      appendTerm(readLast(outLen));
    }
    if (rc === 1) {
      titanStatus.textContent = "running";
      pumping = true;
      requestAnimationFrame(pumpFrame);
    } else if (rc === 0) {
      titanStatus.textContent = "waiting for input";
      setInputEnabled(true);
    } else if (rc >= 2) {
      titanStatus.textContent = `exited (${rc - 2})`;
      setInputEnabled(false);
    } else {
      titanStatus.textContent = "not booted";
      setInputEnabled(false);
    }
  }

  function startPump() {
    if (!pumping) {
      pumping = true;
      requestAnimationFrame(pumpFrame);
    }
  }

  termForm.addEventListener("submit", (ev) => {
    ev.preventDefault();
    if (!wasm || termIn.disabled) {
      return;
    }
    const line = termIn.value;
    termIn.value = "";
    appendTerm(line + "\n", "user-cmd");
    setInputEnabled(false);
    const bytes = encoder.encode(line);
    withBytes(bytes, (ptr, len) => wasm.titan_send(ptr, len));
    startPump();
    termIn.focus();
  });

  logosRun.addEventListener("click", () => {
    if (!wasm) {
      return;
    }
    const bytes = encoder.encode(logosSrc.value);
    const outLen = withBytes(bytes, (ptr, len) => wasm.logos_run(ptr, len, LOGOS_FUEL));
    logosOut.textContent = readLast(outLen);
  });

  async function loadWasm() {
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        const res = await WebAssembly.instantiateStreaming(fetch("neon.wasm"), {});
        return res.instance.exports;
      } catch (_err) {
        // Fallback when the server does not serve application/wasm MIME type.
      }
    }
    const resp = await fetch("neon.wasm");
    if (!resp.ok) {
      throw new Error(`failed to fetch neon.wasm: ${resp.status}`);
    }
    const buf = await resp.arrayBuffer();
    const res = await WebAssembly.instantiate(buf, {});
    return res.instance.exports;
  }

  async function init() {
    try {
      wasm = await loadWasm();
      wasmStatus.textContent = "wasm ready";
      logosRun.disabled = false;

      const elfResp = await fetch("titan.elf");
      if (!elfResp.ok) {
        throw new Error(`failed to fetch titan.elf: ${elfResp.status}`);
      }
      const elfBytes = new Uint8Array(await elfResp.arrayBuffer());
      const rc = withBytes(elfBytes, (ptr, len) => wasm.titan_boot(ptr, len));
      if (rc !== 0) {
        const errLen = wasm.neon_last_len();
        throw new Error(readLast(errLen) || "titan_boot failed");
      }
      titanStatus.textContent = "booting…";
      startPump();
    } catch (err) {
      wasmStatus.textContent = "error";
      titanStatus.textContent = "failed";
      appendTerm(`\n[neon error] ${err && err.message ? err.message : String(err)}\n`);
    }
  }

  init();
})();
