# Calliope Roadmap

Sequenced from `GAP-ANALYSIS.md`. Every milestone lands with new
diagnostics checks (ADR-0009) and, where architecture is set, an ADR.

Items are checkboxes; `scripts/roadmap-check.sh` is the stopping
criterion — it exits non-zero while any milestone item is unchecked.
"Later / research" and "Rejected" are out of scope for the check.

Legend: cost S/M/L · gate = harness acceptance check.

## M1 — The Carved Land (terrain & water depth)

The erosion milestone: the physical world earns its valleys.

- [x] M1.1 Talus thermal-erosion pass before hydrology (S)
- [x] M1.2 Stream-power incision over `filled`/`dirs`/`discharge`, + hillslope diffusion (M)
- [x] M1.3 Secondary/tertiary mountain-range pass — foothill-belt ridged pass in `geo.rs` (S)
- [x] M1.4 Strahler ordering + river width ∝ √discharge in pack/render (S)
- [x] M1.5 Endorheic basins: arid depressions keep terminal salt lakes (M)
- [x] M1.6 Seasonal ITCZ shift → monsoons, wet/dry seasons (S)
- [x] M1.7 Seasonal discharge (wadis; barge seasonality) (M)

Gates: slope/hypsometry distributions in new bands across sweep; river
share band holds post-incision; ≥1 endorheic lake on suitable seeds;
monsoon amplitude band in tropics; determinism hash stable; native gen
< 400 ms at 640×512.

## M2 — Bread and Salt (agriculture, scaling laws, famine)

- [ ] M2.1 Crop packages (wheat/rice/maize/pastoral) from T/P/growing-period; per-package K; <300 mm pastoral boundary (S)
- [ ] M2.2 Carrying capacity from package + tech (Kaplan T^−0.5); keep logistic growth (S)
- [ ] M2.3 Zipf rank-size validation + Gibrat growth correction (S)
- [ ] M2.4 Bettencourt scaling: output ∝ pop^1.15, infrastructure/upkeep ∝ pop^0.85 (S-M)
- [ ] M2.5 Spacing calibration vs 15-30 km market-town band (S)
- [ ] M2.6 Famine coupling: local subsistence failure → demand spike, pop loss, migration, chronicle events (S-M)
- [ ] M2.7 Price-ratio calibration vs medieval price lists (harness check) (S)

Gates: rank-size slope in [−1.3, −0.8]; crop-belt maps legible (wheat
belts, rice deltas, pastoral steppes); famine events fire in dry-shock
sweeps but < X/century in bands; price ratios inside historical envelope.

## M3 — Words and Ways (culture, naming, coherence)

- [ ] M3.1 Culture-styled toponym generics + per-culture formation strategies (S-M)
- [ ] M3.2 Power-law weighted draws in `make_word` (S)
- [ ] M3.3 Etymology glosses on fragments; chronicle cites meanings (M)
- [ ] M3.4 Exonym/endonym doubling on border features (S)
- [ ] M3.5 Pantheon layer: per-culture named gods cited in omens/festivals/war names (M)

Gates: label audit — sampled toponyms classify to their culture ≥ 90 %;
gloss coverage 100 % of fragments; no name-collision regressions.

## M4 — The Great Game (politics with consequences)

- [ ] M4.1 Influence-map territory (pop/tech/war-weighted kernels; live borders) (M)
- [ ] M4.2 War score → peace terms: settlement transfer / tribute / vassalage (M)
- [ ] M4.3 Opinion matrix + aggressive-expansion decay + coalition threshold (S-M)
- [ ] M4.4 Siege state machine; fortification as treasury sink (S)
- [ ] M4.5 Legitimacy/asabiyyah: surge at frontiers, ~3-4-generation decay, rebellion/fragmentation rolls (M)

Gates: ≥ 1 border change per major war in sweeps; polity count rises *and
falls* over 300 y; coalition wars occur; no runaway single-empire outcome
in > 80 % of seeds.

## M5 — Iron and Coin (the local economy)

- [ ] M5.1 Production recipes: ore→metal→tools/weapons, workforce-gated (M)
- [ ] M5.2 Market areas from route connectivity; per-area prices (L)
- [ ] M5.3 Trade income from price gaps (arbitrage) instead of geography-only (with M5.2)
- [ ] M5.4 Gravity-model flow as harness cross-check (S)
- [ ] M5.5 Merchant agents (L — only after M5.2 proves out)

Gates: inter-area price divergence within band; recipe towns emerge;
pinned-price share stays PASS; arbitrage income correlates with price gaps.

## M6 — The Telling (chronicle with memory)

- [ ] M6.1 Structured events (actor/entity ids beside text) (S-M)
- [ ] M6.2 Persistent named non-rulers: generals, prospectors, founders (M)
- [ ] M6.3 Artifacts with provenance (M)
- [ ] M6.4 Drama-pacing modifier layer (S)
- [ ] M6.5 Story sifter: 5-8 Felt-style patterns over the log (M-L)
- [ ] M6.6 Legends browser UI over the entity graph (L)
- [ ] M6.7 Eventfulness scoring + reversal detection ranking the sifter's output (S-M, research/15)
- [ ] M6.8 Narration memory: earned epithets, callbacks, mention-aware templates (M, research/15)
- [ ] M6.9 Two-layer telling: ground-truth log vs. mythologized legend rendering (M, research/13)

Gates: every event carries ≥ 1 entity id; sifter yields ≥ N microstories
per century within dedup bounds; browser renders full cross-link graph.

## M7 — The Atlas Plate (cartographic grammar)

- [ ] M7.1 Coastal vignettes from coast-distance (S)
- [ ] M7.2 Töpfer-law label/settlement culling by zoom (M)
- [ ] M7.3 Letter-spaced area labels (S)
- [ ] M7.4 Climate-blended hypsometric ramp for elevation mode (M)
- [ ] M7.5 Multi-directional hillshade + precomputed texture shading (M)
- [ ] M7.6 Route smoothing + junction merging (M)
- [ ] M7.7 Curved river labels (L)

Gates: label-overlap count 0 at all zooms; density ≈ Töpfer prediction;
screenshot review at 3 zooms × 2 seeds.

## M8 — The Instrument (harness to ERA grade)

- [ ] M8.1 Seam-invariant properties: rivers descend `filled`; settlements route-reachable; pack/unpack byte-identity (S each)
- [ ] M8.2 Metamorphic checks: rainfall↑ ⇒ river cells not↓ across seeds (S-M)
- [ ] M8.3 ERA 2D histograms exported per sweep (M)
- [ ] M8.4 Oatmeal detector: between-seed structural distinctiveness metrics (M-L)

Gates: property suite green across sweep; ERA plots reviewed per milestone.

## M9 — The Patina (residue, strata & the withheld)

Feel milestone from `research/SYNTHESIS-FEEL.md` (dockets 12-15): let
things die and leave marks, let names carry time, let the telling withhold.

- [ ] M9.1 Settlement death & ruins: abandonment (war, depletion, famine) leaves named ruin entities on the map (M)
- [ ] M9.2 Hydronym conservatism + bounded conquest name-layers: rivers keep the oldest culture's names; conquest renames settlements inside the conquered polygon only (S-M)
- [ ] M9.3 Age-keyed name erosion with stored compositional etymology, glossed in the inspector (S)
- [ ] M9.4 History marks the map: battle sites, war-renamed features, disused routes fading (M)
- [ ] M9.5 Berúthiel emissions + disputed/unresolved chronicle entries, bounded share (S)

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
