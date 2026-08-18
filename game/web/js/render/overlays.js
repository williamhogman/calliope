// Marker & route overlays (E9.4): the vector annotation that rides above
// the imagery — trade network, deposits, settlements, ruins, hover marks,
// the scale bar — plus the animated layer (caravans, winds) that draws on
// its own canvas and clock (E9.3) so motion never forces a label repaint.

const TIER_RADIUS = { Camp: 3, Village: 4.5, Town: 6, City: 8 };

// M7.6 — junction-merged, smoothed draw geometry. Every undirected
// cell-to-cell segment draws once, carrying the sum of the traffic that
// shares it; chains between junctions become single polylines; two rounds
// of corner-cutting let roads flow instead of stair-stepping.
export function buildDrawPaths(R) {
  const segs = new Map(); // "ax,ay|bx,by|mode" -> {a, b, m, w, old}
  for (const r of R.routes) {
    const m = r.m || [];
    const wgt = r.w || 1;
    const old = !!r.old; // M9.4 — a way no caravan has walked in years
    for (let i = 1; i < r.pts.length; i++) {
      const mode = m[i] ?? 0;
      const a = r.pts[i - 1], b = r.pts[i];
      const ka = a[0] + "," + a[1], kb = b[0] + "," + b[1];
      const key = (ka < kb ? ka + "|" + kb : kb + "|" + ka) + "|" + mode;
      const e = segs.get(key);
      if (e) { e.w += wgt; e.old = e.old && old; } // one live route keeps it alive
      else segs.set(key, { a: [a[0], a[1]], b: [b[0], b[1]], m: mode, w: wgt, old });
    }
  }
  const adj = new Map(); // "x,y" -> [segKey…]
  for (const [key, e] of segs) {
    for (const n of [e.a, e.b]) {
      const k = n[0] + "," + n[1];
      if (!adj.has(k)) adj.set(k, []);
      adj.get(k).push(key);
    }
  }
  const wClass = (w2) => Math.round(Math.log2(1 + w2) * 2);
  const nodeKey = (n) => n[0] + "," + n[1];
  const other = (e, n) => (e.a[0] === n[0] && e.a[1] === n[1] ? e.b : e.a);
  const isJunction = (k) => (adj.get(k) || []).length !== 2;
  const used = new Set();
  const paths = [];
  const walk = (startSeg, from) => {
    const e0 = segs.get(startSeg);
    used.add(startSeg);
    const pts = [from];
    let cur = startSeg;
    let node = other(e0, from);
    pts.push(node);
    const mode = e0.m, cls = wClass(e0.w), old = !!e0.old;
    let wSum = e0.w, wN = 1;
    while (!isJunction(nodeKey(node))) {
      const nexts = (adj.get(nodeKey(node)) || []).filter((k) => k !== cur && !used.has(k));
      if (nexts.length !== 1) break;
      const e = segs.get(nexts[0]);
      if (e.m !== mode || wClass(e.w) !== cls || !!e.old !== old) break;
      used.add(nexts[0]);
      cur = nexts[0];
      node = other(e, node);
      pts.push(node);
      wSum += e.w; wN++;
    }
    return { pts, m: mode, w: wSum / wN, old };
  };
  for (const [k, list] of adj) {
    if (list.length === 2) continue; // junctions and endpoints seed chains
    for (const segKey of list) {
      if (!used.has(segKey)) paths.push(walk(segKey, k.split(",").map(Number)));
    }
  }
  for (const [key, e] of segs) {
    if (!used.has(key)) paths.push(walk(key, e.a)); // leftover pure loops
  }
  for (const p of paths) {
    let pts = p.pts;
    for (let it = 0; it < 2 && pts.length > 2; it++) {
      const out = [pts[0]];
      for (let i = 0; i < pts.length - 1; i++) {
        const [ax, ay] = pts[i], [bx, by] = pts[i + 1];
        out.push([ax * 0.75 + bx * 0.25, ay * 0.75 + by * 0.25],
                 [ax * 0.25 + bx * 0.75, ay * 0.25 + by * 0.75]);
      }
      out.push(pts[pts.length - 1]);
      pts = out;
    }
    p.draw = pts;
  }
  R.drawPaths = paths;
}

export function routePoint(route, t) {
  const target = t * route.total;
  const { pts, cum } = route;
  let lo = 0, hi = cum.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (cum[mid] < target) lo = mid + 1; else hi = mid;
  }
  const i = Math.max(1, lo);
  const seg = cum[i] - cum[i - 1] || 1;
  const f = (target - cum[i - 1]) / seg;
  const mode = route.m ? (route.m[i] ?? 0) : 0;
  return [
    pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * f,
    pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * f,
    mode,
  ];
}

// ---------- static annotation (draws on damage frames only) -----------------

export function drawRouteNetwork(R, ctx, view) {
  const s = view.scale;
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  // M7.6 — the merged, smoothed network: shared trunks draw once, wider
  // with the traffic they carry, and chains flow instead of stair-stepping
  for (const p of R.drawPaths || []) {
    const wgt = p.w || 1;
    const lw = Math.min(2.8, Math.max(0.8, s * 0.11 * wgt + 0.4));
    const alpha = Math.min(0.7, 0.34 + wgt * 0.16);
    if (p.m === 1) {
      ctx.setLineDash([7, 6]);
      ctx.strokeStyle = `rgba(126, 178, 226, ${alpha})`;
    } else if (p.m === 2) {
      ctx.setLineDash([2, 4]);
      ctx.strokeStyle = `rgba(118, 204, 214, ${alpha})`;
    } else {
      ctx.setLineDash([]);
      ctx.strokeStyle = `rgba(224, 196, 140, ${alpha})`;
    }
    ctx.lineWidth = lw;
    if (p.old) {
      // M9.4 — disused ways: grass in the wheel-ruts, still faintly there
      ctx.setLineDash([2, 5]);
      ctx.strokeStyle = `rgba(168, 164, 152, ${Math.min(0.28, alpha * 0.45)})`;
      ctx.lineWidth = Math.min(1.1, lw * 0.55);
    }
    ctx.beginPath();
    const pts = p.draw;
    for (let k = 0; k < pts.length; k++) {
      const px = view.tx + (pts[k][0] + 0.5) * s;
      const py = view.ty + (pts[k][1] + 0.5) * s;
      if (k === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
    }
    ctx.stroke();
  }
  ctx.setLineDash([]);
  ctx.restore();
}

export function drawDeposits(R, ctx, view) {
  const meta = R.world.header.resources;
  const s = view.scale;
  const rad = Math.max(2.2, Math.min(6, s * 0.45));
  for (const d of R.world.header.deposits) {
    const sx = view.tx + (d.x + 0.5) * s;
    const sy = view.ty + (d.y + 0.5) * s;
    if (sx < -10 || sy < -10 || sx > R.canvas.clientWidth + 10 || sy > R.canvas.clientHeight + 10) continue;
    const dead = d.left === 0; // a spent mine: hollow, grey, remembered
    ctx.beginPath();
    ctx.moveTo(sx, sy - rad);
    ctx.lineTo(sx + rad, sy);
    ctx.lineTo(sx, sy + rad);
    ctx.lineTo(sx - rad, sy);
    ctx.closePath();
    if (dead) {
      ctx.globalAlpha = 0.4;
      ctx.lineWidth = 1.1;
      ctx.strokeStyle = meta[d.r]?.color || "#999";
      ctx.stroke();
      ctx.globalAlpha = 1;
    } else {
      ctx.fillStyle = meta[d.r]?.color || "#ccc";
      ctx.globalAlpha = 0.95;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.lineWidth = 1;
      ctx.strokeStyle = "rgba(0,0,0,0.55)";
      ctx.stroke();
    }
  }
}

// M9.1 — what remains: three walls standing, the fourth long fallen.
function drawRuins(R, ctx, view, state) {
  const ruins = R.world.header.ruins || [];
  if (!ruins.length) return;
  const s = view.scale;
  const a = Math.max(0, Math.min(0.85, (s - 1.1) * 0.45));
  if (a <= 0.02) return;
  const W = R.canvas.clientWidth, H = R.canvas.clientHeight;
  ctx.save();
  ctx.lineCap = "round";
  for (const r of ruins) {
    const sx = view.tx + (r.x + 0.5) * s;
    const sy = view.ty + (r.y + 0.5) * s;
    if (sx < -30 || sy < -30 || sx > W + 30 || sy > H + 30) continue;
    const g = Math.max(2.2, Math.min(4.4, s * 0.5));
    if (state.selectedRuin === r.eid) {
      ctx.beginPath();
      ctx.arc(sx, sy, g + 4.5, 0, Math.PI * 2);
      ctx.lineWidth = 1.6;
      ctx.strokeStyle = "rgba(212, 169, 74, 0.95)";
      ctx.stroke();
    }
    ctx.globalAlpha = a;
    ctx.lineWidth = 1.15;
    ctx.strokeStyle = "rgba(203, 198, 185, 0.92)";
    ctx.beginPath();
    ctx.moveTo(sx - g, sy - g * 0.35);
    ctx.lineTo(sx - g, sy + g * 0.8);
    ctx.lineTo(sx + g, sy + g * 0.8);
    ctx.lineTo(sx + g, sy - g * 0.05);
    ctx.stroke();
    // the fallen lintel, resting where it dropped
    ctx.beginPath();
    ctx.moveTo(sx - g * 0.45, sy - g * 0.95);
    ctx.lineTo(sx + g * 0.6, sy - g * 0.55);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  ctx.restore();
}

export function drawSettlements(R, ctx, view, state) {
  const s = view.scale;
  drawRuins(R, ctx, view, state);
  for (const st of R.world.header.settlements) {
    const sx = view.tx + (st.x + 0.5) * s;
    const sy = view.ty + (st.y + 0.5) * s;
    if (sx < -60 || sy < -30 || sx > R.canvas.clientWidth + 60 || sy > R.canvas.clientHeight + 30) continue;
    const r = TIER_RADIUS[st.tier] || 3;
    const [cr, cg, cb] = R.realmColor(st);
    const selected = state.selectedId === st.id;
    if (selected) {
      ctx.beginPath();
      ctx.arc(sx, sy, r + 4.5, 0, Math.PI * 2);
      ctx.lineWidth = 1.6;
      ctx.strokeStyle = "rgba(212, 169, 74, 0.95)";
      ctx.stroke();
    }
    ctx.beginPath();
    ctx.arc(sx, sy, r, 0, Math.PI * 2);
    ctx.fillStyle = "#f4ecd7";
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = `rgb(${cr | 0},${cg | 0},${cb | 0})`;
    ctx.stroke();
    ctx.lineWidth = 0.75;
    ctx.strokeStyle = "rgba(0,0,0,0.6)";
    ctx.stroke();
    if (st.port) {
      // a harbour ring: this town's trade goes under sail
      ctx.beginPath();
      ctx.arc(sx, sy, r + 2.6, 0, Math.PI * 2);
      ctx.lineWidth = 1.3;
      ctx.strokeStyle = "rgba(126, 178, 226, 0.85)";
      ctx.stroke();
    }
  }
}

export function drawHover(ctx, view, hover) {
  const s = view.scale;
  ctx.strokeStyle = "rgba(255,255,255,0.7)";
  ctx.lineWidth = 1.2;
  ctx.strokeRect(view.tx + hover.x * s, view.ty + hover.y * s, s, s);
}

// The inspected cell: corner ticks, calmer than a full box.
export function drawSelectedCell(ctx, view, cell) {
  const s = Math.max(view.scale, 6);
  const x = view.tx + (cell.x + 0.5) * view.scale - s / 2;
  const y = view.ty + (cell.y + 0.5) * view.scale - s / 2;
  const c = Math.max(3, s * 0.3);
  ctx.save();
  ctx.strokeStyle = "rgba(212, 169, 74, 0.95)";
  ctx.lineWidth = 1.6;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x, y + c); ctx.lineTo(x, y); ctx.lineTo(x + c, y);
  ctx.moveTo(x + s - c, y); ctx.lineTo(x + s, y); ctx.lineTo(x + s, y + c);
  ctx.moveTo(x + s, y + s - c); ctx.lineTo(x + s, y + s); ctx.lineTo(x + s - c, y + s);
  ctx.moveTo(x + c, y + s); ctx.lineTo(x, y + s); ctx.lineTo(x, y + s - c);
  ctx.stroke();
  ctx.restore();
}

// A quiet GIS-style scale bar, bottom centre — the one strip of map the
// side panels never cover, on any screen.
export function drawScaleBar(R, ctx, view) {
  if (!R.world) return;
  const kmPer = R.world.header.km_per_cell || 4;
  const nice = [10, 20, 50, 100, 200, 500, 1000, 2000, 5000];
  let km = 0;
  for (const n of nice) {
    if ((n / kmPer) * view.scale <= 150) km = n;
  }
  if (!km) return;
  const px = (km / kmPer) * view.scale;
  const mobile = window.matchMedia("(max-width: 760px)").matches;
  const x = (R.canvas.clientWidth - px) / 2;
  const y = R.canvas.clientHeight - (mobile ? 92 : 16);
  ctx.save();
  ctx.strokeStyle = "rgba(222, 231, 244, 0.85)";
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  ctx.moveTo(x, y - 5);
  ctx.lineTo(x, y);
  ctx.lineTo(x + px, y);
  ctx.lineTo(x + px, y - 5);
  ctx.moveTo(x + px / 2, y);
  ctx.lineTo(x + px / 2, y - 3.5);
  ctx.stroke();
  const label = `${km.toLocaleString("en-US")} km`;
  ctx.font = "500 10px Inter, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  ctx.lineJoin = "round";
  ctx.lineWidth = 3;
  ctx.strokeStyle = "rgba(5, 9, 16, 0.75)";
  ctx.strokeText(label, x + px / 2, y - 9);
  ctx.fillStyle = "rgba(222, 231, 244, 0.9)";
  ctx.fillText(label, x + px / 2, y - 9);
  ctx.restore();
}

// ---------- the animated layer (its own canvas, its own clock — E9.3) -------

export function drawWinds(R, ctx, view, isPlaying) {
  const H = R.h, W = R.w;
  const s = view.scale;
  const w = R.canvas.clientWidth;
  const drift = isPlaying ? (performance.now() / 1000 * 26) : 0;
  ctx.save();
  ctx.lineWidth = 1.3;
  ctx.lineCap = "round";
  const rowStep = Math.max(18, 30 / Math.max(s / 2, 1));
  for (let wy = rowStep / 2; wy < H; wy += rowStep) {
    const lat = Math.abs((wy / H) * 180 - 90);
    const dir = lat < 30 ? -1 : lat < 60 ? 1 : -1; // grid x direction of travel
    const py = view.ty + wy * s;
    if (py < -20 || py > R.canvas.clientHeight + 20) continue;
    const spacing = 96;
    const shift = ((drift * dir) % spacing + spacing) % spacing;
    const x0 = Math.max(0, view.tx) - spacing;
    const x1 = Math.min(w, view.tx + W * s) + spacing;
    const alpha = 0.34;
    ctx.strokeStyle = `rgba(205, 226, 250, ${alpha})`;
    for (let px = x0 + shift; px < x1; px += spacing) {
      const wob = Math.sin((px + wy * 13) * 0.05) * 3;
      const yy = py + wob;
      const len = 30;
      const tail = px - dir * len;
      ctx.beginPath();
      ctx.moveTo(tail, yy + Math.sin(tail * 0.05) * 1.5);
      ctx.quadraticCurveTo((tail + px) / 2, yy - 2, px, yy);
      ctx.stroke();
      // arrowhead
      ctx.beginPath();
      ctx.moveTo(px - dir * 5.5, yy - 3.4);
      ctx.lineTo(px, yy);
      ctx.lineTo(px - dir * 5.5, yy + 3.4);
      ctx.stroke();
    }
  }
  ctx.restore();
}

// caravans on the roads, sails on the lanes, while time flows
export function drawCaravans(R, ctx, view) {
  const s = view.scale;
  const now = performance.now() / 1000;
  ctx.save();
  for (const r of R.routes) {
    if (r.old) continue; // no caravan takes the disused ways
    const t = ((now / 14 + r.phase) % 1);
    const tt = t < 0.5 ? t * 2 : (1 - t) * 2; // there and back again
    const [wx, wy, mode] = routePoint(r, tt);
    const px = view.tx + (wx + 0.5) * s;
    const py = view.ty + (wy + 0.5) * s;
    if (px < -10 || py < -10 || px > R.canvas.clientWidth + 10 || py > R.canvas.clientHeight + 10) continue;
    if (mode === 1) {
      ctx.beginPath();
      ctx.moveTo(px, py - 3.8);
      ctx.lineTo(px + 2.9, py + 2.5);
      ctx.lineTo(px - 2.9, py + 2.5);
      ctx.closePath();
      ctx.fillStyle = "#dceaf7";
      ctx.fill();
      ctx.lineWidth = 1;
      ctx.strokeStyle = "rgba(18, 38, 60, 0.7)";
      ctx.stroke();
    } else {
      ctx.beginPath();
      ctx.arc(px, py, 2.6, 0, Math.PI * 2);
      ctx.fillStyle = "#f2d9a0";
      ctx.fill();
      ctx.lineWidth = 1;
      ctx.strokeStyle = "rgba(0,0,0,0.65)";
      ctx.stroke();
    }
  }
  ctx.restore();
}
