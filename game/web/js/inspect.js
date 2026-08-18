// Map-side lookups: cell inspection (climate notes, resources, territory),
// the deposit index, and event anchoring. Split from main.js (E8.7) —
// everything here answers "what is at this place?".

import { world, month } from "./ui/state.js";

let ctx = null;
export function initInspect(c) {
  ctx = c;
}

// ---------- deposit lookup by cell (grid is width x height) ----------

let depositIndex = new Map();
export function buildDepositIndex(w) {
  depositIndex = new Map();
  const W = w.header.width || w.header.size;
  for (const d of w.header.deposits) {
    const key = d.y * W + d.x;
    if (!depositIndex.has(key)) depositIndex.set(key, []);
    depositIndex.get(key).push(d);
  }
}

// ---------- event anchors ----------

export function locateEvent(e) {
  const w = world();
  if (!w) return null;
  // events carry their own map anchor when they have one (M6.1/M9.4)
  if (e.x != null && e.x >= 0) return { x: e.x + 0.5, y: e.y + 0.5 };
  if (!e.s) return null;
  const s = w.header.settlements.find((x) => x.name === e.s);
  if (s) return { x: s.x + 0.5, y: s.y + 0.5 };
  const ru = (w.header.ruins || []).find((x) => x.name === e.s || x.of === e.s);
  if (ru) return { x: ru.x + 0.5, y: ru.y + 0.5 };
  const f = (w.header.features || []).find((x) => x.name === e.s);
  if (f) return { x: f.x, y: f.y };
  return null;
}

// ---------- cell inspection ----------

const WIND_NAME = (lat) =>
  lat < 30 ? ["Trade winds", "E \u2192 W", -1]
    : lat < 60 ? ["Westerlies", "W \u2192 E", 1]
      : ["Polar easterlies", "E \u2192 W", -1];

function cellNotes(w, cx, cy, i, h, isWater) {
  const W = w.header.width || w.header.size;
  const H = w.header.size;
  const { height, precip, tamp, flags, fertility } = w.arrays;
  const notes = [];
  const lat = Math.abs((cy / H) * 180 - 90);
  const [, , dir] = WIND_NAME(lat);

  // rain shadow: scan upwind for a crest this air had to climb
  let shadow = false, crestX = -1, crestH = Math.max(h, 0);
  if (!isWater && precip[i] < 480) {
    for (let k = 1; k <= 48; k++) {
      const x = cx - dir * k;
      if (x < 0 || x >= W) break;
      const hh = height[cy * W + x];
      if (hh > crestH) { crestH = hh; crestX = x; }
    }
    if (crestH > Math.max(h + 0.28, 0.5)) shadow = true;
  }
  if (shadow) {
    let rangeName = null, bd = Infinity;
    for (const f of w.header.features || []) {
      if (f.t !== "range") continue;
      const d = Math.hypot(f.x - crestX, f.y - cy);
      if (d < bd && d < 70) { bd = d; rangeName = f.name; }
    }
    notes.push(`Rain shadow \u2014 ${rangeName || "high peaks"} wring${rangeName ? "s" : ""} the winds dry`);
  } else if (!isWater && precip[i] < 380 && lat > 15 && lat < 35) {
    notes.push("Beneath the subtropical high \u2014 sinking air, cloudless skies");
  }
  if (!isWater && lat < 12 && precip[i] > 1300) {
    notes.push("Equatorial convergence \u2014 rising air brings near-daily rains");
  }
  if (!isWater && fertility && fertility[i] > 0.55) {
    let nearRiver = false;
    for (let dy = -2; dy <= 2 && !nearRiver; dy++) {
      for (let dx = -2; dx <= 2; dx++) {
        const nx = cx + dx, ny = cy + dy;
        if (nx < 0 || ny < 0 || nx >= W || ny >= H) continue;
        if (flags[ny * W + nx] & 1) { nearRiver = true; break; }
      }
    }
    notes.push(nearRiver ? "Floodplain silt makes these fields rich" : "Deep fertile soils");
  }
  if (!isWater && Math.abs(tamp[i]) > 17) {
    notes.push("Deep continental interior \u2014 savage swings of season");
  }
  if (flags[i] & 4) {
    notes.unshift("An endorheic basin \u2014 rivers die here and leave their salt");
  } else if (flags[i] & 8) {
    notes.unshift("A wadi \u2014 roaring in the rains, cracked mud by the dry solstice");
  }
  return notes.slice(0, 2);
}

const WATER_FEATURES = new Set(["ocean", "sea", "lake", "river", "bay", "strait", "delta"]);

function nearestFeature(w, cx, cy, isWater) {
  const feats = w.header.features || [];
  let best = null, bestPri = Infinity;
  for (const f of feats) {
    const waterKind = WATER_FEATURES.has(f.t);
    if (waterKind !== isWater) continue;
    const reach = f.t === "ocean" ? 1e9 : Math.sqrt(f.size) * 1.1 + 8;
    const d = Math.hypot(f.x - cx, f.y - cy);
    // prefer the tightest fitting name: a bay over the ocean, a cape over a continent
    const pri = d / Math.max(reach, 1);
    if (d < reach && pri < bestPri) { bestPri = pri; best = f; }
  }
  return best ? best.name : null;
}

export function inspectCell(cx, cy) {
  const w = world();
  if (!w) return null;
  const W = w.header.width || w.header.size;
  const H = w.header.size;
  if (cx < 0 || cy < 0 || cx >= W || cy >= H) return null;
  const i = cy * W + cx;
  const { height, tmean, tamp, precip, discharge, fertility, biomes, flags } = w.arrays;
  const biomeMeta = w.header.biomes[biomes[i]];
  const h = height[i];
  const tNow = tmean[i] + tamp[i] * Math.cos((2 * Math.PI * (month() % 12)) / 12);
  const isWater = h < 0 || (flags[i] & 2) !== 0 || (flags[i] & 4) !== 0;

  const resources = [];
  for (let dy = -1; dy <= 1; dy++) {
    for (let dx = -1; dx <= 1; dx++) {
      const nx = cx + dx, ny = cy + dy;
      if (nx < 0 || ny < 0 || nx >= W || ny >= H) continue;
      for (const d of depositIndex.get(ny * W + nx) || []) {
        const m = w.header.resources[d.r];
        resources.push({ name: d.r, abundance: m.abundance, requires: m.requires });
      }
    }
  }

  // ADR-0018 — both axes read at the cell: whose banner rules (realm) and
  // whose tongue is spoken (people). They differ where conquest outran
  // assimilation, and the difference is the story (M10.6).
  let territory = null;
  const rid = ctx.renderer.ownerRealmAt(i);
  if (rid >= 0) {
    const r = (w.header.realms || [])[rid];
    if (r) {
      territory = r.vassal_of
        ? `Under the banners of ${r.name} \u00b7 sworn to ${r.vassal_of}`
        : `Under the banners of ${r.name}`;
    }
  }
  let folk = null;
  const pid = ctx.renderer.peopleAt(i);
  if (pid >= 0) {
    const p = (w.header.cultures || [])[pid];
    if (p) {
      const crown = rid >= 0 ? (w.header.realms || [])[rid] : null;
      const foreign = crown && crown.people !== pid;
      folk = foreign
        ? `The folk here keep the ${p.people} tongue under a foreign crown`
        : `The ${p.people} tongue is spoken here`;
    }
  }

  let frozen = null;
  if (!isWater && tNow < -1) frozen = "Snowbound";
  else if (h < 0 && tNow < -2) frozen = "Sea ice";

  const lat = Math.abs((cy / H) * 180 - 90);
  const [windName, windArrow] = WIND_NAME(lat);

  return {
    x: cx, y: cy,
    biome: biomeMeta ? biomeMeta.name : "?",
    elevation: Math.round(h * w.header.metres_per_unit),
    tempNow: tNow.toFixed(1),
    tempMean: tmean[i].toFixed(1),
    precip: Math.round(precip[i]),
    fertility: fertility ? fertility[i] : null,
    wind: `${windName} ${windArrow}`,
    river: (flags[i] & 1) !== 0,
    lake: (flags[i] & 2) !== 0,
    salt: (flags[i] & 4) !== 0,
    wadi: (flags[i] & 8) !== 0,
    order: w.arrays.strahler ? w.arrays.strahler[i] : 0,
    flow: Math.round(discharge[i]),
    isWater,
    frozen,
    resources: resources.slice(0, 3),
    territory,
    folk,
    place: nearestFeature(w, cx, cy, h < 0),
    notes: cellNotes(w, cx, cy, i, h, isWater),
  };
}
