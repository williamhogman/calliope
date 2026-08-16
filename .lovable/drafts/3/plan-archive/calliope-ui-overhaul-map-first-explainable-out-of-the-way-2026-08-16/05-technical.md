## Technical plan

### Engine additions (Rust, `game/rust/src/`)

1. **Explain API** — `WasmWorld::explain(kind, id) -> JSON`: a term ledger `{label, value, terms: [{label, value, kind}]}` for: market price (base × scarcity × shock), settlement growth (base r × food × trade × era), site attractiveness (fertility, coast, river, delta, ore pull), culture treasury delta, tech progress. Computed from the same functions the sim uses — no parallel formulas to drift.
2. **Paged chronicle** — keep the full event log in `World` (currently the client truncates at 200); `events(from, to)` export; event records gain the subject's coordinates so any entry can fly the camera.
3. **Price history** — small ring buffer per good (last ~240 months) serialized on demand for sparklines.
4. **Importance score** — one per settlement/feature (pop, tier, size) shipped in the pack for label tiering.

Per project law: each addition ships with diagnose checks — explain terms must recompose to the displayed value within epsilon, paged events must be deterministic and gap-free, history buffers must not perturb `hash_state`. Clean `report.sh` gates every phase.

### Frontend (`game/web/js/`)

- **State:** normalized Solid stores keyed by id (settlements, cultures, routes, deposits, events); selection becomes `{kind, id}` plus a pinned-tooltip list; lens/overlay/time signals unchanged.
- **Picking:** one spatial index (grid-bucketed points + route segment distance) with priority ordering — replaces the settlement-only radius check in `main.js`.
- **Rendering split stays as-is** (wgpu raster + 2D canvas vectors — right for our entity counts; no DOM-per-entity, ever). Canvas gains: selection halo, inspected-route highlight, tier-based label logic with hysteresis.
- **DOM anchoring:** the selection halo chip and pinned tooltips track world coords via transform-only writes in the existing rAF loop.
- **Virtualization:** outliner and chronicle tabs use a lightweight virtual list (`virtua` for Solid) — fine at 65 towns, mandatory at 768-size worlds and thousand-entry chronicles.
- **Typography/icons:** `tabular-nums` everywhere numeric; hand-drawn single-path SVG sprites for the 10 goods + categories, tinted by state.

### Phases (each ends with live Playwright evidence + clean diagnostics)

1. **Shell** — new layout (lens strip, time cluster, inspector dock, outliner rail, brand/omnibox stub), old panels retired, hotkeys, mobile chrome rebuilt.
2. **Picking & entity depth** — unified hit-testing; inspector views for all 8 entity kinds; per-route trade data surfaced; selection halo.
3. **Explainability** — Rust explain API + diagnose checks; expandable term ledgers; pinnable tooltips with breadcrumbs.
4. **Chronicle & notifications** — engine paging; virtualized filterable feed; toasts; situations tray; per-kind tier settings.
5. **Search & semantic zoom** — omnibox with fly-to across all entity types; importance-tiered labels with hysteresis; goods icon set; final polish pass, desktop + mobile screenshots at every tier.

### Docs

ADR "UI shell: edge-docked chrome over a clear map" (interaction grammar, DOM-minimalism rule, explain-API contract) in `docs/adr/`; ROADMAP updated with the five phases as gated items.
