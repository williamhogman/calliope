# Calliope — As-Built Systems Inventory

The precise current state of every engine system, as the baseline the
gap analysis (`GAP-ANALYSIS.md`) measures against. Module paths refer to
`game/rust/src/`.

## 1. Terrain & tectonics — `geo.rs`

- 3D-style Perlin/simplex stack with domain warping and ridged
  multifractals; continental plate mask noise; volcanic island arcs along
  plate-boundary bands; hotspot chains (age-graded, Hawaii-style);
  archipelago clustering (union-find) for island-group identity.
- Ocean-frame falloff guarantees zero border land (ADR-0014).
- Default 640×512 at a declared 4 km/cell (ADR-0004).
- **Not present:** erosion of any kind (hydraulic, thermal, fluvial
  incision), uplift/orogeny history, terraces, glacial landforms.

## 2. Climate — `climate.rs`

- Seasonal temperature from latitude bands + altitude lapse + continentality
  (distance-to-sea damping of seasonal swing).
- Precipitation via iterative moisture advection from ocean sources along
  prevailing zonal winds (trade/westerly bands by latitude), with
  orographic uplift extraction and rain-shadow drying; subtropical
  subsidence penalty; land evapotranspiration recycling.
- Tuned to ~23 % desert share of land (harness band 12-28 %).
- **Not present:** explicit pressure systems/Hadley-cell wind fields,
  monsoon seasonality, ocean currents' thermal transport, ENSO-style
  variability, weather (storms, droughts as events).

## 3. Hydrology — `hydrology.rs`

- D8 flow directions over depression-resolved height (fill/route),
  precipitation-weighted flow accumulation, river extraction by discharge
  threshold, navigability classification for barge highways (ADR-0010).
- Lakes at fill sites; estuary/delta identification feeding settlement
  attraction.
- **Not present:** meanders/oxbows at render scale, seasonal flow variation,
  floods as events, groundwater/springs, waterfalls as named features.

## 4. Biomes, soils & agriculture — `biomes.rs`, `agriculture.rs`

- Whittaker-style biome classification from temperature × precipitation
  with altitude bands (alpine/ice) and coastal modifiers.
- Soil fertility from climate optima, slope penalty, alluvial/silt bonus
  near rivers and deltas.
- **Not present:** crop suites per culture/climate, growing seasons,
  wild flora/fauna, carrying capacity as an explicit layer, pests/blights.

## 5. Resources — `resources.rs`

- 19-type ontology (ported knowledge triples) placed by noise over
  geological/biome masks with deterministic jitter; richness grades.
- Hidden-deposit knowledge model with dawn-knowledge probabilities;
  finite reserves on non-renewables; depletion lifecycle (ADR-0011).
- Per-mineral guaranteed minimum seam counts (ADR-0013).

## 6. Settlements — `settlements.rs`

- Site scoring: coast/harbour, freshwater (tightened river dilation),
  fertility, delta discharge bonus, resource pull (price-weighted unworked
  seams — mining-colony pressure), culture-fit.
- Dawn founding + offshoot colonization waves as population pressure rises;
  ore-led overflow band past the population-justified cap.
- Town classes (river-town/harbour/mining camp…) tracked for diversity
  diagnostics.
- **Not present:** Zipf-calibrated size hierarchy, town growth/decline
  driven by market access (income feeds growth only via trade bonus),
  abandonment/ruins, urban morphology.

## 7. Cultures & language — `culture.rs`, `naming.rs`

- k-means-seeded culture regions (Hellenic, Nordic, Arid, Sylvan, Steppe)
  claiming terrain by fit; per-culture syllabic name generators.
- Toponymy via connected-component labeling: oceans, seas, bays, continents,
  islands & archipelagos, ranges, peaks, forests, marshes, deserts, lakes,
  rivers; emergent Passes and Fords from trade-route squeeze points.
- **Not present:** phonotactic rigor (syllable structure constraints per
  culture), language drift/borrowing, exonyms vs endonyms, name etymology
  surfacing ("-by means town").

## 8. Trade & economy — `trade.rs`, `economy.rs`

- Terrain-priced A* routes: sea ≈ 1/9 land cost, slope/biome multipliers,
  navigable-river barge lanes, harbour fees; mode-typed legs for rendering;
  rescue lifelines for stranded towns (ADR-0010).
- Relative-scarcity market: demand/supply pressure normalized by geometric
  mean, power-curve price targets, clamped and smoothed (ADR-0012);
  discovery/exhaustion market shocks; national treasuries.
- **Not present:** per-town markets and price differentials, merchants/
  caravans as agents, comparative-advantage specialization, taxation
  policy, currency debasement/inflation events.

## 9. Society & polities — `society.rs`

- 21-technology tree across four eras (Stone → High Age); adoption gated on
  prerequisites, resources (discovered, non-exhausted deposits), and
  culture pace; diffusion along trade routes.
- Polity ladder: tribe → chiefdom → kingdom → empire by population/tech;
  territory claims by settlement influence.
- **Not present:** internal politics (succession, factions, civil war),
  diplomacy between polities, religion as a system, laws/institutions.

## 10. Chronicle — `chronicle.rs`

- Dynasties with named rulers, epithets, reign arcs; wars and raids between
  polities; myths and omens; wonders; discovery/depletion beats; founding
  events. Rendered as a scrollable, filterable feed tied to sim time.
- **Not present:** causal chaining (war *because* of resource/succession),
  story sifting (recognizing emergent arcs), character-level actors below
  rulers, historical revisionism/legends drifting from fact.

## 11. Rendering — `render.rs` + WGSL, `game/web/js/`

- wgpu fullscreen-shader pipeline (ADR-0006): analytic hillshade, true-color
  terrain, animated water with specular/foam, seasonal snowline, coastal
  shelf tinting, atmosphere rim; frame governor.
- Canvas overlay: GIS-style typographic label hierarchy with screen-space
  collision culling, mode-styled trade routes, political tinting, markers,
  centered scale bar.
- **Not present:** relief-shaded political mode blending, label placement
  optimization beyond greedy culling, hydrography-aware label curvature
  (river labels along the run), minimap/locator.

## 12. UI — `game/web/js/ui/` (Solid.js, ADR-0008)

- Progressive disclosure: default political-satellite view; layer drawer,
  inspector, chronicle feed, simulation controls (calendar, speeds),
  bottom sheets on mobile with touch pan/pinch.

## 13. Infrastructure

- Rust core → WASM in a worker (ADR-0002); binary pack + version-locked
  loader (ADR-0007); single-seed derived-stream determinism (ADR-0003);
  one-shot generation + monthly ticks (ADR-0005).
- Native diagnostics harness with banded checks and multi-seed sweep
  reports (ADR-0009).
