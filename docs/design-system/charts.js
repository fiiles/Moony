/* Minimal chart helpers for the style guide. They encode the chart rules of the design system
   (recessive grid, right-hand ticks, area under a single value line, 2 px lines, hover crosshair + tooltip,
   rounded bar ends, 2 px gaps). Production uses Recharts with the same rules. */
(function () {
  var tipEl;
  window.showTip = function (cx, cy, html) {
    if (!tipEl) { tipEl = document.createElement("div"); tipEl.className = "tip"; document.body.appendChild(tipEl); }
    tipEl.innerHTML = html; tipEl.style.display = "block";
    var w = tipEl.offsetWidth;
    tipEl.style.left = Math.min(window.innerWidth - w - 12, Math.max(8, cx - w / 2)) + window.scrollX + "px";
    tipEl.style.top = (cy - tipEl.offsetHeight - 12 + window.scrollY) + "px";
  };
  window.hideTip = function () { if (tipEl) tipEl.style.display = "none"; };

  function niceStep(span, n) {
    var raw = span / n, mag = Math.pow(10, Math.floor(Math.log10(raw)));
    return [1, 2, 2.5, 5, 10].map(function (m) { return m * mag; }).find(function (v) { return v >= raw; });
  }
  window.compactKc = function (v) { return v >= 1e6 ? (v / 1e6).toFixed(2).replace(/\.?0+$/, "").replace(".", ",") + " mil." : Math.round(v / 1000) + " tis."; };

  /* points: [{date: Date, value: Number}], opts: {axisEl, height, baselineZero, events: [{date, type:'buy'|'sell'|'div'|'in'|'out', html}], tipHtml(p)} */
  window.drawLine = function (el, points, opts) {
    opts = opts || {};
    var W = el.clientWidth, H = opts.height || el.clientHeight || 160, padR = 54, padT = 14, padB = 6;
    var lo = Infinity, hi = -Infinity;
    points.forEach(function (p) { lo = Math.min(lo, p.value); hi = Math.max(hi, p.value); });
    (opts.cost || []).forEach(function (c) { lo = Math.min(lo, c.value); hi = Math.max(hi, c.value); });
    if (opts.baselineZero) lo = 0;
    if (opts.hline) { lo = Math.min(lo, opts.hline.value); hi = Math.max(hi, opts.hline.value); }
    var span = hi - lo || 1; lo = Math.max(0, lo - span * 0.15); hi = hi + span * 0.08;
    var t0 = points[0].date.getTime(), t1 = points[points.length - 1].date.getTime();
    var x = function (d) { return (d.getTime() - t0) / (t1 - t0) * (W - padR); };
    var y = function (v) { return padT + (1 - (v - lo) / (hi - lo)) * (H - padT - padB); };
    /* opts.splitAt (Date): points after it are drawn dashed (projected future), the area and the end ring stop at the last actual point.
       opts.cost: [{date, value}] dashed step line (cost basis / "Vloženo"). opts.labels / opts.lastLabel override the time axis. */
    var line = "", dash = "", split = opts.splitAt ? opts.splitAt.getTime() : Infinity, lastSolid = null;
    points.forEach(function (p, i) {
      var seg = x(p.date).toFixed(1) + " " + y(p.value).toFixed(1);
      if (p.date.getTime() <= split) { line += (i ? "L" : "M") + seg; lastSolid = p; }
      else dash += dash ? "L" + seg : "M" + (lastSolid ? x(lastSolid.date).toFixed(1) + " " + y(lastSolid.value).toFixed(1) + "L" : "") + seg;
    });
    var area = line ? line + "V" + (H - padB) + "H0Z" : "";
    var cost = ""; (opts.cost || []).forEach(function (c, i, arr) { var cx = x(c.date).toFixed(1), cy = y(c.value).toFixed(1); cost += i ? "H" + cx + "V" + cy : "M" + cx + " " + cy; if (i === arr.length - 1) cost += "H" + x(points[points.length - 1].date).toFixed(1); });
    var grid = "", labs = "", step = niceStep(hi - lo, 4);
    for (var gv = Math.ceil(lo / step) * step; gv <= hi; gv += step) {
      var gy = y(gv).toFixed(1); grid += "M0 " + gy + "H" + (W - padR + 6);
      labs += '<text x="' + (W - padR + 10) + '" y="' + (+gy + 3) + '" fill="var(--chart-axis)" font-size="10" font-weight="500">' + (opts.yFmt ? opts.yFmt(gv) : compactKc(gv)) + "</text>";
    }
    var valueAt = function (d) { var best = points[0], bd = Infinity; points.forEach(function (p) { var dd = Math.abs(p.date - d); if (dd < bd) { bd = dd; best = p; } }); return best.value; };
    var marks = (opts.events || []).map(function (e, i) {
      var ex = x(e.date), ey = y(valueAt(e.date));
      if (e.type === "div" || e.type === "in") return '<circle class="ev ev--div" data-i="' + i + '" cx="' + ex + '" cy="' + ey + '" r="4.5" fill="#fff" stroke="var(--gain)" stroke-width="2"/>';
      if (e.type === "out") return '<circle class="ev ev--out" data-i="' + i + '" cx="' + ex + '" cy="' + ey + '" r="4.5" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/>';
      var s = 6, up = e.type === "buy";
      var pts = up ? (ex + "," + (ey - s) + " " + (ex + s) + "," + (ey + s - 1) + " " + (ex - s) + "," + (ey + s - 1)) : (ex + "," + (ey + s) + " " + (ex + s) + "," + (ey - s + 1) + " " + (ex - s) + "," + (ey - s + 1));
      return '<polygon class="ev" data-i="' + i + '" points="' + pts + '" fill="' + (up ? "var(--chart-line)" : "#fff") + '" stroke="' + (up ? "#fff" : "var(--chart-line)") + '" stroke-width="2"/>';
    }).join("");
    var last = opts.splitAt && lastSolid ? lastSolid : points[points.length - 1], id = "g" + Math.random().toString(36).slice(2, 7);
    el.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '" style="overflow:visible">' +
      '<defs><linearGradient id="' + id + '" x1="0" y1="0" x2="0" y2="1"><stop stop-color="var(--chart-area)" stop-opacity=".8"/><stop offset="1" stop-color="#fbfaf8" stop-opacity="0"/></linearGradient></defs>' +
      '<path d="' + grid + '" stroke="var(--chart-grid)" fill="none"/>' + labs +
      '<path d="' + area + '" fill="url(#' + id + ')"/>' +
      '<path d="' + line + '" fill="none" stroke="var(--chart-line)" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"/>' +
      (dash ? '<path d="' + dash + '" fill="none" stroke="var(--chart-line)" stroke-width="2" stroke-dasharray="4 4" stroke-linecap="round" stroke-linejoin="round"/>' : "") +
      (cost ? '<path d="' + cost + '" fill="none" stroke="var(--chart-cost)" stroke-width="1.5" stroke-dasharray="3 4" stroke-linejoin="round"/>' : "") +
      (opts.hline ? '<path d="M0 ' + y(opts.hline.value).toFixed(1) + "H" + (W - padR) + '" stroke="var(--gain)" stroke-width="1.5" stroke-dasharray="4 4"/><text x="' + (W - padR - 4) + '" y="' + (y(opts.hline.value) - 5).toFixed(1) + '" text-anchor="end" fill="var(--gain)" font-size="10" font-weight="650">' + opts.hline.label + "</text>" : "") +
      '<circle cx="' + x(last.date) + '" cy="' + y(last.value) + '" r="4.5" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/>' +
      '<g class="hover"></g>' + marks +
      '<rect class="hit" x="0" y="0" width="' + (W - padR) + '" height="' + H + '" fill="transparent"/></svg>';
    if (opts.axisEl) {
      opts.axisEl.style.paddingRight = padR + "px";
      var n = opts.ticks || 5, out = opts.labels || [];
      for (var i = 0; !opts.labels && i < n; i++) {
        var dd = new Date(t0 + (t1 - t0) * i / (n - 1));
        out.push(i === n - 1 ? (opts.lastLabel || "Dnes") : (t1 - t0 < 100 * 864e5 ? dd.getDate() + ". " + (dd.getMonth() + 1) + "." : CZ_MONTHS[dd.getMonth()] + " " + String(dd.getFullYear()).slice(2)));
      }
      opts.axisEl.innerHTML = out.map(function (l) { return "<span>" + l + "</span>"; }).join("");
    }
    var hit = el.querySelector(".hit"), hover = el.querySelector(".hover");
    hit.addEventListener("mousemove", function (ev) {
      var r = el.getBoundingClientRect(), mx = ev.clientX - r.left, best = points[0], bd = Infinity;
      points.forEach(function (p) { var dx = Math.abs(x(p.date) - mx); if (dx < bd) { bd = dx; best = p; } });
      var px = x(best.date), py = y(best.value);
      hover.innerHTML = '<path d="M' + px + " " + padT + "V" + (H - padB) + '" stroke="var(--ink-4)" stroke-dasharray="2 3"/><circle cx="' + px + '" cy="' + py + '" r="4" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/>';
      showTip(ev.clientX, r.top + py, opts.tipHtml ? opts.tipHtml(best) : "<b>" + fmtKc(best.value) + "</b><small>" + fmtDay(best.date) + "</small>");
    });
    hit.addEventListener("mouseleave", function () { hover.innerHTML = ""; hideTip(); });
    el.querySelectorAll(".ev").forEach(function (m) {
      m.addEventListener("mouseenter", function () { var e = opts.events[+m.dataset.i], r = m.getBoundingClientRect(); showTip(r.left + r.width / 2, r.top, e.html); });
      m.addEventListener("mouseleave", hideTip);
    });
  };

  /* bars: [{label, a, b}] two series (a = dark, b = light). opts: {axisEl, height, names:[..], net:true} */
  window.drawBars = function (el, bars, opts) {
    opts = opts || {};
    var W = el.clientWidth, H = opts.height || el.clientHeight || 180, padR = 54, padT = 14, padB = 6;
    var hi = 0; bars.forEach(function (b) { hi = Math.max(hi, b.a, b.b); }); hi *= 1.08;
    var y = function (v) { return padT + (1 - v / hi) * (H - padT - padB); };
    var slot = (W - padR) / bars.length, bw = Math.min(22, slot * 0.3), gap = 2;
    var grid = "", labs = "", step = niceStep(hi, 4);
    for (var gv = step; gv <= hi; gv += step) { var gy = y(gv).toFixed(1); grid += "M0 " + gy + "H" + (W - padR + 6); labs += '<text x="' + (W - padR + 10) + '" y="' + (+gy + 3) + '" fill="var(--chart-axis)" font-size="10" font-weight="500">' + compactKc(gv) + "</text>"; }
    var rects = bars.map(function (b, i) {
      var cx = slot * i + slot / 2, base = y(0);
      function bar(v, fill, dx, cls) { var top = y(v); return '<rect class="bar ' + cls + '" data-i="' + i + '" x="' + (cx + dx).toFixed(1) + '" y="' + top.toFixed(1) + '" width="' + bw + '" height="' + Math.max(0, base - top).toFixed(1) + '" rx="3" fill="' + fill + '"/>'; }
      return bar(b.a, "var(--s1)", -bw - gap / 2, "a") + bar(b.b, "var(--s3)", gap / 2, "b");
    }).join("");
    el.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '" style="overflow:visible"><path d="' + grid + '" stroke="var(--chart-grid)" fill="none"/>' + labs + '<path d="M0 ' + y(0) + 'H' + (W - padR + 6) + '" stroke="var(--line)"/>' + rects + "</svg>";
    if (opts.axisEl) { opts.axisEl.style.paddingRight = padR + "px"; } if (opts.axisEl) opts.axisEl.innerHTML = bars.map(function (b) { return "<span>" + b.label + "</span>"; }).join("");
    el.querySelectorAll(".bar").forEach(function (r) {
      r.addEventListener("mouseenter", function () { var b = bars[+r.dataset.i], rr = r.getBoundingClientRect(); showTip(rr.left + rr.width / 2, rr.top, "<b>" + b.label + "</b><small>" + (opts.names ? opts.names[0] : "A") + " " + fmtKc(b.a) + "</small><small>" + (opts.names ? opts.names[1] : "B") + " " + fmtKc(b.b) + "</small><small>bilance " + fmtSigned(b.a - b.b) + "</small>"); });
      r.addEventListener("mouseleave", hideTip);
    });
  };

  /* ring: el gets a conic gradient from segments [{label, pct, color}] */
  window.drawRing = function (el, segs) {
    var acc = 0, stops = segs.map(function (s) { var from = acc; acc += s.pct; return s.color + " " + from + "% " + acc + "%"; });
    el.style.background = "conic-gradient(" + stops.join(", ") + ")";
  };
})();

/* ===== Additions for the Tools pages ===== */
(function () {
  function niceStep(span, n) { var raw = span / n, mag = Math.pow(10, Math.floor(Math.log10(raw))); return [1, 2, 2.5, 5, 10].map(function (m) { return m * mag; }).find(function (v) { return v >= raw; }); }
  function yTicks(W, padR, lo, hi, y) {
    var grid = "", labs = "", step = niceStep(hi - lo, 4);
    for (var gv = Math.ceil(lo / step) * step; gv <= hi; gv += step) { var gy = y(gv).toFixed(1); grid += "M0 " + gy + "H" + (W - padR + 6); labs += '<text x="' + (W - padR + 10) + '" y="' + (+gy + 3) + '" fill="var(--chart-axis)" font-size="10" font-weight="500">' + compactKc(gv) + "</text>"; }
    return '<path d="' + grid + '" stroke="var(--chart-grid)" fill="none"/>' + labs;
  }
  function axis(el, labels, padR) { if (!el) return; el.style.paddingRight = padR + "px"; el.innerHTML = labels.map(function (l) { return "<span>" + l + "</span>"; }).join(""); }

  /* Band: points [{date, value, lo, hi, base}]; value = expected line, lo/hi = scenario band, base = dashed reference (e.g. contributions only).
     opts: {axisEl, height, events:[{date, html, type:'milestone'}], labels:[..]} */
  window.drawBand = function (el, points, opts) {
    opts = opts || {};
    var W = el.clientWidth, H = opts.height || el.clientHeight || 200, padR = 58, padT = 16, padB = 6;
    var lo = Infinity, hi = -Infinity;
    points.forEach(function (p) { lo = Math.min(lo, p.lo != null ? p.lo : p.value, p.base != null ? p.base : p.value); hi = Math.max(hi, p.hi != null ? p.hi : p.value); });
    lo = 0; hi = hi * 1.06;
    var n = points.length, x = function (i) { return i / (n - 1) * (W - padR); }, y = function (v) { return padT + (1 - (v - lo) / (hi - lo)) * (H - padT - padB); };
    var mid = "", band = "", back = "", base = "";
    points.forEach(function (p, i) { mid += (i ? "L" : "M") + x(i).toFixed(1) + " " + y(p.value).toFixed(1); band += (i ? "L" : "M") + x(i).toFixed(1) + " " + y(p.hi).toFixed(1); base += (i ? "L" : "M") + x(i).toFixed(1) + " " + y(p.base).toFixed(1); });
    for (var i = n - 1; i >= 0; i--) back += "L" + x(i).toFixed(1) + " " + y(points[i].lo).toFixed(1);
    var marks = (opts.events || []).map(function (e, k) { var ex = x(e.index), ey = y(points[e.index].value); return '<g class="ev" data-i="' + k + '"><path d="M' + ex + " " + (ey + 8) + "V" + (H - padB) + '" stroke="var(--ink-4)" stroke-dasharray="2 3"/><circle cx="' + ex + '" cy="' + ey + '" r="4.5" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/></g>'; }).join("");
    el.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '" style="overflow:visible">' + yTicks(W, padR, lo, hi, y) +
      '<path d="' + band + back + 'Z" fill="var(--s4)" fill-opacity=".38"/>' +
      '<path d="' + base + '" fill="none" stroke="var(--chart-cost)" stroke-width="1.5" stroke-dasharray="3 4"/>' +
      '<path d="' + mid + '" fill="none" stroke="var(--chart-line)" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"/>' +
      '<circle cx="' + x(n - 1) + '" cy="' + y(points[n - 1].value) + '" r="4.5" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/><g class="hover"></g>' + marks +
      '<rect class="hit" x="0" y="0" width="' + (W - padR) + '" height="' + H + '" fill="transparent"/></svg>';
    axis(opts.axisEl, opts.labels || [], padR);
    var hit = el.querySelector(".hit"), hover = el.querySelector(".hover");
    hit.addEventListener("mousemove", function (ev) { var r = el.getBoundingClientRect(), mx = ev.clientX - r.left, i = Math.max(0, Math.min(n - 1, Math.round(mx / (W - padR) * (n - 1)))), p = points[i], px = x(i), py = y(p.value); hover.innerHTML = '<path d="M' + px + " " + padT + "V" + (H - padB) + '" stroke="var(--ink-4)" stroke-dasharray="2 3"/><circle cx="' + px + '" cy="' + py + '" r="4" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/>'; showTip(ev.clientX, r.top + py, opts.tipHtml ? opts.tipHtml(p, i) : "<b>" + fmtKc(p.value) + "</b>"); });
    hit.addEventListener("mouseleave", function () { hover.innerHTML = ""; hideTip(); });
    el.querySelectorAll(".ev").forEach(function (m) { m.addEventListener("mouseenter", function () { var e = opts.events[+m.dataset.i], r = m.querySelector("circle").getBoundingClientRect(); showTip(r.left + r.width / 2, r.top, e.html); }); m.addEventListener("mouseleave", hideTip); });
  };

  /* Lines: series [{name, points:[{date, value}]}] on the same x grid (same length), direct end labels, fixed s1..s4 order. opts: {axisEl, height, labels, fmt} */
  window.drawLines = function (el, series, opts) {
    opts = opts || {};
    var W = el.clientWidth, H = opts.height || el.clientHeight || 180, padR = 96, padT = 14, padB = 6, fmt = opts.fmt || function (v) { return v.toFixed(0); };
    var lo = Infinity, hi = -Infinity; series.forEach(function (s) { s.points.forEach(function (p) { lo = Math.min(lo, p.value); hi = Math.max(hi, p.value); }); });
    var span = hi - lo || 1; lo -= span * 0.12; hi += span * 0.08;
    var n = series[0].points.length, x = function (i) { return i / (n - 1) * (W - padR); }, y = function (v) { return padT + (1 - (v - lo) / (hi - lo)) * (H - padT - padB); };
    var colors = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s4)"];
    var grid = "", step = niceStep(hi - lo, 4), labs = "";
    for (var gv = Math.ceil(lo / step) * step; gv <= hi; gv += step) { var gy = y(gv).toFixed(1); grid += "M0 " + gy + "H" + (W - padR + 6); labs += '<text x="' + (W - padR + 10) + '" y="' + (+gy + 3) + '" fill="var(--chart-axis)" font-size="10" font-weight="500">' + fmt(gv) + "</text>"; }
    var solid = 0, colorOf = series.map(function (s) { return s.dashed ? "var(--chart-cost)" : colors[solid++]; });
    var labelY = series.map(function (s) { return y(s.points[n - 1].value); });
    var order = series.map(function (_, k) { return k; }).sort(function (a, b) { return labelY[a] - labelY[b]; });
    for (var o = 1; o < order.length; o++) { var prev = order[o - 1], cur = order[o]; if (labelY[cur] - labelY[prev] < 13) labelY[cur] = labelY[prev] + 13; }
    var paths = series.map(function (s, k) { var d = ""; s.points.forEach(function (p, i) { d += (i ? "L" : "M") + x(i).toFixed(1) + " " + y(p.value).toFixed(1); }); var last = s.points[n - 1], col = colorOf[k]; return '<path d="' + d + '" fill="none" stroke="' + col + '" stroke-width="' + (s.dashed ? 1.5 : k === 0 ? 2.25 : 2) + '"' + (s.dashed ? ' stroke-dasharray="3 4"' : "") + ' stroke-linecap="round" stroke-linejoin="round"/><circle cx="' + x(n - 1) + '" cy="' + y(last.value) + '" r="3.5" fill="#fff" stroke="' + col + '" stroke-width="2"/><text x="' + (x(n - 1) + 9) + '" y="' + (labelY[k] + 3.5) + '" fill="var(--ink-2)" font-size="10" font-weight="650">' + s.name + " " + fmt(last.value) + "</text>"; }).join("");
    el.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '" style="overflow:visible"><path d="' + grid + '" stroke="var(--chart-grid)" fill="none"/>' + paths + '<g class="hover"></g><rect class="hit" x="0" y="0" width="' + (W - padR) + '" height="' + H + '" fill="transparent"/></svg>';
    axis(opts.axisEl, opts.labels || [], padR);
    var hit = el.querySelector(".hit"), hover = el.querySelector(".hover");
    hit.addEventListener("mousemove", function (ev) { var r = el.getBoundingClientRect(), mx = ev.clientX - r.left, i = Math.max(0, Math.min(n - 1, Math.round(mx / (W - padR) * (n - 1)))), px = x(i); hover.innerHTML = '<path d="M' + px + " " + padT + "V" + (H - padB) + '" stroke="var(--ink-4)" stroke-dasharray="2 3"/>' + series.map(function (s, k) { return '<circle cx="' + px + '" cy="' + y(s.points[i].value) + '" r="3.5" fill="#fff" stroke="' + colorOf[k] + '" stroke-width="2"/>'; }).join(""); showTip(ev.clientX, r.top + padT, "<b>" + (opts.labels && opts.tipLabel ? opts.tipLabel(i) : "") + "</b>" + series.map(function (s) { return "<small>" + s.name + " " + fmt(s.points[i].value) + "</small>"; }).join("")); });
    hit.addEventListener("mouseleave", function () { hover.innerHTML = ""; hideTip(); });
  };

  /* Stacked bars: bars [{label, a, b}] a at the bottom (s1), b on top (s3), 2px gap. opts: {axisEl, height, names, marks:[{i, html}]} */
  window.drawStacked = function (el, bars, opts) {
    opts = opts || {};
    var W = el.clientWidth, H = opts.height || el.clientHeight || 180, padR = 54, padT = 16, padB = 6;
    var hi = 0; bars.forEach(function (b) { hi = Math.max(hi, b.a + b.b); }); hi *= 1.08;
    var y = function (v) { return padT + (1 - v / hi) * (H - padT - padB); }, slot = (W - padR) / bars.length, bw = Math.min(26, slot * 0.62);
    var rects = bars.map(function (b, i) { var cx = slot * i + slot / 2 - bw / 2, ya = y(b.a), yb = y(b.a + b.b); return '<rect class="bar" data-i="' + i + '" x="' + cx.toFixed(1) + '" y="' + ya.toFixed(1) + '" width="' + bw + '" height="' + Math.max(0, y(0) - ya).toFixed(1) + '" rx="2" fill="var(--s1)"/><rect class="bar" data-i="' + i + '" x="' + cx.toFixed(1) + '" y="' + yb.toFixed(1) + '" width="' + bw + '" height="' + Math.max(0, ya - yb - 2).toFixed(1) + '" rx="2" fill="var(--s3)"/>'; }).join("");
    var marks = (opts.marks || []).map(function (m, k) { var cx = slot * m.i + slot / 2, top = y(bars[m.i].a + bars[m.i].b); return '<g class="ev" data-i="' + k + '"><circle cx="' + cx + '" cy="' + (top - 10) + '" r="4.5" fill="#fff" stroke="var(--chart-line)" stroke-width="2"/></g>'; }).join("");
    el.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + " " + H + '" style="overflow:visible">' + yTicks(W, padR, 0, hi, y) + '<path d="M0 ' + y(0) + "H" + (W - padR + 6) + '" stroke="var(--line)"/>' + rects + marks + "</svg>";
    axis(opts.axisEl, bars.map(function (b) { return b.label; }), padR);
    el.querySelectorAll(".bar").forEach(function (r) { r.addEventListener("mouseenter", function () { var b = bars[+r.dataset.i], rr = r.getBoundingClientRect(); showTip(rr.left + rr.width / 2, rr.top, "<b>" + b.label + "</b><small>" + opts.names[0] + " " + fmtKc(b.a) + "</small><small>" + opts.names[1] + " " + fmtKc(b.b) + "</small>"); }); r.addEventListener("mouseleave", hideTip); });
    el.querySelectorAll(".ev").forEach(function (m) { m.addEventListener("mouseenter", function () { var e = opts.marks[+m.dataset.i], r = m.getBoundingClientRect(); showTip(r.left + r.width / 2, r.top, e.html); }); m.addEventListener("mouseleave", hideTip); });
  };

  /* Piecewise-linear path through anchors [[Date, value], ...] (demo data only). */
  window.pathAt = function (anchors, d) {
    var t = d.getTime(); if (t <= anchors[0][0].getTime()) return anchors[0][1];
    for (var i = 1; i < anchors.length; i++) { var a = anchors[i - 1], b = anchors[i]; if (t <= b[0].getTime()) { var f = (t - a[0].getTime()) / (b[0].getTime() - a[0].getTime()); return a[1] + (b[1] - a[1]) * f; } }
    return anchors[anchors.length - 1][1];
  };

  /* Inline 52-week range bar: returns HTML. */
  window.rangeBar = function (lo, hi, cur, target) {
    var p = Math.max(0, Math.min(100, (cur - lo) / (hi - lo) * 100)), t = target != null ? Math.max(0, Math.min(100, (target - lo) / (hi - lo) * 100)) : null;
    return '<span class="range"><i class="range__track"></i><i class="range__dot" style="left:' + p + '%"></i>' + (t != null ? '<i class="range__target" style="left:' + t + '%"></i>' : "") + "</span>";
  };
})();
