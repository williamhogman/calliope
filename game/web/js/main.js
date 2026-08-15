// Calliope client: state, generation, simulation loop.

import { generateWorld, tickWorld } from "./net.js";
import { Renderer } from "./render.js";
import { View } from "./view.js";
import {
  buildLayerList, buildOverlayList, buildLegend, buildResourceLegend,
  renderSettlements, renderEvents, renderInspector, renderStats, toast,
} from "./ui.js";

const $ = (id) => document.getElementById(id);
const canvas = $("map");
const renderer = new Renderer(canvas);

const state = {
  world: null,        // {header, arrays}
  layer: "biomes",
  overlays: {
    rivers: true, snow: true, settlements: true, routes: true,
    resources: false, hillshade: true,
  },
  month: 0,
  version: 0,
  playing: false,
  speed: 1,
  size: 512,
  hover: null,
  events: [],
  popHistory: [],
};

let dirty = true;
const markDirty = () => { dirty = true; };
const view = new View(canvas, markDirty);

window.__calliope = { state, view, renderer };

// ---------- render loop ----------

function frame() {
  // caravans animate continuously while time flows
  if (state.playing && state.overlays.routes) dirty = true;
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
    buildLegend($("legend"), world.header.biomes);
    buildResourceLegend($("res-legend"), world.header.resources);
    renderSettlements($("settlements"), $("pop-total"), world.header.settlements, onPickSettlement);
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
    if (res.events?.length) {
      state.events.push(...res.events);
      state.events = state.events.slice(-200);
      renderEvents($("events"), state.events, state.world.header.months);
    }
    state.version++;
    renderSettlements($("settlements"), $("pop-total"), res.settlements, onPickSettlement);
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

// ---------- inspector ----------

function inspect(cx, cy) {
  const w = state.world;
  if (!w) return null;
  const size = w.header.size;
  if (cx < 0 || cy < 0 || cx >= size || cy >= size) return null;
  const i = cy * size + cx;
  const { height, tmean, tamp, precip, discharge, biomes, flags } = w.arrays;
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
    if (s) territory = `${s.name} (${s.pop.toLocaleString("en-US")})`;
  }

  let frozen = null;
  if (!isWater && tNow < -1) frozen = "Snowbound";
  else if (h < 0 && tNow < -2) frozen = "Sea ice";

  return {
    x: cx, y: cy,
    biome: biomeMeta ? biomeMeta.name : "?",
    elevation: Math.round(h * w.header.metres_per_unit),
    tempNow: tNow.toFixed(1),
    tempMean: tmean[i].toFixed(1),
    precip: Math.round(precip[i]),
    river: (flags[i] & 1) !== 0,
    lake: (flags[i] & 2) !== 0,
    flow: Math.round(discharge[i]),
    isWater,
    frozen,
    resources: resources.slice(0, 3),
    territory,
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
});

function onPickSettlement(s) {
  view.centerOn(s.x + 0.5, s.y + 0.5, Math.max(view.scale, 6));
}

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
