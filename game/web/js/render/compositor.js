// Compositor (E9.4): everything that turns world fields into pixels on the
// CPU path, plus the political territory/tint machinery both paths share.
//
// Damage model (E9.2/E9.9): territory patches record the row band they
// touched. The political composite and the GPU tint refresh only that band,
// and a tick that changes no territory costs neither a rebuild nor an
// upload — `tintEpoch` only advances when tint content actually moved.

import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, ELEV_ARID_GRAD, SEA_GRAD,
  HYDRO_GRAD, FERT_GRAD, gradient, hash2,
} from "../palette.js";
// M63 — deep-earth lens swatches, generated from the Rust atlas tables:
// the CPU fallback paints with the same colors the GPU palette texture holds.
import { ROCKS, SOILS, LANDFORM_COLORS } from "../gen/constants.js";

const ROCK_RGB = ROCKS.map((r) => r.color);
const SOIL_RGB = SOILS.map((s) => s.color);

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

// smooth value noise on the world grid — bilinear hash, for imagery texture
function noise2(x, y, sc) {
  const gx = x / sc, gy = y / sc;
  const x0 = Math.floor(gx), y0 = Math.floor(gy);
  const fx = gx - x0, fy = gy - y0;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  const n00 = hash2(x0, y0), n10 = hash2(x0 + 1, y0);
  const n01 = hash2(x0, y0 + 1), n11 = hash2(x0 + 1, y0 + 1);
  return n00 + (n10 - n00) * sx + (n01 - n00) * sy + (n00 - n10 - n01 + n11) * sx * sy;
}

// M7.5 — multi-directional oblique-weighted hillshade with a curvature
// accent (texture shading), precomputed once per world: four low suns
// so ridges running every direction carve; the Laplacian etches
// ridgelines bright and valley floors dark.
export function buildShade(R) {
  const w = R.w, hh = R.h;
  const h = R.world.arrays.height;
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
  R.shade = sh;
}

// Chamfer distance (cells) from every sea cell to the nearest land —
// powers the engraved coastal vignette (M7.1) on the CPU path.
function coastDistance(R) {
  const W = R.w, H = R.h;
  const hgt = R.world.arrays.height;
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
export function buildSatellite(R) {
  const W = R.w, H = R.h;
  const { height, tmean, precip, fertility, flags } = R.world.arrays;
  const sat = new Float32Array(W * H * 3);
  const cl = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
  const sm = (a, b, v) => { const t = cl((v - a) / (b - a)); return t * t * (3 - 2 * t); };
  const coast = coastDistance(R);
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
        const swell = (noise2(x, y, 11) - 0.5) * 6 * (1 - depth * 0.75);
        r += swell; g += swell; b += swell;
        // M7.1 — atlas vignette: coast-parallel bands ring the shore like
        // the engraved shallows of an old chart, fading over the shelf
        const cd = coast[i];
        const ring = (0.5 + 0.5 * Math.cos(cd * 2.4)) *
                     (1 - sm(1.2, 10, cd)) * sm(0.2, 0.9, cd);
        r += 7.7 * ring; g += 13.3 * ring; b += 15.3 * ring;
      } else if (flags[i] & 4) {
        // dead seas: blinding mineral crusts with a faint aqua bloom
        const n = (noise2(x, y, 5) - 0.5) * 12;
        r = 202 + n; g = 212 + n; b = 208 + n;
      } else if (flags[i] & 2) {
        const n = (noise2(x, y, 5) - 0.5) * 7;
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
        const n1 = noise2(x, y, 6.5) - 0.5;
        const n2 = noise2(x + 353, y + 127, 2.2) - 0.5;
        const fine = hash2(x, y) - 0.5;
        const m = (n1 * 0.55 + n2 * 0.3 + fine * 0.35) * (5 + veg * 13);
        r += m; g += m * 1.1; b += m * 0.8;
      }
      sat[o] = r; sat[o + 1] = g; sat[o + 2] = b;
    }
  }
  R.sat = sat;
}

export function monthTemp(R, month) {
  const m = ((month % 12) + 12) % 12;
  if (R.tmonthCache.has(m)) return R.tmonthCache.get(m);
  const { tmean, tamp } = R.world.arrays;
  const c = Math.cos((2 * Math.PI * m) / 12);
  const out = new Float32Array(tmean.length);
  for (let i = 0; i < tmean.length; i++) out[i] = tmean[i] + tamp[i] * c;
  if (R.tmonthCache.size > 12) R.tmonthCache.clear();
  R.tmonthCache.set(m, out);
  return out;
}

// ---------- territory & political tint --------------------------------------

// Decode an engine RLE patch ([run, value, run, value, …] row-major) into the
// live territory grid, recording the row band that actually changed (E9.2).
export function decodeTerritory(R, rle) {
  if (!rle || !rle.length) return;
  const owner = new Int16Array(R.w * R.h).fill(-1);
  let i = 0;
  for (let k = 0; k + 1 < rle.length; k += 2) {
    const run = rle[k], v = rle[k + 1];
    if (v >= 0) owner.fill(v, i, i + run);
    i += run;
  }
  const prev = R.ownerIsRealm ? R.territoryOwner : null;
  let rows = { y0: 0, y1: R.h }; // no previous grid: the whole map is the band
  if (prev && prev.length === owner.length) {
    let lo = -1, hi = -1;
    for (let j = 0; j < owner.length; j++) {
      if (owner[j] !== prev[j]) { lo = j; break; }
    }
    if (lo < 0) {
      // borders held exactly — nothing to rebuild, nothing to upload
      R.territoryOwner = owner;
      return;
    }
    for (let j = owner.length - 1; j >= lo; j--) {
      if (owner[j] !== prev[j]) { hi = j; break; }
    }
    // border shading reads one row around a cell — widen the band by one
    const y0 = Math.max(0, Math.floor(lo / R.w) - 1);
    const y1 = Math.min(R.h, Math.floor(hi / R.w) + 2);
    rows = { y0, y1 };
  }
  R.territoryOwner = owner;
  R.ownerIsRealm = true;
  R.tintEpoch++;
  R._polDirty = true;
  R._compRows = mergeRows(R._compRows, rows);
  R._tintRows = mergeRows(R._tintRows, rows);
}

// E4.7 — apply dirty 32×32 tile patches straight into the live grid.
// The damage band falls out of the tile coords: no full-grid diff, and
// the tint/composite rebuilds stay confined to the touched rows (E9.2).
export function applyTerritoryTiles(R, patch) {
  const owner = R.ownerIsRealm ? R.territoryOwner : null;
  if (!owner || !patch || !patch.tiles || !patch.tiles.length) return;
  const W = R.w, H = R.h, tw = patch.tw || 32;
  let lo = H, hi = 0;
  for (const [tx, ty, rle] of patch.tiles) {
    const x0 = tx * tw, y0 = ty * tw;
    const tW = Math.min(tw, W - x0), tH = Math.min(tw, H - y0);
    let j = 0;
    for (let k = 0; k + 1 < rle.length; k += 2) {
      let run = rle[k];
      const v = rle[k + 1];
      while (run-- > 0 && j < tW * tH) {
        owner[(y0 + ((j / tW) | 0)) * W + x0 + (j % tW)] = v;
        j++;
      }
    }
    if (y0 < lo) lo = y0;
    if (y0 + tH > hi) hi = y0 + tH;
  }
  // border shading reads one row around a cell — widen the band by one
  const rows = { y0: Math.max(0, lo - 1), y1: Math.min(H, hi + 1) };
  R.tintEpoch++;
  R._polDirty = true;
  R._compRows = mergeRows(R._compRows, rows);
  R._tintRows = mergeRows(R._tintRows, rows);
}

// undefined = nothing pending; otherwise a row band (full map = {0, H}).
function mergeRows(cur, add) {
  if (!cur) return add;
  return { y0: Math.min(cur.y0, add.y0), y1: Math.max(cur.y1, add.y1) };
}

export function territoryGrid(R, version) {
  // Engine-authoritative influence map (M4.1) when the pack carries one.
  if (R.territoryOwner) return R.territoryOwner;
  if (R.territoryCache.version === version && R.territoryCache.owner) {
    return R.territoryCache.owner;
  }
  // Fallback for packs older than the politics engine: settlement disks.
  const w = R.w, h = R.h;
  const { biomes } = R.world.arrays;
  const owner = new Int16Array(w * h).fill(-1);
  const dist = new Float32Array(w * h).fill(Infinity);
  for (const s of R.world.header.settlements) {
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
  R.territoryCache = { version, owner };
  return owner;
}

export function realmOfSettlement(R) {
  // settlement id -> realm id (the pre-politics fallback grid stores
  // settlement ids; the banner they fly resolves through this map)
  const map = [];
  for (const s of R.world.header.settlements) map[s.id] = s.realm ?? 0;
  return map;
}

// ---------- the people-axis influence grid (M10.6) --------------------------

// Decode the peoples_map RLE ([run, value, …] row-major) into a live grid.
// It moves on generational clocks (assimilation, divergence, merging), so a
// whole-grid decode per arrival is cheap and no damage bands are needed.
export function decodePeoples(R, rle) {
  if (!rle || !rle.length) { return; }
  const owner = new Int16Array(R.w * R.h).fill(-1);
  let i = 0;
  for (let k = 0; k + 1 < rle.length; k += 2) {
    const run = rle[k], v = rle[k + 1];
    if (v >= 0) owner.fill(v, i, i + run);
    i += run;
  }
  R.peoplesOwner = owner;
  R.peoplesEpoch = (R.peoplesEpoch || 0) + 1;
  R._pTintCache = null;
}

// People-axis tint texture for the GPU path: people colour, quiet interior,
// bright frontier where two tongues meet. Cached per peoplesEpoch — the
// grid reships a few times a century, so a full rebuild is the simple truth.
export function peopleTintRgba(R) {
  if (R._pTintCache && R._pTintCache.epoch === R.peoplesEpoch) return R._pTintCache.data;
  const W = R.w, H = R.h;
  const data = new Uint8Array(W * H * 4);
  const owner = R.peoplesOwner;
  if (owner) {
    const rgb = R.peopleRgb || [];
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const i = y * W + x, o = i * 4;
        const ow = owner[i];
        if (ow < 0) continue;
        const c = rgb[ow] || [220, 200, 140];
        const left = x > 0 ? owner[i - 1] : ow;
        const up = y > 0 ? owner[i - W] : ow;
        const right = x < W - 1 ? owner[i + 1] : ow;
        const down = y < H - 1 ? owner[i + W] : ow;
        if (left !== ow || up !== ow || right !== ow || down !== ow) {
          data[o] = Math.min(255, c[0] * 1.18 + 30);
          data[o + 1] = Math.min(255, c[1] * 1.18 + 30);
          data[o + 2] = Math.min(255, c[2] * 1.18 + 30);
          data[o + 3] = 255;
        } else {
          data[o] = c[0]; data[o + 1] = c[1]; data[o + 2] = c[2];
          data[o + 3] = 82;
        }
      }
    }
  }
  R._pTintCache = { epoch: R.peoplesEpoch, data };
  return data;
}

// political tint as an RGBA texture for the GPU path: realm colour with
// alpha for interior/edge, and a bright opaque frontier between realms.
// With the engine grid, only the dirty row band is rebuilt (E9.2).
export function tintRgba(R, version) {
  const W = R.w, H = R.h;
  const engine = !!R.territoryOwner;
  if (R._tintCache) {
    if (engine && R._tintRows === undefined) return R._tintCache.data; // unchanged
    if (!engine && R._tintCache.version === version) return R._tintCache.data;
  }
  const owner = territoryGrid(R, version);
  const realmOf = engine ? null : realmOfSettlement(R);
  const cRgb = R.realmRgb;
  const partial = engine && R._tintCache && R._tintRows &&
    R._tintCache.data.length === W * H * 4;
  const data = partial ? R._tintCache.data : new Uint8Array(W * H * 4);
  const y0 = partial ? R._tintRows.y0 : 0;
  const y1 = partial ? R._tintRows.y1 : H;
  const asR = R.ownerIsRealm;
  const cidOf = (oo) => (oo >= 0 ? (asR ? oo : (realmOf[oo] ?? 0)) : -1);
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < W; x++) {
      const i = y * W + x, o = i * 4;
      const ow = owner[i];
      if (ow < 0) { data[o] = data[o + 1] = data[o + 2] = data[o + 3] = 0; continue; }
      const cid = cidOf(ow);
      const c = cRgb[cid] || [220, 200, 140];
      const left = x > 0 ? owner[i - 1] : ow;
      const up = y > 0 ? owner[i - W] : ow;
      const right = x < W - 1 ? owner[i + 1] : ow;
      const down = y < H - 1 ? owner[i + W] : ow;
      const settBorder = left !== ow || up !== ow || right !== ow || down !== ow;
      const realmBorder = settBorder && (
        cidOf(left) !== cid || cidOf(up) !== cid ||
        cidOf(right) !== cid || cidOf(down) !== cid);
      if (realmBorder) {
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
  R.lastTintRows = partial ? { y0, y1 } : null;
  R._tintRows = undefined; // consumed
  R._tintCache = { version, data };
  return data;
}

// ---------- the layer composite (CPU fallback path) --------------------------

export function composite(R, state) {
  const { layer, overlays, month, version } = state;
  const monthDependent = layer === "temperature" || overlays.snow;
  const base = [
    layer, overlays.rivers, overlays.snow, overlays.hillshade,
    monthDependent ? ((month % 12) + 12) % 12 : "-",
  ].join("|");
  const vKey = layer === "political" ? version
    : layer === "culture" ? "p" + (R.peoplesEpoch || 0) : "-";
  if (base === R.cacheKeyBase && vKey === R.cacheVersion) return;

  const engine = !!R.territoryOwner;
  if (base === R.cacheKeyBase && layer === "political" && engine && !R._polDirty) {
    R.cacheVersion = vKey; // a tick passed but no border moved: free
    return;
  }
  // E9.9 — when only a territory band moved, recompute just those rows
  const partial = base === R.cacheKeyBase && layer === "political" &&
    !!R._img && !!R._compRows;
  const rows = partial ? R._compRows : null;
  R.cacheKeyBase = base;
  R.cacheVersion = vKey;
  R._compRows = undefined;
  if (layer === "political") R._polDirty = false;

  const W = R.w, H = R.h;
  const {
    height, tmean, precip, discharge, fertility, flags, strahler,
    rock, soil, landform,
  } = R.world.arrays;
  if (!R._img || R._img.width !== W || R._img.height !== H) {
    R._img = R.octx.createImageData(W, H);
  }
  const px = R._img.data;
  const shade = R.shade;
  const useShade = overlays.hillshade;
  const tnow = monthDependent ? monthTemp(R, month) : null;
  const owner = layer === "political" ? territoryGrid(R, version) : null;
  const dLogMax = R.dischargeLogMax || 1;
  const sat = R.sat;

  let realmOf = null;
  if (owner) realmOf = realmOfSettlement(R);
  const cRgb = R.realmRgb;
  // M10.6 — the culture lens paints the people-axis grid with people colours
  const pOwner = layer === "culture" ? R.peoplesOwner : null;
  const pRgb = R.peopleRgb || [];

  const yStart = rows ? rows.y0 : 0;
  const yEnd = rows ? rows.y1 : H;
  for (let y = yStart; y < yEnd; y++) {
    for (let x = 0; x < W; x++) {
      const i = y * W + x;
      const o = i * 4;
      const h = height[i];
      const sea = h < 0;
      const lake = (flags[i] & 2) !== 0;
      const isWater = sea || lake;
      let r, g, b;

      if (layer === "biomes" || layer === "political" || layer === "culture") {
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
            const asR = R.ownerIsRealm;
            const cidOf = (oo) => (oo >= 0 ? (asR ? oo : (realmOf[oo] ?? 0)) : -1);
            const cid = cidOf(ow);
            const c = cRgb[cid] || [220, 200, 140];
            const left = x > 0 ? owner[i - 1] : ow;
            const up = y > 0 ? owner[i - W] : ow;
            const right = x < W - 1 ? owner[i + 1] : ow;
            const down = y < H - 1 ? owner[i + W] : ow;
            const settBorder = left !== ow || up !== ow || right !== ow || down !== ow;
            const realmBorder = settBorder && (
              cidOf(left) !== cid || cidOf(up) !== cid ||
              cidOf(right) !== cid || cidOf(down) !== cid);
            if (realmBorder) {
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
        } else if (layer === "culture" && pOwner) {
          // M10.6 — whose tongue is spoken here, over muted imagery
          const lum = 0.3 * r + 0.59 * g + 0.11 * b;
          r = (r * 0.52 + lum * 0.48) * 0.84;
          g = (g * 0.52 + lum * 0.48) * 0.84;
          b = (b * 0.52 + lum * 0.48) * 0.84;
          const ow = pOwner[i];
          if (ow >= 0) {
            const c = pRgb[ow] || [220, 200, 140];
            const left = x > 0 ? pOwner[i - 1] : ow;
            const up = y > 0 ? pOwner[i - W] : ow;
            const right = x < W - 1 ? pOwner[i + 1] : ow;
            const down = y < H - 1 ? pOwner[i + W] : ow;
            if (left !== ow || up !== ow || right !== ow || down !== ow) {
              r = Math.min(255, c[0] * 1.18 + 30);
              g = Math.min(255, c[1] * 1.18 + 30);
              b = Math.min(255, c[2] * 1.18 + 30);
            } else {
              r = r * 0.68 + c[0] * 0.32;
              g = g * 0.68 + c[1] * 0.32;
              b = b * 0.68 + c[2] * 0.32;
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
      } else if (layer === "geology") {
        // M63 — rock province; the geology continues under the sea, dimmed
        // and cooled toward the abyss like an offshore hatch on a printed map
        const c = ROCK_RGB[rock ? rock[i] : 0] || [128, 128, 128];
        if (sea) {
          const deep = Math.min(1, -h / 0.9) * 0.6;
          r = (c[0] * 0.55 + 5) * (1 - deep) + 18 * deep;
          g = (c[1] * 0.55 + 10) * (1 - deep) + 28 * deep;
          b = (c[2] * 0.55 + 23) * (1 - deep) + 48 * deep;
        } else {
          r = c[0]; g = c[1]; b = c[2];
        }
      } else if (layer === "soils") {
        // M63 — soil order; no profile under open water
        if (sea) { r = 20; g = 32; b = 50; }
        else if (lake) { r = 74; g = 128; b = 168; }
        else {
          const c = SOIL_RGB[soil ? soil[i] : 0] || [128, 128, 128];
          r = c[0]; g = c[1]; b = c[2];
        }
      } else if (layer === "landform") {
        // M63 — the vocabulary lens: open sea stays dark, shore-water
        // words keep their swatch, damped by the water they stand in
        const id = landform ? landform[i] : 0;
        if (id === 0) { r = 14; g = 25; b = 42; }
        else {
          const c = LANDFORM_COLORS[id] || [128, 128, 128];
          if (isWater) {
            r = c[0] * 0.62 + 5; g = c[1] * 0.62 + 13; b = c[2] * 0.62 + 26;
          } else {
            r = c[0]; g = c[1]; b = c[2];
          }
        }
      } else { // hydro
        if (sea) { r = 14; g = 28; b = 48; }
        else if (lake) { r = 46; g = 95; b = 143; }
        else {
          const t = Math.log1p(discharge[i]) / dLogMax;
          if (t > 0.42) [r, g, b] = HYDRO_GRAD((t - 0.42) / 0.58);
          else {
            const s = shade[i];
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
        const t = (tnow || monthTemp(R, month))[i];
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
  if (rows) R.octx.putImageData(R._img, 0, 0, 0, rows.y0, W, rows.y1 - rows.y0);
  else R.octx.putImageData(R._img, 0, 0);
}
