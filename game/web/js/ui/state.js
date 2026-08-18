// Reactive world state (Solid signals/stores), shared by the UI tree and
// the simulation driver in main.js.

import { createSignal, createMemo } from "solid-js";
import { createStore } from "solid-js/store";

// ---------- simulation state ----------

export const [world, setWorld] = createSignal(null); // {header, arrays}
export const [settlements, setSettlements] = createSignal([]);
// ADR-0018 — the two axes: `cultures` holds the peoples rows (tongue, gods,
// era, arts — the wire key stays "cultures"), `realms` the political rows
// (crown, ruler, treasury, vassalage).
export const [cultures, setCultures] = createSignal([]);
export const [realms, setRealms] = createSignal([]);
// M13/ADR-0019 — the derived tier above both axes: named civilizations
// with their arc stage, drivers and member rosters.
export const [civs, setCivs] = createSignal([]);
export const [wars, setWars] = createSignal([]);
// M9.1 — what remains of towns that died: [{name, of, x, y, since, why, people, ety, eid}]
export const [ruins, setRuins] = createSignal([]);
export const [market, setMarket] = createSignal([]);
// M5.2 — {hubs:[{id,name,n,p}], of:[areaIdx per settlement], spread:[...]}
export const [areas, setAreas] = createSignal(null);
export const [merchants, setMerchants] = createSignal([]);
export const [events, setEvents] = createSignal([]);
// M6 — the telling: sifted microstories, the full cast, the relics.
export const [stories, setStories] = createSignal([]);
export const [entities, setEntities] = createSignal([]);
export const [artifacts, setArtifacts] = createSignal([]);
export const [month, setMonth] = createSignal(0);
export const [playing, setPlaying] = createSignal(false);
export const [speed, setSpeed] = createSignal(1);
export const [worldSize, setWorldSize] = createSignal(640);
export const [busy, setBusy] = createSignal(false);

// E8.5 — world population history rides a preallocated ring buffer: one
// write per tick, no array copy. Consumers key on popRev() and read
// through popSeries without allocating.
const POP_CAP = 600;
const popBuf = new Float64Array(POP_CAP);
let popStart = 0;
let popLen = 0;
export const [popRev, setPopRev] = createSignal(0);
export function pushPopSample(total) {
  if (popLen < POP_CAP) {
    popBuf[(popStart + popLen) % POP_CAP] = total;
    popLen++;
  } else {
    popBuf[popStart] = total;
    popStart = (popStart + 1) % POP_CAP;
  }
  setPopRev((r) => r + 1);
}
export function resetPopHistory() {
  popStart = 0;
  popLen = 0;
  setPopRev((r) => r + 1);
}
export const popSeries = {
  len: () => popLen,
  at: (i) => popBuf[(popStart + i) % POP_CAP],
};
// bumped whenever the deposit list changes mid-run (discoveries, dead mines)
export const [depositsTick, setDepositsTick] = createSignal(0);
// bumped whenever per-good price history gains a point
export const [marketTick, setMarketTick] = createSignal(0);

// ---------- view state ----------

export const [layer, setLayer] = createSignal("political");
export const [overlays, setOverlays] = createStore({
  rivers: true, snow: true, settlements: true, routes: true,
  resources: false, labels: true, winds: false, hillshade: true,
});

// Selection: one entity of any kind holds the inspector dock.
// {kind: "settlement"|"culture"|"realm"|"cell"|"deposit"|"feature"|"war"|"good", ...}
export const [selection, setSelection] = createSignal(null);

// E8.8 — O(1) settlement lookup, shared by every consumer that used to
// run find() per recompute.
export const settlementsById = createMemo(() => {
  const m = new Map();
  for (const s of settlements()) m.set(s.id, s);
  return m;
});

// The selected settlement object, kept fresh across ticks.
export const selectedSettlement = createMemo(() => {
  const sel = selection();
  if (!sel || sel.kind !== "settlement") return null;
  return settlementsById().get(sel.id) || null;
});

// Transient hover tooltip (desktop): {px, py, ...payload} | null
export const [hoverTip, setHoverTip] = createSignal(null);

export const [seenEvents, setSeenEvents] = createSignal(0);

// ---------- chrome state ----------

export const [worldMenuOpen, setWorldMenuOpen] = createSignal(false);
export const [overlaysOpen, setOverlaysOpen] = createSignal(false);
export const [legendOpen, setLegendOpen] = createSignal(false);
export const [searchOpen, setSearchOpen] = createSignal(false);
export const [notifOpen, setNotifOpen] = createSignal(false);

export function closePopovers() {
  setWorldMenuOpen(false);
  setOverlaysOpen(false);
  setLegendOpen(false);
  setNotifOpen(false);
}

// ---------- toasts ----------
// [{id, kind, text, sub, x, y, sticky}] — newest last, capped by the HUD.

let toastSeq = 0;
export const [toasts, setToasts] = createSignal([]);
export function pushToast(t) {
  const id = ++toastSeq;
  setToasts((list) => [...list, { id, ...t }].slice(-3));
  if (!t.sticky) setTimeout(() => dismissToast(id), t.ttl || 6500);
  return id;
}
export function dismissToast(id) {
  setToasts((list) => list.filter((t) => t.id !== id));
}

// ---------- outliner ----------

const UI_KEY = "calliope.ui.v2";
let savedUi = {};
try { savedUi = JSON.parse(localStorage.getItem(UI_KEY) || "{}"); } catch { /* fresh */ }

export const [outlinerOpen, setOutlinerOpen] = createSignal(savedUi.outlinerOpen !== false);
export const [outlinerTab, setOutlinerTab] = createSignal(savedUi.outlinerTab || "places");
export const [placeSort, setPlaceSort] = createSignal(savedUi.placeSort || "pop");
export const [pins, setPins] = createSignal(new Set(savedUi.pins || []));

// M6.9 — which layer of the telling the reader gets: the ground-truth
// chronicle ("plain") or the fireside legend ("songs").
export const [legendMode, setLegendMode] = createSignal(savedUi.legendMode || "plain");

// Chronicle filters: which event families show in the feed.
export const [chronFilter, setChronFilter] = createStore({
  realm: true, war: true, economy: true, myth: true, nature: true,
  ...(savedUi.chronFilter || {}),
});
export const [chronQuery, setChronQuery] = createSignal("");

// Notification channels: which event families raise toasts.
export const [notif, setNotif] = createStore({
  realm: true, war: true, economy: true, myth: false, nature: true,
  ...(savedUi.notif || {}),
});

export function persistUi() {
  try {
    localStorage.setItem(UI_KEY, JSON.stringify({
      outlinerOpen: outlinerOpen(),
      outlinerTab: outlinerTab(),
      placeSort: placeSort(),
      pins: [...pins()],
      legendMode: legendMode(),
      chronFilter: { ...chronFilter },
      notif: { ...notif },
    }));
  } catch { /* private mode */ }
}

export function togglePin(id) {
  const next = new Set(pins());
  next.has(id) ? next.delete(id) : next.add(id);
  setPins(next);
  persistUi();
}

// ---------- mobile chrome ----------
// On narrow screens the outliner and inspector become bottom sheets.

const mq = window.matchMedia("(max-width: 760px)");
export const [isMobile, setIsMobile] = createSignal(mq.matches);
mq.addEventListener("change", (e) => setIsMobile(e.matches));

export const [sheet, setSheet] = createSignal(null); // null | "outliner" | "inspector"
