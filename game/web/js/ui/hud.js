// Edge chrome: brand + world menu (top-left), lens strip + overlay/legend
// popovers (top-center), alerts (top-right), time cluster (bottom-center),
// toast stack. Everything floats over the map; the map owns the screen.

import { createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, wars, month, playing, speed, worldSize,
  setWorldSize, busy, layer, overlays, toasts, dismissToast,
  worldMenuOpen, setWorldMenuOpen, overlaysOpen, setOverlaysOpen,
  legendOpen, setLegendOpen, setSearchOpen, notifOpen, setNotifOpen,
  notif, setNotif, persistUi, closePopovers, isMobile, sheet, setSheet,
  selection, popHistory,
} from "./state.js";
import { LAYERS, OVERLAYS, EVENT_FAMILIES, FALLBACK_MONTHS, fmt, dateOf } from "./config.js";
import { I } from "./icons.js";
import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, HYDRO_GRAD, FERT_GRAD,
} from "../palette.js";

const monthsOf = () => world()?.header.months || FALLBACK_MONTHS;

// Close popovers when the pointer goes down anywhere outside chrome.
function usePopoverDismiss() {
  const onDown = (e) => {
    if (!e.target.closest(".pop, .pop-anchor")) closePopovers();
  };
  window.addEventListener("pointerdown", onDown);
  onCleanup(() => window.removeEventListener("pointerdown", onDown));
}

// ---------------------------------------------------------------- brand

function WorldMenu(a) {
  let input;
  createEffect(() => {
    const w = world();
    if (w && input) input.value = String(w.header.seed);
  });
  const go = () => { a.generate(input.value); setWorldMenuOpen(false); };
  return html`<div class="pop pop-world" role="dialog" aria-label="World">
    <div class="pop-title">World</div>
    <div class="row">
      <input id="seed" type="text" inputmode="numeric" autocomplete="off"
        spellcheck="false" placeholder="seed"
        ref=${(el) => (input = el)}
        onKeyDown=${(e) => { if (e.key === "Enter") go(); e.stopPropagation(); }} />
      <button class="icon-btn" title="Random seed"
        onClick=${() => { input.value = String(Math.floor(Math.random() * 2147483646) + 1); }}>
        ${I.dice()}
      </button>
    </div>
    <div class="row">
      <div class="seg">
        ${[384, 512, 640, 768].map(
          (sz) => html`<button class=${() => (worldSize() === sz ? "active" : "")}
            onClick=${() => setWorldSize(sz)}>${sz}</button>`
        )}
      </div>
    </div>
    <button class="primary" disabled=${busy} onClick=${go}>
      ${() => (busy() ? "Shaping\u2026" : "Generate world")}
    </button>
    <div class="pop-note">Same seed, same world \u2014 always.</div>
  </div>`;
}

function Brand(a) {
  const name = () => world()?.header.world_name || "\u2026";
  const seedTxt = () => {
    const w = world();
    return w ? `${w.header.seed} \u00b7 ${w.header.width || w.header.size}\u00d7${w.header.size}` : "";
  };
  return html`<div class="brand-cluster pop-anchor">
    <button class=${() => "brand-chip" + (worldMenuOpen() ? " open" : "")}
      onClick=${() => { const v = !worldMenuOpen(); closePopovers(); setWorldMenuOpen(v); }}
      title="World seed & size">
      <span class="brand-mark">C<span class="muse-star">\u2736</span></span>
      <span class="brand-txt">
        <span class="brand-world">${name}</span>
        <span class="brand-seed">${seedTxt}</span>
      </span>
    </button>
    ${() => (worldMenuOpen() ? WorldMenu(a) : "")}
  </div>`;
}

// ---------------------------------------------------------------- lenses

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

function LegendPop() {
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
        ? html`<div class="pop-note">Realms tinted by their people over satellite terrain.</div>`
        : html`<div class="pop-note">True-colour composite \u2014 canopy, soil, rock and snow.</div>`}`;
    }
    if (l === "elevation") return gradLegend(ELEV_LAND_GRAD, 0, 1, "sea level", "high peaks");
    if (l === "temperature") return gradLegend(TEMP_GRAD, -35, 35, "\u221235\u00b0C", "35\u00b0C");
    if (l === "precip") return gradLegend(PRECIP_GRAD, 0, 3000, "arid", "3000 mm");
    if (l === "hydro") return gradLegend(HYDRO_GRAD, 0, 1, "trickle", "torrent");
    if (l === "fertility") return gradLegend(FERT_GRAD, 0, 1, "barren", "black earth");
    return "";
  };
  return html`<div class="pop pop-legend" role="dialog" aria-label="Legend">
    <div class="pop-title">${() => LAYERS.find(([id]) => id === layer())?.[1] || ""}</div>
    ${body}
  </div>`;
}

function OverlaysPop(a) {
  return html`<div class="pop pop-overlays" role="dialog" aria-label="Overlays">
    <div class="pop-title">Overlays</div>
    <div class="option-list">
      ${OVERLAYS.map(
        ([id, label]) => html`<div
          class=${() => "option" + (overlays[id] ? " active" : "")}
          onClick=${() => a.toggleOverlay(id)}>
          <span class="check"><svg viewBox="0 0 24 24" width="10" height="10"><path d="M4 12.5l5 5L20 6.5" fill="none" stroke="#191307" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></svg></span>
          <span>${label}</span>
        </div>`
      )}
    </div>
  </div>`;
}

function LensStrip(a) {
  return html`<div class="lens-cluster pop-anchor">
    <div class="lens-strip" role="tablist" aria-label="Map lens">
      ${LAYERS.map(
        ([id, label, tip], i) => html`<button
          class=${() => "lens" + (layer() === id ? " active" : "")}
          role="tab" aria-selected=${() => String(layer() === id)}
          title=${`${tip} (${i + 1})`}
          onClick=${() => a.setLayer(id)}>
          <span class="lens-key">${i + 1}</span><span class="lens-label">${label}</span>
        </button>`
      )}
      <span class="lens-sep"></span>
      <button class=${() => "lens lens-tool" + (overlaysOpen() ? " active" : "")}
        title="Overlays (o)"
        onClick=${() => { const v = !overlaysOpen(); closePopovers(); setOverlaysOpen(v); }}>
        ${I.layers()}
      </button>
      <button class=${() => "lens lens-tool" + (legendOpen() ? " active" : "")}
        title="Legend (l)"
        onClick=${() => { const v = !legendOpen(); closePopovers(); setLegendOpen(v); }}>
        ${I.legend()}
      </button>
    </div>
    ${() => (overlaysOpen() ? OverlaysPop(a) : "")}
    ${() => (legendOpen() ? LegendPop() : "")}
  </div>`;
}

// ---------------------------------------------------------------- alerts

function NotifPop() {
  const flip = (f) => { setNotif(f, !notif[f]); persistUi(); };
  return html`<div class="pop pop-notif" role="dialog" aria-label="Alerts">
    <div class="pop-title">Alerts</div>
    <div class="option-list">
      ${EVENT_FAMILIES.map(
        ([id, label]) => html`<div
          class=${() => "option" + (notif[id] ? " active" : "")}
          onClick=${() => flip(id)}>
          <span class="check"><svg viewBox="0 0 24 24" width="10" height="10"><path d="M4 12.5l5 5L20 6.5" fill="none" stroke="#191307" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></svg></span>
          <span>${label}</span>
        </div>`
      )}
    </div>
    <div class="pop-note">Chosen families surface as notices while time flows.</div>
  </div>`;
}

function TopRight(a) {
  const warList = () => wars() || [];
  return html`<div class="topright pop-anchor">
    ${() => warList().map((w) => {
      const cs = cultures() || [];
      return html`<button class="sit-chip" title=${`${w.name} \u2014 ${cs[w.a]?.people || "?"} against ${cs[w.b]?.people || "?"}`}
        onClick=${() => a.select({ kind: "war", id: w.name })}>
        <span class="sit-ic">${I.war()}</span>
        <span class="sit-txt">${w.name}</span>
      </button>`;
    })}
    <button class=${() => "hud-btn" + (notifOpen() ? " open" : "")} title="Alerts"
      onClick=${() => { const v = !notifOpen(); closePopovers(); setNotifOpen(v); }}>
      ${I.bell()}
    </button>
    <button class="hud-btn" title="Search (/)"
      onClick=${() => setSearchOpen(true)}>
      ${I.search()}
    </button>
    ${() => (notifOpen() ? NotifPop() : "")}
  </div>`;
}

// ---------------------------------------------------------------- toasts

function ToastStack(a) {
  return html`<div class="toast-stack" aria-live="polite">
    ${() => toasts().map((t) => html`<div
      class=${"toastc" + (t.kind ? ` t-${t.kind}` : "") + (t.x != null ? " clickable" : "")}
      onClick=${() => {
        if (t.x != null) a.flyTo(t.x, t.y, 8);
        dismissToast(t.id);
      }}>
      <span class="toastc-dot" style=${t.color ? `background:${t.color}` : ""}></span>
      <span class="toastc-body">
        <span class="toastc-text">${t.text}</span>
        ${t.sub ? html`<span class="toastc-sub">${t.sub}</span>` : ""}
      </span>
      <button class="toastc-x" aria-label="Dismiss"
        onClick=${(e) => { e.stopPropagation(); dismissToast(t.id); }}>${I.close()}</button>
    </div>`)}
  </div>`;
}

// ---------------------------------------------------------------- time

function TimeCluster(a) {
  const dateText = () => (world() ? dateOf(month(), monthsOf()) : "\u2014");
  const totalPop = createMemo(() => settlements().reduce((acc, s) => acc + s.pop, 0));
  let spark;
  createEffect(() => {
    const hist = popHistory();
    if (!spark) return;
    const ctx = spark.getContext("2d");
    const dpr = window.devicePixelRatio || 1;
    const W = 72, H = 22;
    if (spark.width !== W * dpr) { spark.width = W * dpr; spark.height = H * dpr; }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (hist.length > 1) {
      const min = Math.min(...hist.map((p) => p.pop));
      const max = Math.max(...hist.map((p) => p.pop));
      const span = Math.max(max - min, 1);
      ctx.beginPath();
      hist.forEach((p, i) => {
        const x = (i / (hist.length - 1)) * (W - 4) + 2;
        const y = H - 3 - ((p.pop - min) / span) * (H - 7);
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      });
      ctx.strokeStyle = "rgba(212,169,74,0.9)";
      ctx.lineWidth = 1.4;
      ctx.lineJoin = "round";
      ctx.stroke();
    }
  });
  return html`<div class="time-cluster">
    <div class="time-date">
      <span class="td-date">${dateText}</span>
      <span class="td-pop" title="Souls in the world">${() => fmt(totalPop())} souls</span>
    </div>
    <canvas class="time-spark" width="72" height="22" ref=${(el) => (spark = el)}
      title="Population over time"></canvas>
    <button class="icon-btn" title="Step one month (n)" onClick=${() => a.step()}>${I.step()}</button>
    <button class=${() => "time-play" + (playing() ? " on" : "")}
      aria-label="Play / pause (space)" onClick=${() => a.playPause()}>
      ${() => (playing() ? I.pause() : I.play())}
    </button>
    <div class="seg time-speed">
      ${[[1, "1\u00d7"], [3, "3\u00d7"], [12, "12\u00d7"]].map(
        ([v, label]) => html`<button class=${() => (speed() === v ? "active" : "")}
          onClick=${() => a.setSpeed(v)}>${label}</button>`
      )}
    </div>
  </div>`;
}

// ---------------------------------------------------------------- mobile

function MobileBar(a) {
  const dateText = () => {
    if (!world()) return "\u2014";
    const m = month();
    const months = monthsOf();
    return `Y${Math.floor(m / 12) + 1} ${months[((m % 12) + 12) % 12]}`;
  };
  const flip = (id) => setSheet(sheet() === id ? null : id);
  return html`
    <div class=${() => "scrim" + (isMobile() && sheet() ? " show" : "")}
      onClick=${() => setSheet(null)}></div>
    <nav class="mbar">
      <button class=${() => "mtab" + (sheet() === "inspector" ? " active" : "")}
        onClick=${() => flip("inspector")}>
        ${I.place()}<span>Inspect</span>
        ${() => (selection() && sheet() !== "inspector" ? html`<span class="mdot"></span>` : "")}
      </button>
      <div class="mtime">
        <button class="icon-btn" aria-label="Step one month" onClick=${() => a.step()}>${I.step()}</button>
        <button class="mplay" aria-label="Play / pause" onClick=${() => a.playPause()}>
          ${() => (playing() ? I.pause() : I.play())}
        </button>
        <span class="mdate">${dateText}</span>
      </div>
      <button class=${() => "mtab" + (sheet() === "outliner" ? " active" : "")}
        onClick=${() => flip("outliner")}>
        ${I.book()}<span>Almanac</span>
      </button>
    </nav>`;
}

// ---------------------------------------------------------------- export

export function Hud(a) {
  usePopoverDismiss();
  return html`
    <div class="hud-top">
      ${Brand(a)}
      ${LensStrip(a)}
      ${TopRight(a)}
    </div>
    ${ToastStack(a)}
    <div class="hud-bottom">${TimeCluster(a)}</div>
    ${MobileBar(a)}`;
}
