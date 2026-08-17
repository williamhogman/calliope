// The unified label engine (M7.2/M7.3/M7.7, E9.4/E9.5): one placement pass
// for every name on the map — feature labels and settlement names compete
// for the same ground, mighty-to-minor, and nothing overlaps, ever.
//
// Layout and drawing are split (E9.5): placement, text measurement, river
// course tracing and collision run only when the zoom, the data, or the
// visible region actually change. The result is cached in map-space pixels
// (screen px minus the view translation), so every pan frame — the common
// frame — just translates and paints. Picking reads the same cached boxes
// through labelBoxesAt (E9.6), correct even on frames the loop skipped.

// pri decides who wins when labels collide (lower = mightier)
export const LABEL_STYLE = {
  ocean:     { color: "#a9c9e6", pri: 0 },
  continent: { color: "#e8e2d2", pri: 1 },
  sea:       { color: "#9dbfde", pri: 2 },
  range:     { color: "#dcd8d0", pri: 3 },
  desert:    { color: "#e0c98f", pri: 4 },
  forest:    { color: "#a3c893", pri: 5 },
  archipelago: { color: "#d8d2c2", pri: 5.5 },
  island:    { color: "#d5cfc0", pri: 6 },
  river:     { color: "#93bfe6", pri: 7 },
  lake:      { color: "#93bfe6", pri: 8 },
  highland:  { color: "#d8cfb9", pri: 9 },
  bay:       { color: "#9dbfde", pri: 10 },
  strait:    { color: "#9dbfde", pri: 11 },
  cape:      { color: "#d5cfc0", pri: 12 },
  peak:      { color: "#e6dfd2", pri: 13 },
  marsh:     { color: "#a8c8a4", pri: 14 },
  delta:     { color: "#93bfe6", pri: 15 },
  pass:      { color: "#d9c9a6", pri: 16 },
  ford:      { color: "#a6c6e6", pri: 17 },
};

// fine-grain coastal detail only earns a label once you lean in
const DETAIL_KINDS = new Set(["bay", "strait", "cape", "peak", "marsh", "delta", "pass", "ford"]);

const TIER_RADIUS = { Camp: 3, Village: 4.5, Town: 6, City: 8 };

function labelAlpha(kind, s) {
  if (kind === "ocean" || kind === "sea" || kind === "continent") {
    return Math.max(0, Math.min(0.95, 1.5 - s * 0.17));
  }
  if (kind === "range" || kind === "desert" || kind === "forest" ||
      kind === "highland" || kind === "archipelago") {
    return Math.max(0, Math.min(0.88, (s - 0.95) * 0.9)) *
           Math.max(0, Math.min(1, 2.6 - s * 0.11));
  }
  if (DETAIL_KINDS.has(kind)) {
    return Math.max(0, Math.min(0.85, (s - 3.4) * 0.7));
  }
  return Math.max(0, Math.min(0.85, (s - 2.1) * 0.75));
}

// Settlement label density follows Töpfer's radical law: at scale s the map
// keeps N·√(s/S_full) of the names it would carry fully zoomed in.
function labelBudget(total, s) {
  const S_FULL = 6;
  if (s >= S_FULL) return total;
  return Math.max(4, Math.ceil(total * Math.sqrt(Math.max(s, 0.12) / S_FULL)));
}

// M7.7 — trace the river's course around a label anchor so the name can
// ride the water. Walks the channel both ways along the discharge slope.
function riverPath(R, f) {
  R._riverPaths ??= new Map();
  const ck = f.x + "," + f.y;
  if (R._riverPaths.has(ck)) return R._riverPaths.get(ck);
  const { flags, discharge } = R.world.arrays;
  const W = R.w, H = R.h;
  const at = (x, y) => y * W + x;
  // nearest channel cell to the anchor
  let cx = -1, cy = -1, bd = Infinity;
  for (let dy = -3; dy <= 3; dy++) {
    for (let dx = -3; dx <= 3; dx++) {
      const x = f.x + dx, y = f.y + dy;
      if (x < 0 || y < 0 || x >= W || y >= H) continue;
      if (!(flags[at(x, y)] & 1)) continue;
      const d = dx * dx + dy * dy;
      if (d < bd) { bd = d; cx = x; cy = y; }
    }
  }
  if (cx < 0) { R._riverPaths.set(ck, null); return null; }
  const walk = (down) => {
    const out = [];
    let x = cx, y = cy, prev = -1;
    for (let n = 0; n < 26; n++) {
      const cur = discharge[at(x, y)];
      let bx = -1, by = -1, best = down ? cur : -Infinity;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (!dx && !dy) continue;
          const nx = x + dx, ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= W || ny >= H) continue;
          const i = at(nx, ny);
          if (i === prev || !(flags[i] & 1)) continue;
          const d = discharge[i];
          if (down ? d > best : (d < cur && d > best)) { best = d; bx = nx; by = ny; }
        }
      }
      if (bx < 0) break;
      prev = at(x, y);
      x = bx; y = by;
      out.push([x + 0.5, y + 0.5]);
    }
    return out;
  };
  const up = walk(false).reverse();
  const down = walk(true);
  let pts = [...up, [cx + 0.5, cy + 0.5], ...down];
  if (pts.length < 6) { R._riverPaths.set(ck, null); return null; }
  // two rounds of corner-cutting so the course flows instead of stepping
  for (let it = 0; it < 2; it++) {
    const sm = [pts[0]];
    for (let i = 0; i < pts.length - 1; i++) {
      const [ax, ay] = pts[i], [bx, by] = pts[i + 1];
      sm.push([ax * 0.75 + bx * 0.25, ay * 0.75 + by * 0.25],
              [ax * 0.25 + bx * 0.75, ay * 0.25 + by * 0.75]);
    }
    sm.push(pts[pts.length - 1]);
    pts = sm;
  }
  R._riverPaths.set(ck, pts);
  return pts;
}

// Lay text along a world-space polyline, centred on its arc, in map-space
// pixels at scale s. Returns { box, glyphs } or null when the course is
// too short for the name.
function layoutCurvedText(ctx, pts, s, text) {
  const sp = pts.map(([x, y]) => [x * s, y * s]);
  const cum = [0];
  for (let i = 1; i < sp.length; i++) {
    cum.push(cum[i - 1] + Math.hypot(sp[i][0] - sp[i - 1][0], sp[i][1] - sp[i - 1][1]));
  }
  const total = cum[cum.length - 1];
  const tw = ctx.measureText(text).width;
  if (total < tw * 1.15) return null;
  // read left-to-right: flip when the course runs westward on screen
  if (sp[sp.length - 1][0] < sp[0][0]) {
    sp.reverse();
    for (let i = 0; i < cum.length; i++) cum[i] = total - cum[i];
    cum.reverse();
  }
  const atLen = (d) => {
    let lo = 0, hi = cum.length - 1;
    while (lo < hi) { const m = (lo + hi) >> 1; if (cum[m] < d) lo = m + 1; else hi = m; }
    const i = Math.max(1, lo);
    const seg = cum[i] - cum[i - 1] || 1;
    const f2 = (d - cum[i - 1]) / seg;
    return [
      sp[i - 1][0] + (sp[i][0] - sp[i - 1][0]) * f2,
      sp[i - 1][1] + (sp[i][1] - sp[i - 1][1]) * f2,
      Math.atan2(sp[i][1] - sp[i - 1][1], sp[i][0] - sp[i - 1][0]),
    ];
  };
  let d = (total - tw) / 2;
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const glyphs = [];
  for (const ch of text) {
    const cw = ctx.measureText(ch).width;
    const [gx, gy, ang] = atLen(d + cw / 2);
    glyphs.push([ch, gx, gy, ang]);
    x0 = Math.min(x0, gx - 8); y0 = Math.min(y0, gy - 9);
    x1 = Math.max(x1, gx + 8); y1 = Math.max(y1, gy + 5);
    d += cw;
  }
  return { box: { x0, y0, x1, y1 }, glyphs };
}

// ---------- layout (cached) --------------------------------------------------

// Fraction of the viewport kept as slack around the culled region; the
// cached layout stays valid until the view pans past it.
const MARGIN = 0.15;

export function ensureLabelLayout(R, ctx, view, state) {
  const s = view.scale;
  const w = R.canvas.clientWidth;
  const hgt = R.canvas.clientHeight;
  const sig = [
    s, state.version, !!state.overlays.labels, !!state.overlays.settlements,
    w, hgt,
  ].join("|");
  const L = R._labelLayout;
  if (L && L.sig === sig &&
      Math.abs(view.tx - L.tx0) <= L.mx && Math.abs(view.ty - L.ty0) <= L.my) {
    return L;
  }
  return layoutLabels(R, ctx, view, state, sig);
}

function layoutLabels(R, ctx, view, state, sig) {
  const s = view.scale;
  const w = R.canvas.clientWidth;
  const hgt = R.canvas.clientHeight;
  const mx = w * MARGIN, my = hgt * MARGIN;
  const tx = view.tx, ty = view.ty;
  const items = [];
  const labelBoxes = [];
  const placed = [];
  const stats = { scale: s, candidates: 0, placed: 0, overlaps: 0,
                  setBudget: 0, setPlaced: 0, featPlaced: 0, curved: 0 };

  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";

  const cands = [];

  // -- feature names ----------------------------------------------------
  if (state.overlays.labels) {
    const feats = R.world.header.features || [];
    for (let fi = 0; fi < feats.length; fi++) {
      const f = feats[fi];
      const st = LABEL_STYLE[f.t];
      if (!st) continue;
      const alpha = labelAlpha(f.t, s);
      if (alpha <= 0.03) continue;
      const px = tx + f.x * s;
      const py = ty + f.y * s;
      if (px < -280 - mx || py < -60 - my || px > w + 280 + mx || py > hgt + 60 + my) continue;

      let font, size, text = f.name;
      const lg = Math.log2(Math.max(f.size, 2));
      const areaKind = f.t === "range" || f.t === "desert" || f.t === "forest" ||
                       f.t === "highland" || f.t === "archipelago";
      const grand = f.t === "ocean" || f.t === "sea" || f.t === "continent";
      if (grand) {
        size = Math.min(f.t === "continent" ? 20 : 21, (f.t === "continent" ? 8 : 8.5) + lg * 0.8);
        font = `${f.t === "continent" ? 600 : 500} ${size.toFixed(1)}px Inter, sans-serif`;
        text = f.name.toUpperCase();
      } else if (areaKind) {
        size = 11;
        font = `600 11px Inter, sans-serif`;
        text = f.name.toUpperCase();
      } else {
        size = 10.5;
        font = `italic 500 10.5px Inter, sans-serif`;
      }
      cands.push({ type: "feat", f, fi, st, alpha, px, py, font, size, text,
                   pri: st.pri, mass: f.size, areaKind, grand });
    }
  }

  // -- settlement names, budgeted by the radical law (M7.2) ---------------
  if (state.overlays.settlements) {
    const sets = [...R.world.header.settlements].sort(
      (a, b) => (b.pop - a.pop) || (a.id - b.id));
    stats.setBudget = labelBudget(sets.length, s);
    let taken = 0;
    for (const st of sets) {
      if (taken >= stats.setBudget) break;
      const px = tx + (st.x + 0.5) * s;
      const py = ty + (st.y + 0.5) * s;
      if (px < -80 - mx || py < -40 - my || px > w + 80 + mx || py > hgt + 40 + my) continue;
      taken++;
      const isBig = st.tier === "Town" || st.tier === "City";
      const pri = st.tier === "City" ? 2.3 : st.tier === "Town" ? 3.3
                : st.tier === "Village" ? 6.3 : 7.3;
      cands.push({ type: "set", st, px, py, pri, mass: st.pop, isBig,
                   r: TIER_RADIUS[st.tier] || 3 });
    }
  }

  // -- ruin names — quiet italics, only when the eye is close (M9.1) ------
  if (state.overlays.settlements && state.overlays.labels) {
    const ra = Math.max(0, Math.min(0.8, (s - 3.4) * 0.55));
    if (ra > 0.03) {
      for (const ru of R.world.header.ruins || []) {
        const px = tx + (ru.x + 0.5) * s;
        const py = ty + (ru.y + 0.5) * s;
        if (px < -80 - mx || py < -40 - my || px > w + 80 + mx || py > hgt + 40 + my) continue;
        cands.push({ type: "ruin", ru, px, py, pri: 8.6, mass: 1, alpha: ra });
      }
    }
  }

  stats.candidates = cands.length;
  cands.sort((a, b) => (a.pri - b.pri) || (b.mass - a.mass));

  const collides = (box) =>
    placed.some((b) => box.x0 < b.x1 && box.x1 > b.x0 && box.y0 < b.y1 && box.y1 > b.y0);
  // boxes are computed in screen space here, then stored in map-space
  const toMap = (box) => ({ x0: box.x0 - tx, x1: box.x1 - tx, y0: box.y0 - ty, y1: box.y1 - ty });

  for (const c of cands) {
    if (c.type === "set") {
      // name above the dot, souls below at close zoom; the box spans the
      // whole block so no other name may crowd the town it belongs to
      ctx.font = `600 ${c.isBig ? 12 : 11}px Inter, sans-serif`;
      const name = c.st.name;
      const tw = Math.max(ctx.measureText(name).width, c.r * 2) + 8;
      const yTop = c.py - c.r - 5 - (c.isBig ? 12 : 11);
      const yBot = c.py + c.r + (s > 3 ? 14 : 2);
      const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: yTop, y1: yBot };
      if (collides(box)) continue;
      placed.push(box);
      stats.setPlaced++;
      items.push({
        type: "set", mx: c.px - tx, my: c.py - ty, r: c.r, isBig: c.isBig,
        name, pop: s > 3 ? c.st.pop.toLocaleString("en-US") : null,
      });
      continue;
    }

    if (c.type === "ruin") {
      ctx.font = "italic 500 10px Inter, sans-serif";
      const tw = ctx.measureText(c.ru.name).width + 8;
      const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: c.py - 18, y1: c.py + 5 };
      if (collides(box)) continue;
      placed.push(box);
      items.push({ type: "ruin", mx: c.px - tx, my: c.py - ty, name: c.ru.name, alpha: c.alpha });
      continue;
    }

    // -- feature label ----------------------------------------------------
    ctx.font = c.font;

    // M7.7 — river names ride their water at close zoom
    if (c.f.t === "river" && s >= 3.2) {
      const path = riverPath(R, c.f);
      if (path) {
        const laid = layoutCurvedText(ctx, path, s, c.text);
        if (laid) {
          const abs = { x0: laid.box.x0 + tx, x1: laid.box.x1 + tx,
                        y0: laid.box.y0 + ty, y1: laid.box.y1 + ty };
          if (!collides(abs)) {
            placed.push(abs);
            items.push({ type: "curve", glyphs: laid.glyphs, font: c.font,
                         color: c.st.color, alpha: c.alpha });
            labelBoxes.push({ ...laid.box, index: c.fi });
            stats.featPlaced++;
            stats.curved++;
            continue;
          }
        }
      }
    }

    // M7.3 — area names letter-space to the ground they cover: the name
    // of a great desert strides across it, a lesser wood sits close
    let spacing;
    if (c.grand || c.areaKind) {
      const extent = Math.sqrt(Math.max(c.f.size, 4)) * s * 1.45;
      const baseW = ctx.measureText(c.text).width;
      const maxSp = c.grand ? 18 : 9;
      const minSp = c.grand ? 2.5 : 1.4;
      spacing = Math.max(minSp, Math.min(maxSp,
        (extent - baseW) / Math.max(3, c.text.length - 1)));
    } else {
      spacing = 0.6;
    }
    ctx.letterSpacing = `${spacing.toFixed(1)}px`;
    const tw = ctx.measureText(c.text).width + 10;
    ctx.letterSpacing = "0px";
    const th = c.size + 9;
    const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: c.py - th / 2, y1: c.py + th / 2 };
    if (collides(box)) continue;
    placed.push(box);
    items.push({
      type: "feat", mx: c.px - tx, my: c.py - ty, text: c.text, font: c.font,
      color: c.st.color, alpha: c.alpha, spacing: spacing.toFixed(1),
    });
    labelBoxes.push({ ...toMap(box), index: c.fi });
    stats.featPlaced++;
  }
  ctx.restore();

  // the gate's evidence: recheck every placed box against every other
  for (let i = 0; i < placed.length; i++) {
    for (let j = i + 1; j < placed.length; j++) {
      const a = placed[i], b = placed[j];
      if (a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0) stats.overlaps++;
    }
  }
  stats.placed = placed.length;
  R.labelStatsData = stats;

  R._labelLayout = {
    sig, tx0: tx, ty0: ty, mx, my, items, labelBoxes,
  };
  return R._labelLayout;
}

// ---------- draw (every damage frame — translate and paint) ------------------

export function drawLabels(R, ctx, view, state) {
  const L = ensureLabelLayout(R, ctx, view, state);
  const dx = view.tx, dy = view.ty;
  ctx.save();
  ctx.textAlign = "center";
  ctx.lineJoin = "round";
  for (const it of L.items) {
    if (it.type === "set") {
      ctx.textBaseline = "alphabetic";
      ctx.font = `600 ${it.isBig ? 12 : 11}px Inter, sans-serif`;
      ctx.lineWidth = 3;
      ctx.strokeStyle = "rgba(8,12,20,0.85)";
      ctx.strokeText(it.name, it.mx + dx, it.my + dy - it.r - 5);
      ctx.fillStyle = "#f0ead8";
      ctx.fillText(it.name, it.mx + dx, it.my + dy - it.r - 5);
      if (it.pop) {
        ctx.font = "500 9.5px Inter, sans-serif";
        ctx.strokeText(it.pop, it.mx + dx, it.my + dy + it.r + 11);
        ctx.fillStyle = "#b9c0cf";
        ctx.fillText(it.pop, it.mx + dx, it.my + dy + it.r + 11);
      }
    } else if (it.type === "ruin") {
      ctx.textBaseline = "alphabetic";
      ctx.font = "italic 500 10px Inter, sans-serif";
      ctx.globalAlpha = it.alpha;
      ctx.lineWidth = 2.4;
      ctx.strokeStyle = "rgba(8,12,20,0.8)";
      ctx.strokeText(it.name, it.mx + dx, it.my + dy - 7);
      ctx.fillStyle = "#b7b1a2";
      ctx.fillText(it.name, it.mx + dx, it.my + dy - 7);
      ctx.globalAlpha = 1;
    } else if (it.type === "curve") {
      ctx.font = it.font;
      ctx.textBaseline = "middle";
      ctx.globalAlpha = it.alpha;
      for (const [ch, gx, gy, ang] of it.glyphs) {
        ctx.save();
        ctx.translate(gx + dx, gy + dy);
        ctx.rotate(ang);
        ctx.strokeStyle = "rgba(4, 8, 15, 0.7)";
        ctx.lineWidth = 2.6;
        ctx.strokeText(ch, 0, -3);
        ctx.fillStyle = it.color;
        ctx.fillText(ch, 0, -3);
        ctx.restore();
      }
      ctx.globalAlpha = 1;
    } else { // feat
      ctx.font = it.font;
      ctx.textBaseline = "middle";
      ctx.letterSpacing = `${it.spacing}px`;
      ctx.globalAlpha = it.alpha;
      ctx.strokeStyle = "rgba(4, 8, 15, 0.7)";
      ctx.lineWidth = 2.6;
      ctx.strokeText(it.text, it.mx + dx, it.my + dy);
      ctx.fillStyle = it.color;
      ctx.fillText(it.text, it.mx + dx, it.my + dy);
      ctx.letterSpacing = "0px";
      ctx.globalAlpha = 1;
    }
  }
  ctx.restore();
}

// Picking reads the cached layout translated to the live view (E9.6) —
// hit-testing stays correct even on frames the render loop skipped.
export function labelBoxesAt(R, view) {
  const L = R._labelLayout;
  if (!L) return [];
  const dx = view.tx, dy = view.ty;
  return L.labelBoxes.map((b) => ({
    x0: b.x0 + dx, x1: b.x1 + dx, y0: b.y0 + dy, y1: b.y1 + dy, index: b.index,
  }));
}
