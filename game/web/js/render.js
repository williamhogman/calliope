// Renderer: composites the world into an offscreen canvas, draws to screen.

import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, ELEV_ARID_GRAD, SEA_GRAD,
  HYDRO_GRAD, FERT_GRAD,
  gradient, hash2, hexRgb, settlementColor,
} from "./palette.js";

const TIER_RADIUS = { Camp: 3, Village: 4.5, Town: 6, City: 8 };

// pri decides who wins when labels collide (lower = mightier)
const LABEL_STYLE = {
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

// ---- satellite base palette -----------------------------------------------
// Land colour derives from continuous fields (moisture, warmth, soil, height)
// rather than biome classes, so the terrain reads like imagery: dunes bleed
// into steppe, steppe into savanna, forest darkens toward the snowline.
const VEG_GRAD = gradient([
  [0.0, [191, 168, 128]],   // bare sand and dune
  [0.14, [167, 148, 103]],  // semi-desert scrub
  [0.32, [128, 128, 74]],   // dry grassland
  [0.5, [92, 106, 58]],     // savanna, open woods
  [0.72, [55, 78, 44]],     // closed temperate forest
  [1.0, [30, 54, 32]],      // deep rainforest canopy
]);
const ROCK = [118, 108, 98];
const SNOW = [237, 240, 245];
const TUNDRA = [127, 117, 99];
const ICE_SHEET = [213, 218, 224];

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.off = document.createElement("canvas");
    this.octx = this.off.getContext("2d");
    this.cacheKey = null;
    this.tmonthCache = new Map();
    this.territoryCache = { version: -1, owner: null };
  }

  setWorld(world) {
    this.world = world;
    const w = world.header.width || world.header.size;
    const hh = world.header.size;
    this.w = w;
    this.h = hh;
    this.size = hh; // rows — used by scale-relative heuristics
    this.off.width = w;
    this.off.height = hh;
    this.cacheKey = null;
    this.tmonthCache.clear();
    this.territoryCache = { version: -1, owner: null };
    this._tintCache = null;
    // Engine-authoritative political map (M4.1): owner culture per cell,
    // −1 wilderness. Ships in the pack, updates arrive as RLE patches.
    this.territoryOwner = world.arrays.territory || null;
    this.ownerIsCulture = !!this.territoryOwner;

    // biome palette by id
    this.biomePal = [];
    for (const b of world.header.biomes) this.biomePal[b.id] = b.color;

    // culture colors
    this.cultureRgb = (world.header.cultures || []).map((c) => hexRgb(c.color));

    // M7.5 — multi-directional oblique-weighted hillshade with a curvature
    // accent (texture shading), precomputed once per world: four low suns
    // so ridges running every direction carve; the Laplacian etches
    // ridgelines bright and valley floors dark.
    const h = world.arrays.height;
    const sh = new Float32Array(w * hh);
    const k = hh / 16;
    for (let y = 0; y < hh; y++) {
      const y0 = Math.max(0, y - 1) * w;
      const y1 = Math.min(hh - 1, y + 1) * w;
      const yr = y * w;
      for (let x = 0; x < w; x++) {
        const x0 = Math.max(0, x - 1);
        const x1 = Math.min(w - 1, x + 1);
        const gx = (h[yr + x1] - h[yr + x0]) * 0.5;
        const gy = (h[y1 + x] - h[y0 + x]) * 0.5;
        const mdow = (-gx - gy) * 0.62 + (-gy) * 0.24 + (-gx) * 0.24 + (gx - gy) * 0.08;
        let curv = (h[yr + x1] + h[yr + x0] + h[y1 + x] + h[y0 + x] - 4 * h[yr + x]) * k * 0.55;
        curv = curv < -0.1 ? -0.1 : curv > 0.1 ? 0.1 : curv;
        let s = 1 + k * mdow * 1.05 - curv;
        sh[yr + x] = s < 0.58 ? 0.58 : s > 1.34 ? 1.34 : s;
      }
    }
    this.shade = sh;
    this._riverPaths = new Map(); // curved-label courses, rebuilt per world

    // discharge normaliser
    const d = world.arrays.discharge;
    let max = 0;
    for (let i = 0; i < d.length; i++) if (d[i] > max) max = d[i];
    this.dischargeLogMax = Math.log1p(max);

    this._buildSatellite();
    this.setRoutes(world.header.routes || []);
  }

  // smooth value noise on the world grid — bilinear hash, for imagery texture
  _noise(x, y, sc) {
    const gx = x / sc, gy = y / sc;
    const x0 = Math.floor(gx), y0 = Math.floor(gy);
    const fx = gx - x0, fy = gy - y0;
    const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
    const n00 = hash2(x0, y0), n10 = hash2(x0 + 1, y0);
    const n01 = hash2(x0, y0 + 1), n11 = hash2(x0 + 1, y0 + 1);
    return n00 + (n10 - n00) * sx + (n01 - n00) * sy + (n00 - n10 - n01 + n11) * sx * sy;
  }

  // Chamfer distance (cells) from every sea cell to the nearest land —
  // powers the engraved coastal vignette (M7.1) on the CPU path.
  _coastDistance() {
    const W = this.w, H = this.h;
    const hgt = this.world.arrays.height;
    const INF = 1e9;
    const d = new Float32Array(W * H);
    for (let i = 0; i < W * H; i++) d[i] = hgt[i] >= 0 ? 0 : INF;
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x;
        if (d[i] === 0) continue;
        let best = d[i];
        if (x > 0) best = Math.min(best, d[i - 1] + 1);
        if (y > 0) {
          best = Math.min(best, d[i - W] + 1);
          if (x > 0) best = Math.min(best, d[i - W - 1] + 1.4);
          if (x < W - 1) best = Math.min(best, d[i - W + 1] + 1.4);
        }
        d[i] = best;
      }
    }
    for (let y = H - 1; y >= 0; y--) {
      for (let x = W - 1; x >= 0; x--) {
        const i = y * W + x;
        if (d[i] === 0) continue;
        let best = d[i];
        if (x < W - 1) best = Math.min(best, d[i + 1] + 1);
        if (y < H - 1) {
          best = Math.min(best, d[i + W] + 1);
          if (x < W - 1) best = Math.min(best, d[i + W + 1] + 1.4);
          if (x > 0) best = Math.min(best, d[i + W - 1] + 1.4);
        }
        d[i] = best;
      }
    }
    return d;
  }

  // True-colour composite: what a survey satellite would see in high summer.
  // Computed once per world; seasonal snow rides on top as an overlay.
  _buildSatellite() {
    const W = this.w, H = this.h;
    const { height, tmean, precip, fertility, flags } = this.world.arrays;
    const sat = new Float32Array(W * H * 3);
    const cl = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
    const sm = (a, b, v) => { const t = cl((v - a) / (b - a)); return t * t * (3 - 2 * t); };
    const coast = this._coastDistance();
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x, o = i * 3;
        const h = height[i];
        const t = tmean[i];
        let r, g, b;
        if (h < 0) {
          // coastal shelf glows turquoise, the abyss falls to near-black navy
          const depth = Math.pow(cl(-h / 0.85), 0.48);
          const warm = cl((t + 2) / 24);
          r = (26 + warm * 30) * (1 - depth) + 6 * depth;
          g = (102 + warm * 36) * (1 - depth) + 16 * depth;
          b = (116 + warm * 32) * (1 - depth) + 40 * depth;
          const swell = (this._noise(x, y, 11) - 0.5) * 6 * (1 - depth * 0.75);
          r += swell; g += swell; b += swell;
          // M7.1 — atlas vignette: coast-parallel bands ring the shore like
          // the engraved shallows of an old chart, fading over the shelf
          const cd = coast[i];
          const ring = (0.5 + 0.5 * Math.cos(cd * 2.4)) *
                       (1 - sm(1.2, 10, cd)) * sm(0.2, 0.9, cd);
          r += 7.7 * ring; g += 13.3 * ring; b += 15.3 * ring;
        } else if (flags[i] & 4) {
          // dead seas: blinding mineral crusts with a faint aqua bloom
          const n = (this._noise(x, y, 5) - 0.5) * 12;
          r = 202 + n; g = 212 + n; b = 208 + n;
        } else if (flags[i] & 2) {
          const n = (this._noise(x, y, 5) - 0.5) * 7;
          r = 25 + n; g = 57 + n; b = 69 + n;
        } else {
          const moist = cl((precip[i] - 130) / 1050);
          const warm = cl((t + 9) / 27);
          const soil = fertility ? fertility[i] : 0.4;
          const veg = cl(cl(moist * (0.3 + 0.7 * warm)) * 0.8 + soil * 0.3);
          const c = VEG_GRAD(veg);
          r = c[0]; g = c[1]; b = c[2];
          // cold lands grey toward tundra, then pale into the ice sheet
          const chill = cl((4 - t) / 16) * 0.65;
          r += (TUNDRA[0] - r) * chill;
          g += (TUNDRA[1] - g) * chill;
          b += (TUNDRA[2] - b) * chill;
          const frozen = cl((-9 - t) / 9);
          r += (ICE_SHEET[0] - r) * frozen;
          g += (ICE_SHEET[1] - g) * frozen;
          b += (ICE_SHEET[2] - b) * frozen;
          // altitude: bare rock above the treeline, firn on the peaks
          const rock = cl((h - 0.5) / 0.32) * 0.85;
          r += (ROCK[0] - r) * rock;
          g += (ROCK[1] - g) * rock;
          b += (ROCK[2] - b) * rock;
          const firn = cl((h - 0.7) / 0.22) * cl((8 - t) / 18 + 0.2);
          r += (SNOW[0] - r) * firn;
          g += (SNOW[1] - g) * firn;
          b += (SNOW[2] - b) * firn;
          // mottled canopy and field texture, stronger where growth is thick
          const n1 = this._noise(x, y, 6.5) - 0.5;
          const n2 = this._noise(x + 353, y + 127, 2.2) - 0.5;
          const fine = hash2(x, y) - 0.5;
          const m = (n1 * 0.55 + n2 * 0.3 + fine * 0.35) * (5 + veg * 13);
          r += m; g += m * 1.1; b += m * 0.8;
        }
        sat[o] = r; sat[o + 1] = g; sat[o + 2] = b;
      }
    }
    this.sat = sat;
  }

  setRoutes(routes) {
    if (this.world) this.world.header.routes = routes;
    // trade route geometry (cumulative lengths for caravan animation)
    this.routes = (routes || []).map((r, idx) => {
      const pts = r.path;
      const cum = [0];
      for (let i = 1; i < pts.length; i++) {
        const dx = pts[i][0] - pts[i - 1][0];
        const dy = pts[i][1] - pts[i - 1][1];
        cum.push(cum[i - 1] + Math.hypot(dx, dy));
      }
      return { ...r, pts, cum, total: cum[cum.length - 1] || 1, phase: (idx * 0.37) % 1 };
    });
    this._buildDrawPaths();
  }

  // M7.6 — junction-merged, smoothed draw geometry. Every undirected
  // cell-to-cell segment draws once, carrying the sum of the traffic that
  // shares it; chains between junctions become single polylines; two rounds
  // of corner-cutting let roads flow instead of stair-stepping.
  _buildDrawPaths() {
    const segs = new Map(); // "ax,ay|bx,by|mode" -> {a, b, m, w, old}
    for (const r of this.routes) {
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
    this.drawPaths = paths;
  }

  routePoint(route, t) {
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

  monthTemp(month) {
    const m = ((month % 12) + 12) % 12;
    if (this.tmonthCache.has(m)) return this.tmonthCache.get(m);
    const { tmean, tamp } = this.world.arrays;
    const c = Math.cos((2 * Math.PI * m) / 12);
    const out = new Float32Array(tmean.length);
    for (let i = 0; i < tmean.length; i++) out[i] = tmean[i] + tamp[i] * c;
    if (this.tmonthCache.size > 12) this.tmonthCache.clear();
    this.tmonthCache.set(m, out);
    return out;
  }

  // Decode an engine RLE patch ([run, value, run, value, …] row-major)
  // into the live territory grid; the next composite picks it up.
  setTerritory(rle) {
    if (!rle || !rle.length) return;
    const owner = new Int16Array(this.w * this.h).fill(-1);
    let i = 0;
    for (let k = 0; k + 1 < rle.length; k += 2) {
      const run = rle[k], v = rle[k + 1];
      if (v >= 0) owner.fill(v, i, i + run);
      i += run;
    }
    this.territoryOwner = owner;
    this.ownerIsCulture = true;
    this._tintCache = null;
    this.territoryCache = { version: -1, owner: null };
  }

  territory(version) {
    // Engine-authoritative influence map (M4.1) when the pack carries one.
    if (this.territoryOwner) return this.territoryOwner;
    if (this.territoryCache.version === version && this.territoryCache.owner) {
      return this.territoryCache.owner;
    }
    // Fallback for packs older than the politics engine: settlement disks.
    const w = this.w, h = this.h;
    const { biomes } = this.world.arrays;
    const owner = new Int16Array(w * h).fill(-1);
    const dist = new Float32Array(w * h).fill(Infinity);
    for (const s of this.world.header.settlements) {
      const r = (2 + 2.4 * Math.log10(Math.max(s.pop, 10))) * (h / 512) * 2.2;
      const r2 = r * r;
      const x0 = Math.max(0, Math.floor(s.x - r));
      const x1 = Math.min(w - 1, Math.ceil(s.x + r));
      const y0 = Math.max(0, Math.floor(s.y - r));
      const y1 = Math.min(h - 1, Math.ceil(s.y + r));
      for (let y = y0; y <= y1; y++) {
        for (let x = x0; x <= x1; x++) {
          const i = y * w + x;
          if (biomes[i] === 0) continue;
          const d2 = (x - s.x) ** 2 + (y - s.y) ** 2;
          if (d2 <= r2 && d2 < dist[i]) {
            dist[i] = d2;
            owner[i] = s.id;
          }
        }
      }
    }
    this.territoryCache = { version, owner };
    return owner;
  }

  /// Culture that holds cell i, or −1 for wilderness — semantics-safe
  /// across both the engine grid (culture ids) and the fallback (settlement ids).
  ownerCultureAt(i) {
    if (this.territoryOwner) return this.territoryOwner[i];
    const owner = this.territoryCache.owner;
    if (!owner || owner[i] < 0) return -1;
    return this._cultureOf()[owner[i]] ?? -1;
  }

  // political tint as an RGBA texture for the GPU path: culture colour with
  // alpha for interior/edge, and a bright opaque frontier between cultures
  tintRgba(version) {
    if (this._tintCache && this._tintCache.version === version) {
      return this._tintCache.data;
    }
    const W = this.w, H = this.h;
    const owner = this.territory(version);
    const cultOf = this._cultureOf();
    const cRgb = this.cultureRgb;
    const data = new Uint8Array(W * H * 4);
    const asC = this.ownerIsCulture;
    const cidOf = (oo) => (oo >= 0 ? (asC ? oo : (cultOf[oo] ?? 0)) : -1);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x, o = i * 4;
        const ow = owner[i];
        if (ow < 0) continue;
        const cid = cidOf(ow);
        const c = cRgb[cid] || [220, 200, 140];
        const left = x > 0 ? owner[i - 1] : ow;
        const up = y > 0 ? owner[i - W] : ow;
        const right = x < W - 1 ? owner[i + 1] : ow;
        const down = y < H - 1 ? owner[i + W] : ow;
        const settBorder = left !== ow || up !== ow || right !== ow || down !== ow;
        const cultBorder = settBorder && (
          cidOf(left) !== cid || cidOf(up) !== cid ||
          cidOf(right) !== cid || cidOf(down) !== cid);
        if (cultBorder) {
          data[o] = Math.min(255, c[0] * 1.18 + 30);
          data[o + 1] = Math.min(255, c[1] * 1.18 + 30);
          data[o + 2] = Math.min(255, c[2] * 1.18 + 30);
          data[o + 3] = 255;
        } else {
          data[o] = c[0]; data[o + 1] = c[1]; data[o + 2] = c[2];
          data[o + 3] = settBorder ? 128 : 82;
        }
      }
    }
    this._tintCache = { version, data };
    return data;
  }


  _cultureOf() {
    // settlement id -> culture id (settlements can be added by colonisation)
    const map = [];
    for (const s of this.world.header.settlements) map[s.id] = s.culture ?? 0;
    return map;
  }

  cultureColor(s) {
    return this.cultureRgb[s.culture ?? 0] || settlementColor(s.id);
  }

  composite(state) {
    const { layer, overlays, month, version } = state;
    const monthDependent = layer === "temperature" || overlays.snow;
    const key = [
      layer, overlays.rivers, overlays.snow, overlays.hillshade,
      monthDependent ? ((month % 12) + 12) % 12 : "-",
      layer === "political" ? version : "-",
    ].join("|");
    if (key === this.cacheKey) return;
    this.cacheKey = key;

    const W = this.w, H = this.h;
    const { height, tmean, precip, discharge, fertility, biomes, flags, strahler } = this.world.arrays;
    const img = this.octx.createImageData(W, H);
    const px = img.data;
    const shade = this.shade;
    const useShade = overlays.hillshade;
    const tnow = monthDependent ? this.monthTemp(month) : null;
    const owner = layer === "political" ? this.territory(version) : null;
    const dLogMax = this.dischargeLogMax || 1;
    const sat = this.sat;

    let cultOf = null;
    if (owner) cultOf = this._cultureOf();
    const cRgb = this.cultureRgb;

    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x;
        const o = i * 4;
        const h = height[i];
        const sea = h < 0;
        const lake = (flags[i] & 2) !== 0;
        const isWater = sea || lake;
        let r, g, b;

        if (layer === "biomes" || layer === "political") {
          const o3 = i * 3;
          r = sat[o3]; g = sat[o3 + 1]; b = sat[o3 + 2];
          if (layer === "political") {
            // mute the imagery so informational tints read like annotation
            const lum = 0.3 * r + 0.59 * g + 0.11 * b;
            r = (r * 0.52 + lum * 0.48) * 0.84;
            g = (g * 0.52 + lum * 0.48) * 0.84;
            b = (b * 0.52 + lum * 0.48) * 0.84;
            const ow = owner[i];
            if (ow >= 0) {
              const asC = this.ownerIsCulture;
              const cidOf = (oo) => (oo >= 0 ? (asC ? oo : (cultOf[oo] ?? 0)) : -1);
              const cid = cidOf(ow);
              const c = cRgb[cid] || [220, 200, 140];
              const left = x > 0 ? owner[i - 1] : ow;
              const up = y > 0 ? owner[i - W] : ow;
              const right = x < W - 1 ? owner[i + 1] : ow;
              const down = y < H - 1 ? owner[i + W] : ow;
              const settBorder = left !== ow || up !== ow || right !== ow || down !== ow;
              const cultBorder = settBorder && (
                cidOf(left) !== cid || cidOf(up) !== cid ||
                cidOf(right) !== cid || cidOf(down) !== cid);
              if (cultBorder) {
                // a crisp bright frontier, like a boundary drawn on imagery
                r = Math.min(255, c[0] * 1.18 + 30);
                g = Math.min(255, c[1] * 1.18 + 30);
                b = Math.min(255, c[2] * 1.18 + 30);
              } else {
                const a = settBorder ? 0.5 : 0.32;
                r = r * (1 - a) + c[0] * a;
                g = g * (1 - a) + c[1] * a;
                b = b * (1 - a) + c[2] * a;
              }
            }
          }
        } else if (layer === "elevation") {
          if (sea) {
            const t = Math.min(1, -h / 0.75);
            const c = SEA_GRAD(t);
            r = c[0] * 0.9; g = c[1] * 0.95; b = c[2];
          } else if (flags[i] & 4) {
            r = 198; g = 202; b = 196;
          } else if (lake) {
            r = 74; g = 128; b = 168;
          } else {
            // M7.4 — climate-blended hypsometry: wet country climbs through
            // green, dry country through ochre, frozen lands grey to firn
            const gc = ELEV_LAND_GRAD(h);
            const ac = ELEV_ARID_GRAD(h);
            const arid = 1 - Math.min(1, Math.max(0, (precip[i] - 240) / 700));
            r = gc[0] + (ac[0] - gc[0]) * arid;
            g = gc[1] + (ac[1] - gc[1]) * arid;
            b = gc[2] + (ac[2] - gc[2]) * arid;
            const chill = Math.min(1, Math.max(0, (-2 - tmean[i]) / 14)) * 0.85;
            const h01 = Math.min(1, Math.max(0, h));
            r += (153 + 84 * h01 - r) * chill;
            g += (163 + 77 * h01 - g) * chill;
            b += (176 + 69 * h01 - b) * chill;
          }
        } else if (layer === "temperature") {
          [r, g, b] = TEMP_GRAD(tnow[i]);
          if (sea) { r *= 0.82; g *= 0.85; b *= 0.9; }
        } else if (layer === "precip") {
          if (sea) { r = 22; g = 39; b = 63; }
          else [r, g, b] = PRECIP_GRAD(precip[i]);
        } else if (layer === "fertility") {
          if (sea) { r = 20; g = 33; b = 52; }
          else if (lake) { r = 46; g = 95; b = 143; }
          else [r, g, b] = FERT_GRAD(fertility ? fertility[i] : 0);
        } else { // hydro
          if (sea) { r = 14; g = 28; b = 48; }
          else if (lake) { r = 46; g = 95; b = 143; }
          else {
            const t = Math.log1p(discharge[i]) / dLogMax;
            if (t > 0.42) [r, g, b] = HYDRO_GRAD((t - 0.42) / 0.58);
            else {
              const s = this.shade[i];
              r = 19 * s; g = 26 * s; b = 36 * s;
            }
          }
        }

        // hillshade on land
        if (useShade && !isWater && layer !== "hydro") {
          const s = layer === "temperature" || layer === "precip" || layer === "fertility"
            ? 1 + (shade[i] - 1) * 0.45
            : shade[i];
          r *= s; g *= s; b *= s;
        }

        // rivers overlay — weight follows Strahler order; wadis run pale
        if (overlays.rivers && (flags[i] & 1) && layer !== "hydro") {
          const ord = strahler ? strahler[i] : 1;
          const sw = 0.35 + 0.65 * Math.min(1, (ord - 1) / 6);
          const t = Math.log1p(discharge[i]) / dLogMax;
          let a = Math.min(0.85, 0.3 + t * 0.4 + sw * 0.25);
          let cr = 62, cg = 124, cb = 186;
          if (flags[i] & 8) { a *= 0.55; cr = 122; cg = 152; cb = 178; }
          r = r * (1 - a) + cr * a;
          g = g * (1 - a) + cg * a;
          b = b * (1 - a) + cb * a;
        }

        // snow & sea ice overlay
        if (overlays.snow && layer !== "temperature") {
          const t = (tnow || this.monthTemp(month))[i];
          if (!isWater && t < -1) {
            const a = Math.min(1, (-1 - t) / 6) * 0.85;
            r = r * (1 - a) + 240 * a;
            g = g * (1 - a) + 245 * a;
            b = b * (1 - a) + 250 * a;
          } else if (isWater && t < -2) {
            const a = Math.min(1, (-2 - t) / 8) * 0.9;
            r = r * (1 - a) + 216 * a;
            g = g * (1 - a) + 229 * a;
            b = b * (1 - a) + 240 * a;
          }
        }

        px[o] = r; px[o + 1] = g; px[o + 2] = b; px[o + 3] = 255;
      }
    }
    this.octx.putImageData(img, 0, 0);
  }

  draw(state, view, hover) {
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth;
    const hgt = this.canvas.clientHeight;
    if (this.canvas.width !== (w * dpr) | 0 || this.canvas.height !== (hgt * dpr) | 0) {
      this.canvas.width = (w * dpr) | 0;
      this.canvas.height = (hgt * dpr) | 0;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const gpuOn = !!(this.gpu && this.gpu.hasWorld);
    if (gpuOn) {
      // the wgpu canvas beneath carries the imagery — keep this layer clear
      ctx.clearRect(0, 0, w, hgt);
    } else {
      ctx.fillStyle = "#05080f";
      ctx.fillRect(0, 0, w, hgt);
    }
    if (!this.world) return;

    if (!gpuOn) {
      this.composite(state);
      ctx.save();
      ctx.imageSmoothingEnabled = false;
      ctx.translate(view.tx, view.ty);
      ctx.scale(view.scale, view.scale);
      ctx.drawImage(this.off, 0, 0);
      ctx.restore();
    }

    // a breath of atmosphere: a faint rim on the world, vignette in the void
    ctx.save();
    ctx.shadowColor = "rgba(96, 150, 212, 0.22)";
    ctx.shadowBlur = 12;
    ctx.strokeStyle = "rgba(128, 172, 222, 0.22)";
    ctx.lineWidth = 1;
    ctx.strokeRect(view.tx - 0.5, view.ty - 0.5, this.w * view.scale + 1, this.h * view.scale + 1);
    ctx.restore();
    const vg = ctx.createRadialGradient(
      w / 2, hgt / 2, Math.min(w, hgt) * 0.42,
      w / 2, hgt / 2, Math.hypot(w, hgt) * 0.62);
    vg.addColorStop(0, "rgba(0,0,0,0)");
    vg.addColorStop(1, "rgba(2, 6, 13, 0.5)");
    ctx.fillStyle = vg;
    ctx.fillRect(0, 0, w, hgt);

    // vector overlays in screen space — dots first, then one unified label
    // pass so every name on the map is placed collision-free (M7.2)
    if (state.overlays.winds) this._drawWinds(ctx, view, state);
    if (state.overlays.routes) this._drawRoutes(ctx, view, state);
    if (state.overlays.resources) this._drawDeposits(ctx, view);
    if (state.overlays.settlements) this._drawSettlements(ctx, view, state);
    this._drawLabels(ctx, view, state);
    this._drawScaleBar(ctx, view);
    if (state.selectedCell) this._drawSelectedCell(ctx, view, state.selectedCell);
    if (hover && view.scale > 4) this._drawHover(ctx, view, hover);
  }

  _drawWinds(ctx, view, state) {
    const H = this.h, W = this.w;
    const s = view.scale;
    const w = this.canvas.clientWidth;
    const drift = state.playing ? (performance.now() / 1000 * 26) : 0;
    ctx.save();
    ctx.lineWidth = 1.3;
    ctx.lineCap = "round";
    const rowStep = Math.max(18, 30 / Math.max(s / 2, 1));
    for (let wy = rowStep / 2; wy < H; wy += rowStep) {
      const lat = Math.abs((wy / H) * 180 - 90);
      const dir = lat < 30 ? -1 : lat < 60 ? 1 : -1; // grid x direction of travel
      const py = view.ty + wy * s;
      if (py < -20 || py > this.canvas.clientHeight + 20) continue;
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

  _drawRoutes(ctx, view, state) {
    const s = view.scale;
    ctx.save();
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    // M7.6 — the merged, smoothed network: shared trunks draw once, wider
    // with the traffic they carry, and chains flow instead of stair-stepping
    for (const p of this.drawPaths || []) {
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
    // caravans on the roads, sails on the lanes, while time flows
    if (state.playing) {
      const now = performance.now() / 1000;
      for (const r of this.routes) {
        if (r.old) continue; // no caravan takes the disused ways
        const t = ((now / 14 + r.phase) % 1);
        const tt = t < 0.5 ? t * 2 : (1 - t) * 2; // there and back again
        const [wx, wy, mode] = this.routePoint(r, tt);
        const px = view.tx + (wx + 0.5) * s;
        const py = view.ty + (wy + 0.5) * s;
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
    }
    ctx.restore();
  }

  _drawDeposits(ctx, view) {
    const meta = this.world.header.resources;
    const s = view.scale;
    const rad = Math.max(2.2, Math.min(6, s * 0.45));
    for (const d of this.world.header.deposits) {
      const sx = view.tx + (d.x + 0.5) * s;
      const sy = view.ty + (d.y + 0.5) * s;
      if (sx < -10 || sy < -10 || sx > this.canvas.clientWidth + 10 || sy > this.canvas.clientHeight + 10) continue;
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

  _labelAlpha(kind, s) {
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

  // ---- the unified label engine (M7.2/M7.3/M7.7) ---------------------------
  // One placement pass for every name on the map: feature labels and
  // settlement names compete for the same ground, mighty-to-minor, and
  // nothing overlaps — ever. Settlement label density follows Töpfer's
  // radical law: at scale s the map keeps N·√(s/S_full) of the names it
  // would carry fully zoomed in.

  _labelBudget(total, s) {
    const S_FULL = 6;
    if (s >= S_FULL) return total;
    return Math.max(4, Math.ceil(total * Math.sqrt(Math.max(s, 0.12) / S_FULL)));
  }

  // M7.7 — trace the river's course around a label anchor so the name can
  // ride the water. Walks the channel both ways along the discharge slope.
  _riverPath(f) {
    this._riverPaths ??= new Map();
    const ck = f.x + "," + f.y;
    if (this._riverPaths.has(ck)) return this._riverPaths.get(ck);
    const { flags, discharge } = this.world.arrays;
    const W = this.w, H = this.h;
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
    if (cx < 0) { this._riverPaths.set(ck, null); return null; }
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
    if (pts.length < 6) { this._riverPaths.set(ck, null); return null; }
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
    this._riverPaths.set(ck, pts);
    return pts;
  }

  // Lay text along a world-space polyline, centred on its arc. Returns the
  // screen bbox it covered, or null when the course is too short.
  _drawCurvedText(ctx, view, pts, text, color, alpha, dryRun) {
    const s = view.scale;
    const sp = pts.map(([x, y]) => [view.tx + x * s, view.ty + y * s]);
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
    if (dryRun) return { x0, y0, x1, y1, glyphs };
    ctx.globalAlpha = alpha;
    for (const [ch, gx, gy, ang] of glyphs) {
      ctx.save();
      ctx.translate(gx, gy);
      ctx.rotate(ang);
      ctx.strokeStyle = "rgba(4, 8, 15, 0.7)";
      ctx.lineWidth = 2.6;
      ctx.strokeText(ch, 0, -3);
      ctx.fillStyle = color;
      ctx.fillText(ch, 0, -3);
      ctx.restore();
    }
    ctx.globalAlpha = 1;
    return { x0, y0, x1, y1 };
  }

  _drawLabels(ctx, view, state) {
    const s = view.scale;
    const w = this.canvas.clientWidth;
    const hgt = this.canvas.clientHeight;
    this.labelBoxes = [];
    const placed = [];
    const stats = { scale: s, candidates: 0, placed: 0, overlaps: 0,
                    setBudget: 0, setPlaced: 0, featPlaced: 0, curved: 0 };

    ctx.save();
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.lineJoin = "round";

    const cands = [];

    // -- feature names ------------------------------------------------------
    if (state.overlays.labels) {
      const feats = this.world.header.features || [];
      for (let fi = 0; fi < feats.length; fi++) {
        const f = feats[fi];
        const st = LABEL_STYLE[f.t];
        if (!st) continue;
        const alpha = this._labelAlpha(f.t, s);
        if (alpha <= 0.03) continue;
        const px = view.tx + f.x * s;
        const py = view.ty + f.y * s;
        if (px < -280 || py < -60 || px > w + 280 || py > hgt + 60) continue;

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
      const sets = [...this.world.header.settlements].sort(
        (a, b) => (b.pop - a.pop) || (a.id - b.id));
      stats.setBudget = this._labelBudget(sets.length, s);
      let taken = 0;
      for (const st of sets) {
        if (taken >= stats.setBudget) break;
        const px = view.tx + (st.x + 0.5) * s;
        const py = view.ty + (st.y + 0.5) * s;
        if (px < -80 || py < -40 || px > w + 80 || py > hgt + 40) continue;
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
        for (const ru of this.world.header.ruins || []) {
          const px = view.tx + (ru.x + 0.5) * s;
          const py = view.ty + (ru.y + 0.5) * s;
          if (px < -80 || py < -40 || px > w + 80 || py > hgt + 40) continue;
          cands.push({ type: "ruin", ru, px, py, pri: 8.6, mass: 1, alpha: ra });
        }
      }
    }

    stats.candidates = cands.length;
    cands.sort((a, b) => (a.pri - b.pri) || (b.mass - a.mass));

    const collides = (box) =>
      placed.some((b) => box.x0 < b.x1 && box.x1 > b.x0 && box.y0 < b.y1 && box.y1 > b.y0);

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
        ctx.textBaseline = "alphabetic";
        ctx.lineWidth = 3;
        ctx.strokeStyle = "rgba(8,12,20,0.85)";
        ctx.strokeText(name, c.px, c.py - c.r - 5);
        ctx.fillStyle = "#f0ead8";
        ctx.fillText(name, c.px, c.py - c.r - 5);
        if (s > 3) {
          const pop = c.st.pop.toLocaleString("en-US");
          ctx.font = "500 9.5px Inter, sans-serif";
          ctx.strokeText(pop, c.px, c.py + c.r + 11);
          ctx.fillStyle = "#b9c0cf";
          ctx.fillText(pop, c.px, c.py + c.r + 11);
        }
        ctx.textBaseline = "middle";
        continue;
      }

      if (c.type === "ruin") {
        ctx.font = "italic 500 10px Inter, sans-serif";
        const tw = ctx.measureText(c.ru.name).width + 8;
        const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: c.py - 18, y1: c.py + 5 };
        if (collides(box)) continue;
        placed.push(box);
        ctx.textBaseline = "alphabetic";
        ctx.globalAlpha = c.alpha;
        ctx.lineWidth = 2.4;
        ctx.strokeStyle = "rgba(8,12,20,0.8)";
        ctx.strokeText(c.ru.name, c.px, c.py - 7);
        ctx.fillStyle = "#b7b1a2";
        ctx.fillText(c.ru.name, c.px, c.py - 7);
        ctx.globalAlpha = 1;
        ctx.textBaseline = "middle";
        continue;
      }

      // -- feature label ----------------------------------------------------
      ctx.font = c.font;

      // M7.7 — river names ride their water at close zoom
      if (c.f.t === "river" && s >= 3.2) {
        const path = this._riverPath(c.f);
        if (path) {
          const dry = this._drawCurvedText(ctx, view, path, c.text, c.st.color, c.alpha, true);
          if (dry && !collides(dry)) {
            this._drawCurvedText(ctx, view, path, c.text, c.st.color, c.alpha, false);
            placed.push(dry);
            this.labelBoxes.push({ x0: dry.x0, x1: dry.x1, y0: dry.y0, y1: dry.y1, index: c.fi });
            stats.featPlaced++;
            stats.curved++;
            continue;
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
      const th = c.size + 9;
      const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: c.py - th / 2, y1: c.py + th / 2 };
      if (collides(box)) { ctx.letterSpacing = "0px"; continue; }
      placed.push(box);
      this.labelBoxes.push({ ...box, index: c.fi });
      stats.featPlaced++;
      ctx.globalAlpha = c.alpha;
      ctx.strokeStyle = "rgba(4, 8, 15, 0.7)";
      ctx.lineWidth = 2.6;
      ctx.strokeText(c.text, c.px, c.py);
      ctx.fillStyle = c.st.color;
      ctx.fillText(c.text, c.px, c.py);
      ctx.letterSpacing = "0px";
    }

    ctx.globalAlpha = 1;
    ctx.restore();

    // the gate's evidence: recheck every placed box against every other
    for (let i = 0; i < placed.length; i++) {
      for (let j = i + 1; j < placed.length; j++) {
        const a = placed[i], b = placed[j];
        if (a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0) stats.overlaps++;
      }
    }
    stats.placed = placed.length;
    this.labelStatsData = stats;
  }

  labelStats() {
    return this.labelStatsData || null;
  }

  // M9.1 — what remains: three walls standing, the fourth long fallen.
  _drawRuins(ctx, view, state) {
    const ruins = this.world.header.ruins || [];
    if (!ruins.length) return;
    const s = view.scale;
    const a = Math.max(0, Math.min(0.85, (s - 1.1) * 0.45));
    if (a <= 0.02) return;
    const W = this.canvas.clientWidth, H = this.canvas.clientHeight;
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

  _drawSettlements(ctx, view, state) {
    const s = view.scale;
    this._drawRuins(ctx, view, state);
    for (const st of this.world.header.settlements) {
      const sx = view.tx + (st.x + 0.5) * s;
      const sy = view.ty + (st.y + 0.5) * s;
      if (sx < -60 || sy < -30 || sx > this.canvas.clientWidth + 60 || sy > this.canvas.clientHeight + 30) continue;
      const r = TIER_RADIUS[st.tier] || 3;
      const [cr, cg, cb] = this.cultureColor(st);
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

  _drawHover(ctx, view, hover) {
    const s = view.scale;
    ctx.strokeStyle = "rgba(255,255,255,0.7)";
    ctx.lineWidth = 1.2;
    ctx.strokeRect(view.tx + hover.x * s, view.ty + hover.y * s, s, s);
  }

  // The inspected cell: corner ticks, calmer than a full box.
  _drawSelectedCell(ctx, view, cell) {
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
  _drawScaleBar(ctx, view) {
    if (!this.world) return;
    const kmPer = this.world.header.km_per_cell || 4;
    const nice = [10, 20, 50, 100, 200, 500, 1000, 2000, 5000];
    let km = 0;
    for (const n of nice) {
      if ((n / kmPer) * view.scale <= 150) km = n;
    }
    if (!km) return;
    const px = (km / kmPer) * view.scale;
    const mobile = window.matchMedia("(max-width: 760px)").matches;
    const x = (this.canvas.clientWidth - px) / 2;
    const y = this.canvas.clientHeight - (mobile ? 92 : 16);
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
}
