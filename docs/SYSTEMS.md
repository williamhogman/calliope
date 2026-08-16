# Calliope — As-Built Systems Inventory

The precise current state of every engine system, as the baseline the
gap analysis (`GAP-ANALYSIS.md`) measures against. Module paths refer to
`game/rust/src/`. Refreshed at M9 closure (roadmap M1–M9 complete).

## 1. Terrain & tectonics — `geo.rs`, `erosion.rs`

- 3D-style Perlin/simplex stack with domain warping and ridged
  multifractals; continental plate mask noise; volcanic island arcs along
  plate-boundary bands; hotspot chains (age-graded, Hawaii-style);
  archipelago clustering (union-find) for island-group identity;
  foothill-belt secondary/tertiary range pass (M1.3).
- Erosion before hydrology (M1.1–M1.2): talus thermal pass, stream-power
  fluvial incision over `filled`/`dirs`/`discharge`, hillslope diffusion.
- Ocean-frame falloff guarantees zero border land (ADR-0014).
- Default 640×512 at a declared 4 km/cell (ADR-0004).
- **Not present:** uplift/orogeny history, terraces, glacial landforms,
  GPU erosion compute pass (Later/research).

## 2. Climate — `climate.rs`

- Seasonal temperature from latitude bands + altitude lapse + continentality
  (distance-to-sea damping of seasonal swing).
- Precipitation via iterative moisture advection from ocean sources along
  prevailing zonal winds, with orographic uplift extraction and rain-shadow
  drying; subtropical subsidence penalty; land evapotranspiration recycling.
- Seasonal ITCZ shift → monsoon wet/dry seasons in the tropics (M1.6);
  seasonal discharge downstream (wadis, barge seasonality — M1.7).
- Tuned to ~23 % desert share of land (harness band 12-28 %).
- **Not present:** explicit pressure systems/Hadley-cell wind fields, ocean
  currents' thermal transport, ENSO-style variability.

## 3. Hydrology — `hydrology.rs`

- D8 flow directions over depression-resolved height (fill/route),
  precipitation-weighted flow accumulation, river extraction by discharge
  threshold, Strahler ordering with width ∝ √discharge (M1.4),
  navigability classification for barge highways (ADR-0010).
- Endorheic basins keep terminal salt lakes in arid depressions (M1.5);
  lakes at fill sites; estuary/delta identification feeding settlement
  attraction; river floods and drought shocks as chronicle events.
- **Not present:** meanders/oxbows at render scale, groundwater/springs,
  waterfalls as named features.

## 4. Biomes, soils & agriculture — `biomes.rs`, `agriculture.rs`

- Whittaker-style biome classification from temperature × precipitation
  with altitude bands (alpine/ice) and coastal modifiers.
- Crop packages (wheat/rice/maize/pastoral) from temperature, precipitation
  and growing period; <300 mm pastoral boundary; per-package carrying
  capacity with tech scaling (Kaplan T^−0.5) feeding logistic growth
  (M2.1–M2.2).
- Soil fertility from climate optima, slope penalty, alluvial/silt bonus
  near rivers and deltas.
- Famine coupling: local subsistence failure → demand spike, population
  loss, migration, chronicle events (M2.6).
- **Not present:** wild flora/fauna, vegetation succession, pests/blights
  (Later/research).

## 5. Resources — `resources.rs`

- 19-type ontology (ported knowledge triples) placed by noise over
  geological/biome masks with deterministic jitter; richness grades.
- Hidden-deposit knowledge model with dawn-knowledge probabilities;
  finite reserves on non-renewables; depletion lifecycle (ADR-0011).
- Per-mineral guaranteed minimum seam counts (ADR-0013).

## 6. Settlements — `settlements.rs`, `world.rs`

- Site scoring: coast/harbour, freshwater, fertility, delta discharge
  bonus, resource pull (price-weighted unworked seams — mining-colony
  pressure), culture-fit.
- Dawn founding + offshoot colonization waves as population pressure rises;
  ore-led overflow band past the population-justified cap.
- Zipf rank-size validation with Gibrat growth correction (M2.3);
  Bettencourt scaling — output ∝ pop^1.15, upkeep ∝ pop^0.85 (M2.4);
  spacing calibrated against the 15-30 km market-town band (M2.5).
- Decline and death (M9.1): sustained shrinkage → `failing` state →
  terminal emigration drain → abandonment at the floor, leaving a named
  ruin entity with strata and fate.

## 7. Cultures & language — `culture.rs`, `naming.rs`

- k-means-seeded culture regions claiming terrain by fit; per-culture
  syllabic name generators with power-law weighted draws (M3.2) and
  per-culture toponym formation strategies (M3.1).
- Toponymy via connected-component labeling: oceans, seas, bays, continents,
  islands & archipelagos, ranges, peaks, forests, marshes, deserts, lakes,
  rivers; emergent Passes and Fords from trade-route squeeze points.
- Etymology glosses on every fragment, cited by chronicle and inspector
  (M3.3); exonym/endonym doubling on border features (M3.4); per-culture
  pantheons cited in omens, festivals and war names (M3.5).
- Name time (M9.2–M9.3): hydronym conservatism through conquest, bounded
  conquest renaming inside the conquered polygon, age-keyed name erosion
  with stored compositional etymology and `formerly` strata.
- **Not present:** sound-change language families, myth drift
  (Later/research).

## 8. Trade & economy — `trade.rs`, `economy.rs`

- Terrain-priced A* routes: sea ≈ 1/9 land cost, slope/biome multipliers,
  navigable-river barge lanes, harbour fees; mode-typed legs for rendering;
  rescue lifelines for stranded towns (ADR-0010); minority-component
  bridging via cheapest terrain lifeline (union-find).
- Relative-scarcity market: demand/supply pressure normalized by geometric
  mean, power-curve price targets, clamped and smoothed (ADR-0012);
  discovery/exhaustion market shocks; national treasuries; price-ratio
  calibration against medieval price lists (M2.7).
- Local markets (M5): trade-hub market areas from route connectivity,
  per-area prices with diffusion, arbitrage trade income from price gaps,
  gravity-model flow as harness cross-check, workforce-gated production
  recipes (ore→metal→tools/weapons).
- Disused routes age out and fade (M9.4); caravans skip old ways.
- **Not present:** merchant agents (M5.5 staged Later), taxation policy,
  currency debasement/inflation events.

## 9. Society & technology — `society.rs`

- 21-technology tree across four eras (Stone → High Age); adoption gated on
  prerequisites, resources (discovered, non-exhausted deposits), and
  culture pace; diffusion along trade routes.
- Polity ladder: tribe → chiefdom → kingdom → empire by population/tech.
- **Not present:** religion as a full system beyond pantheon
  (Later/research), laws/institutions.

## 10. Politics & war — `politics.rs`

- Influence-map territory (pop/tech/war-weighted kernels; live borders,
  RLE-encoded to the client) (M4.1).
- War score → peace terms: settlement transfer, tribute, vassalage (M4.2);
  opinion matrix with aggressive-expansion decay and coalition thresholds
  (M4.3); siege state machine with fortification as treasury sink (M4.4).
- Legitimacy/asabiyyah: frontier surge, ~3-4-generation decay,
  rebellion/fragmentation rolls (M4.5).
- Battle sites mark the map; conquest renames only within the conquered
  polygon (M9.2, M9.4).

## 11. Chronicle & the telling — `chronicle.rs`, `entity.rs`, `telling.rs`, `artifact.rs`, `patina.rs`

- Structured events carrying entity ids (M6.1) over a stable registry of
  settlements, cultures, rulers, persons, artifacts, wars, features and
  ruins; persistent named non-rulers (generals, prospectors, founders)
  (M6.2); artifacts with provenance (M6.3).
- Story sifter extracting Felt-style microstories, ranked by eventfulness
  and reversal detection (M6.5, M6.7); narration memory — earned epithets,
  callbacks, mention-aware templates (M6.8); two-layer telling: ground
  truth vs mythologized "Fireside" rendering (M6.9); drama pacing (M6.4).
- The withheld (M9.5): Berúthiel emissions and disputed entries in a
  bounded 2-8 % share, styled apart in the feed.
- Legends browser over the entity graph: sagas, relics, cast (M6.6).

## 12. Rendering — `render.rs` + WGSL, `game/web/js/`

- wgpu fullscreen-shader pipeline (ADR-0006): MDOW hillshade with Laplacian
  curvature etching (M7.5), climate-blended hypsometric ramps (M7.4),
  true-color terrain, animated water with specular/foam, seasonal snowline,
  concentric coastal vignettes (M7.1), atmosphere rim; frame governor.
- Canvas overlay: Töpfer-law label budgeting with zero-overlap placement
  (M7.2), letter-spaced area labels (M7.3), curved river labels (M7.7),
  smoothed and junction-merged routes (M7.6), political tinting, ruins as
  broken-wall glyphs with quiet italic labels, faded dashed old ways,
  centered scale bar.
- **Not present:** minimap/locator, von Thünen land-use rings
  (Later/research).

## 13. UI — `game/web/js/ui/` (Solid.js, ADR-0008)

- Map-first shell: lens strip, time cluster, inspector dock (views for all
  entity kinds incl. ruins and stories), outliner rail (places, peoples,
  market, chronicle, legends), omnibox search with fly-to, toasts and
  notification channels, pinned entities.
- Progressive disclosure; bottom sheets on mobile with touch pan/pinch.

## 14. Infrastructure

- Rust core → WASM in a worker (ADR-0002); binary pack + version-locked
  loader (ADR-0007); single-seed derived-stream determinism (ADR-0003);
  one-shot generation + monthly ticks (ADR-0005).
- Native diagnostics harness with banded checks and multi-seed sweep
  reports (ADR-0009): `diagnose` subcommands for civ, climate, hydrology,
  terrain, resources, economy, telling, patina, properties (seam
  invariants, metamorphic rain check, pack round-trip) and era (ERA
  plates, JSD oatmeal matrix).
- `scripts/roadmap-check.sh` as the roadmap stopping criterion.
