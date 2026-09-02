// Simulation driver: world generation, the flow of months, per-entity
// histories, the telling's refresh cadence, notifications, and crash
// recovery. Split from main.js (E8.7) — everything here answers "what
// happens when time passes or a world arrives?".

import { batch, createEffect, createResource, createRoot, createSignal } from "solid-js";

import {
  generateWorld, tickWorld, explainWorld, timingsWorld,
  storiesWorld, entitiesWorld, entityLogWorld, artifactsWorld,
  abortGenerate, onWorkerLost, onWorkerRestored,
} from "./net.js";
import {
  world, setWorld, setSettlements, setCultures, setRealms, setCivs, setWars,
  setEvents, events,
  setRuins, month, setMonth, setSky, playing, setPlaying, speed, worldSize,
  setBusy, setSelection, market, setMarket, setAreas, setMerchants,
  setDepositsTick, setMarketTick, pushToast, setSeenEvents,
  setStories, setEntities, setArtifacts, notif,
  pushPopSample, resetPopHistory,
} from "./ui/state.js";
import { eventFamily, eventColor } from "./ui/config.js";
import { buildDepositIndex, locateEvent } from "./inspect.js";

const $ = (id) => document.getElementById(id);

let ctx = null;

// ---------- seeds ----------

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

// ---------- per-entity histories, rebuilt for every world ----------

let popHistById = new Map();   // settlement id -> [pop,...]
let priceHist = new Map();     // good -> [price,...]

export const popHistoryOf = (id) => popHistById.get(id) || [];
export const priceHistoryOf = (g) => priceHist.get(g) || [];

function recordHistories(setts, marketRows) {
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
  // E8.5 — world total rides the preallocated ring, no array copy
  pushPopSample(setts.reduce((a, s) => a + s.pop, 0));
}

// ---------- generation ----------

// The muse's stage lines — generation progress narration (E7.5).
const STAGE_LINES = {
  terrain: "RAISING THE LAND",
  erosion: "CARVING THE VALLEYS",
  climate: "BREATHING WIND AND RAIN",
  hydrology: "DRAWING THE RIVERS",
  biomes: "CLOTHING THE WILDS",
  fertility: "SOWING THE SOILS",
  naming: "NAMING WHAT IS",
  resources: "HIDING THE DEEP SEAMS",
  dawn: "WAKING THE FIRST PEOPLES",
};

function setLoadingProgress(p) {
  const sub = $("loading-sub");
  const fill = $("loading-fill");
  if (!p) {
    if (sub) sub.textContent = "THE MUSE IS SHAPING A WORLD";
    if (fill) fill.style.width = "0%";
    return;
  }
  if (sub) sub.textContent = STAGE_LINES[p.stage] || p.stage.toUpperCase();
  if (fill) fill.style.width = `${Math.round((p.i / p.n) * 100)}%`;
}

// Everything the UI must do when a whole world arrives — used by generate
// and by crash recovery (E7.10), which restores in place without refitting.
function applyWorld(w, { fit = true } = {}) {
  popHistById = new Map();
  priceHist = new Map();
  batch(() => {
    setWorld(w);
    setMonth(w.header.month || 0);
    setSky(w.header.sky || 0);
    ctx.version.n++;
    setEvents(w.header.events || []);
    setSeenEvents((w.header.events || []).length);
    setSettlements(w.header.settlements);
    setCultures(w.header.cultures || []);
    setRealms(w.header.realms || []);
    setCivs(w.header.civs || []);
    setWars(w.header.wars || []);
    setRuins(w.header.ruins || []);
    setMarket(w.header.market || []);
    setAreas(w.header.areas || null);
    setMerchants(w.header.merchants || []);
    setStories([]);
    setEntities([]);
    setArtifacts([]);
    resetPopHistory();
    recordHistories(w.header.settlements, w.header.market);
  });
  ctx.renderer.setWorld(w);
  ctx.renderer.gpu?.setWorld(w);
  if (fit) ctx.view.fit(w.header.width || w.header.size, w.header.size);
  buildDepositIndex(w);
  legendsAt = -1000;
  refreshLegends(true); // the dawn already has a cast worth browsing
  history.replaceState(null, "", `?seed=${w.header.seed}&size=${w.header.size}`);
  ctx.markDirty();
}

export async function generate(rawSeed) {
  const seed = seedFrom(rawSeed);
  setPlayingState(false);
  setSelection(null);
  setBusy(true);
  setLoadingProgress(null);
  $("loading").classList.remove("fade");
  try {
    const w = await generateWorld(seed, worldSize(), setLoadingProgress);
    // stage timings live on a debug side channel, not the pack (E3.9)
    timingsWorld()
      .then((t) => console.debug("[calliope] generation timings (s)", t))
      .catch(() => {});
    applyWorld(w);
  } catch (err) {
    if (/abandoned/.test(err.message)) {
      pushToast({ text: "The world was abandoned unfinished.", ttl: 5000 });
    } else {
      console.error(err);
      pushToast({ kind: "error", text: `The muse falters: ${err.message}`, ttl: 9000 });
    }
  } finally {
    setBusy(false);
    $("loading").classList.add("fade");
  }
}

// ---------- time ----------

let tickInFlight = false;
export async function advance(months) {
  const w = world();
  if (!w || tickInFlight) return;
  tickInFlight = true;
  try {
    const res = await tickWorld(w.header.id, months);
    // E8.6 — one flush: every signal the tick touches settles in a single
    // batch, so multi-source consumers recompute once per month, not once
    // per setter.
    batch(() => {
      setMonth(res.month);
      // M89 — the sky scalar crosses only when it moved (E4.2)
      if (res.sky !== undefined) setSky(res.sky);
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
        ctx.renderer.setRoutes(res.routes); // colonies joined the network
      }
      if (res.cultures) {
        // peoples block (ADR-0018 slow axis): era, tech, divergence, death
        w.header.cultures = res.cultures;
        setCultures(res.cultures);
        ctx.renderer.setPeopleRoster(res.cultures);
      }
      if (res.realms) {
        // full realm block: cold half moved — succession, conquest, union
        w.header.realms = res.realms;
        setRealms(res.realms);
        ctx.renderer.setRealmRoster(res.realms);
      } else if (res.r_hot) {
        // heartbeat patch (E4.2): treasury/asab/legit over the held realms
        const rlms = (w.header.realms || []).slice();
        for (const { i, ...p } of res.r_hot) rlms[i] = { ...rlms[i], ...p };
        w.header.realms = rlms;
        setRealms(rlms);
      }
      if (res.civs) {
        // civilization tier moved (M13): stage turns, golden ages, falls
        w.header.civs = res.civs;
        setCivs(res.civs);
      }
      if (res.peoples) {
        // the tongue map moved (M10.6): assimilation, divergence, merging
        w.header.peoples = res.peoples;
        ctx.renderer.setPeoples(res.peoples);
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
        // borders moved wholesale: the engine redrew the political map (M4.1)
        ctx.renderer.setTerritory(res.territory);
      } else if (res.territory_tiles) {
        // borders moved locally: dirty 32×32 tiles patch the live grid (E4.7)
        ctx.renderer.applyTerritoryTiles(res.territory_tiles);
      }
      if (res.events?.length) {
        setEvents([...events(), ...res.events]);
      }
      // Histories stay aligned even when unchanged sections stayed home:
      // the merged settlements and the last-known market carry the series.
      recordHistories(w.header.settlements, res.market || market());
    });
    // E4.8 — toasts come from the engine-picked headline slice.
    if (res.headlines?.length) notifyEvents(res.headlines);
    ctx.version.n++;
    ctx.markDirty();
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

export const playPause = () => setPlayingState(!playing());
export const step = () => advance(1);

export function fitView() {
  const w = world();
  if (w) ctx.view.fit(w.header.width || w.header.size, w.header.size);
}

// ---------- notifications ----------

// Kinds worth interrupting for, even inside an enabled family.
const TOAST_KINDS = new Set([
  "war", "found", "ruler", "wonder", "disaster", "discovery",
  "depletion", "society", "tech", "myth",
]);

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

// ---------- explain (term ledgers from the engine) ----------

export async function explain(kind, id) {
  try {
    return await explainWorld(kind, String(id));
  } catch {
    return null; // older engine or unknown entity: the dock just omits the ledger
  }
}

// ---------- the telling (M6.6, E8.4) ----------

// The sifter reads the whole log, so the client asks sparingly: at most
// once per handful of sim-months, and only while someone is looking.
// The fetch rides createResource — in-flight dedupe and stale-race
// protection come built in; refreshLegends only decides *when* to ask.
let legendsAt = -1000;
let setLegendKey = () => {};

function initLegends() {
  createRoot(() => {
    const [legendKey, setKey] = createSignal(null);
    setLegendKey = (k) => setKey(k);
    const [sift] = createResource(legendKey, () =>
      Promise.all([storiesWorld(), entitiesWorld(), artifactsWorld()])
        .catch((err) => { console.warn("the telling is silent:", err); return null; }));
    createEffect(() => {
      const v = sift();
      if (!v) return;
      batch(() => {
        setStories(v[0]);
        setEntities(v[1]);
        setArtifacts(v[2]);
      });
    });
  });
}

export function refreshLegends(force = false) {
  if (!world()) return;
  if (!force && month() - legendsAt < 6) return;
  legendsAt = month();
  setLegendKey({ at: legendsAt, seed: world().header.seed });
}

export async function entityLog(id) {
  try {
    return await entityLogWorld(id);
  } catch {
    return [];
  }
}

// ---------- wiring ----------

export function initSim(c) {
  ctx = c;
  initLegends();

  // E7.4 — abandon a world mid-shaping; the previous world, if any, survives.
  $("loading-abort")?.addEventListener("click", () => abortGenerate());

  // E7.10 — the worker died mid-story: say so, hold the stage, and apply the
  // deterministically replayed world once the understudy catches up.
  onWorkerLost(() => {
    setPlayingState(false);
    setBusy(true);
    setLoadingProgress(null);
    $("loading").classList.remove("fade");
    pushToast({ kind: "error", text: "The muse stumbled \u2014 reweaving the world\u2026", ttl: 6000 });
  });
  onWorkerRestored((w) => {
    applyWorld(w, { fit: false });
    setBusy(false);
    $("loading").classList.add("fade");
    pushToast({ text: "The thread is rewoven; the chronicle stands.", ttl: 5000 });
  });
}
