// Calliope client: simulation driver, canvas orchestration, picking,
// camera flights and notifications. All chrome DOM is the Solid UI in
// ./ui/app.js reading ./ui/state.js.

import {
  generateWorld, tickWorld, explainWorld, timingsWorld,
  storiesWorld, entitiesWorld, entityLogWorld, artifactsWorld,
} from "./net.js";
import { Renderer } from "./render.js";
import { createGpu, recreateGpuOnGl } from "./gpu.js";
import { View } from "./view.js";
import { pick } from "./picking.js";
import { mountUI } from "./ui/app.js";
import {
  world, setWorld, setSettlements, setCultures, setWars, setEvents, events,
  setRuins,
  month, setMonth, playing, setPlaying, speed, setSpeed,
  worldSize, setWorldSize, setBusy, layer, setLayer, overlays, setOverlays,
  selection, setSelection, setHoverTip,
  popHistory, setPopHistory, setSeenEvents,
  market, setMarket, setAreas, setMerchants, setDepositsTick, setMarketTick, pushToast,
  searchOpen, setSearchOpen, closePopovers,
  overlaysOpen, setOverlaysOpen, legendOpen, setLegendOpen,
  worldMenuOpen, notifOpen, notif, isMobile, sheet, setSheet,
  entities, setStories, setEntities, setArtifacts,
} from "./ui/state.js";
import { LAYERS, eventFamily, eventColor } from "./ui/config.js";

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
  // M7 gate evidence: label placement stats from the last drawn frame
  labelStats: () => renderer.labelStats(),
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
    const sel = selection();
    renderer.draw({
      layer: layer(),
      overlays,
      month: month(),
      version,
      playing: playing(),
      selectedId: sel?.kind === "settlement" ? sel.id : null,
      selectedRuin: sel?.kind === "ruin" ? sel.id : null,
      selectedCell: sel?.kind === "cell" ? sel : null,
    }, view, hover);
    window.__calliope.draws = (window.__calliope.draws || 0) + 1;
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
window.addEventListener("resize", markDirty);

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

// per-entity histories, rebuilt for every world
let popHistById = new Map();   // settlement id -> [pop,...]
let priceHist = new Map();     // good -> [price,...]

function recordHistories(setts, marketRows, m) {
  for (const s of setts) {
    let h = popHistById.get(s.id);
    if (!h) { h = []; popHistById.set(s.id, h); }
    h.push(s.pop);
    if (h.length > 480) h.shift();
  }
  if (marketRows) {
    for (const r of marketRows) {
      let h = priceHist.get(r.g);
      if (!h) { h = []; priceHist.set(r.g, h); }
      h.push(r.p);
      if (h.length > 480) h.shift();
    }
    setMarketTick((t) => t + 1);
  }
  setPopHistory([
    ...popHistory(),
    { m, pop: setts.reduce((a, s) => a + s.pop, 0) },
  ].slice(-600));
}

async function generate(rawSeed) {
  const seed = seedFrom(rawSeed);
  setPlayingState(false);
  setSelection(null);
  setBusy(true);
  $("loading").classList.remove("fade");
  try {
    const w = await generateWorld(seed, worldSize());
    // stage timings live on a debug side channel, not the pack (E3.9)
    timingsWorld()
      .then((t) => console.debug("[calliope] generation timings (s)", t))
      .catch(() => {});
    setWorld(w);
    setMonth(w.header.month || 0);
    version++;
    setEvents(w.header.events || []);
    setSeenEvents((w.header.events || []).length);
    setSettlements(w.header.settlements);
    setCultures(w.header.cultures || []);
    setWars(w.header.wars || []);
    setRuins(w.header.ruins || []);
    setMarket(w.header.market || []);
    setAreas(w.header.areas || null);
    setMerchants(w.header.merchants || []);
    renderer.setWorld(w);
    renderer.gpu?.setWorld(w);
    view.fit(w.header.width || w.header.size, w.header.size);
    buildDepositIndex(w);
    popHistById = new Map();
    priceHist = new Map();
    setPopHistory([]);
    setStories([]);
    setEntities([]);
    setArtifacts([]);
    legendsAt = -1000;
    refreshLegends(true); // the dawn already has a cast worth browsing
    recordHistories(w.header.settlements, w.header.market, w.header.month || 0);
    history.replaceState(null, "", `?seed=${w.header.seed}&size=${w.header.size}`);
    markDirty();
  } catch (err) {
    console.error(err);
    pushToast({ kind: "error", text: `The muse falters: ${err.message}`, ttl: 9000 });
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
    // Tick v2 (E4.2): settlements arrive as a delta — merge by id into the
    // array we hold, drop the gone, append the newly founded in order.
    if (res.settlements || res.settlements_gone || res.s_hot) {
      const gone = new Set(res.settlements_gone || []);
      const delta = new Map((res.settlements || []).map((s) => [s.id, s]));
      // positional heartbeat rows (E4.2): [id, pop, food, k, wealth],
      // null = that field did not move this month
      const HOT = ["pop", "food", "k", "wealth"];
      const hot = new Map((res.s_hot || []).map((r) => [r[0], r]));
      const merged = [];
      for (const s of w.header.settlements) {
        if (gone.has(s.id)) continue;
        if (delta.has(s.id)) {
          merged.push(delta.get(s.id));
          delta.delete(s.id);
        } else if (hot.has(s.id)) {
          const row = hot.get(s.id);
          const patch = { ...s };
          HOT.forEach((k, i) => {
            if (row[i + 1] !== null) patch[k] = row[i + 1];
          });
          merged.push(patch);
        } else {
          merged.push(s);
        }
      }
      for (const s of res.settlements || []) {
        if (delta.has(s.id)) {
          merged.push(s);
          delta.delete(s.id);
        }
      }
      w.header.settlements = merged;
      setSettlements(merged);
    }
    if (res.routes) {
      w.header.routes = res.routes;
      renderer.setRoutes(res.routes); // colonies joined the network
    }
    if (res.cultures) {
      w.header.cultures = res.cultures;
      setCultures(res.cultures);
    } else if (res.c_hot) {
      // heartbeat patch (E4.2): treasury/asab/legit over the held cultures
      const culs = w.header.cultures.slice();
      for (const { i, ...p } of res.c_hot) culs[i] = { ...culs[i], ...p };
      w.header.cultures = culs;
      setCultures(culs);
    }
    if (res.wars) setWars(res.wars);
    if (res.market) {
      setMarket(res.market);
    } else if (res.m_hot) {
      // per-good row patches (E4.3): merge into the held ledger by good
      const byG = new Map(res.m_hot.map((r) => [r.g, r]));
      setMarket((market() || []).map((r) => byG.get(r.g) || r));
    }
    if (res.areas) {
      if (res.areas.of) {
        // full replace: the hub set itself changed (E4.3)
        w.header.areas = res.areas;
      } else {
        // partial: merge changed hub rows by id; fresh spread if present.
        // A row without a name is a price-only patch — merge its goods.
        const held = w.header.areas || { hubs: [], of: [], spread: [] };
        const byId = new Map((res.areas.hubs || []).map((h) => [h.id, h]));
        w.header.areas = {
          ...held,
          hubs: held.hubs.map((h) => {
            const d = byId.get(h.id);
            if (!d) return h;
            return d.name === undefined ? { ...h, p: { ...h.p, ...d.p } } : d;
          }),
          ...(res.areas.spread ? { spread: res.areas.spread } : {}),
        };
      }
      setAreas(w.header.areas);
    }
    if (res.merchants) setMerchants(res.merchants);
    if (res.deposits) {
      // discoveries or dead mines: refresh the map's mineral ledger
      w.header.deposits = res.deposits;
      if (res.deposits_hidden !== undefined) w.header.deposits_hidden = res.deposits_hidden;
      buildDepositIndex(w);
      setDepositsTick((t) => t + 1);
    }
    if (res.features) {
      // the tongues caught up with the map: features gained doubled names
      w.header.features = res.features;
    }
    if (res.ruins) {
      // a town died (M9.1): its ruin joins the map's quiet inventory
      w.header.ruins = res.ruins;
      setRuins(res.ruins);
    }
    if (res.territory) {
      // borders moved: the engine redrew the political map (M4.1)
      renderer.setTerritory(res.territory);
    }
    if (res.events?.length) {
      setEvents([...events(), ...res.events]);
    }
    // E4.8 — toasts come from the engine-picked headline slice.
    if (res.headlines?.length) notifyEvents(res.headlines);
    version++;
    // Histories stay aligned even when unchanged sections stayed home:
    // the merged settlements and the last-known market carry the series.
    recordHistories(w.header.settlements, res.market || market(), res.month);
    markDirty();
  } catch (err) {
    console.error(err);
    setPlayingState(false);
    pushToast({ kind: "error", text: `Time refused to pass: ${err.message}`, ttl: 9000 });
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

// ---------- notifications ----------

// Kinds worth interrupting for, even inside an enabled family.
const TOAST_KINDS = new Set([
  "war", "found", "ruler", "wonder", "disaster", "discovery",
  "depletion", "society", "tech", "myth",
]);

function locateEvent(e) {
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


function notifyEvents(list) {
  let shown = 0;
  for (const e of list) {
    if (shown >= 2) break; // never bury the map in notices
    if (!TOAST_KINDS.has(e.k)) continue;
    if (!notif[eventFamily(e)]) continue;
    const at = locateEvent(e);
    pushToast({
      text: e.text,
      sub: e.s || "",
      color: eventColor(e),
      x: at?.x, y: at?.y,
      ttl: 7000,
    });
    shown++;
  }
}

// ---------- selection ----------

function select(sel) {
  closePopovers();
  if (!sel) {
    setSelection(null);
    markDirty();
    return;
  }
  if (sel.kind === "settlement") {
    const s = world()?.header.settlements.find((x) => x.id === sel.id);
    if (s && sel.fly) view.flyTo(s.x + 0.5, s.y + 0.5, Math.max(view.scale, 6));
  } else if (sel.kind === "feature" && sel.fly) {
    const f = (world()?.header.features || [])[sel.id];
    if (f) {
      const big = f.t === "ocean" || f.t === "continent" || f.t === "sea";
      view.flyTo(f.x, f.y, big ? Math.max(view.scale, 1.6) : Math.max(view.scale, 5));
    }
  } else if (sel.kind === "deposit" && sel.fly) {
    view.flyTo(sel.x + 0.5, sel.y + 0.5, Math.max(view.scale, 9));
  } else if (sel.kind === "ruin" && sel.fly) {
    const r = (world()?.header.ruins || []).find((x) => x.eid === sel.id);
    if (r) view.flyTo(r.x + 0.5, r.y + 0.5, Math.max(view.scale, 6));
  } else if (sel.kind === "entity" && sel.fly) {
    const e = entities().find((x) => x.id === sel.id);
    if (e && e.x >= 0) view.flyTo(e.x + 0.5, e.y + 0.5, Math.max(view.scale, 6));
  }
  setSelection(sel);
  markDirty();
}

// tap/click (not drag, not pinch) picks the most specific thing under the
// cursor. On touch there is no hover, so a tap on ground inspects the cell.
let downAt = null, pointersDown = 0, multiTouch = false;
canvas.addEventListener("pointerdown", (e) => {
  pointersDown++;
  if (pointersDown > 1) multiTouch = true;
  downAt = [e.clientX, e.clientY];
  view.cancelFlight();
  closePopovers();
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
  const hit = pick(w, view, renderer, e.clientX, e.clientY, {
    touch,
    resourcesOn: overlays.resources,
    labelsOn: overlays.labels,
  });
  if (touch) { hover = null; setHoverTip(null); }
  if (!hit) { select(null); return; }
  if (hit.kind === "cell") {
    // clicking open ground: deselect if something was selected, else inspect
    const cur = selection();
    if (cur && cur.kind !== "cell") { select(null); return; }
    if (cur && cur.kind === "cell" && cur.x === hit.x && cur.y === hit.y) { select(null); return; }
  }
  select(hit);
});

// ---------- inspector data ----------

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

function inspectCell(cx, cy) {
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
  const cid = renderer.ownerCultureAt(i);
  if (cid >= 0) {
    const c = (w.header.cultures || [])[cid];
    if (c) {
      territory = c.vassal_of
        ? `Lands of the ${c.people} \u00b7 sworn to the ${c.vassal_of}`
        : `Lands of the ${c.people}`;
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
    notes: cellNotes(w, cx, cy, i, h, isWater),
  };
}

// ---------- hover tooltip ----------

let tipTimer = 0;
canvas.addEventListener("pointermove", (e) => {
  if (e.pointerType === "touch") return; // touch pans; taps inspect
  const w = world();
  if (!w) return;
  if (e.buttons) { hover = null; setHoverTip(null); return; } // dragging
  const [wx, wy] = view.screenToWorld(e.clientX, e.clientY);
  const cx = Math.floor(wx), cy = Math.floor(wy);
  const W = w.header.width || w.header.size;
  const H = w.header.size;
  const inWorld = cx >= 0 && cy >= 0 && cx < W && cy < H;
  hover = inWorld ? { x: cx, y: cy } : null;
  markDirty();
  clearTimeout(tipTimer);
  if (!inWorld) { setHoverTip(null); return; }
  // a light, throttled tooltip — the full story arrives on click
  tipTimer = setTimeout(() => {
    // settlement under cursor? tease its name instead of the ground
    const hit = pick(w, view, renderer, e.clientX, e.clientY, {
      resourcesOn: overlays.resources, labelsOn: false,
    });
    if (hit?.kind === "settlement") {
      const s = w.header.settlements.find((x) => x.id === hit.id);
      const c = (w.header.cultures || [])[s?.culture];
      if (s) {
        setHoverTip({
          px: e.clientX, py: e.clientY,
          title: s.name,
          sub: `${s.tier}${c ? ` of the ${c.people}` : ""} \u00b7 ${s.pop.toLocaleString("en-US")} souls`,
          line: "click to inspect",
        });
        return;
      }
    }
    if (hit?.kind === "ruin") {
      const r = (w.header.ruins || []).find((x) => x.eid === hit.id);
      if (r) {
        setHoverTip({
          px: e.clientX, py: e.clientY,
          title: r.name,
          sub: `abandoned Y${Math.floor(r.since / 12) + 1}${r.people ? ` \u00b7 once of the ${r.people}` : ""}`,
          line: "click to inspect",
        });
        return;
      }
    }
    if (hit?.kind === "deposit") {
      const meta = w.header.resources[hit.id] || {};
      setHoverTip({
        px: e.clientX, py: e.clientY,
        title: hit.id,
        sub: `${meta.category || "resource"} \u00b7 ${meta.abundance || ""}`,
        line: "click to inspect",
      });
      return;
    }
    const info = inspectCell(cx, cy);
    if (!info) { setHoverTip(null); return; }
    setHoverTip({
      px: e.clientX, py: e.clientY,
      title: info.place || info.biome,
      sub: info.place
        ? `${info.biome} \u00b7 ${info.elevation} m \u00b7 ${info.tempNow}\u00b0C`
        : `${info.elevation} m \u00b7 ${info.tempNow}\u00b0C${info.isWater ? "" : ` \u00b7 ${info.precip} mm`}`,
      line: info.notes?.[0] || (info.territory || ""),
    });
  }, 90);
});
canvas.addEventListener("pointerleave", (e) => {
  if (e.pointerType === "touch") return;
  hover = null;
  clearTimeout(tipTimer);
  setHoverTip(null);
  markDirty();
});

// ---------- explain (term ledgers from the engine) ----------

async function explain(kind, id) {
  try {
    return await explainWorld(kind, String(id));
  } catch {
    return null; // older engine or unknown entity: the dock just omits the ledger
  }
}

// ---------- the telling (M6.6) ----------

// The sifter reads the whole log, so the client asks sparingly: at most
// once per handful of sim-months, and only while someone is looking.
let legendsAt = -1000;
let legendsBusy = false;
async function refreshLegends(force = false) {
  if (!world() || legendsBusy) return;
  if (!force && month() - legendsAt < 6) return;
  legendsBusy = true;
  try {
    const [st, en, ar] = await Promise.all([
      storiesWorld(), entitiesWorld(), artifactsWorld(),
    ]);
    setStories(st);
    setEntities(en);
    setArtifacts(ar);
    legendsAt = month();
  } catch (err) {
    console.warn("the telling is silent:", err);
  } finally {
    legendsBusy = false;
  }
}

async function entityLog(id) {
  try {
    return await entityLogWorld(id);
  } catch {
    return [];
  }
}

// ---------- actions ----------

function fitView() {
  const w = world();
  if (w) view.fit(w.header.width || w.header.size, w.header.size);
}

const actions = {
  generate,
  setLayer: (id) => { setLayer(id); markDirty(); },
  toggleOverlay: (id) => { setOverlays(id, !overlays[id]); markDirty(); },
  playPause: () => setPlayingState(!playing()),
  step: () => advance(1),
  setSpeed,
  select,
  flyTo: (x, y, scale) => view.flyTo(x, y, Math.max(view.scale, scale || 6)),
  fitView,
  explain,
  inspectCell,
  locateEvent,
  popHistoryOf: (id) => popHistById.get(id) || [],
  priceHistoryOf: (g) => priceHist.get(g) || [],
  refreshLegends,
  entityLog,
};

mountUI(actions);

// ---------- keyboard ----------

window.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
  if (searchOpen()) return; // the omnibox owns the keys while open
  const k = e.key;
  if (k >= "1" && k <= "7") {
    const lens = LAYERS[Number(k) - 1];
    if (lens) { setLayer(lens[0]); markDirty(); }
  } else if (e.code === "Space") {
    e.preventDefault();
    setPlayingState(!playing());
  } else if (k === "n") {
    advance(1);
  } else if (k === "o") {
    const v = !overlaysOpen(); closePopovers(); setOverlaysOpen(v);
  } else if (k === "l") {
    const v = !legendOpen(); closePopovers(); setLegendOpen(v);
  } else if (k === "/") {
    e.preventDefault();
    setSearchOpen(true);
  } else if (k === "f") {
    fitView();
  } else if (k === "+" || k === "=") {
    view.flyTo(...view.screenToWorld(canvas.clientWidth / 2, canvas.clientHeight / 2), view.scale * 1.6, 240);
  } else if (k === "-") {
    view.flyTo(...view.screenToWorld(canvas.clientWidth / 2, canvas.clientHeight / 2), view.scale / 1.6, 240);
  } else if (k === "Escape") {
    if (worldMenuOpen() || overlaysOpen() || legendOpen() || notifOpen()) closePopovers();
    else if (isMobile() && sheet()) setSheet(null);
    else if (selection()) select(null);
  }
});

// ---------- boot ----------

const params = new URLSearchParams(location.search);
const bootSeed = params.get("seed") ? Number(params.get("seed")) : randomSeed();
const bootSize = Number(params.get("size"));
if ([384, 512, 640, 768].includes(bootSize)) setWorldSize(bootSize);
generate(String(bootSeed));
