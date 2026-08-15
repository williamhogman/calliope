// Calliope client: state, generation, simulation loop.

import { generateWorld, tickWorld } from "./net.js";
import { Renderer } from "./render.js";
import { View } from "./view.js";
import {
  buildLayerList, buildOverlayList, buildLegend, buildResourceLegend,
  buildCultureLegend, renderSettlements, renderEvents, renderInspector,
  renderStats, renderDetail, toast,
} from "./ui.js";

const $ = (id) => document.getElementById(id);
const canvas = $("map");
const renderer = new Renderer(canvas);

const state = {
  world: null,        // {header, arrays}
  layer: "biomes",
  overlays: {
    rivers: true, snow: true, settlements: true, routes: true,
    resources: false, labels: true, winds: false, hillshade: true,
  },
  month: 0,
  version: 0,
  playing: false,
  speed: 1,
  size: 512,
  hover: null,
  events: [],
  popHistory: [],
  selectedId: null,
};

let dirty = true;
const markDirty = () => { dirty = true; };
const view = new View(canvas, markDirty);

window.__calliope = { state, view, renderer };

// ---------- render loop ----------

function frame() {
  // caravans and winds animate continuously while time flows
  if (state.playing && (state.overlays.routes || state.overlays.winds)) dirty = true;
  if (dirty) {
    dirty = false;
    renderer.draw(state, view, state.hover);
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
window.addEventListener("resize", markDirty);

// ---------- generation ----------

function seedFromInput() {
  const raw = $("seed").value.trim();
  if (!raw) return randomSeed();
  const n = Number(raw);
  if (Number.isFinite(n) && Number.isInteger(n) && n > 0) return n % 2147483647 || 1;
  // hash arbitrary strings
  let h = 2166136261;
  for (const ch of raw) { h ^= ch.codePointAt(0); h = Math.imul(h, 16777619); }
  return (h >>> 0) % 2147483647 || 1;
}

const randomSeed = () => (Math.floor(Math.random() * 2147483646) + 1);

async function generate(seed) {
  setPlaying(false);
  closeDetail();
  $("generate").disabled = true;
  $("loading").classList.remove("fade");
  try {
    const world = await generateWorld(seed, state.size);
    state.world = world;
    state.month = world.header.month || 0;
    state.version++;
    state.events = world.header.events || [];
    renderer.setWorld(world);
    view.fit(world.header.size);
    $("tagline").textContent = world.header.world_name
      ? `The world of ${world.header.world_name}` : "Welcome to Calliope";
    buildLegend($("legend"), world.header.biomes);
    buildResourceLegend($("res-legend"), world.header.resources);
    const cultures = world.header.cultures || [];
    $("cultures-group").classList.toggle("hidden", !cultures.length);
    buildCultureLegend($("cultures"), cultures, world.header.settlements);
    renderSettlements($("settlements"), $("pop-total"), world.header.settlements, cultures, onPickSettlement);
    renderEvents($("events"), state.events, world.header.months);
    buildDepositIndex();
    updateDate();
    state.popHistory = [{
      m: state.month,
      pop: world.header.settlements.reduce((a, s) => a + s.pop, 0),
    }];
    renderStats($("stats"), world, state.popHistory);
    history.replaceState(null, "", `?seed=${world.header.seed}&size=${world.header.size}`);
    $("seed").value = String(world.header.seed);
    markDirty();
  } catch (err) {
    console.error(err);
    toast($("toast"), `The muse falters: ${err.message}`);
  } finally {
    $("generate").disabled = false;
    $("loading").classList.add("fade");
  }
}

// deposit lookup by cell
let depositIndex = new Map();
function buildDepositIndex() {
  depositIndex = new Map();
  for (const d of state.world.header.deposits) {
    const key = d.y * state.world.header.size + d.x;
    if (!depositIndex.has(key)) depositIndex.set(key, []);
    depositIndex.get(key).push(d);
  }
}

// ---------- time ----------

function updateDate() {
  const months = state.world?.header.months || [];
  const year = Math.floor(state.month / 12) + 1;
  const mon = months[((state.month % 12) + 12) % 12] || "—";
  $("date").textContent = `Year ${year} · ${mon}`;
}

let tickInFlight = false;
async function advance(months) {
  if (!state.world || tickInFlight) return;
  tickInFlight = true;
  try {
    const res = await tickWorld(state.world.header.id, months);
    state.month = res.month;
    state.world.header.settlements = res.settlements;
    if (res.routes) renderer.setRoutes(res.routes); // colonies joined the network
    if (res.events?.length) {
      state.events.push(...res.events);
      state.events = state.events.slice(-200);
      renderEvents($("events"), state.events, state.world.header.months);
    }
    state.version++;
    const cultures = state.world.header.cultures || [];
    renderSettlements($("settlements"), $("pop-total"), res.settlements, cultures, onPickSettlement);
    buildCultureLegend($("cultures"), cultures, res.settlements);
    refreshDetail();
    updateDate();
    state.popHistory.push({
      m: res.month,
      pop: res.settlements.reduce((a, s) => a + s.pop, 0),
    });
    if (state.popHistory.length > 600) state.popHistory.shift();
    renderStats($("stats"), state.world, state.popHistory);
    markDirty();
  } catch (err) {
    console.error(err);
    setPlaying(false);
    toast($("toast"), `Time refused to pass: ${err.message}`);
  } finally {
    tickInFlight = false;
  }
}

let playTimer = null;
function setPlaying(on) {
  state.playing = on;
  $("icon-play").classList.toggle("hidden", on);
  $("icon-pause").classList.toggle("hidden", !on);
  clearInterval(playTimer);
  if (on) playTimer = setInterval(() => advance(state.speed), 1000);
}

// ---------- settlement detail ----------

function openDetail(s) {
  state.selectedId = s.id;
  const cultures = state.world.header.cultures || [];
  renderDetail($("detail"), s, cultures[s.culture], state.world.header.resources, closeDetail);
  markDirty();
}

function closeDetail() {
  state.selectedId = null;
  renderDetail($("detail"), null);
  markDirty();
}

function refreshDetail() {
  if (state.selectedId == null) return;
  const s = state.world.header.settlements.find((o) => o.id === state.selectedId);
  if (s) openDetail(s); else closeDetail();
}

function onPickSettlement(s) {
  view.centerOn(s.x + 0.5, s.y + 0.5, Math.max(view.scale, 6));
  openDetail(s);
}

// click (not drag) selects a settlement
let downAt = null;
canvas.addEventListener("pointerdown", (e) => { downAt = [e.clientX, e.clientY]; });
canvas.addEventListener("pointerup", (e) => {
  if (!downAt || !state.world) return;
  const moved = Math.hypot(e.clientX - downAt[0], e.clientY - downAt[1]);
  downAt = null;
  if (moved > 5) return;
  let best = null, bestD = Infinity;
  for (const s of state.world.header.settlements) {
    const sx = view.tx + (s.x + 0.5) * view.scale;
    const sy = view.ty + (s.y + 0.5) * view.scale;
    const d = Math.hypot(e.clientX - sx, e.clientY - sy);
    if (d < bestD) { bestD = d; best = s; }
  }
  if (best && bestD <= 14) openDetail(best);
  else if (state.selectedId != null) closeDetail();
});

// ---------- inspector ----------

const WIND_NAME = (lat) =>
  lat < 30 ? ["Trade winds", "E → W", -1]
    : lat < 60 ? ["Westerlies", "W → E", 1]
      : ["Polar easterlies", "E → W", -1];

function explain(cx, cy, i, h, isWater) {
  const w = state.world;
  const size = w.header.size;
  const { height, precip, tamp, flags, fertility } = w.arrays;
  const notes = [];
  const lat = Math.abs((cy / size) * 180 - 90);
  const [, , dir] = WIND_NAME(lat);

  // rain shadow: scan upwind for a crest this air had to climb
  let shadow = false, crestX = -1, crestH = Math.max(h, 0);
  if (!isWater && precip[i] < 480) {
    for (let k = 1; k <= 48; k++) {
      const x = cx - dir * k;
      if (x < 0 || x >= size) break;
      const hh = height[cy * size + x];
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
    notes.push(`Rain shadow — ${rangeName || "high peaks"} wring${rangeName ? "s" : ""} the winds dry`);
  } else if (!isWater && precip[i] < 380 && lat > 15 && lat < 35) {
    notes.push("Beneath the subtropical high — sinking air, cloudless skies");
  }
  if (!isWater && lat < 12 && precip[i] > 1300) {
    notes.push("Equatorial convergence — rising air brings near-daily rains");
  }
  if (!isWater && fertility && fertility[i] > 0.55) {
    let nearRiver = false;
    for (let dy = -2; dy <= 2 && !nearRiver; dy++) {
      for (let dx = -2; dx <= 2; dx++) {
        const nx = cx + dx, ny = cy + dy;
        if (nx < 0 || ny < 0 || nx >= size || ny >= size) continue;
        if (flags[ny * size + nx] & 1) { nearRiver = true; break; }
      }
    }
    notes.push(nearRiver ? "Floodplain silt makes these fields rich" : "Deep fertile soils");
  }
  if (!isWater && Math.abs(tamp[i]) > 17) {
    notes.push("Deep continental interior — savage swings of season");
  }
  return notes.slice(0, 2);
}

function nearestFeature(cx, cy, isWater) {
  const feats = state.world.header.features || [];
  let best = null, bd = Infinity;
  for (const f of feats) {
    const waterKind = f.t === "ocean" || f.t === "sea" || f.t === "lake" || f.t === "river";
    if (waterKind !== isWater) continue;
    const reach = f.t === "ocean" ? 1e9 : Math.sqrt(f.size) * 1.6 + 12;
    const d = Math.hypot(f.x - cx, f.y - cy);
    if (d < reach && d < bd) { bd = d; best = f; }
  }
  return best ? best.name : null;
}

function inspect(cx, cy) {
  const w = state.world;
  if (!w) return null;
  const size = w.header.size;
  if (cx < 0 || cy < 0 || cx >= size || cy >= size) return null;
  const i = cy * size + cx;
  const { height, tmean, tamp, precip, discharge, fertility, biomes, flags } = w.arrays;
  const biomeMeta = w.header.biomes[biomes[i]];
  const h = height[i];
  const tNow = tmean[i] + tamp[i] * Math.cos((2 * Math.PI * (state.month % 12)) / 12);
  const isWater = h < 0 || (flags[i] & 2) !== 0;

  const resources = [];
  for (let dy = -1; dy <= 1; dy++) {
    for (let dx = -1; dx <= 1; dx++) {
      const nx = cx + dx, ny = cy + dy;
      if (nx < 0 || ny < 0 || nx >= size || ny >= size) continue;
      for (const d of depositIndex.get(ny * size + nx) || []) {
        const m = w.header.resources[d.r];
        resources.push({ name: d.r, abundance: m.abundance, requires: m.requires });
      }
    }
  }

  let territory = null;
  const owner = renderer.territoryCache.owner;
  if (owner && owner[i] >= 0) {
    const s = w.header.settlements.find((s) => s.id === owner[i]);
    if (s) {
      const c = (w.header.cultures || [])[s.culture];
      territory = `Lands of ${s.name}${c ? ` · ${c.people}` : ""}`;
    }
  }

  let frozen = null;
  if (!isWater && tNow < -1) frozen = "Snowbound";
  else if (h < 0 && tNow < -2) frozen = "Sea ice";

  const lat = Math.abs((cy / size) * 180 - 90);
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
    flow: Math.round(discharge[i]),
    isWater,
    frozen,
    resources: resources.slice(0, 3),
    territory,
    place: nearestFeature(cx, cy, h < 0),
    notes: explain(cx, cy, i, h, isWater),
  };
}

canvas.addEventListener("pointermove", (e) => {
  if (!state.world) return;
  const [wx, wy] = view.screenToWorld(e.clientX, e.clientY);
  const cx = Math.floor(wx), cy = Math.floor(wy);
  const prev = state.hover;
  if (prev && prev.x === cx && prev.y === cy) return;
  const size = state.world.header.size;
  state.hover = (cx >= 0 && cy >= 0 && cx < size && cy < size) ? { x: cx, y: cy } : null;
  renderInspector($("inspector"), state.hover ? inspect(cx, cy) : null);
  markDirty();
});
canvas.addEventListener("pointerleave", () => {
  state.hover = null;
  renderInspector($("inspector"), null);
  markDirty();
});

// ---------- controls ----------

buildLayerList($("layers"), state.layer, (id) => {
  state.layer = id;
  markDirty();
});

buildOverlayList($("overlays"), state.overlays, (id, on) => {
  state.overlays[id] = on;
  $("res-legend-group").classList.toggle("hidden", !state.overlays.resources);
  markDirty();
});

$("dice").addEventListener("click", () => {
  $("seed").value = String(randomSeed());
});
$("generate").addEventListener("click", () => generate(seedFromInput()));
$("seed").addEventListener("keydown", (e) => {
  if (e.key === "Enter") generate(seedFromInput());
});

$("size-seg").addEventListener("click", (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  state.size = Number(btn.dataset.size);
  $("size-seg").querySelectorAll("button").forEach((b) => b.classList.toggle("active", b === btn));
});

$("speed-seg").addEventListener("click", (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  state.speed = Number(btn.dataset.speed);
  $("speed-seg").querySelectorAll("button").forEach((b) => b.classList.toggle("active", b === btn));
});

$("play").addEventListener("click", () => setPlaying(!state.playing));
$("step").addEventListener("click", () => advance(1));

window.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT") return;
  if (e.code === "Space") { e.preventDefault(); setPlaying(!state.playing); }
  if (e.key === "n") advance(1);
  if (e.key === "Escape") closeDetail();
});

// ---------- boot ----------

const params = new URLSearchParams(location.search);
const bootSeed = params.get("seed") ? Number(params.get("seed")) : randomSeed();
const bootSize = Number(params.get("size"));
if ([256, 384, 512].includes(bootSize)) {
  state.size = bootSize;
  $("size-seg").querySelectorAll("button").forEach((b) =>
    b.classList.toggle("active", Number(b.dataset.size) === bootSize));
}
$("seed").value = String(bootSeed);
generate(bootSeed);
