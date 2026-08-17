# Era I — The Deep Earth (M16–M70)

Full four-field specs for Era I of `../ROADMAP-500.md`: tectonic
prehistory as a generative sketch, ice ages and their carved legacy,
ocean circulation, soils and aquifers, GPU-resolution erosion, and the
landform vocabulary that lets the map explain itself — closed by
Forge I (M66–M70), which re-cuts what the era strained. The one-liners
in the parent file are binding; these specs expand them.

### M16 — Plates Remembered
- **Intent:** Give the deep past a shape so mountains, coasts, and rock stop being arbitrary noise draws.
- **Build:** Add a superseding ADR to the plate-simulation rejection (ADR-0002-era decisions on tectonics) that permits a generative *plate-history sketch*: a coarse Voronoi polygon set over the grid carrying per-plate drift-age and boundary-type (convergent/divergent/transform), consumed only as an input layer to `geo::heightmap`, never advanced in tick time.
- **Touches:** game/rust/src/geo.rs, game/rust/src/state.rs, new: docs/adr/0018-plate-history-sketch.md, new: game/rust/src/plates.rs
- **Gate:** `diagnose terrain` shows plate polygon count and mean drift-age within configured bands, and regenerating the same seed twice yields byte-identical plate polygons and heightmap hash.

### M17 — Orogeny Ages
- **Intent:** Mountains should look their age — young collisions jagged, old belts ground down by eons.
- **Build:** Tag each orogenic range segment with a birth-age drawn from its parent plate-boundary's drift-age in `plates.rs`, then modulate ridge sharpness and relief amplitude in `geo::heightmap` by an age-decay curve (sharpness ∝ e^(−age/τ)) so old ranges get lower relief and rounder crests.
- **Touches:** game/rust/src/geo.rs, game/rust/src/plates.rs, game/rust/src/state.rs
- **Gate:** `diagnose terrain` reports a monotonic relief-vs-age correlation across all range segments and the age field folds into `hash_state` with unchanged determinism on repeat runs.

### M18 — Rock Provinces
- **Intent:** The ground itself should differ by history, not just by height, before anything is mined from it.
- **Build:** Generate a basement-geology grid classifying every cell into shield, sedimentary basin, fold belt, or volcanic terrane using plate age, distance to orogeny, and elevation, stored as a new `RockProvince` enum grid alongside height and biome.
- **Touches:** game/rust/src/geo.rs, game/rust/src/plates.rs, new: game/rust/src/rock.rs, game/rust/src/state.rs, game/rust/src/pack.rs
- **Gate:** `diagnose terrain` adds a province-share check (each of the four classes present at ≥2% of land) and the province grid is included in `hash_state`.

### M19 — Deposits Re-seated
- **Intent:** Ore should sit where geology says it belongs, not wherever noise happened to roll high.
- **Build:** Rewrite `resources::place_resources` to weight deposit-type placement by `RockProvince` (gold and tin favor shields and granite intrusions, coal favors basins, tin favors granitic terranes) while preserving the ADR-0013 per-mineral floor-guarantee pass unchanged in ordering and RNG derivation.
- **Touches:** game/rust/src/resources.rs, game/rust/src/rock.rs, docs/adr/0013-resource-floor-guarantees.md
- **Gate:** `diagnose resources` shows province-consistency ≥90% (gold-in-shield, coal-in-basin, tin-in-granite) across the sweep while floor counts (stone/coal/copper/iron ≥4, silver/gold ≥2, mithril ≥1) still pass every seed.

### M20 — Regional Stone
- **Intent:** Building stone should read as quarried from the land beneath the town, feeding the M14.5 goods roster with geography.
- **Build:** Derive a per-province quarriable-stone type (granite on shields, marble on fold-belt metamorphics, limestone on sedimentary basins) and thread it into `trade::goods_for` so settlements gain a stone-good tag matching their underlying province.
- **Touches:** game/rust/src/rock.rs, game/rust/src/trade.rs, game/rust/src/resources.rs
- **Gate:** `diagnose economy` confirms every settlement's stone good matches its cell's rock province in the sweep and goods-assignment hashes stay stable across reruns.

### M21 — Geologic Legibility
- **Intent:** The province map must read true to a human glance before more systems build on it.
- **Build:** Add harness checks that render-sample the province grid against known landform correlates (shields near cratonic interiors, basins in low-relief troughs, fold belts hugging young orogeny) and flag province/geometry mismatches as diagnostic warnings.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/rock.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose earth` (province-legibility subcheck) passes with zero mismatched-province cells beyond a 3% noise-edge tolerance across the seed sweep.

### M22 — Fault Seams and Earthquakes
- **Intent:** The deep earth should act during simulated history, not only at world-genesis.
- **Build:** Derive fault seams from transform and convergent plate-boundary segments, and during `systems::monthly` roll dated earthquake events with a magnitude drawn from seam stress accumulation (time-since-last-event × boundary type), each event carrying an epicenter cell, magnitude, and tick.
- **Touches:** game/rust/src/plates.rs, game/rust/src/systems.rs, game/rust/src/event.rs, new: game/rust/src/seismic.rs
- **Gate:** `diagnose earth` bands mean quake frequency per fault-length per century, and the seismic event log replays byte-identical from a fixed seed across native and WASM.

### M23 — Live Volcanism
- **Intent:** Arcs and hotspots that already sculpt terrain at genesis should keep erupting through the centuries they're inhabited.
- **Build:** Extend `seismic.rs` with a volcanism model tied to the arc/hotspot chains from `geo::heightmap`, scheduling dated eruptions that raise a local ash-plume fertility bonus and a burn/bury hazard radius, with eruption probability scaled by hotspot age (young islands erupt oftener).
- **Touches:** game/rust/src/geo.rs, game/rust/src/seismic.rs, game/rust/src/systems.rs, game/rust/src/agriculture.rs
- **Gate:** `diagnose earth` shows eruption cadence banded by arc-age tercile and fertile-slope bonus decaying correctly with distance in the sweep.

### M24 — Disaster Wiring
- **Intent:** Quakes and eruptions must leave marks on the world's memory, not vanish as unlogged numbers.
- **Build:** Wire seismic and volcanic events into `chronicle::monthly` as chronicle beats, spawn ruin sites (per M9's ruin vocabulary) for settlements destroyed above a magnitude/proximity threshold, and seed rebuild arcs that regrow population over a bounded recovery window.
- **Touches:** game/rust/src/seismic.rs, game/rust/src/chronicle.rs, game/rust/src/settlements.rs, game/rust/src/event.rs
- **Gate:** `diagnose civilization` confirms every destroying-magnitude event produces exactly one chronicle entry and one ruin, and rebuild arcs restore population within the banded recovery-time window on the sweep.

### M25 — Sea-Level History
- **Intent:** Coasts should carry the memory of ice ages, not sit at one eternal waterline.
- **Build:** Add a eustatic sea-level curve (glacial-cycle-driven, low-stand to high-stand amplitude from the climate research digest) combined with a post-glacial isostatic rebound field keyed to prior ice-load latitude, both applied as a time-varying offset to `geo::heightmap`'s ocean threshold at world-genesis freeze time.
- **Touches:** game/rust/src/geo.rs, game/rust/src/climate.rs, new: game/rust/src/sealevel.rs
- **Gate:** `diagnose terrain` reports final coastline area within ±5% of the pre-M25 baseline for a mid-curve seed and the sea-level offset is folded into `hash_state`.

### M26 — Drowned and Raised Coasts
- **Intent:** The slow breath of ice and sea should leave a legible vocabulary of coastal landforms.
- **Build:** Classify coastal cells by sea-level-history delta into rias (drowned river valleys), skerries (drowned low relief archipelagos), and raised beaches (former high-stand shorelines now inland), storing the classification as a new coastal-landform tag consumed by naming and rendering.
- **Touches:** game/rust/src/sealevel.rs, game/rust/src/geo.rs, game/rust/src/naming.rs, new: game/rust/src/landform.rs
- **Gate:** `diagnose terrain` bands the frequency of each coastal-landform type against sea-level-curve amplitude and the landform tag grid is stable across reruns.

### M27 — Deep-Earth Determinism
- **Intent:** Close Year 1's ground-truth work by proving the new layers don't cost the world its reproducibility or its speed.
- **Build:** Fold plate, rock-province, seismic-event, and sea-level-history state fully into `hash_state`, and profile native world generation to confirm the added passes stay inside the existing generation-time budget.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/state.rs, game/rust/src/plates.rs, game/rust/src/rock.rs, game/rust/src/sealevel.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose determinism` passes bit-identical across native/WASM/chunking with all Year-1 layers included, and generation-time budget stays green in `report.sh`.

### M28 — Ice-Sheet Extent Model
- **Intent:** Give the world an ice age whose footprint is earned from latitude and elevation, not painted on.
- **Build:** Compute ice-sheet extent from a latitude-and-elevation mass-balance heuristic driven by the glacial-cycle phase already established in `sealevel.rs`, producing a per-cell peak-glaciation flag and ice-thickness estimate at the last glacial maximum.
- **Touches:** game/rust/src/sealevel.rs, game/rust/src/climate.rs, new: game/rust/src/ice.rs
- **Gate:** `diagnose earth` bands ice-sheet latitude extent against elevation-adjusted expectation and the ice-extent grid folds into `hash_state`.

### M29 — Ice-Carved Relief
- **Intent:** Where the sheets sat, the land should show it — U-valleys, fjords, cirques, hanging valleys.
- **Build:** Apply a glacial-carving pass over cells flagged peak-glaciated in `ice.rs`, widening and flattening valley floors along prior drainage lines into U-profiles, cutting cirque bowls at former ice-source elevations, and marking hanging valleys where a carved tributary meets a deeper-carved trunk.
- **Touches:** game/rust/src/ice.rs, game/rust/src/geo.rs, game/rust/src/hydrology.rs, game/rust/src/landform.rs
- **Gate:** `diagnose terrain` shows U-valley and cirque counts within earth-analog density bands per glaciated latitude belt, height field remains NaN-free.

### M30 — Depositional Legacy
- **Intent:** Ice leaves behind what it dropped, and that till should feed the farms that follow.
- **Build:** Deposit moraine ridges at former ice margins, drumlin fields aligned with ice-flow direction, and eskers along former subglacial meltwater channels, then raise a till-plain fertility bonus in `agriculture::fertility` for cells within the depositional footprint.
- **Touches:** game/rust/src/ice.rs, game/rust/src/landform.rs, game/rust/src/agriculture.rs
- **Gate:** `diagnose earth` bands moraine/drumlin/esker counts by glaciated belt and `diagnose civilization` shows till-plain settlements with elevated food capacity versus non-till controls.

### M31 — Proglacial Lakes and Spillways
- **Intent:** Melting ice dammed by its own moraines carved the giant lakes and channels that outlive it.
- **Build:** Identify basins bounded by moraine ridges and retreating ice fronts as proglacial lake sites, filling them via `hydrology::fill_depressions` logic at the glacial-retreat timestep, and carve giant spillway channels where lake overflow found an outlet, leaving oversized abandoned valleys in the terrain.
- **Touches:** game/rust/src/ice.rs, game/rust/src/hydrology.rs, game/rust/src/landform.rs
- **Gate:** `diagnose terrain` reports proglacial lake chain count and spillway channel width bands scaled to catchment area, deterministic across reruns.

### M32 — Outwash Plains and Braided Meltwater Rivers
- **Intent:** Below the old ice line the land should still show the wide, restless work of meltwater.
- **Build:** Extend `hydrology.rs` river classification with a braided-channel form for high-sediment, low-slope reaches below the former ice margin, and flatten adjoining cells into outwash-plain terrain with elevated sediment-derived fertility.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/ice.rs, game/rust/src/agriculture.rs, game/rust/src/landform.rs
- **Gate:** `diagnose earth` bands braided-reach share of total river length below the ice line and confirms outwash-plain fertility exceeds background by a fixed band.

### M33 — Permafrost and Patterned Ground
- **Intent:** The cold rim should carry its own frozen-ground signature, distinct from ordinary tundra.
- **Build:** Compute a permafrost-extent field from mean annual temperature and continentality (`climate::continentality`), and generate patterned-ground micro-texture (polygons, stripes) as a rendering/inspector attribute on qualifying cells.
- **Touches:** game/rust/src/climate.rs, new: game/rust/src/permafrost.rs, game/rust/src/landform.rs
- **Gate:** `diagnose earth` shows permafrost extent tracking the −2°C mean-annual isotherm within a defined tolerance band across the sweep.

### M34 — Mountain Glaciers
- **Intent:** Ice should still live on the world's highest ground, responding to the climate that's already simulated.
- **Build:** Place modern mountain glaciers at cells above the climate-derived snowline (elevation where mean annual temperature crosses freezing, adjusted by latitude), sized by a simple accumulation-minus-ablation balance recomputed from `climate::month_temperature`.
- **Touches:** game/rust/src/climate.rs, game/rust/src/ice.rs, game/rust/src/geo.rs
- **Gate:** `diagnose earth` bands modern glacier count and mean elevation against the computed snowline per latitude belt, stable under determinism replay.

### M35 — Glacier-Fed Discharge
- **Intent:** Rivers born of ice should swell in summer melt, extending the seasonal-regime vocabulary M1.7 began.
- **Build:** Add a glacial-melt discharge term to `hydrology::hydrology`'s monthly discharge computation, peaking in the warm months proportional to upstream glacier mass from `ice.rs`, layered onto the existing precipitation-driven regime types.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/ice.rs, game/rust/src/climate.rs
- **Gate:** `diagnose hydrology` classifies glacier-fed rivers with a distinct summer-peak seasonality signature and monthly discharge arrays stay non-negative and NaN-free across the sweep.

### M36 — Ice Diagnostics
- **Intent:** Prove the ice-age stack reads true at a glance before moving to the frozen sea.
- **Build:** Add harness checks banding fjord density, proglacial-lake density, and moraine-field cadence by latitude belt against earth-analog reference ranges compiled from the climate and terrain digests.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/ice.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose earth` ice-cadence subcheck passes with all three landform densities inside their latitude-belt bands across the seed sweep.

### M37 — Sea Ice
- **Intent:** Winter should close the sea the way it closes the land, with straits that freeze shut.
- **Build:** Compute seasonal pack-ice extent from monthly sea-surface temperature proxy (latitude, continentality, month) and mark strait cells below the freezing threshold as winter-closed in the trade-route graph consumed by `trade::astar`.
- **Touches:** game/rust/src/climate.rs, game/rust/src/trade.rs, new: game/rust/src/seaice.rs
- **Gate:** `diagnose economy` shows winter-closed strait routes reopening in the correct months across the sweep, and route costs stay deterministic across reruns.

### M38 — Tundra Honesty
- **Intent:** Biomes under permafrost should behave like real cold ground, not warm-climate defaults pushed north.
- **Build:** Refine `biomes::classify` so treeline placement follows the permafrost boundary and growing-degree-day threshold rather than a raw temperature cutoff, splitting tundra into wet and dry variants keyed on permafrost table depth.
- **Touches:** game/rust/src/biomes.rs, game/rust/src/permafrost.rs, game/rust/src/climate.rs
- **Gate:** `diagnose terrain` confirms treeline latitude tracks the permafrost boundary within a fixed band and tundra subtype shares stay within their configured range.

### M39 — Glacial Calibration vs Earth
- **Intent:** Close Year 2 by proving the ice-age world matches Earth's cold-latitude bones, not just its own internal bands.
- **Build:** Add earth-comparison checks for fjord latitude distribution, proglacial-lake density per glaciated-belt area, and modern-glacier elevation-versus-latitude curve, referencing the ranges compiled from the terrain and climate research digests.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/ice.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose earth` calibration subcheck passes fjord-latitude, lake-density, and glacier-elevation bands simultaneously across the full seed sweep.

### M40 — Wind-Driven Gyres
- **Intent:** The ocean should circulate the way its winds and pressure fields already say it must.
- **Build:** Derive basin-scale surface-current gyres from the existing wind and pressure fields in `climate.rs`, resolving a coarse current-vector grid per ocean basin using Ekman-style deflection (Coriolis-sensed turning of surface wind stress) bounded by coastline geometry.
- **Touches:** game/rust/src/climate.rs, new: game/rust/src/currents.rs, game/rust/src/geo.rs
- **Gate:** `diagnose earth` shows gyre rotation sense matching hemisphere (clockwise north, counterclockwise south) on every basin in the sweep, current field stable under determinism replay.

### M41 — Heat Transport
- **Intent:** Warm and cold currents should bend the coasts they touch, the way real oceans reshape climate.
- **Build:** Compute a current-driven coastal temperature bias from `currents.rs` current-vector magnitude and source-latitude, adding warm-current coastal warming (Gulf-Stream analog) and cold-current coastal cooling/desertification (Humboldt analog) into `climate::temperature_mean`.
- **Touches:** game/rust/src/currents.rs, game/rust/src/climate.rs
- **Gate:** `diagnose terrain` bands warm-current-coast versus cold-current-coast temperature deltas within a target range replicated across the sweep.

### M42 — Current-Aware Climate Re-derivation
- **Intent:** Deserts and rain belts must answer to the new currents, not linger tuned to a current-blind world.
- **Build:** Re-run and re-tune `climate::precipitation`'s subsidence and advection passes with the current-driven temperature bias folded in, re-band desert share and rain-belt placement to account for cold-current coastal deserts and warm-current coastal wet belts.
- **Touches:** game/rust/src/climate.rs, game/rust/src/currents.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose climate` desert-share band (12–28% of land) and new cold-current-desert subcheck both pass across the full seed sweep.

### M43 — The Tides
- **Intent:** The shore should breathe daily, not just seasonally, with a range that answers to the shape of its sea.
- **Build:** Derive a tidal-range field from basin geometry (enclosed-sea amplification, open-coast damping) using a simplified resonance heuristic keyed to basin width and depth, then mark tidal-flat and estuary cells where range and low-slope coastal gradient coincide.
- **Touches:** game/rust/src/geo.rs, game/rust/src/hydrology.rs, new: game/rust/src/tides.rs, game/rust/src/landform.rs
- **Gate:** `diagnose earth` bands tidal-range by basin-enclosure class and confirms tidal-flat cell count scales with range and coastal slope across the sweep.

### M44 — Longshore Drift
- **Intent:** The coast stops being a static outline and starts moving sediment along itself, giving the world spits, barrier islands, and lagoons that harbors must reckon with.
- **Build:** Add a longshore-transport pass in `geo.rs` that reads wave-approach angle (derived from prevailing wind direction already computed for climate) against coastline tangent per coastal cell, accumulates a sediment-flux scalar along the shore direction, and deposits it where flux converges (river mouths, embayment necks) to grow spit and barrier-island landforms with lagoons pinched off behind them; classify affected cells with a new `CoastForm` enum (open, spit, barrier, lagoon) stored per-cell.
- **Touches:** game/rust/src/geo.rs, game/rust/src/hydrology.rs, game/rust/src/climate.rs, new: game/rust/src/coast.rs
- **Gate:** `diagnose terrain` reports spit/barrier/lagoon cell counts within a bounded band (0.5–4% of coastal cells) across the standard seed sweep, and `CoastForm` folds into `hash_state` with byte-identical regeneration.

### M45 — Harbor-Shelter Scoring
- **Intent:** Settlements finally read the coast the way sailors do, so ports rise where geometry actually shelters ships, closing GAP §6.
- **Build:** Compute a per-cell shelter score in `settlements.rs` from local coastline concavity, fetch distance to open water, and the new `CoastForm` classification (lagoons and rias score high, straight exposed shore low), and fold it into the existing site-scoring pass alongside fresh water and terrain so port placement stops relying on adjacency alone; expose the score to `explain.rs` for inspector provenance.
- **Touches:** game/rust/src/settlements.rs, game/rust/src/coast.rs, game/rust/src/explain.rs, game/rust/src/trade.rs
- **Gate:** `diagnose civ` shows port settlements concentrated on top-quartile shelter-score cells (≥70% of ports) across the seed sweep, with shelter score in `hash_state` and unchanged determinism hash for non-coastal seeds.

### M46 — Priced Sea Lanes
- **Intent:** Ships stop pretending all open water costs the same, so trade finally feels the ocean it sails on.
- **Build:** Extend `trade.rs` route costing so sea legs read the M40/M41 gyre and current vectors plus prevailing wind direction, applying a directional multiplier to `OPEN_SEA_COST` (with-current and downwind legs cheaper, against-current and doldrum legs dearer, capped by a floor and ceiling to keep the routing graph well-conditioned) and add a slow-water "doldrum" penalty band near current convergence zones.
- **Touches:** game/rust/src/trade.rs, game/rust/src/climate.rs, game/rust/src/world.rs
- **Gate:** `diagnose economy` shows with-current route travel times at least 15% faster than the seed-matched against-current mirror route, and route cost tables stay within existing price-pinning bands.

### M47 — Upwelling Zones
- **Intent:** Cold nutrient-rich coasts get marked now so Era IV's fisheries have honest ground to harvest later.
- **Build:** Derive an upwelling scalar field in `climate.rs` from offshore wind direction crossing the coast (Ekman transport proxy) combined with cold-current adjacency from M41, mark qualifying coastal cells with a `nutrient_rich` flag, and store the scalar as a new packed field consumed only by diagnostics and inspector for now.
- **Touches:** game/rust/src/climate.rs, game/rust/src/pack.rs, game/rust/src/explain.rs
- **Gate:** `diagnose climate` reports upwelling-coast share in a 3–10% of coastline band matching known west-coast-desert analogues, and the new field is CRC-stable and hash-included.

### M48 — Sea-Route Seasonality
- **Intent:** The sailor's calendar arrives: monsoon winds open and close lanes the way winter ice already does.
- **Build:** Extend `trade.rs`'s seasonal route machinery (which already carries barge-season logic for rivers) to sea legs, deriving a monthly wind-reliability curve per lane from the climate module's monsoon reversal signal and gating summer/winter sailing windows the same way pack-ice closures already gate winter straits; unify both under one `SeasonalClosure` enum.
- **Touches:** game/rust/src/trade.rs, game/rust/src/climate.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose economy` across a 12-month cycle shows monsoon-lane throughput swinging at least 30% between peak and closed months, with month-by-month route state reproducing byte-identically across reruns.

### M49 — Ocean Diagnostics
- **Intent:** The ocean stack earns its own instrument panel before the era trusts it further.
- **Build:** Add `diagnose ocean` (mirroring the terrain/climate/hydro commands) reporting gyre topology (rotation sense, cell count per gyre), current-coast temperature deltas against the zonal mean, upwelling coverage, and sea-lane seasonality spread, each checked against sweet-range/hard-limit bands in the `Checks` framework.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/climate.rs
- **Gate:** `diagnose ocean` runs clean across the seed sweep with all bands green and is wired into `report.sh`'s aggregate SUMMARY.txt.

### M50 — Metamorphic Ocean Checks
- **Intent:** The ocean stack must prove it responds to its own physics, not just look plausible once.
- **Build:** Add metamorphic diagnostic checks that perturb a synthetic warm-current field (zeroing it) and assert the downstream coastal temperature and precipitation bands shift in the predicted direction, and a companion check that route travel-time deltas respond monotonically to injected current-strength changes; wire both into `diagnose ocean` as a `--metamorphic` mode.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/climate.rs, game/rust/src/trade.rs
- **Gate:** removing a warm current in the synthetic harness cools its coast by at least 2°C mean and slows adjacent routes measurably, both asserted as hard pass/fail in `diagnose ocean --metamorphic`.

### M51 — Soil Genesis
- **Intent:** Fertility stops being one scalar guess and becomes the product of what actually built the ground: rock, climate, plants, and slope.
- **Build:** Replace the single fertility scalar in `agriculture.rs` with a `SoilClass` grid computed from rock province (M18), climate regime, vegetation cover, and slope via a factorial soil-formation model in the spirit of Jenny's clorpt (parent material × climate × organisms × relief × time proxy), classifying cells into orders (podzol, chernozem, laterite, andosol, gley, aridisol) that each carry their own fertility, drainage, and depth curves.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/geo.rs, game/rust/src/biomes.rs, game/rust/src/pack.rs
- **Gate:** `diagnose resources` shows soil-order distribution matching a Whittaker-biome-consistent global mix within calibration bands (e.g. chernozem confined to temperate grassland climates), and `SoilClass` is folded into `hash_state`.

### M52 — Alluvium and Loess
- **Intent:** Rivers and old ice leave fertility behind them long after they've moved on, and the soil map should show it.
- **Build:** Add floodplain alluvium deposition in `agriculture.rs` keyed to discharge and valley-floor slope (widening the existing silt-source pass from M2's fertility work into a full `SoilClass` override), and a loess-deposition pass downwind of periglacial outwash plains (M32) using prevailing wind direction and distance decay, both overriding the base soil order with a fertility bonus class.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/hydrology.rs, game/rust/src/climate.rs
- **Gate:** `diagnose resources` shows floodplain and loess cells scoring in the top soil-fertility decile at a rate at least 3x the map baseline, reproducible byte-identically across reruns.

### M53 — Agriculture Re-Based on Soils
- **Intent:** Crop packages finally answer to the soil beneath them instead of a bypassed scalar, closing the loop M51 opened.
- **Build:** Rewrite `crop_packages` in `agriculture.rs` to read `SoilClass` (drainage, depth, base fertility) alongside temperature and precipitation, replacing the flat fertility multiplier with per-order suitability tables drawn from the GAEZ-style multiplicative model already cited in research/08, then re-tune the M8 economy bands that were calibrated against the old scalar.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/economy.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose economy` and `diagnose civ` hold within re-tuned population-growth and price bands across the seed sweep, and crop-suitability output stays deterministic under `diagnose determinism`.

### M54 — Aquifers and Water Tables
- **Intent:** Water hides underground before it ever reaches a river, and the world needs to know how much and where.
- **Build:** Add a water-table field to `hydrology.rs` derived from rock-province permeability (M18), annual rainfall infiltration, and elevation-relative baseflow, solved as a simplified steady-state Darcy diffusion over the height field so the table tracks topography but lags precipitation extremes, feeding an `aquifer_depth` grid.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/geo.rs, game/rust/src/climate.rs, game/rust/src/pack.rs
- **Gate:** `diagnose hydro` shows aquifer depth correlating inversely with valley-floor elevation (Spearman ≥0.5) across the seed sweep, and the field is CRC-stable and included in `hash_state`.

### M55 — Springs, Wells, and Oases
- **Intent:** Dry-land settlement stops cheating on invisible water; a well has to reach an aquifer that's actually there.
- **Build:** Mark spring cells in `hydrology.rs` where the water table daylights on a slope break, add oasis cells in arid biomes where aquifer depth is shallow enough for phreatophyte vegetation, and gate a new well-technology flag in `settlements.rs` so arid-zone settlement requires either surface water, a spring, or an unlocked well tech keyed to aquifer depth.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/settlements.rs, game/rust/src/biomes.rs, game/rust/src/economy.rs
- **Gate:** `diagnose civ` shows zero arid-biome settlements founded without qualifying water access (surface, spring, oasis, or well tech) across the full seed sweep.

### M56 — Karst Country
- **Intent:** Limestone country behaves like limestone country: rivers vanish, caves hollow, and the surface tells on the rock beneath.
- **Build:** Add a karst pass triggered on limestone rock-province cells (M18) that generates sinkhole clusters via a Poisson-disk-seeded dissolution model scaled by precipitation, marks disappearing-river segments where a stream crosses into karst terrain and re-emerges downstream as a spring, and tags qualifying caves for later chronicle/ruin hooks.
- **Touches:** game/rust/src/geo.rs, game/rust/src/hydrology.rs, game/rust/src/biomes.rs
- **Gate:** `diagnose terrain` confirms karst features appear only on limestone provinces and disappearing-river mass-balances against its re-emergence point within 2% across the seed sweep.

### M57 — GPU Erosion Compute Pass
- **Intent:** Erosion finally runs at the resolution the render already promises, closing the gap research/01 flagged as the terrain stack's biggest hole.
- **Build:** Port the stream-power incision and talus passes from `erosion.rs` to a wgpu compute shader operating on the full-resolution height texture, using the same implicit Braun-Willett relaxation toward each cell's D8 receiver so results stay unconditionally stable, with a CPU fallback path that must byte-match the GPU path bit-for-bit under a fixed-point readback contract.
- **Touches:** game/rust/src/erosion.rs, game/rust/src/render.rs, new: game/rust/src/shaders/erosion.wgsl, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose bench` shows the GPU erosion pass completing within the native generation budget, and `diagnose determinism` confirms GPU and CPU fallback paths produce an identical `hash_state`.

### M58 — River Forms II
- **Intent:** Rivers stop reading as single-pixel lines and start showing the meanders, oxbows, terraces, and braids their slope and load actually produce.
- **Build:** Extend `hydrology.rs` with a sinuosity model (Howard-Knutson migration-velocity-proportional-to-curvature) applied to low-slope reaches to generate meander belts and cutoff oxbow lakes, a terrace classification for valley-floor steps left by base-level fall, and a braided-channel flag where sediment load exceeds transport capacity on wide, gentle valley floors.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/erosion.rs, game/rust/src/render.rs
- **Gate:** `diagnose hydro` reports meander sinuosity index rising with decreasing valley slope (monotonic across slope deciles) and oxbow/braid counts within calibration bands across the seed sweep.

### M59 — Sediment Budget
- **Intent:** Eroded material has to go somewhere, and where it lands should grow deltas and silt harbors the way real rivers do.
- **Build:** Add a sediment-transport ledger to `erosion.rs` that tracks material detached upstream against the stream-power law and deposits the excess where transport capacity drops (river mouths, estuaries, lakes), growing delta landforms and progressively shoaling harbor-shelter scores (M45) at high-load river mouths over generation passes.
- **Touches:** game/rust/src/erosion.rs, game/rust/src/hydrology.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose terrain` shows delta cell area scaling with upstream drainage area and sediment load (positive correlation ≥0.6) across the seed sweep, with sediment mass conserved to within 1% end-to-end.

### M60 — Landform Vocabulary
- **Intent:** Every cell should be able to say what it is in one word, so inspector and naming stop guessing from raw scalars.
- **Build:** Add a unified `Landform` classification pass in a new module that folds the era's outputs — fjord, U-valley, moraine, esker, terrace, braid, karst, delta, spit, barrier, lagoon, aquifer-oasis — into one per-cell enum with priority rules for overlapping candidates, replacing the ad hoc per-feature flags scattered across `geo.rs`, `hydrology.rs`, and `erosion.rs` with one canonical lookup.
- **Touches:** new: game/rust/src/landform.rs, game/rust/src/geo.rs, game/rust/src/hydrology.rs, game/rust/src/erosion.rs, game/rust/src/pack.rs
- **Gate:** every land and coastal cell resolves to exactly one `Landform` value with no unclassified cells in the seed sweep, and the grid is CRC-stable and hash-included.

### M61 — "Why Is This Here"
- **Intent:** The inspector should be able to tell a player the causal chain that built any cell, rock to soil, in one card.
- **Build:** Extend `explain.rs` with a provenance-chain query that walks rock province → glacial history → hydrology/erosion events → soil class → landform for a given cell, assembling a human-readable chain of the actual generation decisions recorded at generation time (not re-derived heuristically), surfaced through the existing inspector API.
- **Touches:** game/rust/src/explain.rs, game/rust/src/landform.rs, game/rust/src/geo.rs, game/web/js
- **Gate:** `diagnose terrain --explain` confirms every sampled cell returns a non-empty, well-ordered provenance chain and the chain's terminal landform matches the cell's stored `Landform` value.

### M62 — Geomorphic Toponymy
- **Intent:** Place names should tell the truth about the ground: a fjord town's name should say fjord, in that culture's tongue.
- **Build:** Extend `naming.rs`'s per-culture generic-term tables so landform-driven generics (-dale, -fjord, -fell, -tarn, -karst equivalents) are drawn from the `Landform` classification at the named cell rather than generic terrain buckets, adding a landform-to-generic mapping table per culture style alongside the existing coined/templated name machinery.
- **Touches:** game/rust/src/naming.rs, game/rust/src/landform.rs, game/rust/src/culture.rs
- **Gate:** `diagnose civ` shows landform-generic name matches (e.g. fjord settlements taking fjord-class generics) at ≥90% consistency across cultures in the seed sweep, with name output stable under determinism reruns.

### M63 — The Atlas Learns
- **Intent:** The map itself should finally read like the deep-earth stack that built it, not a flat elevation ramp.
- **Build:** Add hillshade and hypsometric-tint shader passes in the wgpu renderer that read the new `Landform`, rock-province, and soil-class grids as selectable map layers, using multi-directional hillshading (research/10 #18) and a cross-blended hypsometric ramp keyed to climate so deserts stop reading green.
- **Touches:** game/rust/src/render.rs, new: game/rust/src/shaders/hillshade.wgsl, game/web/js/gpu.js, game/rust/src/pack.rs
- **Gate:** rendered layer toggle round-trips through pack v2 without altering `hash_state`, and a synthetic desert-elevation cell renders outside the green hue band under the cross-blended ramp.

### M64 — Calibration vs Earth
- **Intent:** The whole deep-earth stack has to answer to Earth's own numbers before the era can call itself proven.
- **Build:** Add hypsometry, drainage-density, floodplain-share, and coast-type-frequency checks to `diagnose properties`, benchmarking the seed sweep's aggregate distributions against published Earth reference bands, and extend the oatmeal-II expressive-range check (`diagnose era`) to cover the new landform vocabulary so no seed collapses toward a bland mean landform mix.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/landform.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose properties` and `diagnose era` both hold hypsometric-curve, drainage-density, and landform-diversity bands green across the full seed sweep with no seed pinned to a single dominant landform.

### M65 — Era I Gate
- **Intent:** The deep earth closes as a proven, sealed layer before the sky above it gets its turn.
- **Build:** Add `diagnose earth`, a single rollup command composing the terrain, ocean, hydro, resources, and properties checks introduced across M16–M64 into one pass/fail report, wire it into `report.sh`'s standard suite, run the 300-year civilizational sweep against the full deep-earth stack, and record the superseding ADRs (plate-history sketch, GPU erosion reopening) as closed in `docs/adr/README.md`.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, docs/adr/README.md, docs/adr/0003-single-seed-determinism.md
- **Gate:** `diagnose earth` runs green across the seed sweep, the 300-year sweep in `report.sh` shows no regression against the pre-Era-I SUMMARY.txt baseline, and determinism holds across native and WASM builds.

### M66 — Geo/Erosion/Hydrology Recast
- **Intent:** The era's modules earn a re-cut now that hindsight shows their real seams, before the next era builds on top of them.
- **Build:** Refactor `geo.rs`, `erosion.rs`, `hydrology.rs`, and `landform.rs` around the landform pipeline that actually emerged (rock → ice → water → soil → landform as a fixed staged interface rather than the era's incremental bolt-ons), extracting shared grid-neighbor and flow-routing utilities into one module and writing hindsight ADRs for any load-bearing shape changes.
- **Touches:** game/rust/src/geo.rs, game/rust/src/erosion.rs, game/rust/src/hydrology.rs, game/rust/src/landform.rs, docs/adr/README.md
- **Gate:** `diagnose determinism` shows an unchanged `hash_state` before and after the refactor across the seed sweep, and `report.sh` runs fully green.

### M67 — Compute Lane Hardened
- **Intent:** The GPU erosion pass proved itself as a one-off; now it becomes a reusable engine facility for whatever era needs GPU compute next.
- **Build:** Generalize the M57 erosion compute shader into a lane abstraction in `render.rs` (buffer staging, dispatch sizing, CPU-fallback contract, and readback synchronization as shared code) so future compute passes register against the same lane instead of duplicating wgpu boilerplate, and migrate the erosion pass to be its first client.
- **Touches:** game/rust/src/render.rs, new: game/rust/src/compute.rs, game/rust/src/erosion.rs, game/rust/src/shaders/erosion.wgsl
- **Gate:** `diagnose bench` shows erosion compute-lane throughput unchanged or improved versus pre-refactor timings, and `diagnose determinism` confirms GPU/CPU parity is preserved.

### M68 — New Grids into the Registry
- **Intent:** Everything the era grew — soil, aquifer, landform, sediment — lands in the field registry properly instead of riding along as hand-wired extras.
- **Build:** Register `SoilClass`, `aquifer_depth`, `Landform`, `CoastForm`, and the sediment ledger as declared fields in `pack.rs`'s field-registry macro with correct wire quantization (`u16` or `u16sqrt` per field's dynamic range), generate their delta-tick lanes and constants through the existing codegen path rather than hand-mirroring, per ADR-0015/0016.
- **Touches:** game/rust/src/pack.rs, game/rust/src/world.rs, game/rust/src/state.rs, docs/adr/0016-pack-v2-quantized-crc-payload.md
- **Gate:** pack payload round-trips byte-identically through quantize/dequantize for every new field across the seed sweep, and CRC32 validation passes with no fields left hand-mirrored outside the registry macro.

### M69 — Budgets Reheld
- **Intent:** The generation, memory, and payload costs the era piled up have to come back inside the bands the harness enforces.
- **Build:** Profile and trim the full deep-earth generation pipeline (geo through landform classification) against `diagnose bench` and `diagnose perf`, targeting native generation time, peak memory, and packed-payload size bands set before Era I began, applying whatever caching, buffer reuse, or pass-fusion the profile shows is cheapest to fix first.
- **Touches:** game/rust/src/world.rs, game/rust/src/erosion.rs, game/rust/src/hydrology.rs, game/rust/src/pack.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose bench` and `diagnose perf` show generation time, memory, and payload size all back within their pre-Era-I sweet-range bands at the standard world size.

### M70 — Suite Consolidated
- **Intent:** The growing suite has to stay fast and coherent, or every future forge inherits an ever-slower gate.
- **Build:** Consolidate the era's scattered no-NaN, byte-identity, and descent-invariant checks (stream-power monotonic descent, sediment conservation, aquifer non-negativity, landform classification completeness) into one property/round-trip lane in `diagnose properties`, deduplicating overlapping assertions introduced piecemeal across M44–M69.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/landform.rs
- **Gate:** the consolidated property/round-trip lane runs in less wall-clock time than the sum of its pre-consolidation predecessors while covering the same invariant set, and the full suite in `report.sh` remains fully green.
