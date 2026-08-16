// Outliner rail (right edge): Places / Peoples / Market / Chronicle.
// Collapsible to a slim tab; on mobile it becomes the Almanac sheet.

import { createMemo, createSignal, createEffect } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, wars, market, areas, merchants,
  events, month, selection, settlementsById,
  stories, entities, artifacts, legendMode, setLegendMode,
  outlinerOpen, setOutlinerOpen, outlinerTab, setOutlinerTab,
  placeSort, setPlaceSort, pins, togglePin, persistUi,
  chronFilter, setChronFilter, chronQuery, setChronQuery,
  seenEvents, setSeenEvents, isMobile, sheet,
} from "./state.js";
import {
  EVENT_FAMILIES, eventColor, eventFamily, FALLBACK_MONTHS, fmt,
  patternMeta, entityKind,
} from "./config.js";
import { I } from "./icons.js";
import { each, eachIdx } from "./list.js";
import { roveTabs } from "./focus.js";
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
  const shown = createMemo(() => sorted().slice(0, cap()));
  const colorOf = (s) => (cultures() || [])[s.culture]?.color || settlementCss(s.id);
  // E8.2 — keyed on settlement identity: a tick that patches three towns
  // re-renders three rows; the rest keep their DOM.
  return html`<div class="ol-tabbody">
    <div class="ol-toolbar">
      <div class="seg tiny">
        ${[["pop", "Souls"], ["name", "Name"], ["tier", "Rank"]].map(
          ([id, label]) => html`<button class=${() => (placeSort() === id ? "active" : "")}
            aria-pressed=${() => String(placeSort() === id)}
            onClick=${() => { setPlaceSort(id); persistUi(); }}>${label}</button>`)}
      </div>
      <span class="ol-count">${() => settlements().length}</span>
    </div>
    <div class="ol-list">
      ${each(shown, (s) => html`<div
        class=${() => "ol-row" + (selection()?.kind === "settlement" && selection()?.id === s.id ? " picked" : "")}
        onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
        <span class="s-dot" style=${`background:${colorOf(s)}`}></span>
        <span class="s-name">${s.name}</span>
        <span class="s-tier">${s.tier}</span>
        <span class="s-pop">${fmt(s.pop)}</span>
        <button class=${() => "pin-btn" + (pins().has(s.id) ? " on" : "")}
          title="Pin to top" aria-label="Pin"
          aria-pressed=${() => String(pins().has(s.id))}
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
  // E8.2 — position-keyed: the row objects are rebuilt per tick, so Index
  // updates text in place instead of remaking DOM.
  return html`<div class="ol-tabbody">
    ${() => ((wars() || []).length
      ? html`<div class="ol-wars">${eachIdx(() => wars() || [], (w) => {
          const side = (id) => (cultures() || [])[id]?.people || "?";
          return html`<button class="war-banner" onClick=${() => a.select({ kind: "war", id: w().name })}>
            \u2694 ${() => w().name} \u2014 ${() => side(w().a)} against ${() => side(w().b)}
          </button>`;
        })}</div>`
      : "")}
    <div class="ol-list">
      ${eachIdx(rows, (c) => html`<div
        class=${() => "ol-row tall" + (selection()?.kind === "culture" && selection()?.id === c().id ? " picked" : "")}
        onClick=${() => a.select({ kind: "culture", id: c().id })}>
        <span class="s-dot" style=${() => `background:${c().color}`}></span>
        <span class="c-body">
          <span class="c-name">${() => c().people}${() => (atWar(c().id) ? html` <span class="war-mark">\u2694</span>` : "")}</span>
          <span class="c-sub dim">${() => `${c().polity || ""}${c().era ? ` \u00b7 ${c().era}` : ""}${c().ruler ? ` \u00b7 ${c().ruler}` : ""}`}</span>
          <span class="c-sub dim">${() => `${c().n} holding${c().n === 1 ? "" : "s"} \u00b7 ${fmt(c().pop)} souls \u00b7 ${fmt(c().treasury || 0)} coin`}</span>
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
  const spread = () => areas()?.spread || [];
  const traders = createMemo(() =>
    [...(merchants() || [])].sort((x, y) => (y.alive - x.alive) || (y.wealth - x.wealth)).slice(0, 8));
  const hubById = (id) => settlementsById().get(id);
  return html`<div class="ol-tabbody">
    <div class="ol-list">
      ${() => (spread().length ? html`<div class="ol-sect dim">Widest price gaps</div>` : "")}
      ${eachIdx(spread, (r) => html`<div class="ol-row gap"
        onClick=${() => a.select({ kind: "good", id: r().g })}>
        <span class="swatch" style=${() => `background:${world()?.header.resources?.[r().g]?.color || "#8a8fa0"}`}></span>
        <span class="s-name">${() => r().g}</span>
        <span class="c-sub dim gap-route">${() => `${r().lo.hub} ${r().lo.p.toFixed(1)} \u2192 ${r().hi.hub} ${r().hi.p.toFixed(1)}`}</span>
        <span class="m-price">${() => `\u00d7${r().ratio.toFixed(1)}`}</span>
      </div>`)}
      ${() => (spread().length ? html`<div class="ol-sect dim">All goods \u00b7 world mean</div>` : "")}
      ${eachIdx(rows, (r) => {
        const meta = () => world()?.header.resources?.[r().g];
        const t = () => (r().t > 0.02 ? "up" : r().t < -0.02 ? "down" : "");
        return html`<div class=${() => "ol-row" + (selection()?.kind === "good" && selection()?.id === r().g ? " picked" : "")}
          onClick=${() => a.select({ kind: "good", id: r().g })}>
          <span class="swatch" style=${() => `background:${meta()?.color || "#8a8fa0"}`}></span>
          <span class="s-name">${() => r().g}</span>
          <span class=${() => "m-trend " + t()}>${() => (t() === "up" ? "\u25b2" : t() === "down" ? "\u25bc" : "\u00b7")}</span>
          <span class="m-price">${() => r().p.toFixed(2)}</span>
        </div>`;
      })}
      ${() => (!rows().length ? html`<div class="ol-empty dim">The first caravans are still loading.</div>` : "")}
      ${() => (traders().length ? html`<div class="ol-sect dim">Merchants on the roads</div>` : "")}
      ${eachIdx(traders, (m) => {
        const home = () => hubById(m().home);
        return html`<div class=${() => "ol-row" + (m().alive ? "" : " gone")}
          onClick=${() => { const h = home(); if (h) a.select({ kind: "settlement", id: h.id, fly: true }); }}>
          <span class="s-name">${() => m().name}</span>
          <span class="c-sub dim">${() => `${home() ? `of ${home().name}` : ""}${m().alive ? "" : ` \u00b7 ${m().fate || "gone"}`}`}</span>
          <span class="m-price">${() => fmt(Math.round(m().wealth))}</span>
        </div>`;
      })}
    </div>
    ${() => (rows().length ? html`<div class="ol-foot dim">Coin for one load \u2014 each market area prices its own.</div>` : "")}
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
      if (q && !e.text.toLowerCase().includes(q) && !(e.s || "").toLowerCase().includes(q)
        && !(e.legend || "").toLowerCase().includes(q)) continue;
      out.push(e);
    }
    return out;
  });
  // E8.3 — the feed is a window over the filtered log: DOM is capped, and
  // because rows are keyed on the event objects (which persist for the
  // world's whole life), a tick prepends its few new rows and leaves the
  // rest of the window's DOM untouched.
  const shown = createMemo(() => filtered().slice(0, cap()));
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
          aria-pressed=${() => String(!!chronFilter[id])}
          onClick=${() => flip(id)}>${label}</button>`)}
        <button class=${() => "chip fireside" + (legendMode() === "songs" ? " on" : "")}
          aria-pressed=${() => String(legendMode() === "songs")}
          title="Read the fireside telling \u2014 numbers blur into song (M6.9)"
          onClick=${() => { setLegendMode(legendMode() === "songs" ? "plain" : "songs"); persistUi(); }}>
          \u266a Fireside</button>
      </div>
      <input class="chron-search" type="search" placeholder="search the chronicle\u2026"
        value=${chronQuery()}
        onInput=${(e) => setChronQuery(e.target.value)} />
    </div>
    <div class="ol-list chron">
      ${each(shown, (e) => {
        const months = monthsOf();
        const target = a.locateEvent(e);
        return html`<div class=${"event" + (target ? " clickable" : "")}
          onClick=${() => { if (target) a.flyTo(target.x, target.y, 8); }}>
          <span class="e-dot" title=${e.k || ""} style=${`background:${eventColor(e)}`}></span>
          <span class="e-when">Y${Math.floor(e.m / 12) + 1} ${(months[((e.m % 12) + 12) % 12] || "").slice(0, 3)}</span>
          <span class=${() => "e-text" + (legendMode() === "songs" && e.legend ? " sung" : "") + (e.veiled ? " veiled" : "")}
            title=${e.veiled ? "The chronicle is not sure of this (M9.5)" : ""}>
            ${() => (legendMode() === "songs" && e.legend ? e.legend : e.text)}</span>
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

// ---------------------------------------------------------------- legends
// M6.6 — the browser over the telling: sifted sagas, the relics with
// their keepers, and the whole cast of the chronicle, searchable.

const CAST_ORDER = {
  person: 0, artifact: 1, war: 2, ruin: 3,
  culture: 4, settlement: 5, feature: 6,
};

function LegendsTab(a) {
  // ask the engine for a fresh sift while the reader is looking
  createEffect(() => {
    month();
    if (outlinerTab() === "legends" && (outlinerOpen() || sheet() === "outliner")) {
      a.refreshLegends();
    }
  });
  const [castQ, setCastQ] = createSignal("");
  const [cap, setCap] = createSignal(30);
  const cast = createMemo(() => {
    const q = castQ().trim().toLowerCase();
    let list = entities().filter((e) => e.kind !== "good" && e.kind !== "world");
    if (q) list = list.filter((e) => e.name.toLowerCase().includes(q));
    return [...list].sort((x, y) =>
      (CAST_ORDER[x.kind] ?? 9) - (CAST_ORDER[y.kind] ?? 9) || y.since - x.since);
  });
  const castShown = createMemo(() => cast().slice(0, cap()));
  const relics = createMemo(() =>
    [...(artifacts() || [])].sort((x, y) => (x.lost - y.lost) || (x.made - y.made)));
  const holderName = (id) => settlementsById().get(id)?.name;
  const isPicked = (s) =>
    selection()?.kind === "story" && selection()?.story?.pattern === s.pattern
    && selection()?.story?.title === s.title;
  const setMode = (m) => { setLegendMode(m); persistUi(); };
  // E8.2 — the sift returns fresh JSON each pass, so these lists are
  // position-keyed: rows update their text in place across sifts.
  return html`<div class="ol-tabbody">
    <div class="ol-toolbar">
      <div class="seg tiny">
        <button class=${() => (legendMode() === "plain" ? "active" : "")}
          aria-pressed=${() => String(legendMode() === "plain")}
          title="The chronicle as it happened"
          onClick=${() => setMode("plain")}>As it was</button>
        <button class=${() => (legendMode() === "songs" ? "active" : "")}
          aria-pressed=${() => String(legendMode() === "songs")}
          title="The fireside telling \u2014 numbers blur, songs embroider"
          onClick=${() => setMode("songs")}>As sung</button>
      </div>
      <span class="ol-count">${() => stories().length}</span>
    </div>
    <div class="ol-list">
      ${() => (stories().length ? html`<div class="ol-sect dim">Sagas the sifter found</div>` : "")}
      ${eachIdx(stories, (s) => html`<div
        class=${() => "ol-row tall saga" + (isPicked(s()) ? " picked" : "")}
        onClick=${() => a.select({ kind: "story", story: s() })}>
        <span class="s-dot" style=${() => `background:${patternMeta(s().pattern).color}`}></span>
        <span class="c-body">
          <span class="c-name">${() => s().title}</span>
          <span class="c-sub dim">${() => `${patternMeta(s().pattern).label}
            \u00b7 Y${s().y0}${s().y1 !== s().y0 ? `\u2013${s().y1}` : ""}
            \u00b7 ${s().beats.length} beat${s().beats.length === 1 ? "" : "s"}`}</span>
        </span>
      </div>`)}
      ${() => (!stories().length
        ? html`<div class="ol-empty dim">No sagas yet \u2014 the sifter wants years, wars and reversals. Let time pass.</div>`
        : "")}
      ${() => (relics().length ? html`<div class="ol-sect dim">Relics & their keepers</div>` : "")}
      ${eachIdx(relics, (r) => html`<div
        class=${() => "ol-row" + (r().lost ? " gone" : "")
          + (selection()?.kind === "entity" && selection()?.id === r().ent ? " picked" : "")}
        onClick=${() => a.select({ kind: "entity", id: r().ent, fly: !r().lost })}>
        <span class="s-dot" style=${`background:${entityKind("artifact").color}`}></span>
        <span class="s-name">${() => r().name}</span>
        <span class="c-sub dim">${() => (r().lost ? "lost" : holderName(r().holder) ? `at ${holderName(r().holder)}` : "\u2014")}</span>
      </div>`)}
      <div class="ol-sect dim cast-head">The cast
        <input class="chron-search cast-search" type="search" placeholder="search the cast\u2026"
          value=${castQ()} onInput=${(e) => setCastQ(e.target.value)} />
      </div>
      ${eachIdx(castShown, (e) => html`<div
        class=${() => "ol-row" + (selection()?.kind === "entity" && selection()?.id === e().id ? " picked" : "")}
        onClick=${() => a.select({ kind: "entity", id: e().id })}>
        <span class="s-dot" style=${() => `background:${entityKind(e().kind).color}`}></span>
        <span class=${() => "s-name" + (e().until != null ? " dim" : "")}>${() => e().name}</span>
        <span class="s-tier">${() => e().role || entityKind(e().kind).label}</span>
        <span class="s-pop">${() => (e().until != null ? "\u2020" : `Y${Math.floor(e().since / 12) + 1}`)}</span>
      </div>`)}
      ${() => (cast().length > cap()
        ? html`<button class="more-btn" onClick=${() => setCap(cap() + 60)}>
            More of the cast (${cast().length - cap()})</button>`
        : "")}
      ${() => (!cast().length ? html`<div class="ol-empty dim">No one answers to that name.</div>` : "")}
    </div>
    <div class="ol-foot dim">\u2020 marks a story that has ended \u00b7 click anyone for their whole tale.</div>
  </div>`;
}

// ---------------------------------------------------------------- rail

const TABS = [
  ["places", "Places", I.place],
  ["peoples", "Peoples", I.people],
  ["market", "Market", I.market],
  ["chronicle", "Chronicle", I.book],
  ["legends", "Legends", I.quill],
];

export function Outliner(a) {
  const unseen = () => Math.max(0, events().length - seenEvents());
  const pick = (id) => { setOutlinerTab(id); persistUi(); };
  const body = () => {
    switch (outlinerTab()) {
      case "peoples": return PeoplesTab(a);
      case "market": return MarketTab(a);
      case "chronicle": return ChronicleTab(a);
      case "legends": return LegendsTab(a);
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
        <div role="tablist" aria-label="Almanac" style="display:contents"
          ref=${(el) => roveTabs(el)}>
          ${TABS.map(([id, label, icon]) => html`<button
            class=${() => "ol-tab" + (outlinerTab() === id ? " active" : "")}
            role="tab" aria-selected=${() => String(outlinerTab() === id)}
            title=${label} onClick=${() => pick(id)}>
            ${icon()}<span>${label}</span>
            ${() => (id === "chronicle" && unseen() > 0 && outlinerTab() !== "chronicle"
              ? html`<span class="badge">+${unseen()}</span>` : "")}
          </button>`)}
        </div>
        <button class="ol-collapse desktop-only" title="Collapse"
          onClick=${() => { setOutlinerOpen(false); persistUi(); }}>${I.chevR()}</button>
      </div>
      ${body}
    </aside>`;
}
