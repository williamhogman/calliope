# Calliope Roadmap

Sequenced from `GAP-ANALYSIS.md`. Every milestone lands with new
diagnostics checks (ADR-0009) and, where architecture is set, an ADR.
Implementation happens on the mainline tree (this draft carries the docs;
the diagnostics harness lives on mainline).

Legend: cost S/M/L · gate = harness acceptance check.

## M1 — The Carved Land (terrain & water depth)

The erosion milestone: the physical world earns its valleys.

1. Talus thermal-erosion pass before hydrology (S)
2. Stream-power incision, few implicit iterations over `filled`/`dirs`/`discharge`, + hillslope diffusion (M)
3. Secondary/tertiary mountain-range pass (S)
4. Strahler ordering + river width ∝ √discharge in pack/render (S)
5. Endorheic basins: arid depressions keep terminal salt lakes (M)
6. Seasonal ITCZ shift → monsoons, wet/dry seasons (S)
7. Seasonal discharge (12-month array; wadis; barge seasonality) (M)

Gates: slope/hypsometry distributions in new bands across sweep; river
share band holds post-incision; ≥1 endorheic lake on suitable seeds;
monsoon amplitude band in tropics; determinism hash stable; native gen
< 400 ms at 640×512.

## M2 — Bread and Salt (agriculture, scaling laws, famine)

1. Crop packages (wheat/rice/maize/pastoral) from T/P/growing-period; per-package K; <300 mm pastoral boundary (S)
2. Carrying capacity from package + tech (Kaplan T^−0.5); keep logistic growth (S)
3. Zipf rank-size validation + Gibrat growth correction (S)
4. Bettencourt scaling: output ∝ pop^1.15, infrastructure/upkeep ∝ pop^0.85 (S-M)
5. Spacing calibration vs 15-30 km market-town band (S)
6. Famine coupling: local subsistence failure → demand spike, pop loss, migration, chronicle events (S-M)
7. Price-ratio calibration vs medieval price lists (harness check) (S)

Gates: rank-size slope in [−1.3, −0.8]; crop-belt maps legible (wheat
belts, rice deltas, pastoral steppes); famine events fire in dry-shock
sweeps but < X/century in bands; price ratios inside historical envelope.

## M3 — Words and Ways (culture, naming, coherence)

1. Culture-styled toponym generics + per-culture formation strategies (S-M)
2. Power-law weighted draws in `make_word` (S)
3. Etymology glosses on fragments; chronicle cites meanings (M)
4. Exonym/endonym doubling on border features (S)
5. Pantheon layer: per-culture named gods cited in omens/festivals/war names (M)

Gates: label audit — sampled toponyms classify to their culture ≥ 90 %;
gloss coverage 100 % of fragments; no name-collision regressions.

## M4 — The Great Game (politics with consequences)

1. Influence-map territory (pop/tech/war-weighted kernels; live borders) (M)
2. War score → peace terms: settlement transfer / tribute / vassalage (M)
3. Opinion matrix + aggressive-expansion decay + coalition threshold (S-M)
4. Siege state machine; fortification as treasury sink (S)
5. Legitimacy/asabiyyah: surge at frontiers, ~3-4-generation decay, rebellion/fragmentation rolls (M)

Gates: ≥ 1 border change per major war in sweeps; polity count rises *and
falls* over 300 y; coalition wars occur; no runaway single-empire outcome
in > 80 % of seeds.

## M5 — Iron and Coin (the local economy)

1. Production recipes: ore→metal→tools/weapons, workforce-gated (M)
2. Market areas from route connectivity; per-area prices (L)
3. Trade income from price gaps (arbitrage) instead of geography-only (with #2)
4. Gravity-model flow as harness cross-check (S)
5. Merchant agents (L — only after #2 proves out)

Gates: inter-area price divergence within band; recipe towns emerge;
pinned-price share stays PASS; arbitrage income correlates with price gaps.

## M6 — The Telling (chronicle with memory)

1. Structured events (actor/entity ids beside text) (S-M)
2. Persistent named non-rulers: generals, prospectors, founders (M)
3. Artifacts with provenance (M)
4. Drama-pacing modifier layer (S)
5. Story sifter: 5-8 Felt-style patterns over the log (M-L)
6. Legends browser UI over the entity graph (L)
7. Eventfulness scoring + reversal detection ranking the sifter's output (S-M, research/15)
8. Narration memory: earned epithets, callbacks, mention-aware templates (M, research/15)
9. Two-layer telling: ground-truth log vs. mythologized legend rendering (M, research/13)

Gates: every event carries ≥ 1 entity id; sifter yields ≥ N microstories
per century within dedup bounds; browser renders full cross-link graph.

## M7 — The Atlas Plate (cartographic grammar)

1. Coastal vignettes from coast-distance (S)
2. Töpfer-law label/settlement culling by zoom (M)
3. Letter-spaced area labels (S)
4. Climate-blended hypsometric ramp for elevation mode (M)
5. Multi-directional hillshade + precomputed texture shading (M)
6. Route smoothing + junction merging (M)
7. Curved river labels (L)

Gates: label-overlap count 0 at all zooms; density ≈ Töpfer prediction;
screenshot review at 3 zooms × 2 seeds.

## M8 — The Instrument (harness to ERA grade)

1. Seam-invariant properties: rivers descend `filled`; settlements route-reachable; pack/unpack byte-identity; grants for every new pack field (S each)
2. Metamorphic checks: rainfall↑ ⇒ river cells not↓ across seeds (S-M)
3. ERA 2D histograms exported per sweep (M)
4. Oatmeal detector: between-seed structural distinctiveness metrics (M-L)

Gates: property suite green across sweep; ERA plots reviewed per milestone.

## M9 — The Patina (residue, strata & the withheld)

Feel milestone from `research/SYNTHESIS-FEEL.md` (dockets 12-15): let
things die and leave marks, let names carry time, let the telling withhold.

1. Settlement death & ruins: abandonment (war, depletion, famine) leaves named ruin entities on the map (M)
2. Hydronym conservatism + bounded conquest name-layers: rivers keep the oldest culture's names; conquest renames settlements inside the conquered polygon only (S-M)
3. Age-keyed name erosion with stored compositional etymology, glossed in the inspector (S)
4. History marks the map: battle sites, war-renamed features, disused routes fading (M)
5. Berúthiel emissions + disputed/unresolved chronicle entries, bounded share (S)

Gates: mature worlds carry ruins across the sweep (≥ 1 per century after
year 100); river names stable through 100 % of border changes; every
emitted name carries an etymology; withheld/disputed entries within a
2-8 % band; full diagnostics suite stays green.

## Later / research

Trade-graph SIR plagues (L, after M4/M5 give it consequences) · vegetation
succession + wildlife (M) · sound-change language families (L) · religion
slot-grammar beyond pantheon (L) · myth drift (novel) · data-driven world
recipes/templates (M) · GPU erosion compute pass (M-L) · culture-seed
permeation across subsystems (M-L, research/13) · myth with mechanical
stakes (M, research/13) · consequential/atmospheric detail tagging (M,
research/15) · von Thünen land-use rings in render (M, research/14).

## Rejected (do not re-open without a superseding ADR)

Plate-tectonic simulation · D-infinity flow · hydrology-first terrain ·
default-path particle erosion · freeform AI diplomacy. Reasons in
`research/SYNTHESIS.md` and the digests.

## Ready queue (next up, in order)

1. M1.1 talus pass + M1.4 Strahler/width — small, high-visibility, pure
   engine + pack additions.
2. M1.6 ITCZ shift — five lines and a band.
3. M8.1 seam-invariant property suite — protects everything after it.
