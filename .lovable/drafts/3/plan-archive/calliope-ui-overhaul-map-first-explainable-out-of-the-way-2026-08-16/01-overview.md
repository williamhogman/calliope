# Calliope UI overhaul — map-first, explainable, out of the way

## Verdict on the current UI

The audit is blunt: we have two dense glass sidebars permanently covering ~40% of the viewport, stuffed with every section at once (world gen, layers, overlays, stats, legend, resources, peoples, settlements, markets, chronicle). Everything is a list row — cultures and wars aren't even clickable. There is no search, no pinning, no route/deposit/feature inspection, no event filtering, no "why is this number what it is." The map — the best thing we have — is the thing the UI hides.

## What the research says (Paradox school + the other masters)

Distilled from Victoria 3 / CK3 / EU4 / Stellaris, Civ VI, RimWorld, Dwarf Fortress Steam, Frostpunk, Anno, Songs of Syx — including what their forums *criticize*:

1. **Map is the primary surface.** Chrome lives on edges; the center 75%+ of the viewport stays permanently clear. Panels dock, never float centrally, except true modal decisions.
2. **Strict three-tier interaction grammar:** hover = ephemeral tooltip · click = select, populating a stable-position inspector · right-click / long-press = context actions. Never blend selection and action.
3. **Every number explainable, but bounded** (CK3/Vic3 nested tooltips): drill into any stat's contributing terms — with a depth cap and breadcrumb, avoiding Vic3's documented "lost the tooltip chain" failure.
4. **Stable frame, dynamic content** (RimWorld inspect pane): one inspector, one position, content reflows to whatever is selected — no panel proliferation.
5. **Outliner as index of truth** (Stellaris): a persistent, searchable, pinnable sidebar list, because hunting the map doesn't scale.
6. **Tiered notifications:** silent log / ambient toast / blocking modal, per event-kind, plus a "situations" tray for *ongoing* phenomena (wars, depletions) instead of popup spam.
7. **Semantic zoom in discrete tiers with hysteresis** — importance-ranked labels appearing by zoom band, the Google-Maps/Mapbox placement model (we already have greedy AABB culling; it lacks tiers and importance).
8. **Diegetic beats HUD:** encode what has spatial structure into the map itself (we already do routes/winds/snow well); reserve chips for what has no spatial analog.
9. **Sim/UI hard split, DOM minimalism:** sim in the worker (already true), canvas/GPU for all per-entity map drawing (already true), DOM only for the handful of interactive surfaces — positioned with transform-only writes.
10. **Numeric typography:** tabular figures, right-aligned columns, color reserved for signed deltas — the ledger discipline our market and stats tables currently lack.

The plan below rebuilds the shell around these rules, keeps the dark cartographic identity (Cinzel + Inter + gold on deep navy), and adds the one engine capability real explainability needs: the sim reporting *why*.
