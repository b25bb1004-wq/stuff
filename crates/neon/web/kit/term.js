// One terminal widget for every page: line input with history, output append, and an input pump.
// Global `Term`. Build it with Term.mount(root, opts); all text goes in with textContent.
var Term = (() => {
  "use strict";
  const HISTORY_MAX = 100;
  const MAX_OUTPUT_CHARS = 200000; // shortcut: output is trimmed from the top, not paged; raise when logs need scrollback

  // opts.send(line): called with each submitted line.
  // opts.pump(): optional; called once per animation frame while busy. Returns
  //   { status: "busy" | "waiting" | "done", text } where text is appended to the output.
  // opts.prompt: optional label shown before the input.
  function mount(root, opts) {
    const send = opts.send;
    const pump = opts.pump || null;
    const history = [];
    let cursor = -1;
    let frame = 0;
    let chars = 0;

    const out = document.createElement("pre");
    out.className = "term-out";
    out.setAttribute("role", "log");
    out.setAttribute("aria-live", "polite");
    out.tabIndex = 0;

    const status = document.createElement("div");
    status.className = "term-status";
    status.setAttribute("role", "status");

    const form = document.createElement("form");
    form.className = "term-form";
    const label = document.createElement("span");
    label.className = "term-prompt";
    label.setAttribute("aria-hidden", "true");
    label.textContent = opts.prompt || "$";
    const input = document.createElement("input");
    input.type = "text";
    input.className = "term-in";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.setAttribute("aria-label", "terminal input");
    const submit = document.createElement("button");
    submit.type = "submit";
    submit.textContent = "Send";
    form.append(label, input, submit);

    root.replaceChildren(out, status, form);

    function append(text, cls) {
      if (!text) {
        return;
      }
      const node = document.createElement("span");
      if (cls) {
        node.className = cls;
      }
      node.textContent = text;
      out.appendChild(node);
      chars += text.length;
      // Drop whole nodes from the top until the output is under the cap.
      while (chars > MAX_OUTPUT_CHARS && out.firstChild) {
        chars -= (out.firstChild.textContent || "").length;
        out.removeChild(out.firstChild);
      }
      out.scrollTop = out.scrollHeight;
    }

    function setEnabled(on) {
      input.disabled = !on;
      submit.disabled = !on;
      if (on) {
        input.placeholder = "type a line and press Enter";
      }
    }

    function setStatus(text) {
      status.textContent = text;
    }

    function tick() {
      frame = 0;
      const r = pump ? pump() : { status: "waiting", text: "" };
      if (r.text) {
        append(r.text);
      }
      if (r.status === "busy") {
        setStatus("running");
        frame = requestAnimationFrame(tick);
        return;
      }
      setEnabled(r.status === "waiting");
      setStatus(r.status === "waiting" ? "waiting for input" : "done");
    }

    // Start the pump loop if it is not already running.
    function start() {
      if (pump && frame === 0) {
        frame = requestAnimationFrame(tick);
      }
    }

    input.addEventListener("keydown", (ev) => {
      if (ev.key === "ArrowUp" || ev.key === "ArrowDown") {
        ev.preventDefault();
        if (history.length === 0) {
          return;
        }
        if (ev.key === "ArrowUp") {
          cursor = cursor < 0 ? history.length - 1 : Math.max(0, cursor - 1);
        } else {
          cursor = cursor < 0 ? -1 : cursor + 1;
          if (cursor >= history.length) {
            cursor = -1;
          }
        }
        input.value = cursor < 0 ? "" : history[cursor];
      }
    });

    form.addEventListener("submit", (ev) => {
      ev.preventDefault();
      if (input.disabled) {
        return;
      }
      const line = input.value;
      input.value = "";
      if (line !== "" && history[history.length - 1] !== line) {
        history.push(line);
        if (history.length > HISTORY_MAX) {
          history.shift();
        }
      }
      cursor = -1;
      append(line + "\n", "user-cmd");
      setEnabled(false);
      send(line);
      if (pump) {
        start();
      } else {
        setEnabled(true);
      }
      input.focus();
    });

    setEnabled(!pump);
    return {
      append,
      setEnabled,
      setStatus,
      start,
      focus() {
        input.focus();
      },
      destroy() {
        if (frame) {
          cancelAnimationFrame(frame);
          frame = 0;
        }
        root.replaceChildren();
      },
    };
  }

  return { mount };
})();
