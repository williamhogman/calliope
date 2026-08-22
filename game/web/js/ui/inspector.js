// Inspector dock (bottom-left): one contextual card for whatever is
// selected — cell, settlement, people, deposit, feature, war, good,
// story or entity. Hover gets a light cursor tooltip; click promotes
// into this dock.

import { createEffect, createMemo, createResource, on } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, realms, civs, wars, market, areas, month, selection,
  selectedSettlement, settlementsById, hoverTip, isMobile, sheet, setSheet,
  marketTick, stories, entities, legendMode, setLegendMode, persistUi, ruins,
} from "./state.js";
import {
  STYLE_LABEL, fmt, FALLBACK_MONTHS, patternMeta, entityKind, eventColor,
  civStage,
} from "./config.js";
import { I } from "./icons.js";
import { each, eachIdx } from "./list.js";

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
    ${eachIdx(rows, (t) => html`<div class="ledger-row">
      <span class="lg-label">${() => t().l}</span>
      <span class=${() => "lg-val" + (t().v > 0.0005 ? " pos" : t().v < -0.0005 ? " neg" : "")}>
        ${() => `${t().v > 0 ? "+" : ""}${typeof t().v === "number" ? t().v.toFixed(p.data()?.dp ?? 2) : t().v}`}
      </span>
    </div>`)}
    ${() => (p.data()?.total != null
      ? html`<div class="ledger-row ledger-total">
          <span class="lg-label">${() => p.data()?.total_label || "Total"}</span>
          <span class="lg-val">${() => `${p.data().total.toFixed(p.data()?.dp ?? 2)}${p.data()?.unit || ""}`}</span>
        </div>`
      : "")}
  </div>`;
}

// E8.4 — the explain fetch rides createResource: stale-race protection,
// loading and error semantics come built in. `latest` keeps the previous
// ledger on screen while the next month's answer is in flight, so the
// dock never flickers as time passes.
function useExplain(a, keyFn) {
  const [data] = createResource(
    () => {
      const key = keyFn();
      return key ? { ...key, m: month() } : null;
    },
    (key) => a.explain(key.kind, key.id).catch(() => null),
  );
  return () => data.latest ?? null;
}

// ---------------------------------------------------------------- views

function CellView(a, sel) {
  const info = createMemo(() => {
    month(); // temperature is seasonal
    return a.inspectCell(sel.x, sel.y);
  });
  // M61 — "why is this here": the engine's provenance chain for this
  // cell, deep time forward (stone → ice → water → soil → landform).
  const why = useExplain(a, () => ({ kind: "cell", id: `${sel.y},${sel.x}` }));
  return () => {
    const i = info();
    if (!i) return "";
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker">${i.landform || (i.isWater ? "waters" : "land")} \u00b7 ${i.x}; ${i.y}</span>
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
      ${i.folk ? html`<div class="insp-line dim">${i.folk}</div>` : ""}
      ${(i.notes || []).map((n) => html`<div class="insp-note">\u263c ${n}</div>`)}
      ${i.resources.map((r) => html`<div class="insp-note res">\u25c6 ${r.name}
        <span class="dim">(${r.abundance}${r.requires ? `, requires ${r.requires}` : ""})</span></div>`)}
      ${() => {
        const ch = why();
        if (!ch || !ch.chain) return "";
        return html`<div class="insp-chain">
          <div class="chain-t">${ch.title || "Why is this here"}</div>
          ${ch.chain.map((e) => html`<div class="chain-row">
            <span class=${`chain-k k-${e.k}`}>${e.k}</span>
            <span class="chain-b"><b>${e.l}</b> <span class="dim">\u2014 ${e.d}</span></span>
          </div>`)}
        </div>`;
      }}
      <div class="insp-line dim wind">${i.wind}</div>
    </div>`;
  };
}

function SettlementView(a) {
  const s = selectedSettlement;
  // ADR-0018 — the two axes: whose tongue (people) and whose crown (realm),
  // plus the people who coined the name (namer, stable through conquest).
  const people = () => (cultures() || [])[s()?.people];
  const realm = () => (realms() || [])[s()?.realm];
  const namer = () => (cultures() || [])[s()?.namer];
  const explain = useExplain(a, () => (s() ? { kind: "settlement", id: s().id } : null));
  // M5.2 — which market area this town trades in
  const marketArea = () => {
    const st = s(), ar = areas();
    if (!st || !ar?.hubs?.length) return null;
    const idx = settlements().findIndex((x) => x.id === st.id);
    if (idx < 0) return null;
    const hub = ar.hubs[ar.of?.[idx] ?? 0];
    return hub ? { ...hub, seat: hub.id === st.id } : null;
  };
  return () => {
    const st = s();
    if (!st) return html`<div class="insp-body"><div class="insp-note">Lost to the mists.</div></div>`;
    const w = world();
    const resources = w?.header.resources || {};
    const tags = [st.port ? "harbour" : null, st.coastal ? "coastal" : null, st.river ? "fresh water" : null,
      st.quarry ? `${st.quarry} quarries` : null]
      .filter(Boolean);
    const foreignCrown = realm() && people() && realm().people !== st.people;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${realm()?.color || people()?.color || "#999"}`}>
          ${st.tier}${people() ? ` of the ${people().people}` : ""}${realm() ? ` \u00b7 ${realm().name}` : ""}</span>
        <span class="insp-name">${st.name}</span>
        ${st.ety ? html`<span class="insp-sub ety">\u201c${st.ety}\u201d in the tongue of the ${namer()?.people || people()?.people || "first peoples"}</span>` : ""}
      </div>
      ${foreignCrown ? html`<div class="insp-line dim">A ${people().people} town under the banners of ${realm().name}.</div>` : ""}
      ${st.exonym ? html`<div class="insp-line dim">On the crown's rolls it is written <i>${st.exonym}</i>; its own folk keep the old name.</div>` : ""}
      ${(st.formerly || []).length ? html`<div class="insp-strata">
        Once ${st.formerly.join(", then ")} \u2014 the old name${st.formerly.length > 1 ? "s" : ""} linger${st.formerly.length > 1 ? "" : "s"} on shepherds' tongues.</div>` : ""}
      ${st.failing ? html`<div class="insp-note fail">\u26e9 The young take the roads out; houses stand empty by the gate.</div>` : ""}
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
      ${() => {
        const ma = marketArea();
        if (!ma) return "";
        return ma.seat
          ? html`<div class="insp-line dim">Seat of a market of ${ma.n} town${ma.n === 1 ? "" : "s"} \u2014 prices are set here.</div>`
          : html`<div class="insp-line dim">Trades in the market of
              <button class="link-btn" onClick=${() => a.select({ kind: "settlement", id: ma.id, fly: true })}>${ma.name}</button>
              \u00b7 ${ma.n} towns</div>`;
      }}
      ${() => (explain() ? Ledger({ data: explain }) : "")}
      <div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.flyTo(st.x + 0.5, st.y + 0.5, 8)}>${I.fly()} Fly to</button>
        ${people() ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "culture", id: st.people })}>${I.people()} Folk</button>` : ""}
        ${realm() ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "realm", id: st.realm })}>${I.crown()} Crown</button>` : ""}
      </div>
    </div>`;
  };
}

// ADR-0018 — the slow axis: tongue, gods, arts. No coin, no crown; those
// live on RealmView. Wars are realm business and do not appear here.
function CultureView(a, sel) {
  const c = () => (cultures() || [])[sel.id];
  const mine = createMemo(() => settlements().filter((s) => s.people === sel.id));
  const crowns = createMemo(() =>
    (realms() || []).filter((r) => r.alive && r.people === sel.id));
  const top = createMemo(() =>
    mine().slice().sort((x, y) => y.pop - x.pop).slice(0, 6));
  return () => {
    const cu = c();
    if (!cu) return "";
    const pop = mine().reduce((acc, s) => acc + s.pop, 0);
    const parent = cu.parent != null ? (cultures() || [])[cu.parent] : null;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${cu.color}`}>${cu.polity || "people"}${cu.era ? ` \u00b7 ${cu.era}` : ""}</span>
        <span class="insp-name">${cu.people}</span>
        <span class="insp-sub">${STYLE_LABEL[cu.style] || cu.style}</span>
      </div>
      ${cu.alive === false ? html`<div class="insp-note fail">This tongue has fallen silent \u2014 its folk speak other words now; its names remain in the strata.</div>` : ""}
      ${parent ? html`<div class="insp-line dim">Diverged from the
        <button class="link-btn" onClick=${() => a.select({ kind: "culture", id: cu.parent })}>${parent.people}</button>
        \u2014 the old kinship is still heard in the names.</div>` : ""}
      <div class="kv">
        <div><span class="dim">Souls</span><b>${fmt(pop)}</b></div>
        <div><span class="dim">Hearths</span><b>${mine().length}</b></div>
        <div><span class="dim">Crowns</span><b>${crowns().length}</b></div>
        <div><span class="dim">Arts</span><b>${(cu.techs || []).length}</b></div>
      </div>
      ${() => (crowns().length ? html`<div class="ent-chips">
        ${crowns().map((r) => html`<button class="ent-chip" style=${`--ec:${r.color}`}
          onClick=${() => a.select({ kind: "realm", id: r.id })}>
          <span class="s-dot" style=${`background:${r.color}`}></span>${r.name}
        </button>`)}
      </div>` : "")}
      ${(cu.pantheon || []).length ? html`<div class="pantheon">
        <div class="insp-goods-title dim">Pantheon</div>
        ${(cu.pantheon || []).map((g, i) => html`<div class="god-row">
          <span class="god-name">${i === 0 ? "\u2726 " : ""}${g.name}</span>
          <span class="god-domain dim">${g.domain}</span>
        </div>`)}
      </div>` : ""}
      ${(cu.techs || []).length ? html`<div class="insp-goods techs">
        ${(cu.techs || []).slice(-8).map((t) => html`<span class="d-tag">${t}</span>`)}
      </div>` : ""}
      <div class="insp-list">
        ${each(top, (s) => html`<button class="insp-list-row" onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
          <span class="s-name">${s.name}</span><span class="s-tier">${s.tier}</span>
          <span class="s-pop">${fmt(s.pop)}</span>
        </button>`)}
      </div>
    </div>`;
  };
}

// ADR-0018 — the fast axis: crown, coin, banners, wars. The realm holds
// towns of many tongues; its crown people only lends it a court style.
function RealmView(a, sel) {
  const r = () => (realms() || [])[sel.id];
  const mine = createMemo(() => settlements().filter((s) => s.realm === sel.id));
  const atWar = () => (wars() || []).filter((w) => w.a === sel.id || w.b === sel.id);
  const top = createMemo(() =>
    mine().slice().sort((x, y) => y.pop - x.pop).slice(0, 6));
  // the tongues under this banner, most hearths first — polyglot realms
  // are the conquest story made visible
  const tongues = createMemo(() => {
    const n = new Map();
    for (const s of mine()) n.set(s.people, (n.get(s.people) || 0) + 1);
    return [...n.entries()]
      .sort((x, y) => y[1] - x[1])
      .map(([id, towns]) => ({ p: (cultures() || [])[id], towns }))
      .filter((t) => t.p);
  });
  return () => {
    const rm = r();
    if (!rm) return "";
    const pop = mine().reduce((acc, s) => acc + s.pop, 0);
    const crownPeople = (cultures() || [])[rm.people];
    const seat = mine().find((s) => s.id === rm.seat)
      || settlements().find((s) => s.id === rm.seat);
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${rm.color}`}>realm${crownPeople ? ` \u00b7 a ${crownPeople.people} crown` : ""}</span>
        <span class="insp-name">${rm.name}</span>
        <span class="insp-sub">${rm.house}${rm.ruler ? ` \u00b7 ${rm.ruler}` : ""}</span>
      </div>
      ${rm.alive === false ? html`<div class="insp-note fail">Struck from the rolls \u2014 its banners are furled and its lands divided.</div>` : ""}
      ${rm.vassal_of ? html`<div class="insp-line dim">Sworn to ${rm.vassal_of} \u2014 tribute flows upward.</div>` : ""}
      <div class="kv">
        <div><span class="dim">Souls</span><b>${fmt(pop)}</b></div>
        <div><span class="dim">Holdings</span><b>${mine().length}</b></div>
        <div><span class="dim">Treasury</span><b>${fmt(rm.treasury || 0)}</b></div>
        <div><span class="dim">Unity</span><b>${rm.asab != null ? rm.asab.toFixed(2) : "\u2014"}</b></div>
        <div><span class="dim">Unrest</span><b>${rm.unrest != null ? rm.unrest.toFixed(2) : "\u2014"}</b></div>
      </div>
      ${() => atWar().map((w) => html`<button class="insp-note war link-note"
        onClick=${() => a.select({ kind: "war", id: w.name })}>\u2694 ${w.name}</button>`)}
      ${() => (tongues().length > 1 ? html`<div class="insp-line dim">
        Tongues under the banner: ${tongues().map((t) => `${t.p.people} (${t.towns})`).join(" \u00b7 ")}</div>` : "")}
      ${seat ? html`<div class="insp-line dim">Seat of the crown:
        <button class="link-btn" onClick=${() => a.select({ kind: "settlement", id: seat.id, fly: true })}>${seat.name}</button></div>` : ""}
      <div class="insp-list">
        ${each(top, (s) => html`<button class="insp-list-row" onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
          <span class="s-name">${s.name}</span><span class="s-tier">${s.tier}</span>
          <span class="s-pop">${fmt(s.pop)}</span>
        </button>`)}
      </div>
      <div class="insp-actions">
        ${crownPeople ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "culture", id: rm.people })}>${I.people()} Crown people</button>` : ""}
      </div>
    </div>`;
  };
}

// M13.1/M13.3 — the civilization card: the arc made explainable. The kv
// grid shows the very drivers the stage machine read last pass (legit,
// asab, wealth, stretch), so "why is this so?" has a literal answer.
function CivView(a, sel) {
  const c = () => (civs() || []).find((x) => x.id === sel.id);
  return () => {
    const cv = c();
    if (!cv) return "";
    const st = civStage(cv.stage);
    const folk = (cv.peoples || [])
      .map((pid) => (cultures() || [])[pid])
      .filter(Boolean);
    const members = (realms() || [])
      .filter((r) => r.alive && (cv.members || []).includes(r.name));
    const year = Math.floor((cv.founded || 0) / 12) + 1;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${st.color}`}>civilization \u00b7 ${st.label}</span>
        <span class="insp-name">${cv.name}</span>
        <span class="insp-sub">${cv.hegemony ? cv.hegemony : `named in year ${year}`}</span>
      </div>
      ${cv.stage === "golden" ? html`<div class="insp-note">A golden age \u2014 the hymns are loud, the granaries full, the writs obeyed.</div>` : ""}
      ${cv.stage === "interregnum" ? html`<div class="insp-note fail">Interregnum \u2014 the paramount seat stands empty and the crowns contend.</div>` : ""}
      ${cv.stretch > 1 && cv.stage !== "interregnum" ? html`<div class="insp-note warn">The writ outruns the riders \u2014 more courts than the seat can staff.</div>` : ""}
      <div class="kv">
        <div><span class="dim">Crowns</span><b>${cv.crowns}</b></div>
        <div><span class="dim">Towns</span><b>${cv.towns}</b></div>
        <div><span class="dim">Legitimacy</span><b>${cv.legit != null ? cv.legit.toFixed(2) : "\u2014"}</b></div>
        <div><span class="dim">Solidarity</span><b>${cv.asab != null ? cv.asab.toFixed(2) : "\u2014"}</b></div>
        <div><span class="dim">Wealth</span><b>${fmt(cv.wealth || 0)}</b></div>
        <div><span class="dim">Stretch</span><b>${cv.stretch != null ? cv.stretch.toFixed(2) : "\u2014"}</b></div>
        ${cv.golden_ages ? html`<div><span class="dim">Golden ages</span><b>${cv.golden_ages}</b></div>` : ""}
        ${cv.monuments ? html`<div><span class="dim">Monuments</span><b>${cv.monuments}</b></div>` : ""}
      </div>
      ${folk.length ? html`<div class="insp-line dim">Kindred tongues:${" "}
        ${folk.map((p, i) => html`${i ? " \u00b7 " : ""}<button class="link-btn"
          onClick=${() => a.select({ kind: "culture", id: p.id })}>${p.people}</button>`)}</div>` : ""}
      ${members.length ? html`<div class="insp-line dim">Crowns of the family:${" "}
        ${members.map((r, i) => html`${i ? " \u00b7 " : ""}<button class="link-btn"
          onClick=${() => a.select({ kind: "realm", id: r.id })}>${r.name}</button>`)}</div>` : ""}
      ${cv.ent != null ? html`<div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.select({ kind: "entity", id: cv.ent })}>${I.book ? I.book() : ""} Chronicle</button>
      </div>` : ""}
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
        <span class="insp-kicker">${ft.t}${ft.people ? ` \u00b7 named by the ${ft.people}` : ""}</span>
        <span class="insp-name">${ft.name}</span>
        ${ft.ety ? html`<span class="insp-sub ety">\u201c${ft.ety}\u201d in ${ft.people ? `the tongue of the ${ft.people}` : "the Old Tongue"}</span>` : ""}
      </div>
      ${ft.formerly ? html`<div class="insp-strata">The old maps name it ${ft.formerly}.</div>` : ""}
      <div class="kv">
        <div><span class="dim">Extent</span><b>~${fmt(km)} km\u00b2</b></div>
      </div>
      ${ft.alt ? html`<div class="insp-note">The ${ft.alt_people} across the border call it ${ft.alt}.</div>` : ""}
      <div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.flyTo(ft.x, ft.y, ft.t === "ocean" || ft.t === "continent" ? 2 : 6)}>${I.fly()} Fly to</button>
      </div>
    </div>`;
  };
}

// M9.1 — a town that was: why it emptied, whose it had been, what the
// stones still say. The eid doubles as the door into the telling.
function RuinView(a, sel) {
  const r = () => (ruins() || []).find((x) => x.eid === sel.id) || null;
  return () => {
    const ru = r();
    if (!ru) return html`<div class="insp-body"><div class="insp-note">Even the ruin is gone.</div></div>`;
    const y = Math.floor(ru.since / 12) + 1;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker ruin-k">ruin \u00b7 abandoned Y${y}</span>
        <span class="insp-name">${ru.name}</span>
        ${ru.ety ? html`<span class="insp-sub ety">\u201c${ru.ety}\u201d \u2014 so the old name read</span>` : ""}
      </div>
      <div class="insp-note">${ru.why}</div>
      ${ru.people ? html`<div class="insp-line dim">Its folk were of the ${ru.people}.</div>` : ""}
      <div class="insp-actions">
        <button class="ghost-btn" onClick=${() => a.flyTo(ru.x + 0.5, ru.y + 0.5, 8)}>${I.fly()} Fly to</button>
        <button class="ghost-btn" onClick=${() => a.select({ kind: "entity", id: ru.eid })}>${I.book()} The telling</button>
      </div>
    </div>`;
  };
}

function WarView(a, sel) {
  const w = () => (wars() || []).find((x) => x.name === sel.id) || null;
  return () => {
    const war = w();
    const rs = realms() || [];
    if (!war) return html`<div class="insp-body">
      <div class="insp-head"><span class="insp-kicker">war</span><span class="insp-name">${sel.id}</span></div>
      <div class="insp-note">The banners are furled \u2014 this war has ended.</div>
    </div>`;
    // ADR-0018 — wars are fought between realms (banners), not peoples
    const a1 = rs[war.a], b1 = rs[war.b];
    const monthsLeft = Math.max(0, war.until - month());
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker war-k">\u2694 war</span>
        <span class="insp-name">${war.name}</span>
      </div>
      <div class="war-sides">
        <button class="war-side" onClick=${() => a.select({ kind: "realm", id: war.a })}>
          <span class="s-dot" style=${`background:${a1?.color || "#999"}`}></span>${a1?.name || "?"}
        </button>
        <span class="war-vs">against</span>
        <button class="war-side" onClick=${() => a.select({ kind: "realm", id: war.b })}>
          <span class="s-dot" style=${`background:${b1?.color || "#999"}`}></span>${b1?.name || "?"}
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
      .sort((x, y) => y.pop - x.pop).slice(0, 5));
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
      ${() => {
        // M5.2 — where it is dear and where it is cheap, across market areas
        const hubs = (areas()?.hubs || []).filter((h) => h.p && h.p[sel.id] != null);
        if (hubs.length < 2) return "";
        const sorted = [...hubs].sort((x, y) => y.p[sel.id] - x.p[sel.id]);
        const hi = sorted[0], lo = sorted[sorted.length - 1];
        if (hi.p[sel.id] / Math.max(lo.p[sel.id], 1e-9) < 1.05) return "";
        return html`<div class="insp-line dim">Dearest in the market of
          <button class="link-btn" onClick=${() => a.select({ kind: "settlement", id: hi.id, fly: true })}>${hi.name}</button>
          (${hi.p[sel.id].toFixed(2)}) \u00b7 cheapest at
          <button class="link-btn" onClick=${() => a.select({ kind: "settlement", id: lo.id, fly: true })}>${lo.name}</button>
          (${lo.p[sel.id].toFixed(2)})</div>`;
      }}
      ${() => (explain() ? Ledger({ data: explain }) : "")}
      ${() => (producers().length ? html`<div class="insp-goods-title dim">Worked at</div>
        <div class="insp-list">
          ${each(producers, (s) => html`<button class="insp-list-row"
            onClick=${() => a.select({ kind: "settlement", id: s.id, fly: true })}>
            <span class="s-name">${s.name}</span><span class="s-tier">${s.tier}</span>
            <span class="s-pop">${fmt(s.pop)}</span>
          </button>`)}
        </div>` : "")}
    </div>`;
  };
}

// ---------------------------------------------------------------- telling

// Shared beat row: one chronicle entry inside a story or an entity's log,
// rendered in whichever layer of the telling the reader chose (M6.9).
const tellText = (e) => (legendMode() === "songs" && e.legend ? e.legend : e.text);

// E8.2 — beats arrive as fresh JSON on every sift, so rows are keyed by
// position: a growing log updates its text in place and appends.
function BeatRows(a, list) {
  const months = () => world()?.header.months || FALLBACK_MONTHS;
  return html`<div class="story-beats">
    ${eachIdx(list, (b) => html`<div
      class=${() => "beat" + (b().x >= 0 ? " clickable" : "")}
      title=${() => (b().x >= 0 ? "fly to where it happened" : "")}
      onClick=${() => { const e = b(); if (e.x >= 0) a.flyTo(e.x + 0.5, e.y + 0.5, 8); }}>
      <span class="e-dot" title=${() => b().k || ""} style=${() => `background:${eventColor(b())}`}></span>
      <span class="e-when">${() => {
        const e = b();
        return `Y${Math.floor(e.m / 12) + 1} ${(months()[((e.m % 12) + 12) % 12] || "").slice(0, 3)}`;
      }}</span>
      <span class=${() => "e-text" + (legendMode() === "songs" && b().legend ? " sung" : "")}>${() => tellText(b())}</span>
    </div>`)}
  </div>`;
}

// The little "as it was / as sung" switch every telling view carries.
function TellingToggle() {
  const setMode = (m) => { setLegendMode(m); persistUi(); };
  return html`<div class="seg tiny telling-seg">
    <button class=${() => (legendMode() === "plain" ? "active" : "")}
      aria-pressed=${() => String(legendMode() === "plain")}
      onClick=${() => setMode("plain")}>As it was</button>
    <button class=${() => (legendMode() === "songs" ? "active" : "")}
      aria-pressed=${() => String(legendMode() === "songs")}
      onClick=${() => setMode("songs")}>As sung</button>
  </div>`;
}

// Chips linking to other members of the cast — the cross-link graph (M6.6).
function EntityChips(a, list, label) {
  return html`<div class="ent-links">
    ${() => (list().length ? html`
      <div class="insp-goods-title dim">${label}</div>
      <div class="ent-chips">
        ${eachIdx(list, (e) => html`<button class="ent-chip"
          style=${() => `--ec:${entityKind(e().kind).color}`}
          onClick=${() => a.select({ kind: "entity", id: e().id, fly: true })}>
          <span class="s-dot" style=${() => `background:${entityKind(e().kind).color}`}></span>
          ${() => `${e().name}${e().until != null ? " \u2020" : ""}`}
        </button>`)}
      </div>` : "")}
  </div>`;
}

// M6.5/M6.7 — one sifted saga, its beats in order, its cast linked.
function StoryView(a, sel) {
  // prefer the live story from the latest sift — it grows as years pass
  const story = createMemo(() =>
    (stories() || []).find((s) => s.pattern === sel.story.pattern && s.title === sel.story.title)
    || sel.story);
  const cast = createMemo(() => {
    const ids = story().ids || [];
    return ids.map((id) => (entities() || []).find((e) => e.id === id)).filter(Boolean);
  });
  return () => {
    const s = story();
    const pm = patternMeta(s.pattern);
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${pm.color}`}>
          ${pm.label} \u00b7 Y${s.y0}${s.y1 !== s.y0 ? `\u2013${s.y1}` : ""}</span>
        <span class="insp-name">${s.title}</span>
        <span class="insp-sub">${s.beats.length} beats \u00b7 eventfulness ${s.score.toFixed(1)}</span>
      </div>
      ${TellingToggle()}
      ${BeatRows(a, () => story().beats || [])}
      ${EntityChips(a, cast, "The cast")}
    </div>`;
  };
}

// M6.6 — one member of the cast: their life, their log, their links.
function EntityView(a, sel) {
  const ent = createMemo(() =>
    (entities() || []).find((e) => e.id === sel.id) || null);
  // E8.4 — the log rides createResource keyed on (entity, month): stale
  // fetches lose the race by construction, and `latest` holds the previous
  // log while the next one loads.
  const [logRes] = createResource(
    () => ({ id: sel.id, m: month() }),
    (k) => a.entityLog(k.id).catch(() => []),
  );
  const log = () => logRes.latest || [];
  const links = createMemo(() => {
    const seen = new Map();
    for (const e of log()) {
      for (const id of e.ids || []) {
        if (id === sel.id || seen.has(id)) continue;
        const en = (entities() || []).find((x) => x.id === id);
        if (en && en.kind !== "world" && en.kind !== "good") seen.set(id, en);
      }
    }
    return [...seen.values()].slice(0, 14);
  });
  // bridges into the living views, when the entity still walks the map
  const liveTown = () => {
    const e = ent();
    return e?.kind === "settlement" && e.until == null
      ? settlements().find((s) => s.name === e.name) : null;
  };
  const liveCulture = () => {
    const e = ent();
    return e?.kind === "culture"
      ? (cultures() || []).find((c) => c.people === e.name) : null;
  };
  return () => {
    const e = ent();
    if (!e) return html`<div class="insp-body"><div class="insp-note">The telling knows no such name.</div></div>`;
    const ek = entityKind(e.kind);
    const home = e.culture != null ? (cultures() || [])[e.culture] : null;
    const born = Math.floor(e.since / 12) + 1;
    const died = e.until != null ? Math.floor(e.until / 12) + 1 : null;
    return html`<div class="insp-body">
      <div class="insp-head">
        <span class="insp-kicker" style=${`color:${ek.color}`}>
          ${e.role || ek.label}${home ? ` of the ${home.people}` : ""}</span>
        <span class="insp-name">${e.name}${(e.epithets || []).length ? ` ${e.epithets[e.epithets.length - 1]}` : ""}</span>
        <span class="insp-sub">Y${born}${died != null ? ` \u2014 Y${died}` : " \u2014 still in the telling"}</span>
      </div>
      ${(e.epithets || []).length > 1 ? html`<div class="insp-tagrow">
        ${e.epithets.map((t) => html`<span class="d-tag">${t}</span>`)}
      </div>` : ""}
      ${e.fate ? html`<div class="insp-note">${e.fate}</div>` : ""}
      ${TellingToggle()}
      ${() => (log().length ? BeatRows(a, log)
        : html`<div class="insp-note dim">The chronicle has not spoken of ${e.name} yet.</div>`)}
      ${EntityChips(a, links, "Spoken of alongside")}
      <div class="insp-actions">
        ${e.x >= 0 ? html`<button class="ghost-btn" onClick=${() => a.flyTo(e.x + 0.5, e.y + 0.5, 8)}>${I.fly()} Fly to</button>` : ""}
        ${() => { const t = liveTown(); return t ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "settlement", id: t.id, fly: true })}>${I.place()} The town today</button>` : ""; }}
        ${() => { const c = liveCulture(); return c ? html`<button class="ghost-btn" onClick=${() => a.select({ kind: "culture", id: c.id })}>${I.people()} The people today</button>` : ""; }}
      </div>
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
      case "realm": return RealmView(a, sel);
      case "civ": return CivView(a, sel);
      case "deposit": return DepositView(a, sel);
      case "feature": return FeatureView(a, sel);
      case "ruin": return RuinView(a, sel);
      case "war": return WarView(a, sel);
      case "good": return GoodView(a, sel);
      case "story": return StoryView(a, sel);
      case "entity": return EntityView(a, sel);
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
