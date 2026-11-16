// Circuit board for the Monolith home page (T130a). Global `Board`: Board.mount(canvas, systems, opts).
// Canvas 2D only. Frames come from the kit (Motion.canvas, Motion.tween). The frame job stops when no pulse
// or blink is live and while the tab is hidden. Colors come from the CSS tokens on :root.
(function (global) {
  "use strict";

  // Child: parents. Only edges between the 25 board systems. Each child traces to its parents.
  var DEPS = {
    argo: ["sphinx", "mercury"],
    athena: ["calliope", "cobalt", "elysium", "logos"],
    aurum: ["elysium", "logos", "sphinx", "cobalt", "xenon"],
    calliope: ["logos"],
    chronos: ["sphinx", "mercury"],
    cobalt: ["elysium", "logos", "silicon"],
    helios: ["elysium", "logos", "cobalt"],
    iris: ["indium", "krypton", "calliope", "hades"],
    lithium: ["silicon"],
    mercury: ["kairos"],
    odyssey: ["cobalt", "elysium", "indium", "logos", "mercury"],
    pandora: ["argo"],
    titan: ["midas", "kairos", "hades", "krypton", "silicon", "iris"],
    xenon: ["sphinx"]
  };
  // Rows from the top of the board down: apps, services, kernel, hardware. Each row is centered.
  var ROWS = [
    ["odyssey", "helios", "athena", "calliope", "iris", "indium", "cobalt", "logos", "elysium"],
    ["mercury", "xenon", "iridium", "krypton", "chronos", "argo", "pandora", "sphinx", "aurum"],
    ["lithium", "kairos", "titan", "midas", "hades"],
    ["silicon", "zeus"]
  ];
  var BIG = { titan: [240, 112] };

  var BW = 1520;
  var NW = 120;
  var NH = 56;
  var GAP_X = 40;
  var GAP_Y = 120;
  var TOP = 60;
  var BOTTOM = 60;
  var PIN = 8;
  var LANE0 = 28;
  var LANE_STEP = 6;
  var LANES = 14;
  var CHAMFER = 8;
  var TRACE_W = 3;
  var PAD_R = 3.5;
  var PULSE_MS = 400;
  var PULSE_CAP = 24;
  var HALF_TARGET = 22;
  var CAP_W = 1280;
  var CAP_H = 720;
  var MARGIN_CSS = 24;
  var FALLBACK = { mask: "#0B3D2E", maskDeep: "#072A20", copper: "#C8803C", signal: "#FFC46B", silk: "#EEF0E6", silkDim: "#9DB5A8" };
  var TOKEN = { mask: "--mask", maskDeep: "--mask-deep", copper: "--copper", signal: "--signal", silk: "--silk", silkDim: "--silk-dim" };
  var FONT_NAME = '600 12px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_REF = '400 11px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_TITLE = '600 14px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_BODY = '400 13px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';

  function dist(a, b) {
    return Math.hypot(b[0] - a[0], b[1] - a[1]);
  }
  function towards(from, to, k) {
    var d = dist(from, to) || 1;
    return [from[0] + ((to[0] - from[0]) * k) / d, from[1] + ((to[1] - from[1]) * k) / d];
  }
  // Orthogonal polyline with 45-degree chamfers at each corner.
  function chamfer(pts) {
    var out = [pts[0]];
    for (var i = 1; i < pts.length - 1; i++) {
      var c = pts[i];
      var k = Math.min(CHAMFER, 0.45 * Math.min(dist(c, pts[i - 1]), dist(c, pts[i + 1])));
      out.push(towards(c, pts[i - 1], k), towards(c, pts[i + 1], k));
    }
    out.push(pts[pts.length - 1]);
    return out;
  }
  function makePath(pts) {
    var out = [pts[0]];
    for (var i = 1; i < pts.length; i++) {
      var p = pts[i];
      var q = out[out.length - 1];
      if (Math.abs(p[0] - q[0]) > 0.01 || Math.abs(p[1] - q[1]) > 0.01) out.push(p);
    }
    var lens = [0];
    for (i = 1; i < out.length; i++) lens.push(lens[i - 1] + dist(out[i - 1], out[i]));
    return { pts: out, lens: lens, total: lens[lens.length - 1] };
  }
  function pointAt(path, s) {
    var L = path.lens;
    var P = path.pts;
    for (var i = 0; i < P.length - 1; i++) {
      if (s <= L[i + 1] || i === P.length - 2) {
        var seg = L[i + 1] - L[i] || 1;
        var f = Math.min(1, Math.max(0, (s - L[i]) / seg));
        return [P[i][0] + (P[i + 1][0] - P[i][0]) * f, P[i][1] + (P[i + 1][1] - P[i][1]) * f];
      }
    }
    return P[P.length - 1];
  }
  // Points of `path` between arc lengths s0 and s1.
  function sub(path, s0, s1) {
    var out = [pointAt(path, s0)];
    for (var i = 1; i < path.pts.length - 1; i++) {
      if (path.lens[i] > s0 && path.lens[i] < s1) out.push(path.pts[i]);
    }
    out.push(pointAt(path, s1));
    return out;
  }
  function strokePts(ctx, pts, color, lw) {
    ctx.beginPath();
    ctx.moveTo(pts[0][0], pts[0][1]);
    for (var i = 1; i < pts.length; i++) ctx.lineTo(pts[i][0], pts[i][1]);
    ctx.strokeStyle = color;
    ctx.lineWidth = lw;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.stroke();
  }
  function roundRect(ctx, x, y, w, h, r) {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }
  function wrap(ctx, text, max) {
    var out = [];
    var cur = "";
    String(text || "").split(/\s+/).forEach(function (word) {
      var next = cur ? cur + " " + word : word;
      if (cur && ctx.measureText(next).width > max) {
        out.push(cur);
        cur = word;
      } else {
        cur = next;
      }
    });
    if (cur) out.push(cur);
    return out;
  }
  function tokens() {
    var cs = getComputedStyle(document.documentElement);
    var out = {};
    Object.keys(TOKEN).forEach(function (k) {
      out[k] = cs.getPropertyValue(TOKEN[k]).trim() || FALLBACK[k];
    });
    return out;
  }
  function zoomMs() {
    return typeof MOTION_TABLE === "object" && MOTION_TABLE.firm ? MOTION_TABLE.firm.ms : PULSE_MS;
  }

  // Fixed rows, centered. Each chip has real pins (one per connection) and 2 to 4 dummy pins.
  // A pair of chips in adjacent rows routes through the channel between them. Same-row pairs route through
  // the channel below. Pairs that skip rows drop a vertical bus through a clear column gap, and every trace
  // stays within one chip width of its own pads.
  function build(systems) {
    var chips = [];
    var byId = {};
    systems.forEach(function (s, i) {
      var c = {
        id: s.id, name: s.name, what: s.what || "", line: s.line || "", index: i,
        w: NW, h: NH, x: 0, y: 0, row: 0, ref: "", pins: [], real: [], depIds: [], segs: [], flash: -1
      };
      if (BIG[s.id]) {
        c.w = BIG[s.id][0];
        c.h = BIG[s.id][1];
      }
      chips.push(c);
      byId[s.id] = c;
    });
    chips.forEach(function (c) {
      (DEPS[c.id] || []).forEach(function (id) {
        if (byId[id] && id !== c.id && c.depIds.indexOf(id) < 0) c.depIds.push(id);
      });
    });

    var rowLists = ROWS.map(function (ids) {
      return ids.filter(function (id) { return byId[id]; }).map(function (id) { return byId[id]; });
    });
    var listed = {};
    rowLists.forEach(function (l) { l.forEach(function (c) { listed[c.id] = true; }); });
    chips.forEach(function (c) { if (!listed[c.id]) rowLists[1].push(c); });

    var rowTop = [];
    var rowH = [];
    var y = TOP;
    rowLists.forEach(function (list, r) {
      rowH[r] = list.reduce(function (m, c) { return Math.max(m, c.h); }, NH);
      rowTop[r] = y;
      y += rowH[r] + GAP_Y;
    });
    var BH = rowTop[rowLists.length - 1] + rowH[rowLists.length - 1] + BOTTOM;
    rowLists.forEach(function (list, r) {
      var total = list.reduce(function (s, c) { return s + c.w; }, 0) + GAP_X * (list.length - 1);
      var x = (BW - total) / 2;
      list.forEach(function (c) {
        c.row = r;
        c.x = x;
        c.y = rowTop[r];
        x += c.w + GAP_X;
      });
    });
    var refN = 0;
    rowLists.slice().reverse().forEach(function (list) {
      list.forEach(function (c) { c.ref = "U" + (++refN); });
    });

    // Edges: up is the upper endpoint (smaller row), lo the lower one.
    var edges = [];
    chips.forEach(function (a) {
      a.depIds.forEach(function (id) {
        var b = byId[id];
        var e = { index: edges.length, a: a, b: b, up: a, lo: b, segs: [] };
        if (a.row > b.row) {
          e.up = b;
          e.lo = a;
        }
        e.span = e.lo.row - e.up.row;
        e.pu = { edge: e, chip: e.up, side: "bot", other: e.lo, real: true, x: 0, y: 0 };
        e.pl = { edge: e, chip: e.lo, side: e.span === 0 ? "bot" : "top", other: e.up, real: true, x: 0, y: 0 };
        e.up.real.push(e.pu);
        e.lo.real.push(e.pl);
        edges.push(e);
      });
    });

    // Pins: real pins sorted by the other end's x, dummies at the outer ends, all spread across the edge.
    chips.forEach(function (c) {
      var d = 2 + (c.index % 3);
      var nTop = Math.floor(d / 2);
      var nBot = d - nTop;
      ["top", "bot"].forEach(function (side) {
        var list = c.real.filter(function (p) { return p.side === side; });
        list.sort(function (p, q) { return p.other.x - q.other.x || p.edge.index - q.edge.index; });
        var nd = side === "top" ? nTop : nBot;
        for (var k = 0; k < nd; k++) {
          var dummy = { edge: null, chip: c, side: side, real: false, x: 0, y: 0 };
          if (k % 2 === 0) list.push(dummy);
          else list.unshift(dummy);
        }
        list.forEach(function (p, i) {
          p.x = c.x + (c.w * (i + 1)) / (list.length + 1);
          p.y = side === "top" ? c.y - PIN : c.y + c.h + PIN;
        });
        c.pins = c.pins.concat(list);
      });
    });

    // Vertical bus for row-skipping pairs: a clear column gap (or board margin) in every row between them.
    var cands = [24, BW - 24];
    rowLists.forEach(function (list) {
      for (var i = 0; i < list.length - 1; i++) cands.push((list[i].x + list[i].w + list[i + 1].x) / 2);
    });
    function clear(x, r0, r1) {
      for (var r = r0; r <= r1; r++) {
        var hit = rowLists[r].some(function (c) { return x > c.x - 6 && x < c.x + c.w + 6; });
        if (hit) return false;
      }
      return true;
    }
    var used = [];
    edges.forEach(function (e) {
      if (e.span < 2) return;
      var xs = [e.pu.x, e.pl.x];
      var lo = Math.min(xs[0], xs[1]) - NW;
      var hi = Math.max(xs[0], xs[1]) + NW;
      var mid = (xs[0] + xs[1]) / 2;
      var ok = cands.filter(function (x) {
        return x >= lo && x <= hi && clear(x, e.up.row + 1, e.lo.row - 1);
      });
      var fresh = ok.filter(function (x) {
        return !used.some(function (u) {
          return u.x === x && !(u.rl < e.up.row || u.ru > e.lo.row);
        });
      });
      var pool = fresh.length ? fresh : ok;
      pool.sort(function (p, q) { return Math.abs(p - mid) - Math.abs(q - mid) || p - q; });
      e.bus = pool.length ? pool[0] : mid;
      used.push({ x: e.bus, ru: e.up.row, rl: e.lo.row });
    });

    // Horizontal segments per channel. Channel g sits between row g-1 and row g.
    var segs = {};
    function addSeg(e, g, x1, x2) {
      var s = { g: g, x1: Math.min(x1, x2), x2: Math.max(x1, x2), owner: e, lane: 0, y: 0 };
      (segs[g] = segs[g] || []).push(s);
      e.segs.push(s);
    }
    edges.forEach(function (e) {
      if (e.span === 0) addSeg(e, e.up.row + 1, e.pu.x, e.pl.x);
      else if (e.span === 1) addSeg(e, e.lo.row, e.pu.x, e.pl.x);
      else {
        addSeg(e, e.up.row + 1, e.pu.x, e.bus);
        addSeg(e, e.lo.row, e.bus, e.pl.x);
      }
    });
    Object.keys(segs).forEach(function (key) {
      var g = Number(key);
      var list = segs[g].slice().sort(function (p, q) {
        return p.x1 - q.x1 || p.x2 - q.x2 || p.owner.index - q.owner.index;
      });
      var ends = [];
      list.forEach(function (s) {
        var l = 0;
        while (l < ends.length && ends[l] > s.x1 - 4) l++;
        ends[l] = s.x2;
        s.lane = l % LANES;
        var top = rowTop[g - 1] + rowH[g - 1];
        s.y = top + LANE0 + s.lane * LANE_STEP;
      });
    });

    var traces = edges.map(function (e) {
      var pu = e.pu;
      var pl = e.pl;
      var ys = e.segs.map(function (s) { return s.y; });
      var pts;
      if (e.span < 2) {
        pts = [[pu.x, pu.y], [pu.x, ys[0]], [pl.x, ys[0]], [pl.x, pl.y]];
      } else {
        pts = [[pu.x, pu.y], [pu.x, ys[0]], [e.bus, ys[0]], [e.bus, ys[1]], [pl.x, ys[1]], [pl.x, pl.y]];
      }
      if (e.a !== e.up) pts.reverse();
      var fwd = makePath(chamfer(pts));
      var xa = Math.min(pu.x, pl.x) - NW;
      var xb = Math.max(pu.x, pl.x) + NW;
      var stray = 0;
      fwd.pts.forEach(function (p) {
        stray = Math.max(stray, xa - p[0], p[0] - xb);
      });
      return {
        id: e.index, a: e.a.id, b: e.b.id, fwd: fwd,
        rev: makePath(fwd.pts.slice().reverse()), stray: Math.max(0, stray)
      };
    });
    return { chips: chips, byId: byId, traces: traces, bh: BH, bw: BW };
  }

  function traceBetween(traces, a, b) {
    for (var i = 0; i < traces.length; i++) {
      var t = traces[i];
      if ((t.a === a && t.b === b) || (t.a === b && t.b === a)) return t;
    }
    return null;
  }

  function mount(canvas, systems, opts) {
    if (!canvas || canvas.tagName !== "CANVAS") throw new Error("Board.mount: canvas expected");
    if (typeof Motion === "undefined") throw new Error("Board.mount: load the kit motion.js first");
    if (!systems || !systems.length) throw new Error("Board.mount: no systems");
    var g = build(systems);
    var chips = g.chips;
    var byId = g.byId;
    var traces = g.traces;
    var BH = g.bh;
    var pulses = [];
    var handlers = { hover: [], select: [] };
    var hoverId = null;
    var focusId = null;
    var lastHi = null;
    var zoomId = null;
    var cam = null;
    var tween = null;
    var job = null;
    var destroyed = false;
    var timers = [];
    var buttons = {};
    var ro = null;
    var parent = canvas.parentNode;
    var list = document.createElement("ul");

    function emit(ev, arg) {
      handlers[ev].forEach(function (fn) { fn(arg); });
    }
    function highlight() {
      var now = hoverId || focusId || null;
      if (now !== lastHi) {
        lastHi = now;
        emit("hover", now);
      }
    }
    // The whole board, centered in the canvas with even margins.
    function overview() {
      var w = canvas.clientWidth;
      var h = canvas.clientHeight;
      var s = Math.min((w - 2 * MARGIN_CSS) / BW, (h - 2 * MARGIN_CSS) / BH);
      return { s: s, x: (w - BW * s) / 2, y: (h - BH * s) / 2 };
    }
    function zoomView(c) {
      var w = canvas.clientWidth;
      var h = canvas.clientHeight;
      var s = Math.max(overview().s, (Math.min(w, h) * 0.7) / c.w);
      return { s: s, x: w / 2 - (c.x + c.w / 2) * s, y: h / 2 - (c.y + c.h / 2) * s };
    }
    function target() {
      return zoomId ? zoomView(byId[zoomId]) : overview();
    }
    function view() {
      return tween ? cam : target();
    }
    function hitAt(px, py) {
      var v = view();
      if (!v.s) return null;
      var lx = (px - v.x) / v.s;
      var ly = (py - v.y) / v.s;
      var best = null;
      var bestD = Infinity;
      chips.forEach(function (c) {
        var hx = Math.max(c.w / 2, HALF_TARGET / v.s);
        var hy = Math.max(c.h / 2, HALF_TARGET / v.s);
        var dx = lx - (c.x + c.w / 2);
        var dy = ly - (c.y + c.h / 2);
        if (Math.abs(dx) <= hx && Math.abs(dy) <= hy && dx * dx + dy * dy < bestD) {
          best = c;
          bestD = dx * dx + dy * dy;
        }
      });
      return best;
    }

    function advance(dt) {
      pulses = pulses.filter(function (p) {
        p.age += dt;
        if (p.age >= PULSE_MS) {
          p.to.flash = 0;
          return false;
        }
        return true;
      });
      chips.forEach(function (c) {
        if (c.flash >= 0) {
          c.flash += dt;
          if (c.flash >= PULSE_MS) c.flash = -1;
        }
      });
    }
    function alive() {
      return pulses.length > 0 || chips.some(function (c) { return c.flash >= 0; });
    }

    function drawBoard(ctx, t, unit) {
      roundRect(ctx, 0, 0, BW, BH, 14);
      ctx.fillStyle = "rgba(0, 0, 0, 0.14)";
      ctx.fill();
      ctx.strokeStyle = t.silkDim;
      ctx.lineWidth = Math.max(1.2, 1.2 * unit);
      ctx.stroke();
      [[16, 16], [BW - 16, 16], [16, BH - 16], [BW - 16, BH - 16]].forEach(function (p) {
        ctx.beginPath();
        ctx.arc(p[0], p[1], 7, 0, Math.PI * 2);
        ctx.fillStyle = t.mask;
        ctx.fill();
        ctx.strokeStyle = t.copper;
        ctx.lineWidth = Math.max(1.5, 1.5 * unit);
        ctx.stroke();
      });
      [[48, 30], [BW - 48, 30], [BW / 2, BH - 30]].forEach(function (p) {
        ctx.beginPath();
        ctx.arc(p[0], p[1], 4, 0, Math.PI * 2);
        ctx.fillStyle = t.silkDim;
        ctx.fill();
      });
    }

    function paint() {
      var w = canvas.clientWidth;
      var h = canvas.clientHeight;
      if (!w || !h) return;
      var dpr = Math.min(global.devicePixelRatio || 1, 2);
      canvas.width = Math.min(Math.round(w * dpr), CAP_W);
      canvas.height = Math.min(Math.round(h * dpr), CAP_H);
      var ctx = canvas.getContext("2d");
      var v = view();
      var sx = canvas.width / w;
      var sy = canvas.height / h;
      var unit = 1 / v.s;
      var t = tokens();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = t.mask;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.setTransform(sx * v.s, 0, 0, sy * v.s, sx * v.x, sy * v.y);
      var hi = lastHi;
      drawBoard(ctx, t, unit);
      var tw = Math.max(TRACE_W, 2 * unit);
      traces.forEach(function (tr) {
        var on = hi && (tr.a === hi || tr.b === hi);
        strokePts(ctx, tr.fwd.pts, on ? t.signal : t.copper, tw);
      });
      chips.forEach(function (c) {
        c.pins.forEach(function (p) {
          var edgeY = p.side === "top" ? c.y : c.y + c.h;
          var lit = c.flash >= 0 || (hi && hi === c.id);
          strokePts(ctx, [[p.x, edgeY], [p.x, p.y]], lit ? t.signal : t.copper, Math.max(2, 1.6 * unit));
        });
      });
      chips.forEach(function (c) {
        c.pins.forEach(function (p) {
          if (!p.real) return;
          ctx.beginPath();
          ctx.arc(p.x, p.y, Math.max(PAD_R, 2.5 * unit), 0, Math.PI * 2);
          ctx.fillStyle = c.flash >= 0 ? t.signal : t.copper;
          ctx.fill();
        });
      });
      chips.forEach(function (c) {
        drawChip(ctx, c, t, unit, c.id === hi);
      });
      pulses.forEach(function (p) {
        var path = p.forward ? p.trace.fwd : p.trace.rev;
        var d = (path.total * p.age) / PULSE_MS;
        var d0 = Math.max(0, d - Math.min(path.total * 0.4, 120));
        var d1 = Math.max(d0, d - 14);
        var pw = Math.max(TRACE_W + 1, 3 * unit);
        ctx.globalAlpha = 0.45;
        strokePts(ctx, sub(path, d0, d1), t.signal, pw);
        ctx.globalAlpha = 1;
        strokePts(ctx, sub(path, d1, d), t.signal, pw);
      });
      if (hi && byId[hi]) drawLabel(ctx, byId[hi], t, unit);
    }
    // An IC: body, die, pin-1 dot, name, and a reference designator under the chip.
    function drawChip(ctx, c, t, unit, active) {
      roundRect(ctx, c.x, c.y, c.w, c.h, 3);
      ctx.fillStyle = t.maskDeep;
      ctx.fill();
      ctx.strokeStyle = active || c.flash >= 0 ? t.signal : t.copper;
      ctx.lineWidth = active ? Math.max(3, 2.5 * unit) : Math.max(1.5, 1.2 * unit);
      ctx.stroke();
      ctx.fillStyle = "rgba(0, 0, 0, 0.35)";
      ctx.fillRect(c.x + 8, c.y + 18, c.w - 16, c.h - 26);
      ctx.beginPath();
      ctx.arc(c.x + 6, c.y + 8, Math.max(2.2, 2 * unit), 0, Math.PI * 2);
      ctx.fillStyle = t.silk;
      ctx.fill();
      ctx.textAlign = "left";
      ctx.textBaseline = "top";
      ctx.font = FONT_NAME;
      ctx.fillStyle = t.silk;
      ctx.fillText(c.name, c.x + 14, c.y + 3);
      ctx.textAlign = "center";
      ctx.font = FONT_REF;
      ctx.fillStyle = t.silkDim;
      ctx.fillText(c.ref, c.x + c.w / 2, c.y + c.h + PIN + 4);
    }
    function drawLabel(ctx, c, t, unit) {
      ctx.font = FONT_BODY;
      var lines = wrap(ctx, c.line, 260).slice(0, 3);
      ctx.font = FONT_TITLE;
      var wid = ctx.measureText(c.name).width;
      ctx.font = FONT_BODY;
      lines.forEach(function (l) { wid = Math.max(wid, ctx.measureText(l).width); });
      var bw = wid + 20;
      var bh2 = 22 + lines.length * 17 + 8;
      var x = Math.min(Math.max(c.x + c.w / 2 - bw / 2, 4), BW - 4 - bw);
      var y = c.y - 10 - bh2;
      if (y < 4) y = c.y + c.h + 26;
      roundRect(ctx, x, y, bw, bh2, 4);
      ctx.fillStyle = t.maskDeep;
      ctx.fill();
      ctx.strokeStyle = t.signal;
      ctx.lineWidth = Math.max(1, unit);
      ctx.stroke();
      ctx.textAlign = "left";
      ctx.textBaseline = "top";
      ctx.font = FONT_TITLE;
      ctx.fillStyle = t.silk;
      ctx.fillText(c.name, x + 10, y + 7);
      ctx.font = FONT_BODY;
      ctx.fillStyle = t.silkDim;
      lines.forEach(function (l, i) { ctx.fillText(l, x + 10, y + 26 + i * 17); });
    }

    // Frame job from the kit. It stops when idle or hidden, so an idle board costs no frames.
    function wake() {
      if (job || destroyed || document.hidden || Motion.reduced()) return;
      job = Motion.canvas(canvas, function (ctx, dt) {
        advance(dt);
        paint();
        if (!alive()) sleep();
      });
    }
    function sleep() {
      if (job) {
        job.stop();
        job = null;
      }
    }
    // Reduced motion draws each state directly; the kit frame job never starts.
    function touch() {
      if (Motion.reduced()) paint();
      else wake();
    }
    function blink(c) {
      c.flash = 0;
      if (Motion.reduced()) {
        timers.push(setTimeout(function () {
          if (destroyed) return;
          c.flash = -1;
          paint();
        }, PULSE_MS));
      }
      touch();
    }

    function glide() {
      if (tween) tween.cancel();
      var from = view();
      cam = from;
      var tw = Motion.tween(zoomMs(), "firm", function (k) {
        var to = target();
        cam = {
          s: from.s + (to.s - from.s) * k,
          x: from.x + (to.x - from.x) * k,
          y: from.y + (to.y - from.y) * k
        };
        paint();
      });
      tween = tw;
      return tw.done.then(function () {
        if (tween === tw) tween = null;
        return api;
      });
    }

    function select(id) {
      focusId = id;
      if (buttons[id]) buttons[id].focus({ preventScroll: true });
      highlight();
      touch();
      emit("select", id);
    }

    // Hidden, focusable list: Tab moves between systems, Enter selects, Esc zooms out.
    list.className = "board-keys";
    list.setAttribute("aria-label", "Systems");
    list.style.cssText =
      "position:absolute;left:0;top:0;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;" +
      "clip:rect(0 0 0 0);clip-path:inset(50%);border:0;list-style:none;";
    chips.forEach(function (c) {
      var li = document.createElement("li");
      var b = document.createElement("button");
      b.type = "button";
      b.textContent = c.name;
      b.setAttribute("aria-label", c.name + ": " + c.what);
      b.addEventListener("focus", function () {
        focusId = c.id;
        highlight();
        touch();
      });
      b.addEventListener("blur", function () {
        if (focusId === c.id) {
          focusId = null;
          highlight();
          touch();
        }
      });
      b.addEventListener("click", function () { select(c.id); });
      li.appendChild(b);
      list.appendChild(li);
      buttons[c.id] = b;
    });
    list.addEventListener("keydown", function (e) {
      if (e.key === "Escape") api.zoomOut();
    });
    if (parent && getComputedStyle(parent).position === "static") parent.style.position = "relative";
    if (parent) parent.appendChild(list);

    function localPos(e) {
      var r = canvas.getBoundingClientRect();
      return [e.clientX - r.left, e.clientY - r.top];
    }
    function onMove(e) {
      var p = localPos(e);
      var c = hitAt(p[0], p[1]);
      canvas.style.cursor = c ? "pointer" : "";
      var id = c ? c.id : null;
      if (id !== hoverId) {
        hoverId = id;
        highlight();
        touch();
      }
    }
    function onLeave() {
      if (hoverId) {
        hoverId = null;
        highlight();
        touch();
      }
    }
    function onClick(e) {
      var p = localPos(e);
      var c = hitAt(p[0], p[1]);
      if (c) select(c.id);
    }
    function onVis() {
      if (document.hidden) sleep();
      else touch();
    }
    canvas.setAttribute("aria-hidden", "true");
    canvas.style.touchAction = "manipulation";
    canvas.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerleave", onLeave);
    canvas.addEventListener("click", onClick);
    document.addEventListener("visibilitychange", onVis);
    if (global.ResizeObserver) {
      ro = new ResizeObserver(function () { touch(); });
      ro.observe(canvas);
    }

    function traceOf(a, b) {
      return traceBetween(traces, a, b);
    }

    var api = {
      pulse: function (fromId, toId, kind) {
        var t = traceOf(fromId, toId);
        if (!t || !byId[toId]) return false;
        if (Motion.reduced()) {
          blink(byId[toId]);
          return true;
        }
        pulses.push({ trace: t, forward: t.a === fromId, age: 0, kind: kind || "data", to: byId[toId] });
        while (pulses.length > PULSE_CAP) pulses.shift();
        touch();
        return true;
      },
      blink: function (id) {
        if (byId[id]) blink(byId[id]);
        return api;
      },
      focus: function (id) {
        focusId = byId[id] ? id : null;
        if (focusId && buttons[focusId]) buttons[focusId].focus({ preventScroll: true });
        highlight();
        touch();
        return api;
      },
      zoomTo: function (id) {
        if (!byId[id]) return Promise.resolve(null);
        zoomId = id;
        return glide();
      },
      zoomOut: function () {
        zoomId = null;
        return glide();
      },
      chipRect: function (id) {
        var c = byId[id];
        if (!c) return null;
        var v = view();
        return { x: c.x * v.s + v.x, y: c.y * v.s + v.y, w: c.w * v.s, h: c.h * v.s };
      },
      on: function (ev, fn) {
        if (handlers[ev] && typeof fn === "function") handlers[ev].push(fn);
        return api;
      },
      hasTrace: function (a, b) {
        return !!traceOf(a, b);
      },
      layout: function () {
        return chips.map(function (c) { return { id: c.id, x: c.x, y: c.y, w: c.w, h: c.h }; });
      },
      stats: function () {
        return {
          chips: chips.length,
          traces: traces.length,
          live: pulses.length,
          blinking: chips.filter(function (c) { return c.flash >= 0; }).length,
          zoomed: zoomId,
          traceStray: traces.reduce(function (m, t) { return Math.max(m, t.stray); }, 0)
        };
      },
      destroy: function () {
        destroyed = true;
        sleep();
        if (tween) tween.cancel();
        timers.forEach(clearTimeout);
        if (ro) ro.disconnect();
        canvas.removeEventListener("pointermove", onMove);
        canvas.removeEventListener("pointerleave", onLeave);
        canvas.removeEventListener("click", onClick);
        document.removeEventListener("visibilitychange", onVis);
        list.remove();
        pulses = [];
      }
    };
    touch();
    return api;
  }

  global.Board = { mount: mount, DEPS: DEPS, ROWS: ROWS };
})(window);
