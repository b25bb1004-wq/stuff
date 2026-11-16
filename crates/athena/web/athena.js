import { createWorkbench } from "./workbench.js";
import { createDebugUi } from "./debug.js";
import { afterPaint, createRunLine } from "./athena-motion.js";
import { parseDiagnostics, parseHighlight, lineColToCharOffset, renderHighlighted, byteOffsetToCharOffset, charOffsetToByteOffset, domPosToCharOffset, rectContainsPoint } from "./text-spans.js";
import { tokenFromSearch, createFsApi, createFsTree, parentOf, isWithin, remapPath } from "./fs-api.js";

// Served by `athena serve` with ?t=TOKEN: the explorer shows a real folder. Otherwise the public site.
const FS = (() => {
  const token = tokenFromSearch(window.location.search);
  if (!token) return null;
  const api = createFsApi({ token });
  return { api, tree: createFsTree(api, () => workbench.renderFileTree()) };
})();

const STORAGE_KEY = "athena.files.v1";
const TABS_KEY = "athena.tabs.v1";
const ACTIVE_KEY = "athena.active.v1";
const STEPS_PER_FRAME = 2000000n;
const LOGOS_FUEL = 1000000n;
const MAX_SOURCE_BYTES = 64 * 1024;
const DEBOUNCE_MS = 300;
const LINE_HEIGHT_PX = 20;

const SEED_FILES = {
  "main.logos":
    'fn print(s: str) uses io = extern "io.print"\n\nfn main() uses io {\n  var sum = 0\n  for i in 1..6 {\n    sum = sum + i\n    print("step " + str(i) + ": sum=" + str(sum) + "\\n")\n  }\n}\n',
  "math.logos":
    'fn add(a: int, b: int) -> int {\n  a + b\n}\n\nfn max2(a: int, b: int) -> int {\n  if a > b { a } else { b }\n}\n\ntest "add works" {\n  assert(add(2, 3) == 5)\n}\n\ntest "max2 works" {\n  assert(max2(7, 3) == 7)\n  assert(max2(-2, 4) == 4)\n}\n\nprop "max2 >= both" (a: int, b: int) {\n  let m = max2(a, b)\n  assert(m >= a)\n  assert(m >= b)\n}\n',
  "broken.logos": 'fn main() {\n  let x: int = "not an int"\n}\n'
};

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const $ = (id) => document.getElementById(id);
const hasOwn = (obj, key) => Object.prototype.hasOwnProperty.call(obj, key);

const [
  btnRun, btnTest, btnFormat, editorStatus, hoverTooltip,
  editorEmpty, editorWrap, gutter, codeHighlight, codeHighlightInner, codeInput,
  runOut, testSummary, testList, titanStatus
] = [
  "btn-run", "btn-test", "btn-format", "editor-status", "hover-tooltip",
  "editor-empty", "editor-wrap", "editor-gutter", "code-highlight", "code-highlight-inner", "code-input",
  "run-out", "test-summary", "test-list", "titan-status"
].map($);
hoverTooltip.setAttribute("role", "tooltip");

const storageGet = (k) => { try { return window.localStorage.getItem(k); } catch (_e) { return null; } };
const storageSet = (k, v) => { try { window.localStorage.setItem(k, v); return true; } catch (_e) { return false; } };

function loadFiles() {
  const raw = storageGet(STORAGE_KEY);
  if (raw) {
    try {
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const clean = {};
        for (const [k, v] of Object.entries(parsed)) {
          if (typeof k === "string" && k.trim() !== "" && typeof v === "string") clean[k] = v;
        }
        if (Object.keys(clean).length > 0) return clean;
      }
    } catch (_e) { /* fall back */ }
  }
  const seeded = { ...SEED_FILES };
  storageSet(STORAGE_KEY, JSON.stringify(seeded));
  return seeded;
}

function loadTabsState(fileMap) {
  const raw = storageGet(TABS_KEY);
  if (raw) {
    try {
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === "object") {
        const tabs = Array.isArray(parsed.tabs)
          ? parsed.tabs.filter((n, i, a) => typeof n === "string" && hasOwn(fileMap, n) && a.indexOf(n) === i)
          : [];
        let active = typeof parsed.active === "string" && hasOwn(fileMap, parsed.active) ? parsed.active : null;
        if (active && !tabs.includes(active)) tabs.push(active);
        if (!active && tabs.length > 0) active = tabs[0];
        return { openTabs: tabs, activeFile: active };
      }
    } catch (_e) { /* fall back */ }
  }
  const keys = Object.keys(fileMap);
  const saved = storageGet(ACTIVE_KEY);
  const initial = keys.length === 0 ? null : (saved && hasOwn(fileMap, saved)) ? saved : (hasOwn(fileMap, "main.logos") ? "main.logos" : keys[0]);
  return { openTabs: initial ? [initial] : [], activeFile: initial };
}

let files = FS ? {} : loadFiles();
let drafts = {};
let { openTabs, activeFile } = loadTabsState(files);
let diagnosticsByFile = {};
let lastSaveText = "saved";
let wasm = null;
let debounceTimer = null;
let titanBootStarted = false;
let transientStatus = false;
let hoverTimer = null;
let hoverShown = null;
let formatUndo = null;

const saveFilesToStorage = () => {
  if (FS) return; // the folder on disk is the store; browser storage keeps only the public site's files
  lastSaveText = storageSet(STORAGE_KEY, JSON.stringify(files)) ? "saved" : "memory only";
};
const saveTabsState = () => {
  if (FS) return;
  storageSet(TABS_KEY, JSON.stringify({ tabs: openTabs, active: activeFile }));
  if (activeFile) storageSet(ACTIVE_KEY, activeFile);
};
const getFileContent = (name) => (!name ? "" : hasOwn(drafts, name) ? drafts[name] : files[name] || "");
const isDirty = (name) => Boolean(name && hasOwn(drafts, name) && drafts[name] !== files[name]);
const sortedFileNames = () => Object.keys(files).sort((a, b) => a.localeCompare(b));

function validateFileName(name, currentName) {
  const trimmed = name.trim();
  if (!trimmed) return { ok: false, error: "Name cannot be empty." };
  if (trimmed.includes("/") || trimmed.includes("\\")) return { ok: false, error: "Slashes are not allowed." };
  if (trimmed !== currentName && hasOwn(files, trimmed)) return { ok: false, error: `"${trimmed}" already exists.` };
  return { ok: true, name: trimmed };
}

function setActiveFile(name) {
  if (FS && name && !hasOwn(files, name)) return openRemote(name);
  if (name && !hasOwn(files, name)) return;
  activeFile = name || null;
  if (activeFile && !openTabs.includes(activeFile)) openTabs.push(activeFile);
  transientStatus = false;
  formatUndo = null;
  hideTooltip();
  saveTabsState();
  syncEditorFromState();
  workbench.renderAll();
}

function saveFile(name) {
  if (!name || !hasOwn(files, name)) return;
  if (FS) return saveRemote(name);
  if (debounceTimer !== null && name === activeFile) { clearTimeout(debounceTimer); debounceTimer = null; }
  if (hasOwn(drafts, name)) { files[name] = drafts[name]; delete drafts[name]; }
  saveFilesToStorage();
  workbench.renderTabs();
  workbench.renderFileTree();
  refreshAnalysis();
  updateCursorMeta();
}

function discardDraft(name) {
  if (!name) return;
  delete drafts[name];
  if (name === activeFile) syncEditorFromState();
  else { refreshOpenTabProblems(); workbench.renderTabs(); workbench.renderFileTree(); }
}

function closeTab(name) {
  const idx = openTabs.indexOf(name);
  if (idx === -1) return;
  openTabs.splice(idx, 1);
  delete diagnosticsByFile[name];
  if (activeFile === name) {
    activeFile = openTabs.length === 0 ? null : openTabs[Math.min(idx, openTabs.length - 1)];
    syncEditorFromState();
  } else {
    refreshOpenTabProblems();
  }
  saveTabsState();
  workbench.renderAll();
}

function cycleTab(dir) {
  if (openTabs.length <= 1) return;
  const idx = openTabs.indexOf(activeFile);
  setActiveFile(openTabs[((idx === -1 ? 0 : idx) + dir + openTabs.length) % openTabs.length]);
}

function createFile(rawName) {
  if (FS) return createRemote(rawName);
  const check = validateFileName(rawName, null);
  if (!check.ok) return check;
  files[check.name] = check.name.endsWith(".logos") ? "fn main() {\n  \n}\n" : "";
  saveFilesToStorage();
  setActiveFile(check.name);
  return { ok: true, name: check.name };
}

function renameFile(oldName, rawNewName) {
  if (FS) return renameRemote(oldName, rawNewName);
  const check = validateFileName(rawNewName, oldName);
  if (!check.ok) return check;
  const nextName = check.name;
  if (nextName !== oldName) {
    files[nextName] = files[oldName];
    delete files[oldName];
    for (const store of [drafts, diagnosticsByFile]) {
      if (hasOwn(store, oldName)) { store[nextName] = store[oldName]; delete store[oldName]; }
    }
    openTabs = openTabs.map((t) => (t === oldName ? nextName : t));
    if (activeFile === oldName) activeFile = nextName;
    saveFilesToStorage();
    saveTabsState();
  }
  syncEditorFromState();
  return { ok: true, name: nextName };
}

function deleteFile(name) {
  if (FS) return deleteRemote(name);
  delete files[name];
  delete drafts[name];
  delete diagnosticsByFile[name];
  openTabs = openTabs.filter((t) => t !== name);
  if (activeFile === name) {
    if (openTabs.length > 0) activeFile = openTabs[0];
    else {
      const remaining = sortedFileNames();
      activeFile = remaining.length > 0 ? remaining[0] : null;
      if (activeFile) openTabs.push(activeFile);
    }
  }
  saveFilesToStorage();
  saveTabsState();
  syncEditorFromState();
}

// Folder mode: each explorer action runs on disk first; the open state then follows the new paths.
const LOGOS_TEMPLATE = "fn main() {\n  \n}\n";

function movePaths(from, to) {
  for (const store of [files, drafts, diagnosticsByFile]) {
    for (const key of Object.keys(store)) {
      const next = remapPath(key, from, to);
      if (next !== key) { store[next] = store[key]; delete store[key]; }
    }
  }
  openTabs = openTabs.map((t) => remapPath(t, from, to));
  if (activeFile) activeFile = remapPath(activeFile, from, to);
}

function dropPaths(path) {
  for (const store of [files, drafts, diagnosticsByFile]) {
    for (const key of Object.keys(store)) if (isWithin(key, path)) delete store[key];
  }
  openTabs = openTabs.filter((t) => !isWithin(t, path));
  if (activeFile && isWithin(activeFile, path)) activeFile = openTabs.length > 0 ? openTabs[0] : null;
}

async function openRemote(name) {
  try {
    files[name] = await FS.api.read(name);
  } catch (err) {
    setEditorStatus(`Could not open ${name}: ${err.message}`, true);
    return;
  }
  setActiveFile(name);
}

async function saveRemote(name) {
  const text = getFileContent(name);
  try {
    await FS.api.write(name, text);
    files[name] = text;
    if (drafts[name] === text) delete drafts[name];
    lastSaveText = "saved";
  } catch (err) {
    lastSaveText = `save failed: ${err.message}`;
  }
  workbench.renderTabs();
  workbench.renderFileTree();
  refreshAnalysis();
  updateCursorMeta();
}

async function createRemote(rawName) {
  const typed = rawName.trim();
  const isDir = typed.endsWith("/");
  const path = typed.replace(/\/+$/, "");
  if (!path) return { ok: false, error: "Name cannot be empty." };
  const isLogos = path.endsWith(".logos");
  try {
    await FS.api.create(path, isDir ? "dir" : "file");
    if (!isDir && isLogos) await FS.api.write(path, LOGOS_TEMPLATE);
  } catch (err) {
    return { ok: false, error: err.message };
  }
  await FS.tree.reveal(isDir ? path : parentOf(path));
  if (!isDir) {
    files[path] = isLogos ? LOGOS_TEMPLATE : "";
    setActiveFile(path);
  }
  return { ok: true, name: path };
}

async function renameRemote(oldName, rawNewName) {
  const next = rawNewName.trim().replace(/^\/+|\/+$/g, "");
  if (!next) return { ok: false, error: "Name cannot be empty." };
  if (next === oldName) return { ok: true, name: next };
  try {
    await FS.api.rename(oldName, next);
  } catch (err) {
    return { ok: false, error: err.message };
  }
  movePaths(oldName, next);
  FS.tree.forget(oldName);
  await FS.tree.load(parentOf(oldName));
  await FS.tree.reveal(parentOf(next));
  syncEditorFromState();
  return { ok: true, name: next };
}

async function deleteRemote(path) {
  try {
    await FS.api.trash(path);
  } catch (err) {
    setEditorStatus(`Could not move ${path} to the trash: ${err.message}`, true);
    return;
  }
  dropPaths(path);
  FS.tree.forget(path);
  await FS.tree.load(parentOf(path));
  syncEditorFromState();
}

function withBytes(bytes, fn) {
  const len = bytes.length;
  const ptr = wasm.athena_alloc(len);
  try {
    if (len > 0) new Uint8Array(wasm.memory.buffer, ptr, len).set(bytes);
    return fn(ptr, len);
  } finally {
    wasm.athena_free(ptr, len);
  }
}

const readNeonLast = (len) =>
  len <= 0 ? "" : decoder.decode(new Uint8Array(wasm.memory.buffer, wasm.neon_last_ptr(), len));
const readAthenaLast = (len) =>
  len <= 0 ? "" : decoder.decode(new Uint8Array(wasm.memory.buffer, wasm.athena_last_ptr(), len));

function setEditorStatus(msg, isError) {
  editorStatus.textContent = msg || "";
  editorStatus.classList.toggle("is-error", Boolean(isError && msg));
}

function openFileAt(fileName, line, col, selLen) {
  if (fileName && fileName !== activeFile) setActiveFile(fileName);
  if (!activeFile) return;
  if (workbench.isMobile()) workbench.setMobileView("editor");
  const src = codeInput.value;
  const offset = lineColToCharOffset(src, line, col);
  codeInput.focus();
  codeInput.setSelectionRange(offset, selLen > 0 ? Math.min(src.length, offset + selLen) : offset);
  codeInput.scrollTop = Math.max(0, (line - 3) * LINE_HEIGHT_PX);
  syncScroll();
  updateCursorMeta();
}

function hideTooltip() {
  if (hoverTimer !== null) { clearTimeout(hoverTimer); hoverTimer = null; }
  hoverShown = null;
  hoverTooltip.hidden = true;
  hoverTooltip.textContent = "";
}

function showTooltipAt(text, anchorX, anchorY, byPointer) {
  hoverTooltip.textContent = text;
  hoverTooltip.hidden = false;
  hoverTooltip.style.left = "0px";
  hoverTooltip.style.top = "0px";
  const tipRect = hoverTooltip.getBoundingClientRect();
  const margin = 8;
  const left = Math.max(margin, Math.min(anchorX, window.innerWidth - tipRect.width - margin));
  let top = anchorY - tipRect.height - 6;
  if (top < margin) top = Math.min(window.innerHeight - tipRect.height - margin, anchorY + 22);
  hoverTooltip.style.left = `${Math.round(left)}px`;
  hoverTooltip.style.top = `${Math.round(top)}px`;
  hoverShown = { x: anchorX, y: anchorY, byPointer };
}

function charRangeRect(root, charOffset, len) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let acc = 0;
  let startNode = null;
  let startOff = 0;
  let endNode = null;
  let endOff = 0;
  const targetEnd = charOffset + len;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const nLen = node.nodeValue.length;
    if (!startNode && acc + nLen >= charOffset) { startNode = node; startOff = Math.max(0, charOffset - acc); }
    if (startNode && acc + nLen >= targetEnd) { endNode = node; endOff = Math.max(0, targetEnd - acc); break; }
    acc += nLen;
  }
  if (!startNode || !endNode) return null;
  try {
    const range = document.createRange();
    range.setStart(startNode, startOff);
    range.setEnd(endNode, endOff);
    return range.getBoundingClientRect();
  } catch (_e) { return null; }
}

function pointToCharOffset(clientX, clientY) {
  const src = codeInput.value;
  if (!src) return null;
  const er = codeInput.getBoundingClientRect();
  if (clientX < er.left || clientX > er.right || clientY < er.top || clientY > er.bottom) return null;
  let node = null;
  let nodeOffset = 0;
  const prevIn = codeInput.style.pointerEvents;
  const prevHl = codeHighlight.style.pointerEvents;
  codeInput.style.pointerEvents = "none";
  codeHighlight.style.pointerEvents = "auto";
  try {
    if (typeof document.caretPositionFromPoint === "function") {
      const pos = document.caretPositionFromPoint(clientX, clientY);
      if (pos) { node = pos.offsetNode; nodeOffset = pos.offset; }
    } else if (typeof document.caretRangeFromPoint === "function") {
      const range = document.caretRangeFromPoint(clientX, clientY);
      if (range) { node = range.startContainer; nodeOffset = range.startOffset; }
    }
  } finally {
    codeInput.style.pointerEvents = prevIn;
    codeHighlight.style.pointerEvents = prevHl;
  }
  if (!node || !codeHighlightInner.contains(node)) return null;
  let charOff = domPosToCharOffset(codeHighlightInner, node, nodeOffset);
  if (charOff === null) return null;
  charOff = Math.min(charOff, src.length);
  if (charOff < src.length && src.charAt(charOff) !== "\n") {
    const r = charRangeRect(codeHighlightInner, charOff, 1);
    if (r && rectContainsPoint(r, clientX, clientY)) return charOff;
  }
  if (charOff > 0 && src.charAt(charOff - 1) !== "\n") {
    const rPrev = charRangeRect(codeHighlightInner, charOff - 1, 1);
    if (rPrev && rectContainsPoint(rPrev, clientX, clientY)) return charOff - 1;
  }
  return null;
}

function queryHoverAt(src, bytes, charOffset) {
  const byteOff = charOffsetToByteOffset(src, charOffset);
  const outLen = wasm.athena_hover ? withBytes(bytes, (ptr, len) => wasm.athena_hover(ptr, len, byteOff)) : 0;
  return outLen > 0 ? readAthenaLast(outLen) : "";
}

function showHoverAtPoint(clientX, clientY) {
  if (!wasm || !activeFile) return;
  const charOff = pointToCharOffset(clientX, clientY);
  if (charOff === null) return hideTooltip();
  const src = codeInput.value;
  const text = queryHoverAt(src, encoder.encode(src), charOff);
  if (!text) return hideTooltip();
  showTooltipAt(text, clientX, clientY, true);
}

function showHoverAtCaret() {
  if (!wasm || !activeFile) return;
  const src = codeInput.value;
  const pos = codeInput.selectionStart || 0;
  const bytes = encoder.encode(src);
  let usedOffset = pos;
  let text = queryHoverAt(src, bytes, pos);
  if (!text && pos > 0 && /[A-Za-z0-9_]/.test(src.charAt(pos - 1))) {
    usedOffset = pos - 1;
    text = queryHoverAt(src, bytes, usedOffset);
  }
  if (!text) return hideTooltip();
  const rect = charRangeRect(codeHighlightInner, usedOffset, 1) || codeInput.getBoundingClientRect();
  showTooltipAt(text, rect.left, rect.top, false);
}

function queryDefinitionAt(src, bytes, charOffset) {
  const outLen = withBytes(bytes, (ptr, len) => wasm.athena_definition(ptr, len, charOffsetToByteOffset(src, charOffset)));
  if (outLen <= 0) return null;
  const parts = readAthenaLast(outLen).trim().split(" ");
  if (parts.length < 2) return null;
  const s = Number.parseInt(parts[0], 10);
  const e = Number.parseInt(parts[1], 10);
  return Number.isNaN(s) || Number.isNaN(e)
    ? null
    : { startChar: byteOffsetToCharOffset(src, s), endChar: byteOffsetToCharOffset(src, e) };
}

function goToDefinitionAtOffset(charOffset, allowPrevChar) {
  if (!wasm || !activeFile || typeof wasm.athena_definition !== "function") return;
  hideTooltip();
  const src = codeInput.value;
  const bytes = encoder.encode(src);
  let def = queryDefinitionAt(src, bytes, charOffset);
  if (!def && allowPrevChar && charOffset > 0 && /[A-Za-z0-9_]/.test(src.charAt(charOffset - 1))) {
    def = queryDefinitionAt(src, bytes, charOffset - 1);
  }
  if (!def) { transientStatus = true; setEditorStatus("No definition found.", false); return; }
  transientStatus = false;
  codeInput.focus();
  codeInput.setSelectionRange(def.startChar, def.endChar);
  const line = src.slice(0, def.startChar).split("\n").length;
  const lineTop = (line - 1) * LINE_HEIGHT_PX;
  if (lineTop < codeInput.scrollTop || line * LINE_HEIGHT_PX > codeInput.scrollTop + codeInput.clientHeight) {
    codeInput.scrollTop = Math.max(0, (line - 3) * LINE_HEIGHT_PX);
  }
  syncScroll();
  updateCursorMeta();
  refreshAnalysis();
}

function updateDraftFromBuffer() {
  if (!activeFile) return;
  if (codeInput.value === files[activeFile]) delete drafts[activeFile];
  else drafts[activeFile] = codeInput.value;
}

function applyFormattedValue(formatted) {
  const before = codeInput.value;
  const selStart = codeInput.selectionStart || 0;
  const selEnd = codeInput.selectionEnd || 0;
  const beforeParts = before.slice(0, selStart).split("\n");
  codeInput.focus();
  codeInput.setSelectionRange(0, before.length);
  let applied = false;
  try { applied = document.execCommand("insertText", false, formatted); } catch (_e) { applied = false; }
  if (!applied || codeInput.value !== formatted) {
    codeInput.value = formatted;
    formatUndo = { file: activeFile, before, after: formatted, selStart, selEnd };
  } else formatUndo = null;
  if (debounceTimer !== null) { clearTimeout(debounceTimer); debounceTimer = null; }
  updateDraftFromBuffer();
  updateGutter(codeInput.value);
  const formattedLines = codeInput.value.split("\n");
  const clampedLine = Math.min(beforeParts.length, formattedLines.length);
  const clampedCol = Math.min(beforeParts[beforeParts.length - 1].length + 1, formattedLines[clampedLine - 1].length + 1);
  const nextOffset = lineColToCharOffset(codeInput.value, clampedLine, clampedCol);
  codeInput.setSelectionRange(nextOffset, nextOffset);
  updateCursorMeta();
  syncScroll();
  transientStatus = false;
  refreshAnalysis();
  workbench.renderTabs();
  workbench.renderFileTree();
}

function formatActiveFile() {
  if (!wasm || !activeFile || typeof wasm.athena_format !== "function") return;
  hideTooltip();
  const src = codeInput.value;
  const resp = readAthenaLast(withBytes(encoder.encode(src), (ptr, len) => wasm.athena_format(ptr, len)));
  if (resp.startsWith("error: ")) { transientStatus = true; setEditorStatus(resp.slice(7), true); return; }
  if (!resp.startsWith("ok\n")) { transientStatus = true; setEditorStatus(resp || "Format failed.", true); return; }
  const formatted = resp.slice(3);
  if (formatted === src) { transientStatus = false; refreshAnalysis(); } else applyFormattedValue(formatted);
}

const checkFileDiagnostics = (name) =>
  !wasm || !name || !name.endsWith(".logos")
    ? []
    : parseDiagnostics(readAthenaLast(withBytes(encoder.encode(getFileContent(name)), (ptr, len) => wasm.athena_check(ptr, len))));

function refreshOpenTabProblems() {
  const entries = [];
  for (const name of openTabs) {
    if (!hasOwn(diagnosticsByFile, name)) diagnosticsByFile[name] = checkFileDiagnostics(name);
    for (const d of diagnosticsByFile[name]) entries.push({ file: name, ...d });
  }
  workbench.updateProblems(entries);
}

function refreshAnalysis() {
  if (!activeFile) { codeHighlightInner.textContent = ""; setEditorStatus("", false); return refreshOpenTabProblems(); }
  const src = codeInput.value;
  if (!wasm) {
    codeHighlightInner.textContent = src.endsWith("\n") ? `${src}\n` : src;
    setEditorStatus("", false);
    return refreshOpenTabProblems();
  }
  const bytes = encoder.encode(src);
  const runs = parseHighlight(readAthenaLast(withBytes(bytes, (ptr, len) => wasm.athena_highlight(ptr, len))));
  const diags = activeFile.endsWith(".logos")
    ? parseDiagnostics(readAthenaLast(withBytes(bytes, (ptr, len) => wasm.athena_check(ptr, len))))
    : [];
  diagnosticsByFile[activeFile] = diags;
  renderHighlighted(codeHighlightInner, src, bytes, runs, diags);
  refreshOpenTabProblems();
  if (diags.length > 0) {
    transientStatus = false;
    setEditorStatus(`error: ${diags[0].line}:${diags[0].col}: ${diags[0].message}`, true);
  } else if (!transientStatus) setEditorStatus("", false);
}

function scheduleAnalysis() {
  if (debounceTimer !== null) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => { debounceTimer = null; refreshAnalysis(); }, DEBOUNCE_MS);
}

function updateGutter(src) {
  const lines = src.split("\n").length;
  const nums = [];
  for (let i = 1; i <= lines; i += 1) nums.push(String(i));
  gutter.textContent = nums.join("\n");
}

function updateCursorMeta() {
  let line = 1;
  let col = 1;
  if (activeFile) {
    const parts = codeInput.value.slice(0, codeInput.selectionStart || 0).split("\n");
    line = parts.length;
    col = parts[parts.length - 1].length + 1;
  }
  workbench.updateStatusBar({
    activeFile, dirty: isDirty(activeFile), saveText: lastSaveText, line, col, wasmReady: Boolean(wasm)
  });
}

const syncScroll = () => {
  codeHighlight.scrollTop = codeInput.scrollTop;
  codeHighlight.scrollLeft = codeInput.scrollLeft;
  gutter.scrollTop = codeInput.scrollTop;
};

function syncEditorFromState() {
  const hasFile = activeFile !== null && hasOwn(files, activeFile);
  editorEmpty.hidden = hasFile;
  editorWrap.hidden = !hasFile;
  if (!hasFile) {
    codeInput.value = "";
    codeHighlightInner.textContent = "";
    gutter.textContent = "";
    setEditorStatus("", false);
    hideTooltip();
    refreshOpenTabProblems();
    return updateCursorMeta();
  }
  const src = getFileContent(activeFile);
  codeInput.value = src;
  updateGutter(src);
  updateCursorMeta();
  refreshAnalysis();
  syncScroll(); debug.refresh();
}

codeInput.addEventListener("input", () => {
  if (!activeFile) return;
  transientStatus = false;
  formatUndo = null;
  hideTooltip();
  updateDraftFromBuffer();
  updateGutter(codeInput.value);
  updateCursorMeta();
  workbench.renderTabs();
  workbench.renderFileTree();
  scheduleAnalysis();
});
codeInput.addEventListener("scroll", () => { hideTooltip(); syncScroll(); });
codeInput.addEventListener("blur", hideTooltip);
codeInput.addEventListener("mouseleave", hideTooltip);
codeInput.addEventListener("mousemove", (ev) => {
  if (hoverTimer !== null) { clearTimeout(hoverTimer); hoverTimer = null; }
  if (hoverShown && hoverShown.byPointer && (Math.abs(ev.clientX - hoverShown.x) > 8 || Math.abs(ev.clientY - hoverShown.y) > 8)) hideTooltip();
  if (!wasm || !activeFile) return;
  const { clientX: x, clientY: y } = ev;
  hoverTimer = setTimeout(() => { hoverTimer = null; showHoverAtPoint(x, y); }, 400);
});
codeInput.addEventListener("click", (ev) => {
  updateCursorMeta();
  if (ev.ctrlKey || ev.metaKey) goToDefinitionAtOffset(codeInput.selectionStart || 0, false); else hideTooltip();
});
codeInput.addEventListener("select", updateCursorMeta);
codeInput.addEventListener("keyup", (ev) => {
  updateCursorMeta();
  if (hoverShown && !hoverShown.byPointer && !(ev.ctrlKey || ev.metaKey) && ev.key !== "Control" && ev.key !== "Meta") hideTooltip();
});
document.addEventListener("selectionchange", () => { if (document.activeElement === codeInput) updateCursorMeta(); });

function undoLastFormat() {
  const undo = formatUndo;
  formatUndo = null;
  codeInput.value = undo.before;
  codeInput.setSelectionRange(undo.selStart, undo.selEnd);
  if (debounceTimer !== null) { clearTimeout(debounceTimer); debounceTimer = null; }
  updateDraftFromBuffer();
  updateGutter(undo.before);
  updateCursorMeta();
  syncScroll();
  transientStatus = false;
  refreshAnalysis();
  workbench.renderTabs();
  workbench.renderFileTree();
}

codeInput.addEventListener("keydown", (ev) => {
  const mod = ev.ctrlKey || ev.metaKey;
  if (ev.key === "Escape") return hideTooltip();
  if (ev.key === "F12" && !ev.shiftKey && !mod && !ev.altKey) { ev.preventDefault(); return goToDefinitionAtOffset(codeInput.selectionStart || 0, true); }
  if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "i") { ev.preventDefault(); return showHoverAtCaret(); }
  if (ev.shiftKey && ev.altKey && !mod && (ev.key.toLowerCase() === "f" || ev.code === "KeyF")) { ev.preventDefault(); return formatActiveFile(); }
  if (mod && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "z" && formatUndo && formatUndo.file === activeFile && codeInput.value === formatUndo.after) {
    ev.preventDefault();
    return undoLastFormat();
  }
  if (ev.key === "Tab" && !mod && !ev.altKey) {
    ev.preventDefault();
    const start = codeInput.selectionStart;
    codeInput.value = `${codeInput.value.slice(0, start)}    ${codeInput.value.slice(codeInput.selectionEnd)}`;
    codeInput.setSelectionRange(start + 4, start + 4);
    codeInput.dispatchEvent(new Event("input", { bubbles: true }));
  } else if (mod && ev.key === "Enter") {
    ev.preventDefault();
    runActiveFile();
  }
});

document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") hideTooltip();
  else if (ev.target !== codeInput && ev.shiftKey && ev.altKey && !ev.ctrlKey && !ev.metaKey && (ev.key.toLowerCase() === "f" || ev.code === "KeyF")) { ev.preventDefault(); formatActiveFile(); }
  else if (ev.target !== codeInput && ev.key === "F12" && !ev.shiftKey && !ev.ctrlKey && !ev.metaKey && !ev.altKey) { ev.preventDefault(); goToDefinitionAtOffset(codeInput.selectionStart || 0, true); }
});

function runActiveFile() {
  if (!wasm || !activeFile) return;
  workbench.setPanelTab("output");
  runOut.classList.remove("is-error");
  const src = codeInput.value;
  // Same cap as the Rust exports (service::MAX_INPUT_BYTES): a huge paste would hang the tab.
  if (encoder.encode(src).length > MAX_SOURCE_BYTES) {
    runOut.classList.add("is-error");
    runOut.textContent = `error: input too large, the limit is ${MAX_SOURCE_BYTES} bytes`;
    return;
  }
  runOut.textContent = "running…";
  runLine.start();
  afterPaint(() => {
    const text = readNeonLast(withBytes(encoder.encode(src), (ptr, len) => wasm.logos_run(ptr, len, LOGOS_FUEL)));
    runOut.textContent = text || "(no output)";
    if (text.startsWith("error: ") || text.startsWith("trap: ")) runOut.classList.add("is-error");
    runLine.land(text);
    refreshAnalysis();
  });
}

function renderTestResults(raw) {
  if (!raw) {
    testSummary.className = "test-summary";
    testSummary.textContent = "No unit tests or properties found in this file.";
    testList.replaceChildren();
    return { passed: 0, failed: 0 };
  }
  let passed = 0, failed = 0;
  const frag = document.createDocumentFragment();
  for (const line of raw.split("\n").filter(Boolean)) {
    const li = document.createElement("li");
    const isPass = line.startsWith("PASS ");
    if (isPass) passed += 1; else failed += 1;
    if (isPass || line.startsWith("FAIL ")) {
      li.className = isPass ? "test-item is-pass" : "test-item is-fail";
      const badge = document.createElement("span");
      badge.className = isPass ? "test-badge-pass" : "test-badge-fail";
      badge.textContent = isPass ? "PASS " : "FAIL ";
      li.append(badge, document.createTextNode(line.slice(5)));
    } else {
      li.className = "test-item is-fail";
      li.textContent = line;
    }
    frag.appendChild(li);
  }
  testSummary.className = failed === 0 ? "test-summary is-ok" : "test-summary is-bad";
  testSummary.textContent = `${passed} passed, ${failed} failed`;
  testList.replaceChildren(frag);
  return { passed, failed };
}

function testActiveFile() {
  if (!wasm || !activeFile) return;
  workbench.setPanelTab("tests");
  testSummary.className = "test-summary";
  testSummary.textContent = "running tests…";
  const src = codeInput.value;
  afterPaint(() => renderTestResults(readAthenaLast(withBytes(encoder.encode(src), (p, n) => wasm.athena_test(p, n, LOGOS_FUEL)))));
}

function runAllTests() {
  if (!wasm) return null;
  const combined = [], byFile = {};
  let totalPassed = 0, totalFailed = 0;
  for (const name of sortedFileNames().filter((n) => n.endsWith(".logos"))) {
    const raw = readAthenaLast(withBytes(encoder.encode(getFileContent(name)), (p, n) => wasm.athena_test(p, n, LOGOS_FUEL)));
    if (!raw) { byFile[name] = "0"; continue; }
    let fp = 0, ff = 0;
    for (const line of raw.split("\n").filter(Boolean)) {
      const ok = line.startsWith("PASS ");
      if (ok) { fp += 1; combined.push(`PASS ${name}: ${line.slice(5)}`); }
      else { ff += 1; combined.push(`FAIL ${name}: ${line.startsWith("FAIL ") ? line.slice(5) : line}`); }
    }
    totalPassed += fp;
    totalFailed += ff;
    byFile[name] = ff > 0 ? `${ff} fail` : `${fp} pass`;
  }
  workbench.setPanelTab("tests");
  renderTestResults(combined.join("\n"));
  return { passed: totalPassed, failed: totalFailed, byFile };
}

btnRun.addEventListener("click", runActiveFile);
btnTest.addEventListener("click", testActiveFile);
btnFormat.addEventListener("click", formatActiveFile);

// Titan pump: one slice of guest steps per animation frame; Term appends the output it returns.
function termPump() {
  if (!wasm) return { status: "done", text: "" };
  const rc = wasm.titan_pump(STEPS_PER_FRAME);
  const outLen = wasm.titan_output();
  const text = outLen > 0 ? readNeonLast(outLen) : "";
  if (rc === 1) { titanStatus.textContent = "running"; return { status: "busy", text }; }
  if (rc === 0) { titanStatus.textContent = "waiting for input"; return { status: "waiting", text }; }
  titanStatus.textContent = rc >= 2 ? `exited (${rc - 2})` : "not booted";
  return { status: "done", text };
}

const term = Term.mount($("term-root"), {
  prompt: "$",
  pump: termPump,
  send: (line) => {
    if (!wasm) return;
    withBytes(encoder.encode(line), (ptr, len) => wasm.titan_send(ptr, len));
  }
});

async function bootTitanIfNeeded() {
  if (titanBootStarted || !wasm) return;
  titanBootStarted = true;
  titanStatus.textContent = "fetching kernel…";
  try {
    const elfResp = await fetch("titan.elf");
    if (!elfResp.ok) throw new Error(`failed to fetch titan.elf: ${elfResp.status}`);
    const rc = withBytes(new Uint8Array(await elfResp.arrayBuffer()), (ptr, len) => wasm.titan_boot(ptr, len));
    if (rc !== 0) throw new Error(readNeonLast(wasm.neon_last_len()) || "titan_boot failed");
    titanStatus.textContent = "booting…";
    term.start();
  } catch (err) {
    titanStatus.textContent = "failed";
    term.append(`\n[titan error] ${err && err.message ? err.message : String(err)}\n`);
  }
}

async function loadWasm() {
  return (await Neon.load("athena.wasm")).exports;
}

const runLine = createRunLine(document.getElementById("editor-area"));
const debug = createDebugUi({ wasm: () => wasm, withBytes, readAthenaLast, activeFile: () => activeFile, setStatus: (m, e) => { transientStatus = true; setEditorStatus(m, e); } });
const workbench = createWorkbench({
  getFileNames: sortedFileNames, getFileContent, getOpenTabs: () => [...openTabs],
  getTreeRows: () => (FS ? FS.tree.rows() : sortedFileNames().map((n) => ({ path: n, name: n, depth: 0, dir: false }))),
  toggleDir: (path) => FS && FS.tree.toggle(path),
  getActiveFile: () => activeFile, isDirty, setActiveFile, openFileAt, saveFile,
  discardDraft, closeTab, cycleTab, createFile, renameFile, deleteFile,
  runActiveFile, testActiveFile, runAllTests, formatActiveFile, debugCommand: (id) => debug.command(id),
  goToDefinitionAtCaret: () => goToDefinitionAtOffset(codeInput.selectionStart || 0, true),
  showHoverAtCaret, focusEditor: () => codeInput.focus(),
  focusTerminalInput: () => { bootTitanIfNeeded(); term.focus(); },
  onTerminalTabOpened: bootTitanIfNeeded,
  setTransientStatus: (msg, isError) => { transientStatus = true; setEditorStatus(msg, isError); }
});

// The worker answers every GET from its cache, which would hide live folder listings and files.
if (!FS && "serviceWorker" in navigator) navigator.serviceWorker.register("sw.js").catch(() => {});

async function init() {
  workbench.renderAll();
  syncEditorFromState();
  if (FS) {
    const listError = await FS.tree.load("");
    if (listError) setEditorStatus(`Folder not listed: ${listError}`, true);
  }
  try {
    wasm = await loadWasm();
    workbench.setWasmStatus("wasm ok");
    diagnosticsByFile = {};
    syncEditorFromState();
  } catch (err) {
    workbench.setWasmStatus("wasm error");
    runOut.classList.add("is-error");
    runOut.textContent = err && err.message ? err.message : String(err);
  }
}

init();
