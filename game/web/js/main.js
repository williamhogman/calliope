// Calliope client: simulation driver, canvas orchestration, inspector logic.
// All panel DOM is rendered by the Solid UI in ./ui/app.js from ./ui/state.js.

import { generateWorld, tickWorld } from "./net.js";
import { Renderer } from "./render.js";
import { createGpu, recreateGpuOnGl } from "./gpu.js";
import { View } from "./view.js";
import { mountUI } from "./ui/app.js";
import {
  world, setWorld, setSettlements, setCultures, setWars, setEvents, events,
  month, setMonth, playing, setPlaying, speed, setSpeed,
  worldSize, setWorldSize, setBusy, layer, setLayer, overlays, setOverlays,
  selected, setSelected, setHoverInfo, popHistory, setPopHistory, setSeenEvents,
  setMarket, setDepositsTick,
} from "./ui/state.js";

const $ = (id) => document.getElementById(id);
const canvas = $("map");
const renderer = new Renderer(canvas);

let version = 0;
let dirty = true;
const markDirty = () => { dirty = true; };
const view = new View(canvas, markDirty);
let hover = null;

window.__calliope = {
  view, renderer, world, month, advance: (m) => advance(m),
  gpuMode: () => (gpuLive ? "live" : "on-demand"),
  gpuForceLive: () => { governorOn = false; gpuLive = true; },
};

// GPU imagery: bring up the Rust wgpu engine (WebGPU, else WebGL2).
// If no adapter exists the CPU compositor stays in charge.
let glCanvas = $("gl");
createGpu(glCanvas)
  .then((gpu) => {
    renderer.gpu = gpu;
    const w = world();
    if (w) gpu.setWorld(w);
    markDirty();
  })
  .catch((err) => {
    console.warn("GPU engine unavailable; CPU compositor in charge:", err);
    glCanvas.remove();
  });

// ---------- GPU present audit ----------
//
// Some browsers hand out a GPU device that never puts a pixel on screen
// (broken WebGPU drivers, headless software rasterisers). After the engine
// has had a fair number of frames with a world, read the canvas back once:
// if it is still fully transparent, the imagery never arrived — retry on
// WebGL2 with a fresh canvas, and past that let the CPU compositor carry.
const gpuAudit = { engine: null, frames: 0 };

function gpuCanvasHasPixels() {
  const t = document.createElement("canvas");
  t.width = 16; t.height = 16;
  const c = t.getContext("2d", { willReadFrequently: true });
  try { c.drawImage(glCanvas, 0, 0, 16, 16); } catch { return true; }
  const d = c.getImageData(0, 0, 16, 16).data;
  for (let i = 3; i < d.length; i += 4) if (d[i] !== 0) return true;
  return false;
}

function handleBlankGpu() {
  const gpu = renderer.gpu;
  if (!gpu) return;
  if (gpu.backend() === "webgpu") {
    console.warn("calliope: WebGPU presents nothing — retrying on WebGL2");
    renderer.gpu = null;
    markDirty();
    recreateGpuOnGl(glCanvas)
      .then(({ gpu: g, canvas: fresh }) => {
        glCanvas = fresh;
        renderer.gpu = g;
        const w = world();
        if (w) g.setWorld(w);
        markDirty();
      })
      .catch((err) => {
        console.warn("calliope: WebGL2 retry failed — CPU compositor in charge:", err);
        glCanvas.remove();
        markDirty();
      });
  } else {
    console.warn("calliope: GL engine presents nothing — CPU compositor in charge");
    renderer.gpu = null;
    glCanvas.remove();
    markDirty();
  }
}

// ---------- render loop ----------

// Frame governor: on hardware GL the fullscreen pass is ~free, so it runs
// every frame (living water, gliding seasons). Software rasterisers can't
// afford that — when the frame time stays heavy, fall back to on-demand
// GL rendering: everything still works, the water just holds still.
let lastTs = 0;
let frameEma = 16;
let gpuLive = true;
let governorOn = true;

function frame(ts) {
  window.__calliope.frames = (window.__calliope.frames || 0) + 1;
  if (lastTs) {
    frameEma += (Math.min(ts - lastTs, 250) - frameEma) * 0.05;
    if (governorOn && gpuLive && ts > 6000 && frameEma > 70) {
      gpuLive = false;
      dirty = true;
      console.info("calliope: slow rasteriser detected — GL renders on demand");
    }
  }
  lastTs = ts;
  const gpu = renderer.gpu;
  if (gpu && gpu.hasWorld && renderer.world && (gpuLive || dirty)) {
    if (layer() === "political") gpu.setTint(renderer.tintRgba(version), version);
    try {
      gpu.render(
        { layer: layer(), overlays, month: month() },
        view, canvas.clientWidth, canvas.clientHeight,
      );
      // Present audit once the engine has had 40 world frames. WebGL2 reads
      // back in the same task (without preserveDrawingBuffer the drawing
      // buffer is only valid here). WebGPU reads back a macrotask later:
      // drawImage then snapshots the *presented* frame — exactly the thing
      // a broken driver never delivers, while a same-task read would see
      // the submitted texture and miss the failure.
      if (gpuAudit.engine !== gpu) { gpuAudit.engine = gpu; gpuAudit.frames = 0; }
      if (++gpuAudit.frames === 40) {
        if (gpu.backend() === "webgpu") {
          setTimeout(() => {
            if (renderer.gpu === gpu && !gpuCanvasHasPixels()) handleBlankGpu();
          }, 0);
        } else if (!gpuCanvasHasPixels()) {
          handleBlankGpu();
        }
      }
    } catch (err) {
      // one bad GPU frame must not kill the annotation layer or the loop
      console.error("calliope: GPU frame failed, CPU compositor takes over:", err);
      renderer.gpu = null;
      glCanvas.remove();
      dirty = true;
    }
  }
  // caravans and winds animate continuously while time flows
  if (playing() && (overlays.routes || overlays.winds)) dirty = true;
  if (dirty) {
    dirty = false;
    renderer.draw({
      layer: layer(),
      overlays,
      month: month(),
      version,
      playing: playing(),
      selectedId: selected()?.id ?? null,
    }, view, hover);
    window.__calliope.draws = (window.__calliope.draws || 0) + 1;
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
window.addEventListener("resize", markDirty);

function toast(msg, ms = 4200) {
  const el = $("toast");
  el.textContent = msg;
  el.classList.remove("hidden");
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.add("hidden"), ms);
}

// ---------- generation ----------

const randomSeed = () => Math.floor(Math.random() * 2147483646) + 1;

function seedFrom(raw) {
  raw = (raw || "").trim();
  if (!raw) return randomSeed();
  const n = Number(raw);
  if (Number.isFinite(n) && Number.isInteger(n) && n > 0) return n % 2147483647 || 1;
  let h = 2166136261;
  for (const ch of raw) { h ^= ch.codePointAt(0); h = Math.imul(h, 16777619); }
  return (h >>> 0) % 2147483647 || 1;
}

async function generate(rawSeed) {
  const seed = seedFrom(rawSeed);
  setPlayingState(false);
  setSelected(null);
  setBusy(true);
  $("loading").classList.remove("fade");
  try {
    const w = await generateWorld(seed, worldSize());
    setWorld(w);
    setMonth(w.header.month || 0);
    version++;
    setEvents(w.header.events || []);
    setSeenEvents((w.header.events || []).length);
    setSettlements(w.header.settlements);
    setCultures(w.header.cultures || []);
    setWars(w.header.wars || []);
    setMarket(w.header.market || []);
    renderer.setWorld(w);
    renderer.gpu?.setWorld(w);
    view.fit(w.header.width || w.header.size, w.header.size);
    buildDepositIndex(w);
    setPopHistory([{
      m: w.header.month || 0,
      pop: w.header.settlements.reduce((a, s) => a + s.pop, 0),
    }]);
    history.replaceState(null, "", `?seed=${w.header.seed}&size=${w.header.size}`);
    markDirty();
  } catch (err) {
    console.error(err);
    toast(`The muse falters: ${err.message}`);
  } finally {
    setBusy(false);
    $("loading").classList.add("fade");
  }
}

// deposit lookup by cell (grid is width x height after ocean margins)
let depositIndex = new Map();
function buildDepositIndex(w) {
  depositIndex = new Map();
  const W = w.header.width || w.header.size;
  for (const d of w.header.deposits) {
    const key = d.y * W + d.x;
    if (!depositIndex.has(key)) depositIndex.set(key, []);
    depositIndex.get(key).push(d);
  }
}

// ---------- time ----------

let tickInFlight = false;
async function advance(months) {
  const w = world();
  if (!w || tickInFlight) return;
  tickInFlight = true;
  try {
    const res = await tickWorld(w.header.id, months);
    setMonth(res.month);
    w.header.settlements = res.settlements;
    setSettlements(res.settlements);
    if (res.routes) {
      w.header.routes = res.routes;
      renderer.setRoutes(res.routes); // colonies joined the network
    }
    if (res.cultures) {
      w.header.cultures = res.cultures;
      setCultures(res.cultures);
    }
    if (res.wars) setWars(res.wars);
    if (res.market) setMarket(res.market);
    if (res.deposits) {
      // discoveries or dead mines: refresh the map's mineral ledger
      w.header.deposits = res.deposits;
      if (res.deposits_hidden !== undefined) w.header.deposits_hidden = res.deposits_hidden;
      buildDepositIndex(w);
      setDepositsTick((t) => t + 1);
    }
    if (res.events?.length) {
      setEvents([...events(), ...res.events].slice(-200));
    }
    version++;
    // keep the selected settlement fresh across ticks
    const sel = selected();
    if (sel) setSelected(res.settlements.find((o) => o.id === sel.id) || null);
    setPopHistory([
      ...popHistory(),
      { m: res.month, pop: res.settlements.reduce((a, s) => a + s.pop, 0) },
    ].slice(-600));
    markDirty();
  } catch (err) {
    console.error(err);
    setPlayingState(false);
    toast(`Time refused to pass: ${err.message}`);
  } finally {
    tickInFlight = false;
  }
}

let playTimer = null;
function setPlayingState(on) {
  setPlaying(on);
  clearInterval(playTimer);
  if (on) playTimer = setInterval(() => advance(speed()), 1000);
}

// ---------- selection ----------

function pickSettlement(s) {
  view.centerOn(s.x + 0.5, s.y + 0.5, Math.max(view.scale, 6));
  setSelected(s);
  markDirty();
}

function closeDetail() {
  setSelected(null);
  markDirty();
}

// tap/click (not drag, not pinch) selects a settlement; on touch a tap on
// open ground inspects the cell instead — there is no hover on a phone.
let downAt = null, pointersDown = 0, multiTouch = false;
canvas.addEventListener("pointerdown", (e) => {
  pointersDown++;
  if (pointersDown > 1) multiTouch = true;
  downAt = [e.clientX, e.clientY];
});
canvas.addEventListener("pointercancel", () => {
  pointersDown = Math.max(0, pointersDown - 1);
  if (pointersDown === 0) { multiTouch = false; downAt = null; }
});
canvas.addEventListener("pointerup", (e) => {
  pointersDown = Math.max(0, pointersDown - 1);
  if (pointersDown > 0) return; // other fingers still down
  const wasPinch = multiTouch;
  multiTouch = false;
  const w = world();
  if (!downAt || !w || wasPinch) { downAt = null; return; }
  const moved = Math.hypot(e.clientX - downAt[0], e.clientY - downAt[1]);
  downAt = null;
  if (moved > 5) return;
  const touch = e.pointerType === "touch";
  let best = null, bestD = Infinity;
  for (const s of w.header.settlements) {
    const sx = view.tx + (s.x + 0.5) * view.scale;
    const sy = view.ty + (s.y + 0.5) * view.scale;
    const d = Math.hypot(e.clientX - sx, e.clientY - sy);
    if (d < bestD) { bestD = d; best = s; }
  }
  if (best && bestD <= (touch ? 22 : 14)) {
    if (touch) { hover = null; setHoverInfo(null); }
    pickSettlement(best);
    return;
  }
  if (selected()) closeDetail();
  if (touch) {
    const [wx, wy] = view.screenToWorld(e.clientX, e.clientY);
    const cx = Math.floor(wx), cy = Math.floor(wy);
    const W = w.header.width || w.header.size;
    const H = w.header.size;
    hover = (cx >= 0 && cy >= 0 && cx < W && cy < H) ? { x: cx, y: cy } : null;
    setHoverInfo(hover ? inspect(cx, cy) : null);
    markDirty();
  }
});

// ---------- inspector ----------

const WIND_NAME = (lat) =>
  lat < 30 ? ["Trade winds", "E \u2192 W", -1]
    : lat < 60 ? ["Westerlies", "W \u2192 E", 1]
      : ["Polar easterlies", "E \u2192 W", -1];

function explain(w, cx, cy, i, h, isWater) {
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
  let best = null, bd = Infinity, bestPri = Infinity;
  for (const f of feats) {
    const waterKind = WATER_FEATURES.has(f.t);
    if (waterKind !== isWater) continue;
    const reach = f.t === "ocean" ? 1e9 : Math.sqrt(f.size) * 1.1 + 8;
    const d = Math.hypot(f.x - cx, f.y - cy);
    // prefer the tightest fitting name: a bay over the ocean, a cape over a continent
    const pri = d / Math.max(reach, 1);
    if (d < reach && pri < bestPri) { bestPri = pri; bd = d; best = f; }
  }
  return best ? best.name : null;
}

function inspect(cx, cy) {
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

  let territory = null;
  const owner = renderer.territoryCache.owner;
  if (owner && owner[i] >= 0) {
    const s = w.header.settlements.find((s) => s.id === owner[i]);
    if (s) {
      const c = (w.header.cultures || [])[s.culture];
      territory = `Lands of ${s.name}${c ? ` \u00b7 ${c.people}` : ""}`;
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
    place: nearestFeature(w, cx, cy, h < 0),
    notes: explain(w, cx, cy, i, h, isWater),
  };
}

canvas.addEventListener("pointermove", (e) => {
  if (e.pointerType === "touch") return; // touch pans; taps inspect
  const w = world();
  if (!w) return;
  const [wx, wy] = view.screenToWorld(e.clientX, e.clientY);
  const cx = Math.floor(wx), cy = Math.floor(wy);
  if (hover && hover.x === cx && hover.y === cy) return;
  const W = w.header.width || w.header.size;
  const H = w.header.size;
  hover = (cx >= 0 && cy >= 0 && cx < W && cy < H) ? { x: cx, y: cy } : null;
  setHoverInfo(hover ? inspect(cx, cy) : null);
  markDirty();
});
canvas.addEventListener("pointerleave", (e) => {
  if (e.pointerType === "touch") return; // keep tapped info up after the finger lifts
  hover = null;
  setHoverInfo(null);
  markDirty();
});

// ---------- controls ----------

mountUI({
  generate,
  setLayer: (id) => { setLayer(id); markDirty(); },
  toggleOverlay: (id) => { setOverlays(id, !overlays[id]); markDirty(); },
  playPause: () => setPlayingState(!playing()),
  step: () => advance(1),
  setSpeed,
  pickSettlement,
  closeDetail,
  clearHover: () => { hover = null; setHoverInfo(null); markDirty(); },
});

window.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT") return;
  if (e.code === "Space") { e.preventDefault(); setPlayingState(!playing()); }
  if (e.key === "n") advance(1);
  if (e.key === "Escape") closeDetail();
});

// ---------- boot ----------

const params = new URLSearchParams(location.search);
const bootSeed = params.get("seed") ? Number(params.get("seed")) : randomSeed();
const bootSize = Number(params.get("size"));
if ([256, 384, 512].includes(bootSize)) setWorldSize(bootSize);
generate(String(bootSeed));
