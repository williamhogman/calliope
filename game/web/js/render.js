// Renderer: composites the world into an offscreen canvas, draws to screen.

import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, SEA_GRAD, HYDRO_GRAD, FERT_GRAD,
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

    // biome palette by id
    this.biomePal = [];
    for (const b of world.header.biomes) this.biomePal[b.id] = b.color;

    // culture colors
    this.cultureRgb = (world.header.cultures || []).map((c) => hexRgb(c.color));

    // hillshade (light from NW)
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
        const dzdx = (h[yr + x1] - h[yr + x0]) * 0.5;
        const dzdy = (h[y1 + x] - h[y0 + x]) * 0.5;
        let s = 1 + k * (-dzdx - dzdy) * 0.9;
        sh[yr + x] = s < 0.6 ? 0.6 : s > 1.32 ? 1.32 : s;
      }
    }
    this.shade = sh;

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

  // True-colour composite: what a survey satellite would see in high summer.
  // Computed once per world; seasonal snow rides on top as an overlay.
  _buildSatellite() {
    const W = this.w, H = this.h;
    const { height, tmean, precip, fertility, flags } = this.world.arrays;
    const sat = new Float32Array(W * H * 3);
    const cl = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
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

  territory(version) {
    if (this.territoryCache.version === version && this.territoryCache.owner) {
      return this.territoryCache.owner;
    }
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
    const cidOf = (oo) => (oo >= 0 ? (cultOf[oo] ?? 0) : -1);
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x, o = i * 4;
        const ow = owner[i];
        if (ow < 0) continue;
        const cid = cultOf[ow] ?? 0;
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
              const cid = cultOf[ow] ?? 0;
              const c = cRgb[cid] || [220, 200, 140];
              const left = x > 0 ? owner[i - 1] : ow;
              const up = y > 0 ? owner[i - W] : ow;
              const right = x < W - 1 ? owner[i + 1] : ow;
              const down = y < H - 1 ? owner[i + W] : ow;
              const settBorder = left !== ow || up !== ow || right !== ow || down !== ow;
              const cidOf = (oo) => (oo >= 0 ? (cultOf[oo] ?? 0) : -1);
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
            [r, g, b] = ELEV_LAND_GRAD(h);
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

    // vector overlays in screen space
    if (state.overlays.winds) this._drawWinds(ctx, view, state);
    if (state.overlays.routes) this._drawRoutes(ctx, view, state);
    if (state.overlays.resources) this._drawDeposits(ctx, view);
    if (state.overlays.labels) this._drawLabels(ctx, view);
    else this.labelBoxes = [];
    if (state.overlays.settlements) this._drawSettlements(ctx, view, state);
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
    for (const r of this.routes) {
      const wgt = r.w || 1;
      const lw = Math.min(2.4, Math.max(0.8, s * 0.11 * wgt + 0.4));
      const alpha = Math.min(0.68, 0.34 + wgt * 0.16);
      const m = r.m || [];
      const pts = r.pts;
      // draw runs of the same travel mode: road, sea lane, or river barge
      let i = 1;
      while (i < pts.length) {
        const mode = m[i] ?? 0;
        let j = i;
        while (j + 1 < pts.length && (m[j + 1] ?? 0) === mode) j++;
        if (mode === 1) {
          ctx.setLineDash([7, 6]);
          ctx.strokeStyle = `rgba(126, 178, 226, ${alpha})`;
        } else if (mode === 2) {
          ctx.setLineDash([2, 4]);
          ctx.strokeStyle = `rgba(118, 204, 214, ${alpha})`;
        } else {
          ctx.setLineDash([]);
          ctx.strokeStyle = `rgba(224, 196, 140, ${alpha})`;
        }
        ctx.lineWidth = lw;
        ctx.beginPath();
        for (let k = i - 1; k <= j; k++) {
          const px = view.tx + (pts[k][0] + 0.5) * s;
          const py = view.ty + (pts[k][1] + 0.5) * s;
          if (k === i - 1) ctx.moveTo(px, py); else ctx.lineTo(px, py);
        }
        ctx.stroke();
        i = j + 1;
      }
    }
    ctx.setLineDash([]);
    // caravans on the roads, sails on the lanes, while time flows
    if (state.playing) {
      const now = performance.now() / 1000;
      for (const r of this.routes) {
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

  _drawLabels(ctx, view) {
    const feats = this.world.header.features || [];
    this.labelBoxes = [];
    if (!feats.length) return;
    const s = view.scale;
    const w = this.canvas.clientWidth;
    const hgt = this.canvas.clientHeight;
    ctx.save();
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.lineJoin = "round";

    // gather visible candidates, then place mighty-to-minor so nothing collides
    const cands = [];
    for (let fi = 0; fi < feats.length; fi++) {
      const f = feats[fi];
      const st = LABEL_STYLE[f.t];
      if (!st) continue;
      const alpha = this._labelAlpha(f.t, s);
      if (alpha <= 0.03) continue;
      const px = view.tx + f.x * s;
      const py = view.ty + f.y * s;
      if (px < -240 || py < -60 || px > w + 240 || py > hgt + 60) continue;

      let font, size, spacing, text = f.name;
      const lg = Math.log2(Math.max(f.size, 2));
      if (f.t === "ocean" || f.t === "sea") {
        size = Math.min(21, 8.5 + lg * 0.85);
        font = `500 ${size.toFixed(1)}px Inter, sans-serif`;
        text = f.name.toUpperCase();
        spacing = 4;
      } else if (f.t === "continent") {
        size = Math.min(20, 8 + lg * 0.8);
        font = `600 ${size.toFixed(1)}px Inter, sans-serif`;
        text = f.name.toUpperCase();
        spacing = 5;
      } else if (f.t === "range" || f.t === "desert" || f.t === "forest" ||
                 f.t === "highland" || f.t === "archipelago") {
        size = 11;
        font = `600 11px Inter, sans-serif`;
        text = f.name.toUpperCase();
        spacing = 1.8;
      } else {
        size = 10.5;
        font = `italic 500 10.5px Inter, sans-serif`;
        spacing = 0.6;
      }
      cands.push({ f, fi, st, alpha, px, py, font, size, spacing, text });
    }
    cands.sort((a, b) => (a.st.pri - b.st.pri) || (b.f.size - a.f.size));

    const placed = [];
    for (const c of cands) {
      ctx.font = c.font;
      ctx.letterSpacing = `${c.spacing}px`;
      const tw = ctx.measureText(c.text).width + 10;
      const th = c.size + 9;
      const box = { x0: c.px - tw / 2, x1: c.px + tw / 2, y0: c.py - th / 2, y1: c.py + th / 2 };
      if (placed.some((b) => box.x0 < b.x1 && box.x1 > b.x0 && box.y0 < b.y1 && box.y1 > b.y0)) {
        ctx.letterSpacing = "0px";
        continue; // a mightier name already holds this ground
      }
      placed.push(box);
      this.labelBoxes.push({ ...box, index: c.fi }); // clickable names
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
  }

  _drawSettlements(ctx, view, state) {
    const s = view.scale;
    const showAllLabels = s > 1.6;
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
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

      const isBig = st.tier === "Town" || st.tier === "City";
      if (showAllLabels || isBig) {
        ctx.font = `600 ${isBig ? 12 : 11}px Inter, sans-serif`;
        ctx.lineWidth = 3;
        ctx.strokeStyle = "rgba(8,12,20,0.85)";
        ctx.lineJoin = "round";
        ctx.strokeText(st.name, sx, sy - r - 5);
        ctx.fillStyle = "#f0ead8";
        ctx.fillText(st.name, sx, sy - r - 5);
        if (s > 3) {
          const pop = st.pop.toLocaleString("en-US");
          ctx.font = "500 9.5px Inter, sans-serif";
          ctx.strokeText(pop, sx, sy + r + 11);
          ctx.fillStyle = "#b9c0cf";
          ctx.fillText(pop, sx, sy + r + 11);
        }
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
