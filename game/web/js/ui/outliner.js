// Outliner rail (right edge): Places / Peoples / Market / Chronicle.
// Collapsible to a slim tab; on mobile it becomes the Almanac sheet.

import { createMemo, createSignal, createEffect } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, wars, market, events, month, selection,
  outlinerOpen, setOutlinerOpen, outlinerTab, setOutlinerTab,
  placeSort, setPlaceSort, pins, togglePin, persistUi,
  chronFilter, setChronFilter, chronQuery, setChronQuery,
  seenEvents, setSeenEvents, isMobile, sheet,
} from "./state.js";
import {
  EVENT_FAMILIES, eventColor, eventFamily, FALLBACK_MONTHS, fmt,
} from "./config.js";
import { I } from "./icons.js";
import { settlementCss } from "../palette.js";

const monthsOf = () => world()?.header.months || FALLBACK_MONTHS;

const TIER_ORDER = { City: 0, Town: 1, Village: 2, Hamlet: 3, Camp: 4 };

// ---------------------------------------------------------------- places

function PlacesTab(a) {
  const sorted = createMemo(() => {
    const list = [...settlements()];
    const mode = placeSort();
    if (mode === "name") list.sort((x, y) => x.name.localeCompare(y.name));
    else if (mode === "tier") list.sort((x, y) =>
      (TIER_ORDER[x.tier] ?? 9) - (TIER_ORDER[y.tier] ?? 9) || y.pop - x.pop);
    else list.sort((x, y) => y.pop - x.pop);
    const p = pins();
    if (p.size) {
      const pinned = list.filter((s) => p.has(s.id));
      const rest = list.filter((s) => !p.has(s.id));
      return [...pinned, ...rest];
    }
    return list;
  });
  const [cap, setCap] = createSignal(60);
  const colorOf = (s) => (cultures() || [])[s.culture]?.color || settlementCss(s.id);
  return html`<div class="ol-tabbody">
    <div class="ol-toolbar">
      <div class="seg tiny">
        ${[["pop", "Souls"], ["name", "Name"], ["tier", "Rank"]].map(
          ([id, label]) => html`<button class=${() => (placeSort() === id ? "active" : "")}
            onClick=${() => { setPlaceSort(id); persistUi(); }}>${label}</button>`)}
      </div>
      <span class="ol-count">${() => settlements().length}</span>
    </div>
    <div class="ol-list">
      ${() => sorted().slice(0, cap()).map((s) => html`<div
        class=${() => "ol-row" + (selection()?.kind === "settlement" && selection()?.id === s.id ? " picked" : "")}
        onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
        <span class="s-dot" style=${`background:${colorOf(s)}`}></span>
        <span class="s-name">${s.name}</span>
        <span class="s-tier">${s.tier}</span>
        <span class="s-pop">${fmt(s.pop)}</span>
        <button class=${() => "pin-btn" + (pins().has(s.id) ? " on" : "")}
          title="Pin to top" aria-label="Pin"
          onClick=${(e) => { e.stopPropagation(); togglePin(s.id); }}>
          ${() => (pins().has(s.id) ? I.pinOn() : I.pin())}
        </button>
      </div>`)}
      ${() => (sorted().length > cap()
        ? html`<button class="more-btn" onClick=${() => setCap(cap() + 120)}>
            Show more (${sorted().length - cap()})</button>`
        : "")}
    </div>
  </div>`;
}

// ---------------------------------------------------------------- peoples

function PeoplesTab(a) {
  const rows = createMemo(() => {
    const setts = settlements();
    return (cultures() || []).map((c) => {
      const mine = setts.filter((s) => s.culture === c.id);
      return { ...c, n: mine.length, pop: mine.reduce((acc, s) => acc + s.pop, 0) };
    }).sort((x, y) => y.pop - x.pop);
  });
  const atWar = (id) => (wars() || []).some((w) => w.a === id || w.b === id);
  return html`<div class="ol-tabbody">
    ${() => ((wars() || []).length
      ? html`<div class="ol-wars">${(wars() || []).map((w) => {
          const cs = cultures() || [];
          return html`<button class="war-banner" onClick=${() => a.select({ kind: "war", id: w.name })}>
            \u2694 ${w.name} \u2014 ${cs[w.a]?.people || "?"} against ${cs[w.b]?.people || "?"}
          </button>`;
        })}</div>`
      : "")}
    <div class="ol-list">
      ${() => rows().map((c) => html`<div
        class=${() => "ol-row tall" + (selection()?.kind === "culture" && selection()?.id === c.id ? " picked" : "")}
        onClick=${() => a.select({ kind: "culture", id: c.id })}>
        <span class="s-dot" style=${`background:${c.color}`}></span>
        <span class="c-body">
          <span class="c-name">${c.people}${atWar(c.id) ? html` <span class="war-mark">\u2694</span>` : ""}</span>
          <span class="c-sub dim">${c.polity || ""}${c.era ? ` \u00b7 ${c.era}` : ""}${c.ruler ? ` \u00b7 ${c.ruler}` : ""}</span>
          <span class="c-sub dim">${c.n} holding${c.n === 1 ? "" : "s"} \u00b7 ${fmt(c.pop)} souls \u00b7 ${fmt(c.treasury || 0)} coin</span>
        </span>
      </div>`)}
      ${() => (!rows().length ? html`<div class="ol-empty dim">No peoples yet \u2014 the first camps are forming.</div>` : "")}
    </div>
  </div>`;
}

// ---------------------------------------------------------------- market

function MarketTab(a) {
  const rows = createMemo(() =>
    [...(market() || [])].sort((x, y) => y.p - x.p));
  return html`<div class="ol-tabbody">
    <div class="ol-list">
      ${() => rows().map((r) => {
        const meta = world()?.header.resources?.[r.g];
        const t = r.t > 0.02 ? "up" : r.t < -0.02 ? "down" : "";
        return html`<div class=${() => "ol-row" + (selection()?.kind === "good" && selection()?.id === r.g ? " picked" : "")}
          onClick=${() => a.select({ kind: "good", id: r.g })}>
          <span class="swatch" style=${`background:${meta?.color || "#8a8fa0"}`}></span>
          <span class="s-name">${r.g}</span>
          <span class=${"m-trend " + t}>${t === "up" ? "\u25b2" : t === "down" ? "\u25bc" : "\u00b7"}</span>
          <span class="m-price">${r.p.toFixed(2)}</span>
        </div>`;
      })}
      ${() => (!rows().length ? html`<div class="ol-empty dim">The first caravans are still loading.</div>` : "")}
    </div>
    ${() => (rows().length ? html`<div class="ol-foot dim">Coin for one load \u2014 scarcity sets the price.</div>` : "")}
  </div>`;
}

// ---------------------------------------------------------------- chronicle

function ChronicleTab(a) {
  const [cap, setCap] = createSignal(120);
  const filtered = createMemo(() => {
    const q = chronQuery().trim().toLowerCase();
    const out = [];
    const evs = events();
    for (let i = evs.length - 1; i >= 0; i--) {
      const e = evs[i];
      if (!chronFilter[eventFamily(e)]) continue;
      if (q && !e.text.toLowerCase().includes(q) && !(e.s || "").toLowerCase().includes(q)) continue;
      out.push(e);
    }
    return out;
  });
  // reading the chronicle clears the unseen badge
  createEffect(() => {
    if (outlinerTab() === "chronicle" && (outlinerOpen() || sheet() === "outliner")) {
      setSeenEvents(events().length);
    }
  });
  const flip = (f) => { setChronFilter(f, !chronFilter[f]); persistUi(); };
  return html`<div class="ol-tabbody">
    <div class="ol-toolbar wrap">
      <div class="chron-filters">
        ${EVENT_FAMILIES.map(([id, label]) => html`<button
          class=${() => "chip" + (chronFilter[id] ? " on" : "")}
          onClick=${() => flip(id)}>${label}</button>`)}
      </div>
      <input class="chron-search" type="search" placeholder="search the chronicle\u2026"
        value=${chronQuery()}
        onInput=${(e) => setChronQuery(e.target.value)} />
    </div>
    <div class="ol-list chron">
      ${() => filtered().slice(0, cap()).map((e) => {
        const months = monthsOf();
        const target = a.locateEvent(e);
        return html`<div class=${"event" + (target ? " clickable" : "")}
          onClick=${() => { if (target) a.flyTo(target.x, target.y, 8); }}>
          <span class="e-dot" title=${e.k || ""} style=${`background:${eventColor(e)}`}></span>
          <span class="e-when">Y${Math.floor(e.m / 12) + 1} ${(months[((e.m % 12) + 12) % 12] || "").slice(0, 3)}</span>
          <span class="e-text">${e.text}</span>
        </div>`;
      })}
      ${() => (filtered().length > cap()
        ? html`<button class="more-btn" onClick=${() => setCap(cap() + 240)}>
            Earlier entries (${filtered().length - cap()})</button>`
        : "")}
      ${() => (!filtered().length
        ? html`<div class="ol-empty dim">${events().length ? "Nothing matches." : "Nothing yet \u2014 let time pass."}</div>`
        : "")}
    </div>
  </div>`;
}

// ---------------------------------------------------------------- rail

const TABS = [
  ["places", "Places", I.place],
  ["peoples", "Peoples", I.people],
  ["market", "Market", I.market],
  ["chronicle", "Chronicle", I.book],
];

export function Outliner(a) {
  const unseen = () => Math.max(0, events().length - seenEvents());
  const pick = (id) => { setOutlinerTab(id); persistUi(); };
  const body = () => {
    switch (outlinerTab()) {
      case "peoples": return PeoplesTab(a);
      case "market": return MarketTab(a);
      case "chronicle": return ChronicleTab(a);
      default: return PlacesTab(a);
    }
  };
  return html`
    <button class=${() => "ol-opener" + (outlinerOpen() || isMobile() ? " hidden" : "")}
      title="Open the almanac" onClick=${() => { setOutlinerOpen(true); persistUi(); }}>
      ${I.chevL()}
      ${() => (unseen() > 0 ? html`<span class="badge">+${unseen()}</span>` : "")}
    </button>
    <aside class=${() => {
      let cls = "outliner";
      if (isMobile()) cls += sheet() === "outliner" ? " as-sheet open" : " as-sheet";
      else if (!outlinerOpen()) cls += " closed";
      return cls;
    }}>
      <div class="ol-tabs">
        ${TABS.map(([id, label, icon]) => html`<button
          class=${() => "ol-tab" + (outlinerTab() === id ? " active" : "")}
          title=${label} onClick=${() => pick(id)}>
          ${icon()}<span>${label}</span>
          ${() => (id === "chronicle" && unseen() > 0 && outlinerTab() !== "chronicle"
            ? html`<span class="badge">+${unseen()}</span>` : "")}
        </button>`)}
        <button class="ol-collapse desktop-only" title="Collapse"
          onClick=${() => { setOutlinerOpen(false); persistUi(); }}>${I.chevR()}</button>
      </div>
      ${body}
    </aside>`;
}
