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
  // 0 hardware (bottom), 1 kernel, 2 services, 3 apps (top).
  var LAYER = {
    silicon: 0,
    lithium: 1, titan: 1, zeus: 1, midas: 1, kairos: 1,
    mercury: 2, xenon: 2, iridium: 2, chronos: 2, argo: 2, aurum: 2, pandora: 2, krypton: 2,
    logos: 2, elysium: 2, cobalt: 2, hades: 2, sphinx: 2,
    calliope: 3, indium: 3, odyssey: 3, helios: 3, athena: 3, iris: 3
  };

  var COLS = 8;
  var CHIP_W = 120;
  var CHIP_H = 56;
  var PITCH_X = 150;
  var ROW_PITCH = 120;
  var MARGIN = 40;
  var TOP = 60;
  var BOTTOM = 70;
  var BW = MARGIN * 2 + COLS * PITCH_X;
  var LANE = 5;
  var CHAMFER = 8;
  var TRACE_W = 3;
  var PAD_R = 3.5;
  var STUB = 16;
  var PULSE_MS = 400;
  var PULSE_CAP = 24;
  var HALF_TARGET = 22;
  var CAP_W = 1280;
  var CAP_H = 720;
  // Vertical bus x in the gaps between columns, and at both outer edges. Jitter spreads shared buses.
  var BUS = [20, 190, 340, 490, 640, 790, 940, 1090, 1260];
  var FALLBACK = { mask: "#0B3D2E", maskDeep: "#072A20", copper: "#C8803C", signal: "#FFC46B", silk: "#EEF0E6", silkDim: "#9DB5A8" };
  var TOKEN = { mask: "--mask", maskDeep: "--mask-deep", copper: "--copper", signal: "--signal", silk: "--silk", silkDim: "--silk-dim" };
  var FONT_NAME = '600 15px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_WHAT = '400 11px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_TITLE = '600 14px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';
  var FONT_BODY = '400 13px Bahnschrift, "DIN Alternate", "Arial Narrow", sans-serif';

  function dist(a, b) {
    return Math.hypot(b[0] - a[0], b[1] - a[1]);
  }
  function towards(from, to, k) {
    var d = dist(from, to) || 1;
    return [from[0] + ((to[0] - from[0]) * k) / d, from[1] + ((to[1] - from[1]) * k) / d];
  }
  // Orthogonal polyline with 45-degree chamfers at each corner, then cumulative lengths.
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

  // Chips, rows, pads and traces. Rows are layers from the top (apps) down to hardware; a layer wider than
  // COLS wraps. Chips sit on a fixed column grid, so a vertical bus in a column gap clears every row.
  function build(systems) {
    var chips = [];
    var byId = {};
    systems.forEach(function (s, i) {
      var layer = typeof s.layer === "number" ? s.layer : LAYER[s.id];
      var c = {
        id: s.id, name: s.name, what: s.what || "", line: s.line || "", index: i,
        layer: typeof layer === "number" ? layer : 2,
        depIds: [], pads: [], x: 0, y: 0, row: 0, col: 0, flash: -1
      };
      chips.push(c);
      byId[s.id] = c;
    });
    chips.forEach(function (c) {
      var raw = Array.isArray(systems[c.index].deps) ? systems[c.index].deps : DEPS[c.id] || [];
      raw.forEach(function (id) {
        if (byId[id] && id !== c.id && c.depIds.indexOf(id) < 0) c.depIds.push(id);
      });
    });

    var rowsByLayer = [[], [], [], []];
    for (var L = 0; L < 4; L++) {
      var keyed = chips.filter(function (c) { return c.layer === L; }).map(function (c) {
        var below = c.depIds.map(function (id) { return byId[id]; }).filter(function (d) { return d.layer < L; });
        var key = below.length
          ? below.reduce(function (sum, d) { return sum + d.col; }, 0) / below.length
          : 1000 + c.index;
        return { c: c, key: key };
      });
      keyed.sort(function (p, q) { return p.key - q.key || p.c.index - q.c.index; });
      for (var s0 = 0; s0 < keyed.length; s0 += COLS) {
        var chunk = keyed.slice(s0, s0 + COLS).map(function (k) { return k.c; });
        var start = Math.floor((COLS - chunk.length) / 2);
        chunk.forEach(function (c, i) { c.col = start + i; });
        rowsByLayer[L].push(chunk);
      }
    }
    var rows = [].concat(rowsByLayer[3], rowsByLayer[2], rowsByLayer[1], rowsByLayer[0]);
    rows.forEach(function (chunk, r) {
      chunk.forEach(function (c) {
        c.row = r;
        c.x = MARGIN + PITCH_X / 2 + c.col * PITCH_X - CHIP_W / 2;
        c.y = TOP + r * ROW_PITCH;
      });
    });
    var bh = TOP + (rows.length - 1) * ROW_PITCH + CHIP_H + BOTTOM;

    var edges = [];
    chips.forEach(function (a) {
      a.depIds.forEach(function (id) {
        var b = byId[id];
        var e = { index: edges.length, a: a, b: b };
        if (a.row === b.row) {
          e.sa = "bot"; e.sb = "bot"; e.ga = a.row + 1; e.gb = e.ga;
        } else if (b.row > a.row) {
          e.sa = "bot"; e.sb = "top"; e.ga = a.row + 1; e.gb = b.row;
        } else {
          e.sa = "top"; e.sb = "bot"; e.ga = a.row; e.gb = b.row + 1;
        }
        e.pa = { chip: a, side: e.sa, other: b, edge: e, x: 0, y: 0 };
        e.pb = { chip: b, side: e.sb, other: a, edge: e, x: 0, y: 0 };
        a.pads.push(e.pa);
        b.pads.push(e.pb);
        edges.push(e);
      });
    });
    chips.forEach(function (c) {
      ["top", "bot"].forEach(function (side) {
        var list = c.pads.filter(function (p) { return p.side === side; });
        list.sort(function (p, q) { return p.other.x - q.other.x || p.edge.index - q.edge.index; });
        list.forEach(function (p, i) {
          p.x = c.x + (CHIP_W * (i + 1)) / (list.length + 1);
          p.y = side === "top" ? c.y : c.y + CHIP_H;
        });
      });
    });
    // Chips with no edges get a short stub from the bottom edge so every chip has a pad.
    chips.forEach(function (c) {
      if (c.pads.length === 0) {
        var pad = { chip: c, side: "bot", other: null, edge: null, x: c.x + CHIP_W / 2, y: c.y + CHIP_H };
        c.pads.push(pad);
        c.stub = [[pad.x, pad.y], [pad.x, pad.y + STUB]];
      }
    });

    var lanes = {};
    function lane(g) {
      var n = lanes[g] || 0;
      lanes[g] = n + 1;
      var top = g === 0 ? 0 : TOP + (g - 1) * ROW_PITCH + CHIP_H;
      return top + 6 + (n % 10) * LANE;
    }
    var traces = edges.map(function (e) {
      var pa = e.pa;
      var pb = e.pb;
      var pts;
      if (e.a.row === e.b.row) {
        var y = lane(e.ga);
        pts = [[pa.x, pa.y], [pa.x, y], [pb.x, y], [pb.x, pb.y]];
      } else {
        var ya = lane(e.ga);
        var yb = lane(e.gb);
        var bx = BUS[e.index % BUS.length] + ((Math.floor(e.index / BUS.length) % 3) - 1) * 4;
        pts = [[pa.x, pa.y], [pa.x, ya], [bx, ya], [bx, yb], [pb.x, yb], [pb.x, pb.y]];
      }
      var fwd = makePath(chamfer(pts));
      return { id: e.index, a: e.a.id, b: e.b.id, fwd: fwd, rev: makePath(fwd.pts.slice().reverse()) };
    });
    return { chips: chips, byId: byId, traces: traces, bh: bh };
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
    opts = opts || {};
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
    function overview() {
      var w = canvas.clientWidth;
      var h = canvas.clientHeight;
      var s = Math.min(w / BW, h / BH) * 0.96;
      return { s: s, x: (w - BW * s) / 2, y: (h - BH * s) / 2 };
    }
    function zoomView(c) {
      var w = canvas.clientWidth;
      var h = canvas.clientHeight;
      var s = Math.max(overview().s, (Math.min(w, h) * 0.7) / CHIP_W);
      return { s: s, x: w / 2 - (c.x + CHIP_W / 2) * s, y: h / 2 - (c.y + CHIP_H / 2) * s };
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
      var hx = Math.max(CHIP_W / 2, HALF_TARGET / v.s);
      var hy = Math.max(CHIP_H / 2, HALF_TARGET / v.s);
      var best = null;
      var bestD = Infinity;
      chips.forEach(function (c) {
        var dx = lx - (c.x + CHIP_W / 2);
        var dy = ly - (c.y + CHIP_H / 2);
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
      var tw = Math.max(TRACE_W, 2 * unit);
      traces.forEach(function (tr) {
        var on = hi && (tr.a === hi || tr.b === hi);
        strokePts(ctx, tr.fwd.pts, on ? t.signal : t.copper, tw);
      });
      chips.forEach(function (c) {
        if (c.stub) strokePts(ctx, c.stub, t.copper, tw);
      });
      chips.forEach(function (c) {
        c.pads.forEach(function (p) {
          ctx.beginPath();
          ctx.arc(p.x, p.y, Math.max(PAD_R, 2.5 * unit), 0, Math.PI * 2);
          ctx.fillStyle = c.flash >= 0 ? t.signal : t.copper;
          ctx.fill();
        });
      });
      chips.forEach(function (c) {
        var active = c.id === hi;
        roundRect(ctx, c.x, c.y, CHIP_W, CHIP_H, 3);
        ctx.fillStyle = t.maskDeep;
        ctx.fill();
        ctx.strokeStyle = active || c.flash >= 0 ? t.signal : t.copper;
        ctx.lineWidth = active ? Math.max(3, 2.5 * unit) : Math.max(1.5, 1.2 * unit);
        ctx.stroke();
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.font = FONT_NAME;
        ctx.fillStyle = t.silk;
        ctx.fillText(c.name, c.x + CHIP_W / 2, c.y + 22);
        ctx.font = FONT_WHAT;
        ctx.fillStyle = t.silkDim;
        ctx.fillText(c.what, c.x + CHIP_W / 2, c.y + 40);
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
    function drawLabel(ctx, c, t, unit) {
      ctx.font = FONT_BODY;
      var lines = wrap(ctx, c.line, 260).slice(0, 3);
      ctx.font = FONT_TITLE;
      var wid = ctx.measureText(c.name).width;
      ctx.font = FONT_BODY;
      lines.forEach(function (l) { wid = Math.max(wid, ctx.measureText(l).width); });
      var bw = wid + 20;
      var bh2 = 22 + lines.length * 17 + 8;
      var x = Math.min(Math.max(c.x + CHIP_W / 2 - bw / 2, 4), BW - 4 - bw);
      var y = c.y - 10 - bh2;
      if (y < 4) y = c.y + CHIP_H + 10;
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
        return { x: c.x * v.s + v.x, y: c.y * v.s + v.y, w: CHIP_W * v.s, h: CHIP_H * v.s };
      },
      on: function (ev, fn) {
        if (handlers[ev] && typeof fn === "function") handlers[ev].push(fn);
        return api;
      },
      hasTrace: function (a, b) {
        return !!traceOf(a, b);
      },
      layout: function () {
        return chips.map(function (c) { return { id: c.id, x: c.x, y: c.y, w: CHIP_W, h: CHIP_H }; });
      },
      stats: function () {
        return {
          chips: chips.length,
          traces: traces.length,
          live: pulses.length,
          blinking: chips.filter(function (c) { return c.flash >= 0; }).length,
          zoomed: zoomId
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

  global.Board = { mount: mount, DEPS: DEPS, LAYER: LAYER };
})(window);
