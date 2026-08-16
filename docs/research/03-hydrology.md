# 03 — Hydrology & Rivers

## Sources

1. **Priority-flood depression filling** — Barnes (2014) — https://arxiv.org/abs/1511.04463 — READ. O(N log N) fill; ε-gradient over flats. Calliope's `fill_depressions` matches.
2. **RichDEM docs** — https://richdem.readthedocs.io/en/latest/depression_filling.html — SKIM. Pits vs depressions; breaching vs filling.
3. **D-infinity flow** — Tarboton (1997) — https://doi.org/10.1029/97WR01380 — READ. Vector flow split across facets; kills D8 fingering, complicates the DAG.
4. **Terrain from Hydrology** — Génevaux et al. (2013) — READ. River grammar first, terrain second.
5. **Meander project** — Hodgin (2020) — https://roberthodgin.com/project/meander — SKIM. Fisk-style migrating channels, oxbows.
6. **Azgaar: River Systems** — https://azgaar.wordpress.com/2017/05/08/river-systems/ — READ. Width ∝ √discharge; jitter against straight-line reading.
7. **HDA: Sprucing up rivers** — SKIM. Polygonal river bodies, confluence joins.
8. **Undiscovered Worlds: Better Basins** — https://undiscoveredworlds.blogspot.com/2019/03/better-basins.html — READ. Endorheic basins: terminal salt lakes where evaporation beats inflow.
9. **Meandering conditions** — Howard & Knutson (1984) — ABSTRACT. Migration velocity ∝ curvature.
10. **meanderpy** — Sylvester — https://github.com/zsylvester/meanderpy — SKIM. Ikeda-Parker-Sawai centerline migration.
11. **Procedural Riverscapes** — Peytavie et al. (2019) — https://perso.liris.cnrs.fr/egalin/Articles/2019-riverscapes.pdf — SKIM.
12. **Scaling laws for river networks** — Dodds & Rothman (1999) — READ. Hack's law L ≈ C·A^0.55-0.6; Strahler universality.
13. **GRASS r.stream.order** — SKIM. Strahler/Horton/Shreve/Hack orders.
14. **LeatherBee: water table** — SKIM. Springs where table meets surface.
15. **Waterfall scenes** — Emilien et al. (2015) — ABSTRACT. Knickpoint migration.
16. **Orbis Multiplex groundwater** — SKIM. Nile-strip river-moisture diffusion.
17. **Delta channel networks** — Hiatt et al. (2019) — SKIM. Deltas are graphs with cycles, not trees.
18. **Terrain Generation: River Networks** — Janert (2024) — READ. Modern D8 vs erosion comparison.
19. **Artificial drainage basins** — Fischer et al. (2022) — SKIM. Basins-first partitioning.
20. **Braided channel simulation** — OSTI (1992) — ABSTRACT.
21. **TauDEM slides** — SKIM. Hydro-flattened lakes.
22. **Horton fractality** — Serizawa (2019) — ABSTRACT.
23. **Scalable river animation** — Yu et al. (2009) — READ. v ∝ √(S·R).
24. **CAESAR cellular LEM** — Coulthard (2007) — ABSTRACT.
25. **Landlab gravel transport** — SKIM.
26. **SINUOUS meander model docs** — CSDMS — SKIM.

## Synthesis

Three phases: **topological routing** (priority-flood + D8 → DAG; Calliope's implementation is textbook), **morphological scaling** (Hack's law L∝A^0.57; width ∝ √Q — a river twice the volume is only ~40 % wider; Strahler ordering for hierarchy), **fluvial dynamics** (meander belts in flat low-slope reaches, oxbows at cutoffs, endorheic terminal lakes where Q − PET ≤ 0, seasonal discharge from monthly precipitation).

## Calliope

`hydrology.rs` has phases 1 fully and 2 partially (discharge exists; rivers are boolean-thresholded; navigability classified). Missing: ordering, width, sinuosity, endorheic basins, seasons.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Strahler ordering (second pass over the sorted cell list) | S | Tiered rendering + label importance for rivers |
| 2 | Width ∝ √discharge instead of boolean river mask | S | Kills the stringy look; great rivers feel great |
| 3 | Endorheic sinks + salt lakes in arid basins (water balance in `fill_depressions`) | M | Caspian/Aral/Chad-class features; desert believability |
| 4 | Sinuosity jitter (render-side meander belt from 1/slope) | M | Breaks the grid staircase without simulation |
| 5 | Seasonal discharge (monthly array) — feeds barge trade + floodplain agriculture | M | Economy and biome coupling |
| 6 | D-infinity | L | Low — breaks the DAG the rest of the engine relies on |
