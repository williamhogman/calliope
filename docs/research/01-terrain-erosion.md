# 01 — Terrain, Tectonics & Erosion

Depth markers: READ (studied) · SKIM (surveyed) · ABSTRACT (summary only).

## Sources

1. **Polygonal Map Generation for Games** — Amit Patel (2010/2025) — http://www-cs-students.stanford.edu/~amitp/game-programming/polygon-map-generation/ — READ. Voronoi/Delaunay dual graph, Lloyd relaxation, radial+noise island mask, Whittaker biomes, rivers along Delaunay edges. Explicitly game-first, not simulation-first.
2. **Mapgen2** — Amit Patel (2017/2025) — https://www.redblobgames.com/maps/mapgen2/ — READ. mapgen4 derives rainfall/rivers from painted elevation — climate as fitting step, not constraint.
3. **mapgen2 repo** — https://github.com/redblobgames/mapgen2 — SKIM.
4. **Generating fantasy maps** — Martin O'Leary (2016) — archived: https://web.archive.org/web/2023/http://mewo2.com/notes/terrain/ — READ. Irregular grids to kill axis artifacts; height from additive landmass primitives because pure fbm has "fine detail with no large-scale structure."
5. **mewo2/terrain** — https://github.com/mewo2/terrain — SKIM.
6-10. **Here Dragons Abound** — Scott Turner — https://heredragonsabound.blogspot.com/ — SKIM (land shapes at 8× scale; Azgaar template analysis; deltas needing post-process without sediment transport; meanders ABSTRACT).
11. **Clustered Convection for Procedural Plate Tectonics** — Nick McDonald (2020) — https://nickmcd.me/2020/12/03/clustered-convection-for-simulating-plate-tectonics/ — READ. Plates as deformable point-cloud clusters (~130 LOC) + uplift/subduction/rift rules; argues noise-as-initial-topography is physically unjustified.
12. **SimpleErosion** — weigert — https://github.com/weigert/SimpleErosion — READ. Particle droplets: volume, velocity, sediment capacity; deposition/erosion vs capacity, evaporation, death threshold.
13. **SimpleHydrology** — weigert — https://github.com/weigert/SimpleHydrology — READ. Droplet erosion + incremental priority-flood pooling → streams and lakes from one pass.
14. **SimpleTectonics** — weigert — https://github.com/weigert/SimpleTectonics — SKIM.
15. **Terrain Generation Based on Hydrology** — Génevaux, Galin et al. (ACM TOG 2013) — https://hal.science/hal-01339224 — SKIM. River network drawn first (Horton-Strahler grammar), terrain synthesized to fit — watersheds correct by construction.
16. **Large Scale Terrain from Tectonic Uplift and Fluvial Erosion** — Cordonnier et al. (EG 2016) — https://inria.hal.science/hal-01262376 — SKIM. Uplift field + implicit stream-power solver, multigrid → continent-scale LEM in seconds. Closest published match to Calliope's speed budget.
17. **FastScape / StreamPowerChannel** — Braun et al. — https://fastscape.org/ — READ. Canonical implicit SPL solver: E = K·A^m·S^n (m=0.4, n=1), O(n) unconditionally stable (Braun & Willett 2013).
18. **Landscape evolution base equation** — Wickert — https://geomorphonline.github.io/landscape-evolution/base_equation/ — READ. ∂z/∂t = k_h∇²z − K·A^m·S^n + U.
19. **PyPlatec / plate-tectonics** — https://github.com/Mindwerks/plate-tectonics — SKIM. Rigid plates, collision/subduction/rift on a heightfield.
20. **WorldEngine** — https://github.com/Mindwerks/worldengine — SKIM. Pipeline: plates → precip (rain shadow) → erosion → biomes — same stage order Calliope uses, plus tectonics and erosion.
21-24. **Undiscovered Worlds** — https://undiscoveredworlds.blogspot.com/ — SKIM. Secondary mountain-range passes to break single-scale belts; critique that one ridge-noise range reads as "nothing you'd see on Earth"; two-scale global/regional zoom.
25. **Synthesis and rendering of eroded fractal terrains** — Musgrave, Kolb, Mace (SIGGRAPH 1989) — https://dl.acm.org/doi/10.1145/74334.74337 — ABSTRACT. Ancestor of heightfield-space thermal/hydraulic filters.
26. **Musgrave dissertation** (Yale 1993) — https://www.kenmusgrave.com/dissertation.pdf — SKIM. Ridged/hetero/hybrid multifractals.
27. **Realtime Procedural Terrain Generation** — Olsen (2004) — https://web.mit.edu/cesium/Public/terrain.pdf — READ. Fast talus thermal erosion: move Δh·k to steepest lower neighbour when slope > talus angle; O(N·cells), parallel, deterministic.
28. **GPU Hydraulic Erosion (Avalanche/KTH)** — Isheden (2022) — https://www.diva-portal.org/smash/get/diva2:1646074/FULLTEXT01.pdf — ABSTRACT.
29. **Infinite WFC** — Kleineberg (2023) — https://marian42.de/article/infinite-wfc/ — ABSTRACT. Chunk-deterministic generation; wrong tool for continuous fields.
30-31. **Dwarf Fortress worldgen interviews** — Adams — ABSTRACT. Biome-first philosophy; terrain fidelity budgeted against downstream consumers.
32-34. **Azgaar heightmap templates** — https://azgaar.wordpress.com/2017/04/01/heightmap/ — READ. Height as ordered recipe of primitive ops (Hill, Range, Trough, Strait, Mask); templates as data.

## Synthesis

Five families, each patching the previous one's failure:

- **Noise (fbm/ridged/warp)** — the substrate. Failure mode: fine roughness, no large-scale structure; mountain belts read as isotropic blobs. Fixes: bias fields, masks, domain warp — all of which Calliope has.
- **Procedural primitives / templates (Azgaar, mewo2)** — height composed from parametrized landform operators in an ordered recipe. Calliope's `geo.rs` is a hard-coded instance; Azgaar's contribution is making the recipe *data*.
- **Tectonics (PlaTec, clustered convection, Cordonnier)** — physically motivated belt/coast placement. Heavy; competes with the tuned primitive stack.
- **Erosion / LEMs** — the decisive family. Three lineages: (a) grid-space talus filters (Olsen/Musgrave) — cheap, deterministic, parallel; (b) particle droplets (weigert) — best alluvial detail, stochastic, expensive, determinism-fragile; (c) stream-power-law LEMs (FastScape): ∂z/∂t = U − K·A^m·S^n + k_h∇²z with A = drainage area — exactly the D8/discharge Calliope already computes.
- **Hydrology-first (Génevaux)** — rivers before terrain; architecturally invasive for an elevation-first engine.

## Calliope

We already have: warped fbm + radial bias, inland-gated ridged mountains, island arcs/hotspots/archipelagos (a template pipeline in code), priority-flood + D8 + accumulation. **We have no erosion of any kind** — `filled` height drives discharge but the heightfield is never carved. That is the single biggest terrain gap.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Stream-power incision reusing `filled`/`dirs`/`discharge` (Δh = −K·A^m·S^n per iteration, few iterations) | M | Real valleys carved by the rivers that flow in them |
| 2 | Grid-space thermal/talus erosion pass before hydrology | S | Natural scree slopes instead of raw ridged noise; tens of ms |
| 3 | Secondary/tertiary mountain-range pass | S | Breaks single-scale belts |
| 4 | Data-driven heightmap templates (world "recipes": archipelago-heavy, single-continent…) | M | Variety without new code paths |
| 5 | Hillslope diffusion term coupled to #1 | S | Rounds post-incision ridgelines |
| 6 | Hetero/hybrid multifractal variants | S | Varied silhouettes |
| 7 | Particle droplet erosion (offline/high-detail option) | L | Best detail; determinism + budget risk |
| 8 | Plate simulation | L | Correct long-term answer; only if heuristics hit a ceiling |
