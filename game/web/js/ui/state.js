// Reactive world state (Solid signals/stores), shared by the UI tree and
// the simulation driver in main.js.

import { createSignal, createMemo } from "solid-js";
import { createStore } from "solid-js/store";

// ---------- simulation state ----------

export const [world, setWorld] = createSignal(null); // {header, arrays}
export const [settlements, setSettlements] = createSignal([]);
export const [cultures, setCultures] = createSignal([]);
export const [wars, setWars] = createSignal([]);
export const [market, setMarket] = createSignal([]);
export const [events, setEvents] = createSignal([]);
export const [month, setMonth] = createSignal(0);
export const [playing, setPlaying] = createSignal(false);
export const [speed, setSpeed] = createSignal(1);
export const [worldSize, setWorldSize] = createSignal(640);
export const [busy, setBusy] = createSignal(false);
export const [popHistory, setPopHistory] = createSignal([]);
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
// {kind: "settlement"|"culture"|"cell"|"deposit"|"feature"|"war"|"good", ...}
export const [selection, setSelection] = createSignal(null);

// The selected settlement object, kept fresh across ticks.
export const selectedSettlement = createMemo(() => {
  const sel = selection();
  if (!sel || sel.kind !== "settlement") return null;
  return settlements().find((s) => s.id === sel.id) || null;
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
