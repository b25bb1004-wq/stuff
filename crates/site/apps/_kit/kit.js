(() => {
  "use strict";

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let wasmPromise = null;

  function makeStore(appId, key) {
    const fullKey = `monolith.${appId}.${key}`;
    return {
      key: fullKey,
      get(fallback = null) {
        try {
          const val = window.localStorage.getItem(fullKey);
          return val === null ? fallback : val;
        } catch (_err) {
          return fallback;
        }
      },
      set(value) {
        try {
          window.localStorage.setItem(fullKey, String(value));
          return true;
        } catch (_err) {
          return false;
        }
      },
      getJson(fallback = null) {
        try {
          const raw = window.localStorage.getItem(fullKey);
          return raw === null ? fallback : JSON.parse(raw);
        } catch (_err) {
          return fallback;
        }
      },
      setJson(value) {
        try {
          window.localStorage.setItem(fullKey, JSON.stringify(value));
          return true;
        } catch (_err) {
          return false;
        }
      },
      remove() {
        try {
          window.localStorage.removeItem(fullKey);
          return true;
        } catch (_err) {
          return false;
        }
      }
    };
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

  function isStandaloneMode() {
    if (window.navigator && window.navigator.standalone === true) {
      return true;
    }
    if (window.matchMedia) {
      if (
        window.matchMedia("(display-mode: standalone)").matches ||
        window.matchMedia("(display-mode: window-controls-overlay)").matches
      ) {
        return true;
      }
    }
    return false;
  }

  async function instantiateWasm() {
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        const res = await WebAssembly.instantiateStreaming(fetch("../site.wasm"), {});
        return res.instance.exports;
      } catch (_err) {
        /* Fallback when server sends a non-wasm MIME type. */
      }
    }
    const resp = await fetch("../site.wasm");
    if (!resp.ok) {
      throw new Error(`failed to fetch site.wasm: ${resp.status}`);
    }
    const buf = await resp.arrayBuffer();
    const res = await WebAssembly.instantiate(buf, {});
    return res.instance.exports;
  }

  function withGuestBytes(exportsObj, bytes, fn) {
    const len = bytes.length;
    const ptr = exportsObj.neon_alloc(len);
    try {
      if (len > 0) {
        new Uint8Array(exportsObj.memory.buffer, ptr, len).set(bytes);
      }
      return fn(ptr, len);
    } finally {
      exportsObj.neon_free(ptr, len);
    }
  }

  function readNeonLast(exportsObj, len) {
    if (len <= 0) {
      return "";
    }
    const ptr = exportsObj.neon_last_ptr();
    return decoder.decode(new Uint8Array(exportsObj.memory.buffer, ptr, len));
  }

  function readSiteLastBytes(exportsObj) {
    const len = exportsObj.site_last_len();
    if (len <= 0) {
      return new Uint8Array(0);
    }
    const ptr = exportsObj.site_last_ptr();
    return new Uint8Array(exportsObj.memory.buffer, ptr, len).slice();
  }

  function wrapWasm(exportsObj) {
    return {
      exports: exportsObj,
      encoder,
      decoder,
      withGuestBytes(bytes, fn) {
        return withGuestBytes(exportsObj, bytes, fn);
      },
      readNeonLast(len) {
        return readNeonLast(exportsObj, len);
      },
      readSiteLastBytes() {
        return readSiteLastBytes(exportsObj);
      },
      runDemo(demoId, inputText) {
        const idBytes = encoder.encode(demoId);
        const inBytes = encoder.encode(inputText);
        const rc = withGuestBytes(exportsObj, idBytes, (idPtr, idLen) =>
          withGuestBytes(exportsObj, inBytes, (inPtr, inLen) =>
            exportsObj.site_demo(idPtr, idLen, inPtr, inLen)
          )
        );
        const bytes = readSiteLastBytes(exportsObj);
        if (rc === 1) {
          return {
            kind: "pixels",
            w: exportsObj.site_pixels_w(),
            h: exportsObj.site_pixels_h(),
            rgba: bytes
          };
        }
        if (rc === 0) {
          return {
            kind: "text",
            text: decoder.decode(bytes)
          };
        }
        return {
          kind: "error",
          text: decoder.decode(bytes) || "demo failed"
        };
      }
    };
  }

  function loadWasmOnce() {
    if (!wasmPromise) {
      wasmPromise = instantiateWasm()
        .then((exportsObj) => wrapWasm(exportsObj))
        .catch((err) => {
          wasmPromise = null;
          throw err;
        });
    }
    return wasmPromise;
  }

  function download(name, text) {
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.hidden = true;
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function pickFile() {
    return new Promise((resolve) => {
      const input = document.createElement("input");
      input.type = "file";
      input.hidden = true;
      let settled = false;

      function finish(val) {
        if (settled) {
          return;
        }
        settled = true;
        input.remove();
        resolve(val);
      }

      input.addEventListener("change", () => {
        const file = input.files && input.files[0];
        if (!file) {
          finish(null);
          return;
        }
        file
          .text()
          .then((text) => finish({ name: file.name, text }))
          .catch(() => finish(null));
      });

      input.addEventListener("cancel", () => finish(null));
      document.body.appendChild(input);
      input.click();
    });
  }

  function mount(opts) {
    const appId = opts.id || "app";
    const appName = opts.name || appId;
    const aboutText = opts.about || "";
    const sourcePath = opts.source || `crates/${appId}`;
    const helpText = opts.help || "";
    const themeStore = makeStore(appId, "theme");

    document.title = `Monolith ${appName}`;
    document.body.classList.add("kit-app-body");

    if (window.navigator && "serviceWorker" in window.navigator) {
      window.navigator.serviceWorker.register("./sw.js").catch(() => {});
    }

    const header = createEl("header", "kit-titlebar");
    const brand = createEl("div", "kit-titlebar-brand");
    const icon = createEl("img", "kit-titlebar-icon");
    icon.src = "icon-192.png";
    icon.alt = "";
    icon.width = 20;
    icon.height = 20;
    const title = createEl("span", "kit-titlebar-name", `Monolith ${appName}`);
    brand.appendChild(icon);
    brand.appendChild(title);

    const actions = createEl("div", "kit-titlebar-actions");

    const installBtn = createEl("button", "kit-btn-accent", "Install");
    installBtn.type = "button";
    installBtn.hidden = true;
    installBtn.setAttribute("aria-label", `Install Monolith ${appName}`);

    const themeBtn = createEl("button", "", "Theme: Auto");
    themeBtn.type = "button";
    themeBtn.setAttribute("aria-label", "Toggle color theme");

    const aboutBtn = createEl("button", "", "About");
    aboutBtn.type = "button";
    aboutBtn.setAttribute("aria-label", `About Monolith ${appName}`);

    actions.appendChild(installBtn);
    actions.appendChild(themeBtn);
    actions.appendChild(aboutBtn);
    header.appendChild(brand);
    header.appendChild(actions);

    const dialog = createEl("dialog", "kit-dialog");
    dialog.setAttribute("aria-labelledby", "kit-about-title");

    const dHead = createEl("div", "kit-dialog-head");
    const dTitle = createEl("h2", "kit-dialog-title", `Monolith ${appName}`);
    dTitle.id = "kit-about-title";
    const dClose = createEl("button", "", "Close");
    dClose.type = "button";
    dClose.setAttribute("aria-label", "Close About dialog");
    dHead.appendChild(dTitle);
    dHead.appendChild(dClose);

    const dBody = createEl("div", "kit-dialog-body");
    dBody.appendChild(createEl("p", "", aboutText));

    const metaRow = createEl("div", "kit-dialog-meta");
    metaRow.appendChild(createEl("span", "", "Source:"));
    metaRow.appendChild(createEl("code", "kit-code-pill", sourcePath));
    dBody.appendChild(metaRow);

    if (helpText) {
      dBody.appendChild(createEl("span", "demo-toolbar-label", "CLI tool usage"));
      dBody.appendChild(createEl("pre", "kit-help-pre", helpText));
    }

    const linksRow = createEl("div", "kit-dialog-links");
    const appsLink = createEl("a", "kit-link-btn", "All apps");
    appsLink.href = "../apps/";
    const siteLink = createEl("a", "kit-link-btn", "Main site");
    siteLink.href = "../";
    linksRow.appendChild(appsLink);
    linksRow.appendChild(siteLink);
    dBody.appendChild(linksRow);

    dialog.appendChild(dHead);
    dialog.appendChild(dBody);

    document.body.insertBefore(header, document.body.firstChild);
    document.body.appendChild(dialog);

    function applyTheme(theme) {
      if (theme === "light" || theme === "dark") {
        document.documentElement.setAttribute("data-theme", theme);
        themeBtn.textContent = theme === "light" ? "Light" : "Dark";
        themeStore.set(theme);
      } else {
        document.documentElement.removeAttribute("data-theme");
        themeBtn.textContent = "Auto";
        themeStore.remove();
      }
    }

    applyTheme(themeStore.get(null));

    themeBtn.addEventListener("click", () => {
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

    aboutBtn.addEventListener("click", () => {
      if (typeof dialog.showModal === "function") {
        dialog.showModal();
      } else {
        dialog.setAttribute("open", "open");
      }
    });

    dClose.addEventListener("click", () => {
      if (typeof dialog.close === "function") {
        dialog.close();
      } else {
        dialog.removeAttribute("open");
      }
    });

    dialog.addEventListener("click", (ev) => {
      if (ev.target === dialog && typeof dialog.close === "function") {
        dialog.close();
      }
    });

    let deferredPrompt = null;
    window.addEventListener("beforeinstallprompt", (ev) => {
      ev.preventDefault();
      deferredPrompt = ev;
      if (!isStandaloneMode()) {
        installBtn.hidden = false;
      }
    });

    installBtn.addEventListener("click", async () => {
      if (!deferredPrompt) {
        return;
      }
      const promptEv = deferredPrompt;
      deferredPrompt = null;
      installBtn.hidden = true;
      try {
        await promptEv["prompt"]();
        await promptEv.userChoice;
      } catch (_err) {
        /* Ignore prompt dismissal errors. */
      }
    });

    window.addEventListener("appinstalled", () => {
      deferredPrompt = null;
      installBtn.hidden = true;
    });

    return {
      store(key) {
        return makeStore(appId, key);
      }
    };
  }

  window.Kit = {
    mount,
    wasm: loadWasmOnce,
    store(key) {
      return makeStore("global", key);
    },
    download,
    pickFile,
    createEl
  };
})();
