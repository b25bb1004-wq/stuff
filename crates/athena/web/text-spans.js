// Pure text, offset and span helpers for the editor. No DOM state except the element passed in.
const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function parseDiagnostics(raw) {
  if (!raw) return [];
  const out = [];
  for (const line of raw.split("\n")) {
    const p = line ? line.split(":") : [];
    if (p.length >= 5) {
      out.push({
        line: Number.parseInt(p[0], 10) || 1,
        col: Number.parseInt(p[1], 10) || 1,
        endLine: Number.parseInt(p[2], 10) || 1,
        endCol: Number.parseInt(p[3], 10) || 1,
        message: p.slice(4).join(":")
      });
    }
  }
  return out;
}

export function parseHighlight(raw) {
  if (!raw) return [];
  const runs = [];
  for (const line of raw.split("\n")) {
    const p = line ? line.split(" ") : [];
    if (p.length >= 3) {
      const start = Number.parseInt(p[0], 10);
      const len = Number.parseInt(p[1], 10);
      if (!Number.isNaN(start) && !Number.isNaN(len) && len > 0) runs.push({ start, end: start + len, cls: p[2] });
    }
  }
  return runs;
}

export function lineColToCharOffset(src, targetLine, targetCol) {
  let line = 1;
  let idx = 0;
  while (line < targetLine && idx < src.length) {
    if (src.charCodeAt(idx) === 10) line += 1;
    idx += 1;
  }
  let col = 1;
  while (col < targetCol && idx < src.length && src.charCodeAt(idx) !== 10) {
    idx += 1;
    col += 1;
  }
  return idx;
}

export function buildDiagByteRanges(src, totalBytes, diags) {
  const ranges = [];
  for (const d of diags) {
    const startChar = lineColToCharOffset(src, d.line, d.col);
    let endChar = lineColToCharOffset(src, d.endLine, d.endCol);
    if (endChar <= startChar) endChar = startChar < src.length && src.charCodeAt(startChar) !== 10 ? startChar + 1 : startChar;
    const startByte = encoder.encode(src.slice(0, startChar)).length;
    let endByte = encoder.encode(src.slice(0, endChar)).length;
    if (endByte <= startByte && startByte < totalBytes) endByte = Math.min(totalBytes, startByte + 1);
    if (startByte < endByte) ranges.push([startByte, endByte]);
  }
  return ranges;
}

export const hasDiagOverlap = (s, e, ranges) => ranges.some(([ds, de]) => s < de && e > ds);

export function renderHighlighted(container, src, bytes, runs, diags) {
  if (runs.length === 0) { container.textContent = src; return; }
  const diagRanges = buildDiagByteRanges(src, bytes.length, diags);
  const cuts = new Set();
  for (const r of runs) { cuts.add(r.start); cuts.add(r.end); }
  for (const [ds, de] of diagRanges) { cuts.add(ds); cuts.add(de); }
  const sortedCuts = Array.from(cuts).sort((a, b) => a - b);
  const frag = document.createDocumentFragment();
  let runIdx = 0;
  for (let i = 0; i + 1 < sortedCuts.length; i += 1) {
    const segStart = sortedCuts[i];
    const segEnd = sortedCuts[i + 1];
    if (segStart >= segEnd) continue;
    while (runIdx < runs.length && runs[runIdx].end <= segStart) runIdx += 1;
    if (runIdx >= runs.length) break;
    const run = runs[runIdx];
    const text = decoder.decode(bytes.subarray(segStart, segEnd));
    const isDiag = hasDiagOverlap(segStart, segEnd, diagRanges);
    if (run.cls === "plain" && !isDiag) frag.appendChild(document.createTextNode(text));
    else {
      const span = document.createElement("span");
      span.className = isDiag ? `hl-${run.cls} diag-err` : `hl-${run.cls}`;
      span.textContent = text;
      frag.appendChild(span);
    }
  }
  if (src.endsWith("\n")) frag.appendChild(document.createTextNode("\n"));
  container.replaceChildren(frag);
}

export const utf8LenOfCodePoint = (cp) => (cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4);
export const charOffsetToByteOffset = (src, off) => encoder.encode(src.slice(0, Math.max(0, Math.min(off, src.length)))).length;

export function byteOffsetToCharOffset(src, byteOffset) {
  let bytes = 0;
  let chars = 0;
  while (chars < src.length && bytes < byteOffset) {
    const cp = src.codePointAt(chars);
    bytes += utf8LenOfCodePoint(cp);
    chars += cp > 0xffff ? 2 : 1;
  }
  return chars;
}

export function domPosToCharOffset(root, targetNode, targetOffset) {
  try {
    const r = document.createRange();
    r.setStart(root, 0);
    r.setEnd(targetNode, targetOffset);
    return r.toString().length;
  } catch (_e) {
    return null;
  }
}

export const rectContainsPoint = (r, x, y) =>
  r.width > 0 && r.height > 0 && x >= r.left - 2 && x <= r.right + 2 && y >= r.top - 2 && y <= r.bottom + 2;
