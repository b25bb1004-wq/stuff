// One loader for every neon-based page. Global `Neon`; no modules, no network beyond the given URL.
// Helpers match the C ABI of crates/neon/src/wasm.rs: neon_alloc, neon_free, neon_last_ptr, neon_last_len.
var Neon = (() => {
  "use strict";
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  // Wrap the exports of one instance in the helpers. Views are created per call, because a
  // grown memory replaces its buffer.
  function wrap(exports) {
    const api = {
      exports,
      alloc(len) {
        return exports.neon_alloc(len);
      },
      free(ptr, len) {
        exports.neon_free(ptr, len);
      },
      // Copy bytes into guest memory. The caller frees with free(ptr, len).
      copyIn(bytes) {
        const ptr = exports.neon_alloc(bytes.length);
        if (bytes.length > 0) {
          new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes);
        }
        return ptr;
      },
      // Run fn(ptr, len) with bytes copied in, and always free them afterwards.
      withBytes(bytes, fn) {
        const ptr = api.copyIn(bytes);
        try {
          return fn(ptr, bytes.length);
        } finally {
          exports.neon_free(ptr, bytes.length);
        }
      },
      // Copy of the last output buffer (safe to keep after the next call).
      lastBytes(len) {
        const n = len === undefined ? exports.neon_last_len() : len;
        if (n <= 0) {
          return new Uint8Array(0);
        }
        return new Uint8Array(exports.memory.buffer, exports.neon_last_ptr(), n).slice();
      },
      lastText(len) {
        return decoder.decode(api.lastBytes(len));
      },
      encode(text) {
        return encoder.encode(text);
      },
    };
    return api;
  }

  // Load a wasm module from url. Uses instantiateStreaming, then falls back to a plain
  // fetch + instantiate (for servers that do not send application/wasm).
  async function load(url, imports) {
    const env = imports || {};
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        const res = await WebAssembly.instantiateStreaming(fetch(url), env);
        return wrap(res.instance.exports);
      } catch (_err) {
        // Fall through to the buffer path.
      }
    }
    const resp = await fetch(url);
    if (!resp.ok) {
      throw new Error(`failed to fetch ${url}: ${resp.status}`);
    }
    const res = await WebAssembly.instantiate(await resp.arrayBuffer(), env);
    return wrap(res.instance.exports);
  }

  return { load };
})();
