// Inspector dock (bottom-left): one contextual card for whatever is
// selected — cell, settlement, people, deposit, feature, war or good.
// Hover gets a light cursor tooltip; click promotes into this dock.

import { createEffect, createMemo, createSignal, on } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, wars, market, month, selection,
  selectedSettlement, hoverTip, isMobile, sheet, setSheet, marketTick,
} from "./state.js";
import { STYLE_LABEL, fmt, FALLBACK_MONTHS } from "./config.js";
import { I } from "./icons.js";

const monthsOf = () => world()?.header.months || FALLBACK_MONTHS;

// ---------------------------------------------------------------- sparkline

function Spark(p) {
  let el;
  createEffect(() => {
    const pts = p.points();
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    const W = p.w || 250, H = p.h || 40;
    el.style.width = `${W}px`;
    el.style.height = `${H}px`;
    if (el.width !== W * dpr) { el.width = W * dpr; el.height = H * dpr; }
    const ctx = el.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    if (!pts || pts.length < 2) {
      ctx.fillStyle = "rgba(138,146,163,0.7)";
      ctx.font = "10px Inter, sans-serif";
      ctx.fillText(p.empty || "let the years pass", 4, H / 2 + 3);
      return;
    }
    const min = Math.min(...pts), max = Math.max(...pts);
    const span = Math.max(max - min, max * 0.001, 1e-9);
    ctx.beginPath();
    pts.forEach((v, i) => {
      const x = (i / (pts.length - 1)) * (W - 6) + 3;
      const y = H - 5 - ((v - min) / span) * (H - 12);
      i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
    });
    ctx.strokeStyle = p.color || "#d4a94a";
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "round";
    ctx.stroke();
    // end dot
    const lx = W - 3, lv = pts[pts.length - 1];
    const ly = H - 5 - ((lv - min) / span) * (H - 12);
    ctx.beginPath();
    ctx.arc(lx, ly, 2, 0, Math.PI * 2);
    ctx.fillStyle = p.color || "#d4a94a";
    ctx.fill();
  });
  return html`<canvas class="spark" ref=${(e) => (el = e)}></canvas>`;
}

// ---------------------------------------------------------------- ledger

// Victoria-style "why": signed contribution rows that sum to a total.
function Ledger(p) {
  const rows = () => p.data()?.terms || [];
  return html`<div class="ledger">
    <div class="ledger-head">${I.why()}<span>${() => p.data()?.title || "Why?"}</span></div>
    ${() => rows().map((t) => html`<div class="ledger-row">
      <span class="lg-label">${t.l}</span>
      <span class=${() => "lg-val" + (t.v > 0.0005 ? " pos" : t.v < -0.0005 ? " neg" : "")}>
        ${t.v > 0 ? "+" : ""}${typeof t.v === "number" ? t.v.toFixed(p.data()?.dp ?? 2) : t.v}
      </span>
    </div>`)}
    ${() => (p.data()?.total != null
      ? html`<div class="ledger-row ledger-total">
          <span class="lg-label">${p.data()?.total_label || "Total"}</span>
          <span class="lg-val">${p.data().total.toFixed(p.data()?.dp ?? 2)}${p.data()?.unit || ""}</span>
        </div>`
      : "")}
  </div>`;
}

// Async explain fetch, guarded against stale selections.
function useExplain(a, keyFn) {
  const [data, setData] = createSignal(null);
  let token = 0;
  createEffect(() => {
    const key = keyFn();
    month(); // refresh the ledger as time passes
    const t = ++token;
    if (!key) { setData(null); return; }
    a.explain(key.kind, key.id).then((d) => {
      if (t === token) setData(d);
    }).catch(() => { if (t === token) setData(null); });
  });
  return data;
}

// ---------------------------------------------------------------- views

function CellView(a, sel) {
  const info = createMemo(() => {
    month(); // temperature is seasonal
    return a.inspectCell(sel.x, sel.y);
  });
  return () => {
    const i = info();
    if (!i) return "";
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker">${i.isWater ? "waters" : "land"} \u00b7 ${i.x}; ${i.y}</span>
        <span class="insp-name">${i.place || i.biome}</span>
        ${i.place ? html`<span class="insp-sub">${i.biome}</span>` : ""}
      </div>
      <div class="kv">
        <div><span class="dim">Elevation</span><b>${fmt(i.elevation)} m</b></div>
        <div><span class="dim">Now</span><b>${i.tempNow}\u00b0C</b></div>
        <div><span class="dim">Mean</span><b>${i.tempMean}\u00b0C</b></div>
        ${!i.isWater ? html`<div><span class="dim">Rain</span><b>${fmt(i.precip)} mm</b></div>` : ""}
        ${!i.isWater && i.fertility != null ? html`<div><span class="dim">Fertility</span><b>${Math.round(i.fertility * 100)}%</b></div>` : ""}
        ${i.river ? html`<div><span class="dim">${i.wadi ? "Wadi" : "River"} flow</span><b>${fmt(i.flow)}${i.order > 1 ? html` <span class="dim">\u00b7 ord ${i.order}</span>` : ""}</b></div>` : ""}
        ${i.salt ? html`<div><span class="dim">Water</span><b>Salt lake</b></div>` : i.lake ? html`<div><span class="dim">Water</span><b>Lake</b></div>` : ""}
      </div>
      ${i.frozen ? html`<div class="insp-tagrow"><span class="d-tag">${i.frozen}</span></div>` : ""}
      ${i.territory ? html`<div class="insp-line">${i.territory}</div>` : ""}
      ${(i.notes || []).map((n) => html`<div class="insp-note">\u263c ${n}</div>`)}
      ${i.resources.map((r) => html`<div class="insp-note res">\u25c6 ${r.name}
        <span class="dim">(${r.abundance}${r.requires ? `, requires ${r.requires}` : ""})</span></div>`)}
      <div class="insp-line dim wind">${i.wind}</div>
    </div>`;
  };
}

function SettlementView(a) {
  const s = selectedSettlement;
  const culture = () => (cultures() || [])[s()?.culture];
  const explain = useExplain(a, () => (s() ? { kind: "settlement", id: s().id } : null));
  return () => {
    const st = s();
    if (!st) return html`<div class="insp-body"><div class="insp-note">Lost to the mists.</div></div>`;
    const w = world();
    const resources = w?.header.resources || {};
    const tags = [st.port ? "harbour" : null, st.coastal ? "coastal" : null, st.river ? "fresh water" : null]
      .filter(Boolean);
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${culture()?.color || "#999"}`}>
          ${st.tier}${culture() ? ` of the ${culture().people}` : ""}</span>
        <span class="insp-name">${st.name}</span>
      </div>
      <div class="kv">
        <div><span class="dim">Souls</span><b>${fmt(st.pop)}</b></div>
        <div><span class="dim">Food</span><b>${st.food}</b></div>
        <div><span class="dim">Routes</span><b>${st.connections ?? 0}</b></div>
        <div><span class="dim">Coin</span><b>${fmt(st.wealth || 0)}</b></div>
      </div>
      ${Spark({ points: () => a.popHistoryOf(st.id), color: "#d4a94a", h: 36, empty: "population \u2014 let the years pass" })}
      ${tags.length ? html`<div class="insp-tagrow">${tags.map((t) => html`<span class="d-tag">${t}</span>`)}</div>` : ""}
      ${(st.goods || []).length ? html`<div class="insp-goods">
        ${(st.goods || []).map((g) => html`<button class="d-good" style=${`--gc:${resources[g]?.color || "#ccc"}`}
          title=${g === st.exports ? "chief export \u2014 open the market view" : "open the market view"}
          onClick=${() => a.select({ kind: "good", id: g })}>${g}${g === st.exports ? html`<span class="d-star">\u2605</span>` : ""}</button>`)}
      </div>` : ""}
      ${() => (explain() ? Ledger({ data: explain }) : "")}
      <div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.flyTo(st.x + 0.5, st.y + 0.5, 8)}>${I.fly()} Fly to</button>
        ${culture() ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "culture", id: st.culture })}>${I.people()} People</button>` : ""}
      </div>
    </div>`;
  };
}

function CultureView(a, sel) {
  const c = () => (cultures() || [])[sel.id];
  const mine = createMemo(() => settlements().filter((s) => s.culture === sel.id));
  const atWar = () => (wars() || []).filter((w) => w.a === sel.id || w.b === sel.id);
  return () => {
    const cu = c();
    if (!cu) return "";
    const pop = mine().reduce((acc, s) => acc + s.pop, 0);
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${cu.color}`}>${cu.polity || "people"}${cu.era ? ` \u00b7 ${cu.era}` : ""}</span>
        <span class="insp-name">${cu.people}</span>
        <span class="insp-sub">${STYLE_LABEL[cu.style] || cu.style}${cu.ruler ? ` \u00b7 led by ${cu.ruler}` : ""}</span>
      </div>
      <div class="kv">
        <div><span class="dim">Souls</span><b>${fmt(pop)}</b></div>
        <div><span class="dim">Holdings</span><b>${mine().length}</b></div>
        <div><span class="dim">Treasury</span><b>${fmt(cu.treasury || 0)}</b></div>
        <div><span class="dim">Arts</span><b>${(cu.techs || []).length}</b></div>
      </div>
      ${() => atWar().map((w) => html`<div class="insp-note war">\u2694 ${w.name}</div>`)}
      ${(cu.techs || []).length ? html`<div class="insp-goods techs">
        ${(cu.techs || []).slice(-8).map((t) => html`<span class="d-tag">${t}</span>`)}
      </div>` : ""}
      <div class="insp-list">
        ${() => mine().slice().sort((x, y) => y.pop - x.pop).slice(0, 6).map(
          (s) => html`<button class="insp-list-row" onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
            <span class="s-name">${s.name}</span><span class="s-tier">${s.tier}</span>
            <span class="s-pop">${fmt(s.pop)}</span>
          </button>`)}
      </div>
    </div>`;
  };
}

function DepositView(a, sel) {
  const d = () => {
    const w = world();
    return (w?.header.deposits || []).find((x) => x.x === sel.x && x.y === sel.y && x.r === sel.id) || null;
  };
  return () => {
    const dep = d();
    const w = world();
    if (!dep || !w) return "";
    const meta = w.header.resources[dep.r] || {};
    const row = (market() || []).find((r) => r.g === dep.r);
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${meta.color}`}>${meta.category || "resource"} \u00b7 ${dep.x}; ${dep.y}</span>
        <span class="insp-name">${dep.r}</span>
        <span class="insp-sub">${meta.abundance}${meta.requires ? ` \u00b7 requires ${meta.requires}` : ""}</span>
      </div>
      <div class="kv">
        ${dep.left != null ? html`<div><span class="dim">Remaining</span><b>${dep.left === 0 ? "spent" : fmt(dep.left)}</b></div>` : ""}
        ${row ? html`<div><span class="dim">Price</span><b>${row.p.toFixed(2)}</b></div>` : ""}
      </div>
      ${row ? html`<div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.select({ kind: "good", id: dep.r })}>${I.market()} Market</button>
        <button class="ghost-btn" onClick=${() => a.flyTo(dep.x + 0.5, dep.y + 0.5, 10)}>${I.fly()} Fly to</button>
      </div>` : ""}
    </div>`;
  };
}

function FeatureView(a, sel) {
  const f = () => (world()?.header.features || [])[sel.id] || null;
  return () => {
    const ft = f();
    if (!ft) return "";
    const w = world();
    const km = (w?.header.km_per_cell || 4) ** 2 * ft.size;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker">${ft.t}</span>
        <span class="insp-name">${ft.name}</span>
      </div>
      <div class="kv">
        <div><span class="dim">Extent</span><b>~${fmt(km)} km\u00b2</b></div>
      </div>
      <div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.flyTo(ft.x, ft.y, ft.t === "ocean" || ft.t === "continent" ? 2 : 6)}>${I.fly()} Fly to</button>
      </div>
    </div>`;
  };
}

function WarView(a, sel) {
  const w = () => (wars() || []).find((x) => x.name === sel.id) || null;
  return () => {
    const war = w();
    const cs = cultures() || [];
    if (!war) return html`<div class="insp-body">
      <div class="insp-head"><span class="insp-kicker">war</span><span class="insp-name">${sel.id}</span></div>
      <div class="insp-note">The banners are furled \u2014 this war has ended.</div>
    </div>`;
    const a1 = cs[war.a], b1 = cs[war.b];
    const monthsLeft = Math.max(0, war.until - month());
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker war-k">\u2694 war</span>
        <span class="insp-name">${war.name}</span>
      </div>
      <div class="war-sides">
        <button class="war-side" onClick=${() => a.select({ kind: "culture", id: war.a })}>
          <span class="s-dot" style=${`background:${a1?.color || "#999"}`}></span>${a1?.people || "?"}
        </button>
        <span class="war-vs">against</span>
        <button class="war-side" onClick=${() => a.select({ kind: "culture", id: war.b })}>
          <span class="s-dot" style=${`background:${b1?.color || "#999"}`}></span>${b1?.people || "?"}
        </button>
      </div>
      <div class="insp-line dim">Perhaps ${Math.max(1, Math.round(monthsLeft / 12))} more year${monthsLeft > 18 ? "s" : ""} of bloodshed, unless peace comes early.</div>
    </div>`;
  };
}

function GoodView(a, sel) {
  const row = () => (market() || []).find((r) => r.g === sel.id) || null;
  const meta = () => world()?.header.resources?.[sel.id] || {};
  const producers = createMemo(() =>
    settlements().filter((s) => (s.goods || []).includes(sel.id))
      .sort((x, y) => y.pop - x.pop));
  const explain = useExplain(a, () => ({ kind: "good", id: sel.id }));
  return () => {
    const r = row();
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${meta().color || "#8a8fa0"}`}>${meta().category || "good"}</span>
        <span class="insp-name">${sel.id}</span>
        <span class="insp-sub">${meta().abundance || ""}${meta().requires ? ` \u00b7 requires ${meta().requires}` : ""}</span>
      </div>
      ${r ? html`<div class="kv">
        <div><span class="dim">Price</span><b>${r.p.toFixed(2)}</b></div>
        <div><span class="dim">Base</span><b>${r.b}</b></div>
        <div><span class="dim">Trend</span><b class=${r.t > 0.02 ? "pos" : r.t < -0.02 ? "neg" : ""}>${r.t > 0.02 ? "\u25b2 rising" : r.t < -0.02 ? "\u25bc falling" : "steady"}</b></div>
      </div>` : html`<div class="insp-note">Not yet traded.</div>`}
      ${() => { marketTick(); return Spark({ points: () => a.priceHistoryOf(sel.id), color: meta().color || "#9fd0c8", h: 36, empty: "price \u2014 let the caravans roll" }); }}
      ${() => (explain() ? Ledger({ data: explain }) : "")}
      ${() => (producers().length ? html`<div class="insp-goods-title dim">Worked at</div>
        <div class="insp-list">
          ${producers().slice(0, 5).map((s) => html`<button class="insp-list-row"
            onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
            <span class="s-name">${s.name}</span><span class="s-tier">${s.tier}</span>
            <span class="s-pop">${fmt(s.pop)}</span>
          </button>`)}
        </div>` : "")}
    </div>`;
  };
}

// ---------------------------------------------------------------- dock

export function InspectorDock(a) {
  const view = createMemo(() => {
    const sel = selection();
    if (!sel) return null;
    switch (sel.kind) {
      case "cell": return CellView(a, sel);
      case "settlement": return SettlementView(a);
      case "culture": return CultureView(a, sel);
      case "deposit": return DepositView(a, sel);
      case "feature": return FeatureView(a, sel);
      case "war": return WarView(a, sel);
      case "good": return GoodView(a, sel);
      default: return null;
    }
  });
  // On mobile a selection raises the inspector sheet.
  createEffect(on(selection, (sel) => {
    if (sel && isMobile()) setSheet("inspector");
    if (!sel && isMobile() && sheet() === "inspector") setSheet(null);
  }, { defer: true }));
  return html`<div class=${() => {
    let cls = "inspector-dock";
    if (!selection()) cls += " hidden";
    if (isMobile()) cls += sheet() === "inspector" ? " as-sheet open" : " as-sheet";
    return cls;
  }}>
    <button class="insp-close" aria-label="Close (esc)" onClick=${() => a.select(null)}>${I.close()}</button>
    ${() => { const v = view(); return v ? v() : ""; }}
  </div>`;
}

// ---------------------------------------------------------------- tooltip

// Featherweight hover tooltip riding the cursor. Data comes from main.js.
export function HoverTip() {
  return html`<div class=${() => "hover-tip" + (hoverTip() && !isMobile() ? "" : " hidden")}
    style=${() => {
      const t = hoverTip();
      if (!t) return "";
      const x = Math.min(t.px + 16, window.innerWidth - 240);
      const y = Math.max(12, t.py - 14);
      return `transform:translate(${x}px,${y}px)`;
    }}>
    ${() => {
      const t = hoverTip();
      if (!t) return "";
      return html`
        <div class="ht-title">${t.title}</div>
        ${t.sub ? html`<div class="ht-sub">${t.sub}</div>` : ""}
        ${t.line ? html`<div class="ht-line">${t.line}</div>` : ""}`;
    }}
  </div>`;
}
