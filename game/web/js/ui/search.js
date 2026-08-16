// Omnibox (/) — one search across places, peoples, features, goods,
// lenses and commands. Arrow keys to move, Enter to go, Esc to leave.

import { createMemo, createSignal, createEffect } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, market, searchOpen, setSearchOpen, playing,
} from "./state.js";
import { LAYERS, fmt } from "./config.js";
import { I } from "./icons.js";

function score(name, q) {
  const n = name.toLowerCase();
  if (n === q) return 0;
  if (n.startsWith(q)) return 1;
  const idx = n.indexOf(q);
  if (idx > 0 && (n[idx - 1] === " " || n[idx - 1] === "'")) return 2;
  if (idx >= 0) return 3;
  return -1;
}

export function Search(a) {
  const [q, setQ] = createSignal("");
  const [cursor, setCursor] = createSignal(0);
  let inputEl;

  createEffect(() => {
    if (searchOpen()) {
      setQ("");
      setCursor(0);
      queueMicrotask(() => inputEl?.focus());
    }
  });

  const results = createMemo(() => {
    const query = q().trim().toLowerCase();
    const out = [];
    if (!query) {
      out.push({ group: "Commands", label: playing() ? "Pause time" : "Let time flow", hint: "space", run: () => a.playPause() });
      out.push({ group: "Commands", label: "Step one month", hint: "n", run: () => a.step() });
      out.push({ group: "Commands", label: "Fit the world", hint: "f", run: () => a.fitView() });
      for (const [id, label] of LAYERS.slice(0, 4)) {
        out.push({ group: "Lenses", label: `Lens: ${label}`, run: () => a.setLayer(id) });
      }
      return out;
    }
    const push = (group, label, sub, sc, run, color) => {
      if (sc < 0) return;
      out.push({ group, label, sub, sc, run, color });
    };
    for (const s of settlements()) {
      push("Places", s.name, `${s.tier} \u00b7 ${fmt(s.pop)} souls`, score(s.name, query),
        () => a.select({ kind: "settlement", id: s.id, fly: true }),
        (cultures() || [])[s.culture]?.color);
    }
    const feats = world()?.header.features || [];
    feats.forEach((f, i) => {
      push("Geography", f.name, f.t, score(f.name, query),
        () => a.select({ kind: "feature", id: i, fly: true }));
    });
    for (const c of cultures() || []) {
      push("Peoples", c.people, c.polity || "people", score(c.people, query),
        () => a.select({ kind: "culture", id: c.id }), c.color);
    }
    for (const r of market() || []) {
      push("Goods", r.g, `${r.p.toFixed(2)} coin`, score(r.g, query),
        () => a.select({ kind: "good", id: r.g }),
        world()?.header.resources?.[r.g]?.color);
    }
    for (const [id, label] of LAYERS) {
      push("Lenses", `Lens: ${label}`, "", score(label, query), () => a.setLayer(id));
    }
    out.sort((x, y) => x.sc - y.sc || x.label.length - y.label.length);
    return out.slice(0, 12);
  });

  createEffect(() => { results(); setCursor(0); });

  const go = (r) => {
    if (!r) return;
    setSearchOpen(false);
    r.run();
  };

  const onKey = (e) => {
    if (e.key === "Escape") { setSearchOpen(false); e.stopPropagation(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setCursor(Math.min(cursor() + 1, results().length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setCursor(Math.max(cursor() - 1, 0)); }
    else if (e.key === "Enter") go(results()[cursor()]);
    e.stopPropagation();
  };

  return html`<div class=${() => "search-veil" + (searchOpen() ? "" : " hidden")}
    onPointerDown=${(e) => { if (e.target.classList.contains("search-veil")) setSearchOpen(false); }}>
    <div class="search-box" role="dialog" aria-label="Search">
      <div class="search-row">
        ${I.search()}
        <input ref=${(el) => (inputEl = el)} type="text" placeholder="Search the world\u2026"
          value=${q} onInput=${(e) => setQ(e.target.value)} onKeyDown=${onKey} />

        <span class="search-esc dim">esc</span>
      </div>
      <div class="search-results">
        ${() => {
          let lastGroup = null;
          return results().map((r, i) => {
            const head = r.group !== lastGroup ? html`<div class="sr-group">${r.group}</div>` : "";
            lastGroup = r.group;
            return html`${head}<button
              class=${() => "sr-row" + (cursor() === i ? " cur" : "")}
              onMouseEnter=${() => setCursor(i)}
              onClick=${() => go(r)}>
              ${r.color ? html`<span class="s-dot" style=${`background:${r.color}`}></span>` : html`<span class="s-dot ghost"></span>`}
              <span class="sr-label">${r.label}</span>
              ${r.sub ? html`<span class="sr-sub dim">${r.sub}</span>` : ""}
              ${r.hint ? html`<span class="sr-hint">${r.hint}</span>` : ""}
            </button>`;
          });
        }}
        ${() => (!results().length ? html`<div class="ol-empty dim">The muse knows no such name.</div>` : "")}
      </div>
    </div>
  </div>`;
}
