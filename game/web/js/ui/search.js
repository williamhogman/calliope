// Omnibox (/) — one search across places, peoples, features, goods,
// lenses and commands. Arrow keys to move, Enter to go, Esc to leave.

import { createMemo, createSignal, createEffect } from "solid-js";
import html from "solid-js/html";

import {
  world, settlements, cultures, realms, market, searchOpen, setSearchOpen,
  playing, stories, entities,
} from "./state.js";
import { LAYERS, fmt, patternMeta, entityKind } from "./config.js";
import { I } from "./icons.js";

// Score against a pre-lowercased name (E8.9): 0 exact, 1 prefix, 2 word
// start, 3 substring, -1 no match.
function score(lname, q) {
  if (lname === q) return 0;
  if (lname.startsWith(q)) return 1;
  const idx = lname.indexOf(q);
  if (idx > 0 && (lname[idx - 1] === " " || lname[idx - 1] === "'")) return 2;
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

  // E8.9 — the candidate index is a memo over the world data: it rebuilds
  // when settlements/features/market/telling change, never per keystroke.
  // Names are lowercased once here; a keystroke only scores and sorts.
  const index = createMemo(() => {
    const out = [];
    const add = (group, label, sub, run, color) =>
      out.push({ group, label, lname: label.toLowerCase(), sub, run, color });
    for (const s of settlements()) {
      add("Places", s.name, `${s.tier} \u00b7 ${fmt(s.pop)} souls`,
        () => a.select({ kind: "settlement", id: s.id, fly: true }),
        (realms() || [])[s.realm]?.color);
    }
    const feats = world()?.header.features || [];
    feats.forEach((f, i) => {
      add("Geography", f.name, f.t, () => a.select({ kind: "feature", id: i, fly: true }));
    });
    // ADR-0018 — both axes searchable: crowns (political) and tongues.
    for (const r of realms() || []) {
      if (!r.alive) continue;
      add("Crowns", r.name, r.ruler || r.house || "realm",
        () => a.select({ kind: "realm", id: r.id }), r.color);
    }
    for (const c of cultures() || []) {
      if (c.alive === false) continue;
      add("Peoples", c.people, c.polity || "people",
        () => a.select({ kind: "culture", id: c.id }), c.color);
    }
    for (const r of market() || []) {
      add("Goods", r.g, `${r.p.toFixed(2)} coin`,
        () => a.select({ kind: "good", id: r.g }),
        world()?.header.resources?.[r.g]?.color);
    }
    // the telling: sagas and the cast — persons, relics, wars, ruins (M6.6)
    for (const s of stories() || []) {
      add("Sagas", s.title, `${patternMeta(s.pattern).label} \u00b7 Y${s.y0}\u2013${s.y1}`,
        () => a.select({ kind: "story", story: s }), patternMeta(s.pattern).color);
    }
    for (const en of entities() || []) {
      if (!["person", "artifact", "war", "ruin"].includes(en.kind)) continue;
      add("The cast", en.name,
        `${en.role || entityKind(en.kind).label}${en.until != null ? " \u00b7 \u2020" : ""}`,
        () => a.select({ kind: "entity", id: en.id, fly: true }), entityKind(en.kind).color);
    }
    for (const [id, label] of LAYERS) {
      add("Lenses", `Lens: ${label}`, "", () => a.setLayer(id));
    }
    return out;
  });

  const results = createMemo(() => {
    const query = q().trim().toLowerCase();
    if (!query) {
      return [
        { group: "Commands", label: playing() ? "Pause time" : "Let time flow", hint: "space", run: () => a.playPause() },
        { group: "Commands", label: "Step one month", hint: "n", run: () => a.step() },
        { group: "Commands", label: "Fit the world", hint: "f", run: () => a.fitView() },
        ...LAYERS.slice(0, 4).map(([id, label]) =>
          ({ group: "Lenses", label: `Lens: ${label}`, run: () => a.setLayer(id) })),
      ];
    }
    const out = [];
    for (const c of index()) {
      const sc = score(c.lname, query);
      if (sc >= 0) out.push({ ...c, sc });
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
