// Reactive world state (Solid signals/stores), shared by the UI tree and
// the simulation driver in main.js.

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";

// ---------- simulation state ----------

export const [world, setWorld] = createSignal(null); // {header, arrays}
export const [settlements, setSettlements] = createSignal([]);
export const [cultures, setCultures] = createSignal([]);
export const [wars, setWars] = createSignal([]);
export const [events, setEvents] = createSignal([]);
export const [month, setMonth] = createSignal(0);
export const [playing, setPlaying] = createSignal(false);
export const [speed, setSpeed] = createSignal(1);
export const [worldSize, setWorldSize] = createSignal(512);
export const [busy, setBusy] = createSignal(false);
export const [popHistory, setPopHistory] = createSignal([]);

// ---------- view state ----------

export const [layer, setLayer] = createSignal("biomes");
export const [overlays, setOverlays] = createStore({
  rivers: true, snow: true, settlements: true, routes: true,
  resources: false, labels: true, winds: false, hillshade: true,
});
export const [selected, setSelected] = createSignal(null); // settlement | null
export const [hoverInfo, setHoverInfo] = createSignal(null);
export const [seenEvents, setSeenEvents] = createSignal(0);

// ---------- mobile chrome ----------
// On narrow screens the side panels become bottom sheets driven by a tab bar.

const mq = window.matchMedia("(max-width: 760px)");
export const [isMobile, setIsMobile] = createSignal(mq.matches);
mq.addEventListener("change", (e) => setIsMobile(e.matches));

export const [sheet, setSheet] = createSignal(null); // null | "world" | "almanac"

// ---------- progressive disclosure ----------
// Sections remember whether the reader left them open; everything else is
// summarised in the collapsed header so nothing goes silent.

const DISCLOSURE_KEY = "calliope.disclosure.v1";
let saved = {};
try { saved = JSON.parse(localStorage.getItem(DISCLOSURE_KEY) || "{}"); } catch { /* fresh */ }

export const [open, setOpen] = createStore({
  world: false,      // seed & size: summarised once a world exists
  layers: true,      // primary control, open
  overlays: false,   // summarised as "n of m"
  stats: true,
  legend: false,     // contextual to the active layer
  resources: true,   // only rendered while the overlay is on
  peoples: false,    // auto-opens with the political layer
  settlements: true,
  chronicle: false,  // collects a badge while collapsed
  ...saved,
});

export function toggleOpen(id) {
  setOpen(id, !open[id]);
  try { localStorage.setItem(DISCLOSURE_KEY, JSON.stringify({ ...open })); } catch { /* private mode */ }
}
