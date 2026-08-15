// Solid UI — progressive-disclosure panels, contextual legends, detail
// panel and hover inspector. Mounted once by main.js with an actions object.

import { createEffect, createMemo, createSignal, on } from "solid-js";
import { render } from "solid-js/web";
import html from "solid-js/html";

import {
  world, settlements, cultures, events, month, playing, speed,
  worldSize, setWorldSize, busy, layer, overlays, selected, hoverInfo,
  popHistory, open, toggleOpen, seenEvents, setSeenEvents,
} from "./state.js";
import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, HYDRO_GRAD, FERT_GRAD,
  settlementCss,
} from "../palette.js";

export const LAYERS = [
  ["biomes", "Biomes"],
  ["elevation", "Elevation"],
  ["temperature", "Temperature"],
  ["precip", "Precipitation"],
  ["hydro", "Hydrology"],
  ["fertility", "Fertility"],
  ["political", "Political"],
];

export const OVERLAYS = [
  ["rivers", "Rivers"],
  ["snow", "Snow & sea ice"],
  ["settlements", "Settlements"],
  ["routes", "Trade routes"],
  ["resources", "Resources"],
  ["labels", "Place names"],
  ["winds", "Winds"],
  ["hillshade", "Relief shading"],
];

const STYLE_LABEL = {
  hellenic: "coastal south", nordic: "far north", arid: "desert marches",
  sylvan: "deep woods", steppe: "open plains", old: "old tongue",
};

const FALLBACK_MONTHS = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"];
const monthsOf = () => world()?.header.months || FALLBACK_MONTHS;
const fmt = (n) => Math.round(n).toLocaleString("en-US");

// ---------------------------------------------------------------- section

function chevron() {
  return html`<svg class="chev" viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">
    <path d="M5 3l6 5-6 5" fill="none" stroke="currentColor" stroke-width="1.8"
      stroke-linecap="round" stroke-linejoin="round"/></svg>`;
}

// Collapsible section: collapsed headers keep a one-line summary and an
// optional accent badge, so hidden state never goes silent.
function Section(p) {
  const isOpen = () => !!open[p.id];
  const summary = () => {
    if (isOpen()) return "";
    return typeof p.summary === "function" ? p.summary() : (p.summary || "");
  };
  return html`
    <section class=${() => "group disclosable" + (isOpen() ? " open" : "")}>
      <button class="group-head" aria-expanded=${() => String(isOpen())}
        onClick=${() => toggleOpen(p.id)}>
        ${chevron()}
        <span class="group-title">${p.title}</span>
        ${() => {
          const b = p.badge ? p.badge() : "";
          return b ? html`<span class="badge">${b}</span>` : "";
        }}
        <span class="group-summary">${summary}</span>
      </button>
      <div class="group-body" style=${() => (isOpen() ? "" : "display:none")}>
        ${p.children}
      </div>
    </section>`;
}

function gradCss(grad, lo, hi) {
  const stops = [];
  for (let i = 0; i <= 8; i++) {
    const [r, g, b] = grad(lo + ((hi - lo) * i) / 8);
    stops.push(`rgb(${r | 0},${g | 0},${b | 0}) ${((i / 8) * 100).toFixed(0)}%`);
  }
  return `linear-gradient(90deg,${stops.join(",")})`;
}

function gradLegend(grad, lo, hi, leftLabel, rightLabel) {
  return html`<div>
    <div class="grad-bar" style=${`background:${gradCss(grad, lo, hi)}`}></div>
    <div class="grad-labels"><span>${leftLabel}</span><span>${rightLabel}</span></div>
  </div>`;
}

// ---------------------------------------------------------------- left

function Brand() {
  const tagline = () => {
    const w = world();
    return w ? `The world of ${w.header.world_name}` : "The muse is shaping a world\u2026";
  };
  return html`<header class="brand">
    <h1>Calliope</h1>
    <div class="tagline">${tagline}</div>
  </header>`;
}

function WorldSection(a) {
  let input;
  createEffect(() => {
    const w = world();
    if (w && input) input.value = String(w.header.seed);
  });
  const summary = () => {
    const w = world();
    if (!w) return "\u2014";
    return `${w.header.seed} \u00b7 ${w.header.width || w.header.size}\u00d7${w.header.size}`;
  };
  const go = () => a.generate(input.value);
  return Section({
    id: "world", title: "World", summary,
    children: html`
      <div class="row">
        <input id="seed" type="text" inputmode="numeric" autocomplete="off"
          spellcheck="false" placeholder="seed"
          ref=${(el) => (input = el)}
          onKeyDown=${(e) => { if (e.key === "Enter") go(); }} />
        <button class="icon-btn" title="Random seed"
          onClick=${() => { input.value = String((Math.floor(Math.random() * 2147483646) + 1)); }}>
          <svg viewBox="0 0 24 24" width="14" height="14" fill="none"
            stroke="currentColor" stroke-width="2">
            <rect x="3" y="3" width="18" height="18" rx="4"/>
            <circle cx="8.5" cy="8.5" r="1.4" fill="currentColor" stroke="none"/>
            <circle cx="15.5" cy="15.5" r="1.4" fill="currentColor" stroke="none"/>
            <circle cx="15.5" cy="8.5" r="1.4" fill="currentColor" stroke="none"/>
            <circle cx="8.5" cy="15.5" r="1.4" fill="currentColor" stroke="none"/>
          </svg>
        </button>
      </div>
      <div class="row">
        <div class="seg">
          ${[256, 384, 512].map(
            (sz) => html`<button class=${() => (worldSize() === sz ? "active" : "")}
              onClick=${() => setWorldSize(sz)}>${sz}</button>`
          )}
        </div>
      </div>
      <button class="primary" disabled=${busy} onClick=${go}>Generate world</button>`,
  });
}

function LayerSection(a) {
  return Section({
    id: "layers", title: "Layer",
    summary: () => LAYERS.find(([id]) => id === layer())?.[1] || "",
    children: html`<div class="option-list">
      ${LAYERS.map(
        ([id, label]) => html`<div
          class=${() => "option" + (layer() === id ? " active" : "")}
          onClick=${() => a.setLayer(id)}>
          <span class="dot"></span><span>${label}</span>
        </div>`
      )}
    </div>`,
  });
}



function OverlaySection(a) {
  const onCount = () => OVERLAYS.filter(([id]) => overlays[id]).length;
  return Section({
    id: "overlays", title: "Overlays",
    summary: () => `${onCount()} of ${OVERLAYS.length} on`,
    children: html`<div class="option-list">
      ${OVERLAYS.map(
        ([id, label]) => html`<div
          class=${() => "option" + (overlays[id] ? " active" : "")}
          onClick=${() => a.toggleOverlay(id)}>
          <span class="check"><svg viewBox="0 0 24 24" width="10" height="10"><path d="M4 12.5l5 5L20 6.5" fill="none" stroke="#191307" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></svg></span>
          <span>${label}</span>
        </div>`
      )}
    </div>`,
  });
}

function TimeSection(a) {
  const dateText = () => {
    if (!world()) return "\u2014";
    const m = month();
    const months = monthsOf();
    return `Year ${Math.floor(m / 12) + 1} \u00b7 ${months[((m % 12) + 12) % 12]}`;
  };
  return html`<section class="group">
    <div class="group-title static">Time</div>
    <div class="date">${dateText}</div>
    <div class="row">
      <button class="icon-btn" title="Play / pause (space)" onClick=${() => a.playPause()}>
        ${() => (playing()
          ? html`<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>`
          : html`<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M7 5.5v13a1 1 0 0 0 1.5.87l11-6.5a1 1 0 0 0 0-1.74l-11-6.5A1 1 0 0 0 7 5.5z"/></svg>`)}
      </button>
      <button class="icon-btn" title="Step one month (n)" onClick=${() => a.step()}>
        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M5 5.5v13a1 1 0 0 0 1.5.87l9-6.5a1 1 0 0 0 0-1.74l-9-6.5A1 1 0 0 0 5 5.5z"/><rect x="17" y="5" width="3" height="14" rx="1"/></svg>
      </button>
      <div class="seg">
        ${[[1, "1\u00d7"], [3, "3\u00d7"], [12, "12\u00d7"]].map(
          ([v, label]) => html`<button class=${() => (speed() === v ? "active" : "")}
            onClick=${() => a.setSpeed(v)}>${label}</button>`
        )}
      </div>
    </div>
  </section>`;
}

function LeftPanel(a) {
  return html`<div class="panel-body">${Brand()}${WorldSection(a)}${LayerSection(a)}${OverlaySection(a)}${TimeSection(a)}</div>`;
}

// ---------------------------------------------------------------- right

function StatsSection() {
  let spark;
  const totalPop = createMemo(() => settlements().reduce((acc, s) => acc + s.pop, 0));
  const landStats = createMemo(() => {
    const w = world();
    if (!w) return null;
    const counts = new Map();
    const biomes = w.arrays.biomes;
    for (let i = 0; i < biomes.length; i++) {
      counts.set(biomes[i], (counts.get(biomes[i]) || 0) + 1);
    }
    const total = biomes.length;
    const land = total - (counts.get(0) || 0);
    const dominant = w.header.biomes
      .filter((b) => b.id !== 0 && counts.get(b.id))
      .sort((x, y) => (counts.get(y.id) || 0) - (counts.get(x.id) || 0))
      .slice(0, 3)
      .map((b) => b.name);
    return { pct: ((land / total) * 100).toFixed(1), dominant };
  });
  createEffect(() => {
    const hist = popHistory();
    const isOpen = open.stats;
    if (!spark || !isOpen) return;
    const ctx = spark.getContext("2d");
    ctx.clearRect(0, 0, spark.width, spark.height);
    if (hist.length > 1) {
      const min = Math.min(...hist.map((p) => p.pop));
      const max = Math.max(...hist.map((p) => p.pop));
      const span = Math.max(max - min, 1);
      ctx.beginPath();
      hist.forEach((p, i) => {
        const x = (i / (hist.length - 1)) * (spark.width - 4) + 2;
        const y = spark.height - 5 - ((p.pop - min) / span) * (spark.height - 12);
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      });
      ctx.strokeStyle = "#d4a94a";
      ctx.lineWidth = 1.6;
      ctx.lineJoin = "round";
      ctx.stroke();
    } else {
      ctx.fillStyle = "rgba(138,146,163,0.7)";
      ctx.font = "10.5px Inter, sans-serif";
      ctx.fillText("population over time \u2014 let the years pass", 4, 26);
    }
  });
  return Section({
    id: "stats", title: "The world",
    summary: () => (world() ? `${fmt(totalPop())} souls` : ""),
    children: html`<div class="stats">
      <div class="stat-row"><span class="dim">Land</span><span>${() => (landStats() ? `${landStats().pct}%` : "\u2014")}</span></div>
      <div class="stat-row"><span class="dim">Dominant</span><span>${() => landStats()?.dominant.join(", ") || "\u2014"}</span></div>
      <div class="stat-row"><span class="dim">Souls</span><span class="stat-mono">${() => fmt(totalPop())}</span></div>
      <div class="stat-row"><span class="dim">Settlements</span><span class="stat-mono">${() => settlements().length}</span></div>
      <canvas id="spark" width="220" height="44" ref=${(el) => (spark = el)}></canvas>
    </div>`,
  });
}

function LegendSection() {
  const label = () => LAYERS.find(([id]) => id === layer())?.[1] || "";
  const body = () => {
    const w = world();
    if (!w) return "";
    const l = layer();
    if (l === "biomes" || l === "political") {
      return html`<div class="legend">
        ${w.header.biomes
          .filter((b) => b.id !== 0)
          .map((b) => html`<div class="legend-item">
            <span class="swatch" style=${`background:rgb(${b.color.join(",")})`}></span>
            <span>${b.name}</span>
          </div>`)}
      </div>${l === "political"
        ? html`<div class="legend-note">Realms are tinted by their people \u2014 see Peoples below.</div>`
        : ""}`;
    }
    if (l === "elevation") return gradLegend(ELEV_LAND_GRAD, 0, 1, "sea level", "high peaks");
    if (l === "temperature") return gradLegend(TEMP_GRAD, -35, 35, "\u221235\u00b0C", "35\u00b0C");
    if (l === "precip") return gradLegend(PRECIP_GRAD, 0, 3000, "arid", "3000 mm");
    if (l === "hydro") return gradLegend(HYDRO_GRAD, 0, 1, "trickle", "torrent");
    if (l === "fertility") return gradLegend(FERT_GRAD, 0, 1, "barren", "black earth");
    return "";
  };
  return Section({
    id: "legend",
    title: "Legend",
    summary: label,
    children: html`<div class="legend-wrap">${body}</div>`,
  });
}

function ResourcesSection() {
  const list = () => {
    const w = world();
    if (!w) return [];
    const res = w.header.resources || {};
    const counts = new Map();
    for (const d of w.header.deposits || []) {
      counts.set(d.r, (counts.get(d.r) || 0) + 1);
    }
    return Object.keys(res)
      .filter((n) => !res[n].virtual)
      .sort((x, y) => (res[x].category + x).localeCompare(res[y].category + y))
      .map((n) => ({ name: n, meta: res[n], n: counts.get(n) || 0 }));
  };
  return Section({
    id: "resources", title: "Resources",
    summary: () => `${list().reduce((a, r) => a + r.n, 0)} deposits`,
    children: html`<div class="legend legend-res">
      ${() => list().map(
        (r) => html`<div class="legend-item"
          title=${`${r.meta.category} \u00b7 ${r.meta.abundance}` + (r.meta.requires ? ` \u00b7 requires ${r.meta.requires}` : "")}>
          <span class="swatch" style=${`background:${r.meta.color}`}></span>
          <span>${r.name}${r.n ? html` <span class="dim">\u00d7${r.n}</span>` : ""}</span>
        </div>`
      )}
    </div>`,
  });
}

function PeoplesSection() {
  // Selecting the political layer pulls this section open.
  createEffect(on(layer, (l) => {
    if (l === "political" && !open.peoples) toggleOpen("peoples");
  }, { defer: true }));
  const rows = () => {
    const setts = settlements();
    return (cultures() || []).map((c) => {
      const mine = setts.filter((s) => s.culture === c.id);
      return { ...c, n: mine.length, pop: mine.reduce((a, s) => a + s.pop, 0) };
    });
  };
  return Section({
    id: "peoples", title: "Peoples",
    summary: () => `${cultures().length}`,
    children: html`<div class="cultures">${() => rows().map(
      (c) => html`<div class="culture">
        <span class="s-dot" style=${`background:${c.color}`}></span>
        <span class="c-body">
          <span class="c-name">${c.people}</span>
          <span class="c-sub dim">${STYLE_LABEL[c.style] || c.style} \u00b7 ${c.n} settlement${c.n === 1 ? "" : "s"} \u00b7 ${fmt(c.pop)}</span>
        </span>
      </div>`
    )}</div>`,
  });
}

function SettlementsSection(a) {
  const [showAll, setShowAll] = createSignal(false);
  const sorted = createMemo(() => [...settlements()].sort((x, y) => y.pop - x.pop));
  const shown = () => (showAll() ? sorted() : sorted().slice(0, 7));
  const colorOf = (s) => {
    const c = (cultures() || [])[s.culture];
    return c?.color || settlementCss(s.id);
  };
  return Section({
    id: "settlements", title: "Settlements",
    summary: () => `${settlements().length}`,
    children: html`<div class="settlement-list">
      ${() => shown().map((s) => html`<div
        class=${() => "settlement" + (selected()?.id === s.id ? " picked" : "")}
        onClick=${() => a.pickSettlement(s)}>
        <span class="s-dot" style=${`background:${colorOf(s)}`}></span>
        <span class="s-name">${s.name}</span>
        <span class="s-tier">${s.tier}</span>
        <span class="s-pop">${fmt(s.pop)}</span>
      </div>`)}
      ${() => (sorted().length > 7
        ? html`<button class="more-btn" onClick=${() => setShowAll(!showAll())}>
            ${() => (showAll() ? "Show fewer" : `Show all ${sorted().length}`)}
          </button>`
        : "")}
    </div>`,
  });
}

function ChronicleSection() {
  const [showAll, setShowAll] = createSignal(false);
  const latest = createMemo(() => [...events()].reverse());
  const shown = () => (showAll() ? latest().slice(0, 40) : latest().slice(0, 6));
  const unseen = () => Math.max(0, events().length - seenEvents());
  createEffect(() => {
    if (open.chronicle) setSeenEvents(events().length);
  });
  return Section({
    id: "chronicle", title: "Chronicle",
    summary: () => (events().length ? `${events().length} entries` : "quiet so far"),
    badge: () => (!open.chronicle && unseen() > 0 ? `+${unseen()}` : ""),
    children: html`<div class="events">
      ${() => (shown().length === 0
        ? html`<div class="dim event-empty">Nothing yet \u2014 let time pass.</div>`
        : shown().map((e) => {
            const months = monthsOf();
            return html`<div class="event">
              <span class="e-when">Y${Math.floor(e.m / 12) + 1} ${(months[((e.m % 12) + 12) % 12] || "").slice(0, 3)}</span>
              <span>${e.text}</span>
            </div>`;
          }))}
      ${() => (latest().length > 6
        ? html`<button class="more-btn" onClick=${() => setShowAll(!showAll())}>
            ${() => (showAll() ? "Show fewer" : `Show more (${latest().length})`)}
          </button>`
        : "")}
    </div>`,
  });
}

function RightPanel(a) {
  return html`<div class="panel-body">${StatsSection()}${LegendSection()}${() =>
    (overlays.resources && world() ? ResourcesSection() : "")}${() =>
    (cultures().length ? PeoplesSection() : "")}${SettlementsSection(a)}${ChronicleSection()}</div>`;
}

// ---------------------------------------------------------------- detail

function DetailPanel(a) {
  const body = () => {
    const s = selected();
    if (!s) return "";
    const w = world();
    const culture = (cultures() || [])[s.culture];
    const resources = w?.header.resources || {};
    const goods = (s.goods || []).map((g) => {
      const m = resources[g];
      return html`<span class="d-good" style=${`--gc:${m?.color || "#ccc"}`}>${g}${
        g === s.exports ? html`<span class="d-star" title="chief export">\u2605</span>` : ""
      }</span>`;
    });
    const tags = [s.coastal ? "coastal" : null, s.river ? "fresh water" : null]
      .filter(Boolean);
    return html`
      <div class="d-head">
        <span class="d-name">${s.name}</span>
        <button class="d-close" aria-label="Close" onClick=${() => a.closeDetail()}>\u00d7</button>
      </div>
      <div class="d-row">
        <span class="s-dot" style=${`background:${culture?.color || "#999"}`}></span>
        <span>${s.tier} of the ${culture?.people || "first peoples"}</span>
      </div>
      <div class="d-stats">
        <div><span class="dim">Souls</span><b>${fmt(s.pop)}</b></div>
        <div><span class="dim">Food</span><b>${s.food}</b></div>
        <div><span class="dim">Routes</span><b>${s.connections ?? 0}</b></div>
      </div>
      ${tags.length ? html`<div class="d-tags">${tags.map((t) => html`<span class="d-tag">${t}</span>`)}</div>` : ""}
      ${goods.length ? html`<div class="d-goods-title dim">Produce</div><div class="d-goods">${goods}</div>` : ""}`;
  };
  return html`<div class=${() => "detail" + (selected() ? "" : " hidden")}>${body}</div>`;
}

// -------------------------------------------------------------- inspector

function Inspector() {
  const body = () => {
    const info = hoverInfo();
    if (!info) return "";
    const bits = [];
    bits.push(html`<span class="chip">${info.x}; ${info.y}</span>`);
    bits.push(html`<span class="i-biome">${info.biome}</span>`);
    bits.push(html`<span class="i-val"><b>${info.elevation}</b> m</span>`);
    bits.push(html`<span class="i-val"><b>${info.tempNow}</b> \u00b0C <span class="dim">(mean ${info.tempMean})</span></span>`);
    if (!info.isWater) bits.push(html`<span class="i-val"><b>${info.precip}</b> mm/yr</span>`);
    if (!info.isWater && info.fertility != null && info.fertility >= 0.05) {
      bits.push(html`<span class="i-val">Fertility <b>${Math.round(info.fertility * 100)}%</b></span>`);
    }
    if (info.wind) bits.push(html`<span class="i-val dim">${info.wind}</span>`);
    if (info.river) bits.push(html`<span class="i-val">River \u00b7 flow <b>${info.flow}</b></span>`);
    if (info.lake) bits.push(html`<span class="i-val">Lake</span>`);
    if (info.frozen) bits.push(html`<span class="i-val">${info.frozen}</span>`);
    for (const r of info.resources) {
      bits.push(html`<span class="i-res">\u25c6 ${r.name} <span class="dim">(${r.abundance}${r.requires ? `, requires ${r.requires}` : ""})</span></span>`);
    }
    if (info.place) bits.push(html`<span class="i-place">${info.place}</span>`);
    if (info.territory) bits.push(html`<span class="i-terr">${info.territory}</span>`);
    for (const n of info.notes || []) {
      bits.push(html`<span class="i-note">\u263c ${n}</span>`);
    }
    return bits;
  };
  return html`<div class=${() => "inspector" + (hoverInfo() ? "" : " hidden")}>${body}</div>`;
}

// ------------------------------------------------------------------ mount

export function mountUI(actions) {
  render(() => LeftPanel(actions), document.getElementById("left-root"));
  render(() => RightPanel(actions), document.getElementById("right-root"));
  render(() => DetailPanel(actions), document.getElementById("float-root"));
  render(() => Inspector(), document.getElementById("insp-root"));
}
