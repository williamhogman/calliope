// DOM: option lists, legend, settlement list, chronicle, inspector.

import { settlementCss } from "./palette.js";

const CHECK_SVG = `<svg viewBox="0 0 24 24" width="10" height="10"><path d="M4 12.5l5 5L20 6.5" fill="none" stroke="#191307" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

export const LAYERS = [
  ["biomes", "Biomes"],
  ["elevation", "Elevation"],
  ["temperature", "Temperature"],
  ["precip", "Precipitation"],
  ["hydro", "Hydrology"],
  ["political", "Political"],
];

export const OVERLAYS = [
  ["rivers", "Rivers"],
  ["snow", "Snow & sea ice"],
  ["settlements", "Settlements"],
  ["routes", "Trade routes"],
  ["resources", "Resources"],
  ["hillshade", "Relief shading"],
];

export function renderStats(el, world, popHistory) {
  const { header, arrays } = world;
  const size = header.size;
  const total = size * size;
  const counts = new Map();
  for (let i = 0; i < arrays.biomes.length; i++) {
    counts.set(arrays.biomes[i], (counts.get(arrays.biomes[i]) || 0) + 1);
  }
  const land = total - (counts.get(0) || 0);
  const byName = header.biomes
    .filter((b) => b.id !== 0 && counts.get(b.id))
    .sort((a, b) => (counts.get(b.id) || 0) - (counts.get(a.id) || 0))
    .slice(0, 3);
  const pop = popHistory.length ? popHistory[popHistory.length - 1].pop : 0;

  el.innerHTML = `
    <div class="stat-row"><span class="dim">Seed</span><span class="stat-mono">${header.seed}</span></div>
    <div class="stat-row"><span class="dim">Land</span><span>${((land / total) * 100).toFixed(1)}%</span></div>
    <div class="stat-row"><span class="dim">Dominant</span><span>${byName.map((b) => b.name).join(", ") || "—"}</span></div>
    <div class="stat-row"><span class="dim">Souls</span><span class="stat-mono">${pop.toLocaleString("en-US")}</span></div>
    <canvas id="spark" width="220" height="44"></canvas>`;

  const c = el.querySelector("#spark");
  const ctx = c.getContext("2d");
  ctx.clearRect(0, 0, c.width, c.height);
  if (popHistory.length > 1) {
    const min = Math.min(...popHistory.map((p) => p.pop));
    const max = Math.max(...popHistory.map((p) => p.pop));
    const span = Math.max(max - min, 1);
    ctx.beginPath();
    popHistory.forEach((p, i) => {
      const x = (i / (popHistory.length - 1)) * (c.width - 4) + 2;
      const y = c.height - 5 - ((p.pop - min) / span) * (c.height - 12);
      if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = "#d4a94a";
    ctx.lineWidth = 1.6;
    ctx.lineJoin = "round";
    ctx.stroke();
  } else {
    ctx.fillStyle = "rgba(138,146,163,0.7)";
    ctx.font = "10.5px Inter, sans-serif";
    ctx.fillText("population over time — let the years pass", 4, 26);
  }
}

export function buildLayerList(el, current, onPick) {
  el.innerHTML = "";
  for (const [id, label] of LAYERS) {
    const row = document.createElement("div");
    row.className = "option" + (id === current ? " active" : "");
    row.dataset.id = id;
    row.innerHTML = `<span class="dot"></span><span>${label}</span>`;
    row.addEventListener("click", () => {
      el.querySelectorAll(".option").forEach((r) => r.classList.remove("active"));
      row.classList.add("active");
      onPick(id);
    });
    el.appendChild(row);
  }
}

export function buildOverlayList(el, state, onToggle) {
  el.innerHTML = "";
  for (const [id, label] of OVERLAYS) {
    const row = document.createElement("div");
    row.className = "option" + (state[id] ? " active" : "");
    row.innerHTML = `<span class="check">${CHECK_SVG}</span><span>${label}</span>`;
    row.addEventListener("click", () => {
      row.classList.toggle("active");
      onToggle(id, row.classList.contains("active"));
    });
    el.appendChild(row);
  }
}

export function buildLegend(el, biomes) {
  el.innerHTML = "";
  for (const b of biomes) {
    if (b.id === 0) continue; // water reads from the map itself
    const item = document.createElement("div");
    item.className = "legend-item";
    item.innerHTML = `<span class="swatch" style="background:rgb(${b.color.join(",")})"></span><span>${b.name}</span>`;
    el.appendChild(item);
  }
}

export function buildResourceLegend(el, resources) {
  el.innerHTML = "";
  const names = Object.keys(resources).sort(
    (a, b) => (resources[a].category + a).localeCompare(resources[b].category + b));
  for (const name of names) {
    const m = resources[name];
    const item = document.createElement("div");
    item.className = "legend-item";
    item.title = `${m.category} · ${m.abundance}` + (m.requires ? ` · requires ${m.requires}` : "");
    item.innerHTML = `<span class="swatch" style="background:${m.color}"></span><span>${name}</span>`;
    el.appendChild(item);
  }
}

export function renderSettlements(el, popEl, settlements, onPick) {
  const sorted = [...settlements].sort((a, b) => b.pop - a.pop);
  const total = settlements.reduce((acc, s) => acc + s.pop, 0);
  popEl.textContent = ` · ${total.toLocaleString("en-US")} souls`;
  el.innerHTML = "";
  for (const s of sorted) {
    const row = document.createElement("div");
    row.className = "settlement";
    row.innerHTML = `
      <span class="s-dot" style="background:${settlementCss(s.id)}"></span>
      <span class="s-name">${s.name}</span>
      <span class="s-tier">${s.tier}</span>
      <span class="s-pop">${s.pop.toLocaleString("en-US")}</span>`;
    row.addEventListener("click", () => onPick(s));
    el.appendChild(row);
  }
}

export function renderEvents(el, events, months) {
  el.innerHTML = "";
  if (!events.length) {
    el.innerHTML = `<div class="dim event-empty">Nothing yet — let time pass.</div>`;
    return;
  }
  const latest = events.slice(-14).reverse();
  for (const e of latest) {
    const row = document.createElement("div");
    row.className = "event";
    const year = Math.floor(e.m / 12) + 1;
    const mon = months[((e.m % 12) + 12) % 12];
    row.innerHTML = `<span class="e-when">Y${year} ${mon.slice(0, 3)}</span><span>${e.text}</span>`;
    el.appendChild(row);
  }
}

export function renderInspector(el, info) {
  if (!info) {
    el.classList.add("hidden");
    return;
  }
  el.classList.remove("hidden");
  const bits = [];
  bits.push(`<span class="chip">${info.x}; ${info.y}</span>`);
  bits.push(`<span class="i-biome">${info.biome}</span>`);
  bits.push(`<span class="i-val"><b>${info.elevation}</b> m</span>`);
  bits.push(`<span class="i-val"><b>${info.tempNow}</b> °C <span class="dim">(mean ${info.tempMean})</span></span>`);
  if (!info.isWater) bits.push(`<span class="i-val"><b>${info.precip}</b> mm/yr</span>`);
  if (info.river) bits.push(`<span class="i-val">River · flow <b>${info.flow}</b></span>`);
  if (info.lake) bits.push(`<span class="i-val">Lake</span>`);
  if (info.frozen) bits.push(`<span class="i-val">${info.frozen}</span>`);
  for (const r of info.resources) {
    bits.push(`<span class="i-res">◆ ${r.name} <span class="dim">(${r.abundance}${r.requires ? `, requires ${r.requires}` : ""})</span></span>`);
  }
  if (info.territory) {
    bits.push(`<span class="i-terr">Territory of ${info.territory}</span>`);
  }
  el.innerHTML = bits.join("");
}

export function toast(el, msg, ms = 4200) {
  el.textContent = msg;
  el.classList.remove("hidden");
  clearTimeout(el._t);
  el._t = setTimeout(() => el.classList.add("hidden"), ms);
}
