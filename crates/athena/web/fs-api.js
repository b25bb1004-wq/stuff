// Client for the file API that `athena serve` runs. The page uses it only when its URL carries ?t=.

export const TOKEN_HEADER = "X-Athena-Token";
const API_BASE = "/api/fs/";

// The token is the hex string `athena serve` prints. Anything else means the page is the public site.
export function tokenFromSearch(search) {
  const t = new URLSearchParams(search).get("t");
  return t && /^[0-9a-f]{32,}$/.test(t) ? t : null;
}

// `fetchFn` has the signature of fetch; tests pass a fake that records the calls.
export function createFsApi({ token, fetchFn = (url, init) => fetch(url, init), base = API_BASE }) {
  async function call(method, op, params, body) {
    const query = Object.entries(params)
      .map(([k, v]) => `${k}=${encodeURIComponent(v)}`)
      .join("&");
    const res = await fetchFn(`${base}${op}?${query}`, {
      method,
      headers: { [TOKEN_HEADER]: token },
      body,
    });
    if (!res.ok) {
      let message = `HTTP ${res.status}`;
      try {
        message = (await res.json()).error || message;
      } catch (_e) { /* not JSON: keep the status */ }
      throw new Error(message);
    }
    return op === "read" ? res.text() : res.json();
  }
  return {
    list: (path) => call("GET", "list", { path }),
    read: (path) => call("GET", "read", { path }),
    write: (path, text) => call("PUT", "write", { path }, text),
    create: (path, kind) => call("POST", "create", { path, kind }),
    rename: (from, to) => call("POST", "rename", { from, to }),
    trash: (path) => call("POST", "trash", { path }),
  };
}

// Paths are folder-relative strings joined with "/". "" is the served folder.
export const parentOf = (path) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
export const isWithin = (key, path) => key === path || key.startsWith(`${path}/`);
export const remapPath = (key, from, to) =>
  key === from ? to : isWithin(key, from) ? to + key.slice(from.length) : key;

// The explorer's model: which folders are open, and the listing of each folder loaded so far.
// `api` is anything with the createFsApi shape; `onChange` runs after every change to the tree.
export function createFsTree(api, onChange = () => {}) {
  const cache = new Map();
  const expanded = new Set();
  const loading = new Set();
  let error = "";

  const sorted = (entries) =>
    [...entries].sort((a, b) => (b.kind === "dir") - (a.kind === "dir") || a.name.localeCompare(b.name));

  async function load(dir) {
    if (loading.has(dir)) return;
    loading.add(dir);
    try {
      cache.set(dir, sorted(await api.list(dir)));
      error = "";
    } catch (err) {
      error = err && err.message ? err.message : String(err);
    } finally {
      loading.delete(dir);
      onChange();
    }
    return error;
  }

  return {
    load,
    error: () => error,
    // Rows in display order: open folders show their children, indented by depth.
    rows() {
      const out = [];
      const walk = (dir, depth) => {
        for (const e of cache.get(dir) || []) {
          const path = dir ? `${dir}/${e.name}` : e.name;
          const isDir = e.kind === "dir";
          out.push({ path, name: e.name, depth, dir: isDir, open: expanded.has(path) });
          if (isDir && expanded.has(path)) walk(path, depth + 1);
        }
      };
      walk("", 0);
      return out;
    },
    async toggle(path) {
      if (expanded.has(path)) expanded.delete(path);
      else {
        expanded.add(path);
        if (!cache.has(path)) await load(path);
      }
      onChange();
    },
    // Open every folder on the way to `dir` and load it, so a new entry shows up.
    async reveal(dir) {
      const parts = dir ? dir.split("/") : [];
      for (let i = 1; i <= parts.length; i += 1) expanded.add(parts.slice(0, i).join("/"));
      await load(dir);
    },
    // Forget a folder and everything under it after it was renamed away or trashed.
    forget(path) {
      for (const key of [...cache.keys()]) if (isWithin(key, path)) cache.delete(key);
      for (const key of [...expanded]) if (isWithin(key, path)) expanded.delete(key);
    },
  };
}
