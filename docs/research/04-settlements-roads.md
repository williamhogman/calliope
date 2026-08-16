# 04 — Settlements, Roads & Urbanism

## Sources

1-4. **Central Place Theory** — Christaller (1933) via transportgeography.org, geog.leeds.ac.uk, HyperGeo, Wikipedia — SKIM. k=3 marketing / k=4 transport / k=7 administrative hexagonal nesting; threshold vs range of goods; hamlet→village→town→city tiers. Real medieval market-town spacing ≈ a day's round trip, 15-30 km (4-8 cells at 4 km/cell).
5. **Zipf's Law and the Growth of Cities** — Gabaix (1999, AER) — https://www.aeaweb.org/articles?id=10.1257%2Faer.89.2.129 — ABSTRACT. Gibrat's law (size-independent proportional growth) + lower reflecting barrier ⇒ Zipf (rank × pop ≈ const).
6-8. **Gibrat/Zipf empirics** — Soo (2005), Córdoba (2003) — ABSTRACT/SKIM. Exponent varies 0.8-1.3 by country/era — validation band, not hard constraint.
9-11. **Batty: cities as complex systems** — CASA/UCL — SKIM. Rank-size + fractal growth as emergent targets.
12. **The Origins of Scaling in Cities** — Bettencourt (Science 2013) — https://www.colorado.edu/socialreactors/sites/default/files/attached-files/bettencourt_2013_science.pdf — READ. Y = Y₀·N^β: infrastructure β≈0.85 (sublinear), socioeconomic output β≈1.15 (superlinear), derived from transport-cost vs mixing-benefit balance.
13-15. **Bettencourt/West PNAS 2007, EPJ B 2008, SciAm** — ABSTRACT/SKIM. Doubling size ⇒ ~15 % more output per capita, ~15 % less infrastructure per capita.
16-20. **Azgaar: Settlements, states, routes rework** — https://azgaar.wordpress.com/2017/11/21/settlements/ — READ/SKIM. Suitability-scored burg placement with spacing blackout (structurally = Calliope's); cost-expansion state territories; 2024 route-hierarchy rework because straight A* lines "read artificial."
21-22. **watabou Medieval Fantasy City Generator devlogs** — SKIM. Explicitly beauty-first, not model-first; city-internal detail downstream of siting.
23. **Procedural Modeling of Cities** — Parish & Müller (SIGGRAPH 2001) — https://people.eecs.berkeley.edu/~sequin/CS285/PAPERS/Parish_Muller01.pdf — READ. L-system roads under population-density and slope/water constraints; hierarchy emerges from the growth rule.
24. **Procedural Generation of Roads** — Galin et al. (EG 2010) — https://perso.liris.cnrs.fr/egalin/Articles/2010-roads.pdf — READ. Anisotropic weighted shortest path (slope, water, vegetation), junction snapping, Bezier smoothing, bridges/tunnels at cost thresholds — the paper form of `trade.rs`.
25-26. **Space colonization road growth** — Runions et al. (2007) via Minho thesis + implementations — ABSTRACT/SKIM. Attractor-driven organic network growth.
27-28. **Here Dragons Abound: city symbols, label force layout** — SKIM. Settlement rank as first-class rendering variable.
29-30. **Red Blob: movement costs, A*** — READ/SKIM. Canonical terrain-cost edge weighting; Calliope's A* is consistent with it.
31-32. **Fort siting, Fen River Basin (npj Heritage Sci 2025); Onondaga viewshed (Am. Antiquity)** — ABSTRACT. Empirical defensibility scoring: viewshed, relative elevation, chokepoints.
33. **Early Rome / Tiber ford** — JRS 2021 — ABSTRACT. Fords as settlement magnets distinct from generic river proximity.
34. **UFC 4-150-06 harbor siting** — SKIM. Shelter, depth, littoral drift — beyond "coastal == true".
35-36. **Settlement persistence/abandonment** — PMC 2021, Antiquity — ABSTRACT. Abandonment as stochastic outcome of sustained resource/network disadvantage.
37. **Pre-modern town clustering** — RSUE 2021 — ABSTRACT. Spacing distributions.

Honestly flagged by the researcher: no citable primary design docs found for DF civ placement, Songs of Syx, Kenshi, M&B settlement economics.

## Synthesis

Three traditions to reconcile: **geography** (Christaller: spacing/hierarchy from threshold+range; terrain magnets the theory ignores — fords, viewsheds, sheltered harbors), **urban economics** (Gibrat ⇒ Zipf rank-size as emergent regularity; Bettencourt's 0.85/1.15 sub/superlinear split as within-city law), and **PCG practice** (suitability scoring + spacing; A* roads that need smoothing/junction-merging and hierarchy or they read robotic; lifecycle/ruins as the thinnest-covered area).

## Calliope

`settlements.rs`/`trade.rs` already match Azgaar-level siting and Galin-level anisotropic routing. Gaps:

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Zipf rank-size validation + Gibrat-style growth correction (target slope −0.8..−1.3, harness check) | S | Worlds read as real settlement systems |
| 2 | Bettencourt scaling: wealth/trade ∝ pop^1.15, infrastructure/upkeep ∝ pop^0.85 | S-M | Big cities disproportionately rich — for free |
| 3 | Day's-walk spacing calibration of `min_dist`/territory vs 15-30 km literature band | S | Grounds a magic number |
| 4 | Route smoothing + junction merging (Galin post-process) | M | Kills the "robotic A*" read |
| 5 | Abandonment/ruins from sustained deficit | M | Dead cities; chronicle fodder |
| 6 | Christaller market-area layer (population-weighted Voronoi driving goods flow) | M | Hinterlands and market hierarchy |
| 7 | Defensibility term (cheap prominence/chokepoint proxy, not full viewshed) | M | Hillforts, pass-guard towns |
| 8 | Harbor quality via coastline concavity | S | Sheltered-bay ports |
