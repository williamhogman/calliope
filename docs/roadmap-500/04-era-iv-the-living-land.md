# Era IV — The Living Land (M181–M235)

Full four-field specs for Era IV of `../ROADMAP-500.md`: vegetation
succession and disturbance, wildlife and fisheries stocks, timber and
soil as depletable wealth, blights, murrains, and plague on the trade
graph, the ecology instrumented and calibrated — closed by Forge IV
(M231–M235), which re-cuts stocks, flows, and tick budgets. The
one-liners in the parent file are binding; these specs expand them.

### M181 — The Growing Land
- **Intent:** Vegetation stops being a static biome paint and starts aging, so the map can carry the marks of what happened to it.
- **Build:** Add a succession-state grid (grass, shrub, young wood, old forest) per land cell, advanced by a Markov state-transition model on deterministic clocks keyed to biome type, following the LANDIS/FireBGCv2 gap-model chain of grid state-transition with age; each cell holds an age counter and a target climax state derived from the existing biome table, and transitions fire on age thresholds seeded from the world RNG stream.
- **Touches:** game/rust/src/biomes.rs, game/rust/src/state.rs, game/rust/src/world.rs, new: game/rust/src/succession.rs
- **Gate:** `diagnose systems` shows every land cell's succession state advancing monotonically with age until climax, with zero cells regressing state absent a disturbance event.

### M182 — The Reset Clock
- **Intent:** Fire, windthrow, and the axe give the young forest a reason to stay young, coupling disturbance to the succession clock.
- **Build:** Introduce a disturbance event table (fire, windthrow, clearing) that zeroes a cell's succession age and rewinds its state to grass or shrub, driven by deterministic hazard fields (drought-linked fire risk, wind-linked windthrow) sampled once per year alongside the existing famine drought noise; clearing hooks into settlement land-use so the axe is a first-class disturbance source.
- **Touches:** game/rust/src/succession.rs, game/rust/src/world.rs, game/rust/src/event.rs, game/rust/src/famine.rs
- **Gate:** `diagnose systems` confirms disturbed cells reset to age zero exactly once per disturbance event and that disturbance rates track the seeded hazard fields within 5% over a 100-year run.

### M183 — Succession Bands and Determinism
- **Intent:** The succession mosaic must read as real terrain, not noise, and must survive the determinism gate untouched.
- **Build:** Tune succession-band coverage (grass/shrub/young/old ratios) against literature envelopes for temperate and boreal forest turnover cycles, and fold the succession grid into the determinism hash as a packed per-cell byte array; add a diagnostics table reporting the stationary distribution of states per biome.
- **Touches:** game/rust/src/succession.rs, game/rust/src/world.rs, game/rust/src/pack.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose determinism` hash includes succession state and stays byte-identical across repeated runs of the same seed; state-band ratios per biome sit within ±10% of their target stationary distribution.

### M184 — The Axe Takes Land
- **Intent:** Settlements should visibly eat the land around them for field and fuel, scaled by the mouths they must feed and the tools they wield.
- **Build:** Add a clearing model where settlement population and tech level drive an annual land-claim radius that converts forest/shrub succession states to cleared farmland or fuel-cut shrub, following Kaplan's Land = Land₀·T⁻⁰·⁵ per-capita land-technology scaling; claimed cells are marked as worked land distinct from natural succession states.
- **Touches:** game/rust/src/succession.rs, game/rust/src/settlements.rs, game/rust/src/agriculture.rs, game/rust/src/world.rs
- **Gate:** `diagnose civ` shows cleared-land area per settlement scaling with population and inversely with tech per Kaplan's T⁻⁰·⁵ law within a 15% band across three seeds.

### M185 — The Green Return
- **Intent:** Land that empties of people should slowly forget the plow, rejoining the wild the way M9's ruin decay already lets buildings do.
- **Build:** Give abandoned worked-land cells (settlement gone or shrunk past a threshold) a re-wilding path back into the succession chain, reusing M9's ruin-growth timing so overgrown ruins and reclaimed fields decay on the same clock; abandonment is detected from settlement population history, not a special flag.
- **Touches:** game/rust/src/succession.rs, game/rust/src/settlements.rs, game/rust/src/world.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose systems` shows abandoned cells re-enter the succession Markov chain within one tick of abandonment and reach shrub state inside the same age window as the M9 ruin-overgrowth baseline.

### M186 — The Map Remembers Harvest
- **Intent:** Centuries of clearing should be legible directly in the biome layer, deepening M14.8's terrain-history rendering with real land-use scars.
- **Build:** Extend the biome classification output to distinguish natural climax states from worked and recovering land, exposing a harvest-history overlay derived from cumulative clearing/re-wilding counts per cell so the atlas and renderer can show deforestation footprints, per M14.8's existing legibility work.
- **Touches:** game/rust/src/biomes.rs, game/rust/src/succession.rs, game/rust/src/render.rs, game/rust/src/explain.rs
- **Gate:** a 300-year sweep shows cumulative-clearing overlay values strictly non-decreasing per cell except across re-wilding events, verified by `diagnose sweep`.

### M187 — The Wild Stocks
- **Intent:** Herbivore and predator populations need real, boundedly dynamic numbers per region before hunting or trophic balance can mean anything.
- **Build:** Add per-region herbivore and predator stock grids updated by seasonal Ricker maps N_{t+1}=N_t·e^{r(1−N/K)} as recommended for discrete population models that must stay positive and stable, with K derived from succession state (old forest and grassland richness) and predator K coupled to prey stock; stocks are stored at region-bucket resolution to keep the state small.
- **Touches:** game/rust/src/succession.rs, new: game/rust/src/wildlife.rs, game/rust/src/state.rs, game/rust/src/world.rs
- **Gate:** across a 200-year run every wildlife stock stays non-negative and bounded within its region's K envelope with no Neimark-Sacker oscillation blow-up, checked by `diagnose systems`.

### M188 — The Hunt
- **Intent:** Food and furs should visibly draw the wild stocks down, giving hunting an economic and ecological cost.
- **Build:** Couple settlement food/fur demand to a hunting-offtake term subtracted from local wildlife stocks each season, with offtake capped by stock availability and feeding into existing resource and diet accounting; hunting pressure scales with population and available hunter labor the way the fishery and fur trade already draw on `resources.rs` abundance.
- **Touches:** game/rust/src/wildlife.rs, game/rust/src/resources.rs, game/rust/src/economy.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose economy` confirms hunted goods (meat, furs) never exceed available stock in a tick and total offtake tracks settlement population within a 10% band across seeds.

### M189 — Collapse and Refuge
- **Intent:** Overhunted regions should empty out for good, while the deep wood keeps a last herd alive, per the overkill literature's slow-breeder extinction risk.
- **Build:** Add extirpation logic where sustained hunting pressure above a threshold drives a region's stock to zero permanently (no natural K recovery) following the 2%/yr overkill extinction curve for slow breeders, while remote, low-accessibility regions (old forest, low route density) get a refuge multiplier that dampens offtake and preserves residual stock as a re-seeding source for neighboring regions.
- **Touches:** game/rust/src/wildlife.rs, game/rust/src/trade.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** `diagnose sweep` over 300 years shows extirpation occurring only in high-pressure regions and refuge regions retaining nonzero stock in at least 90% of seeds.

### M190 — Wilderness Layers
- **Intent:** The untouched land needs its own map, not just a byproduct of settlement and hunting logic.
- **Build:** Derive three renderable layers — game richness (from wildlife stocks), old-forest extent (from succession state), and untouched wilderness (cells never claimed or hunted) — and expose them through the explain/atlas pipeline as first-class map layers alongside the existing biome and resource layers.
- **Touches:** game/rust/src/wildlife.rs, game/rust/src/succession.rs, game/rust/src/render.rs, game/rust/src/explain.rs
- **Gate:** `diagnose systems` reports all three wilderness layers computed for every land cell with values in [0,1] and stable across repeated runs of the same seed.

### M191 — Beasts in the Telling
- **Intent:** The wild stocks earn a voice in the chronicle — wolf winters, the last aurochs, named hunts that mark a place in memory.
- **Build:** Add chronicle event templates keyed to wildlife thresholds: extirpation crossing zero fires a "last of its kind" beat, harsh winters crossing a stock-stress threshold fire "wolf winter" beats, and named megafauna species get individually tracked until their regional extinction, reusing the naming and telling machinery from `naming.rs`/`telling.rs`.
- **Touches:** game/rust/src/wildlife.rs, game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/rust/src/naming.rs
- **Gate:** `diagnose telling` over 150 years produces at least one wildlife-driven chronicle beat per extirpation event and zero duplicate beats for the same event across seeds.

### M192 — Ecology Determinism and Budget
- **Intent:** The new succession and wildlife systems must not cost more than the world can spare, and must hash exactly the same every run.
- **Build:** Pack succession and wildlife state into the registry-quantized snapshot format, fold both into the determinism hash, and profile the added tick cost against the generation and tick budgets established by earlier eras, trimming update frequency (e.g. seasonal rather than monthly wildlife steps) if budgets slip.
- **Touches:** game/rust/src/pack.rs, game/rust/src/state.rs, game/rust/src/world.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose determinism` and `diagnose perf` both stay green with succession and wildlife included: hash stable across reruns, generation and tick budgets within their existing bands.

### M193 — Fish on the Shelves
- **Intent:** The continental shelves and upwellings Era I marked on the map finally hold something worth fishing.
- **Build:** Add a fish-stock grid over coastal shelf and upwelling cells (from `hydrology.rs`/climate upwelling markers), updated by the same seasonal Ricker growth model as land wildlife, with K set by shelf productivity and upwelling strength rather than terrestrial succession.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/climate.rs, new: game/rust/src/fisheries.rs, game/rust/src/state.rs
- **Gate:** `diagnose hydro` shows fish stock K correlating with marked upwelling/shelf cells and all stocks staying non-negative and bounded over a 200-year run.

### M194 — The Fleets
- **Intent:** Coastal towns should work the banks for real, with the catch feeding their people the way farmland feeds inland towns.
- **Build:** Add fishing-fleet offtake for coastal settlements proportional to workforce and shelf accessibility, feeding caught fish into settlement carrying capacity and famine subsistence checks alongside crop yield, so fishing towns get a genuine population ceiling independent of farmland.
- **Touches:** game/rust/src/fisheries.rs, game/rust/src/settlements.rs, game/rust/src/famine.rs, game/rust/src/agriculture.rs
- **Gate:** `diagnose civ` shows coastal-only settlements sustaining population above the pastoral/hunter-gatherer K band via fish contribution alone, within 15% of the fleet-catch-derived ceiling.

### M195 — Collapse and Shift
- **Intent:** Overfished banks should empty out and stocks should follow the climate, echoing real herring-year shifts in the historical record.
- **Build:** Extend fish-stock offtake to allow local collapse under sustained overfishing (Ricker K driven to near-zero recovers slowly), and add a climate-linked drift term that shifts upwelling-driven K between neighboring shelf regions over decades, modeling herring-year-style stock migration.
- **Touches:** game/rust/src/fisheries.rs, game/rust/src/climate.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** `diagnose sweep` over 300 years shows at least one collapse-and-recovery cycle per seed and stock-K drift correlating with the underlying climate shift field.

### M196 — Timber as Stock
- **Intent:** Shipyards and hearths should draw the forest down for real, with regrowth slow and mapped, not an infinite tap.
- **Build:** Add a timber-stock ledger per forested region derived from succession-state old-forest area, drawn down by shipbuilding and fuel-wood demand in `economy.rs`/`resources.rs`, with regrowth bound to the succession clock rather than an independent recovery rate so timber and vegetation stay one system.
- **Touches:** game/rust/src/succession.rs, game/rust/src/resources.rs, game/rust/src/economy.rs, game/rust/src/trade.rs
- **Gate:** `diagnose economy` confirms timber stock never exceeds the standing old-forest area it is derived from and regrowth tracks succession-state transitions exactly.

### M197 — Charcoal and the Smelters
- **Intent:** Metal production should measurably eat the forest, tying smithing prosperity to a real ecological cost.
- **Build:** Add a charcoal-conversion step in the recipe chain (ore + charcoal → metal) that draws timber stock down per unit of metal produced, using a fixed historical charcoal-per-metal ratio, so smelting towns visibly deforest their hinterland over generations.
- **Touches:** game/rust/src/economy.rs, game/rust/src/resources.rs, game/rust/src/succession.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose sweep` shows timber stock near smelting settlements declining measurably faster than the regional baseline, in proportion to cumulative metal output.

### M198 — Naval Stores
- **Intent:** Mast trees and pitch belong in the goods catalogue as strategic materials, extending M14's resource work into the forest ledger.
- **Build:** Add mast-timber and pitch/tar as goods gated on old-forest and coniferous-succession presence respectively, feeding shipbuilding recipes and creating a scarcity dynamic distinct from ordinary timber, per M14's catalogue-extension pattern.
- **Touches:** game/rust/src/resources.rs, game/rust/src/economy.rs, game/rust/src/trade.rs, game/rust/src/succession.rs
- **Gate:** `diagnose economy` shows mast-timber and pitch prices rising as regional old-forest/conifer stock depletes, tracked across a 200-year sweep.

### M199 — Soil Exhaustion
- **Intent:** Monoculture should press yields down over time, and fallow and rotation techs should be the answer, not a free pass.
- **Build:** Add a per-cell soil-fertility depletion term that reduces the agriculture fertility scalar under continuous single-crop cultivation, with fallow periods and rotation technologies (as tech-gated multipliers) restoring fertility on a slower clock, following Clark's English yield-envelope figures for calibration.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/culture.rs, game/rust/src/famine.rs, game/rust/src/state.rs
- **Gate:** `diagnose civ` shows continuously-farmed cells losing 10-30% fertility over a century absent rotation tech, recovering within Clark's grain-yield envelope once rotation is adopted.

### M200 — Pasture Degradation
- **Intent:** Overgrazed marginal steppe should turn to waste and stay that way, the pastoral mirror of soil exhaustion.
- **Build:** Add overgrazing pressure on pastoral-package cells that pushes succession state toward a degraded shrub/waste terminal state with hysteresis — once crossed, the state-and-transition model does not revert to grassland on its own — following Brischke's overgrazed-grassland irreversibility finding.
- **Touches:** game/rust/src/succession.rs, game/rust/src/agriculture.rs, game/rust/src/culture.rs
- **Gate:** `diagnose systems` confirms degraded-waste cells never revert to grassland absent an explicit long-recovery disturbance event, across a 300-year run.

### M201 — Land-Care Calibration
- **Intent:** Exhaustion and recovery cadences must sit inside historical envelopes, not just look plausible.
- **Build:** Tune soil-exhaustion and pasture-degradation rate constants against the England 1209–1869 grain (60-100 p/km²) and sheep-corn (20-40 p/km²) density envelopes and Blyakharchuk's pastoral-aridity boundary, adding a diagnostics table comparing simulated regional yields to those bands across biome and tech-era combinations.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/succession.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose civ` reports regional population-density-from-land figures within the Clark grain/sheep-corn envelopes for at least 90% of sampled settlements across three seeds.

### M202 — Press-Harvest Metamorphics
- **Intent:** Every harvestable stock — timber, fish, game, soil — must obey one iron law: harder press, lower stock, never higher.
- **Build:** Add a harness-driven metamorphic test that sweeps harvest-pressure multipliers across timber, fisheries, wildlife, and soil-fertility systems and asserts stock trajectories are monotonically non-increasing in pressure at matched time horizons, reusing the property-test pattern already used for other harness metamorphics.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/wildlife.rs, game/rust/src/fisheries.rs, game/rust/src/succession.rs, game/rust/src/agriculture.rs
- **Gate:** `diagnose properties` runs the press-harvest sweep across all four stock systems and reports zero pressure-inversions across all seeds.

### M203 — The Living Conservation Ledger
- **Intent:** Nothing eaten should exceed what grew — the conservation-of-matter discipline of M15.6 now covers the living world too.
- **Build:** Extend the existing conservation ledger to track cumulative growth versus cumulative offtake for timber, fish, and wildlife stocks per region, asserting offtake never exceeds cumulative growth plus initial stock, matching M15.6's exact-accounting standard for the mineral economy.
- **Touches:** game/rust/src/resources.rs, game/rust/src/wildlife.rs, game/rust/src/fisheries.rs, game/rust/src/succession.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose properties` confirms cumulative offtake never exceeds cumulative growth plus initial endowment for any living stock, exact to floating-point tolerance, across all seeds.

### M204 — Renewable Diagnostics
- **Intent:** Collapse should be a real, if rare, outcome under heavy press, and otherwise the living stocks should sit quietly in band.
- **Build:** Add a standing diagnostics report classifying every renewable-stock region's trajectory as stable, recovering, or collapsed, and assert collapse frequency rises sharply under the press-sweep multipliers from M202 while staying under a low baseline rate (under historical overharvest thresholds) in ordinary runs.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/wildlife.rs, game/rust/src/fisheries.rs, game/rust/src/succession.rs
- **Gate:** `diagnose sweep` shows baseline collapse rate under 5% of regions per 200-year run and collapse rate exceeding 40% under M202's high-pressure sweep, across all seeds.

### M205 — Crop Blights
- **Intent:** Dated regional harvest-killers should test the granaries the way real blight years tested medieval Europe.
- **Build:** Add a deterministic blight event generator layered on the existing drought noise field, striking a region's dominant crop package for one to three seasons with a severe yield penalty, feeding directly into the famine-pass subsistence check so blight years read as sharper, spatially concentrated famines.
- **Touches:** game/rust/src/famine.rs, game/rust/src/agriculture.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose sweep` shows blight-year famine severity distinctly higher than ordinary drought-year famine at matched settlements, with blight event frequency stable across reruns of the same seed.

### M206 — Murrains
- **Intent:** Herd collapses should starve the pastoral belts the way blight starves the grain belts.
- **Build:** Add a livestock-murrain event generator analogous to crop blight but targeting pastoral-package settlements' herd stock, drawing down the pastoral carrying-capacity contribution for several seasons and feeding the same famine-pass subsistence check.
- **Touches:** game/rust/src/famine.rs, game/rust/src/agriculture.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose sweep` shows pastoral settlements experiencing famine-severity spikes during murrain years comparable in magnitude to blight-year spikes in grain settlements.

### M207 — Locust Years
- **Intent:** The dry margins need their own moving disaster, one that travels with the wind rather than sitting still.
- **Build:** Add a locust-swarm event that spawns on arid/pastoral-boundary regions and advects across the map along the prevailing wind field from `climate.rs` for several months, applying a blight-grade yield penalty to every crop cell it crosses before dissipating.
- **Touches:** game/rust/src/climate.rs, game/rust/src/famine.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose systems` confirms locust-swarm paths follow the wind-field direction within a 30-degree deviation band and dissipate within their designed lifespan across all seeds.

### M208 — Trade-Graph SIR
- **Intent:** Plague should ride the roads the way goods do, giving the trade network a second, darker use, with latency built into every mile.
- **Build:** Add an SIR epidemic model (R₀ 2-3, recovery rate γ≈0.1) running over the trade-route graph rather than the raster grid, with slow overland diffusion along route edges weighted by travel cost from `trade.rs` and fast port-to-port jumps for coastal settlements, per the Yue/Boerner-Severgnini finding that betweenness centrality and sea links set infection order.
- **Touches:** game/rust/src/trade.rs, new: game/rust/src/disease.rs, game/rust/src/settlements.rs, game/rust/src/world.rs
- **Gate:** `diagnose systems` shows infection reaching high-betweenness port settlements before low-connectivity inland settlements in at least 80% of seeded outbreaks, with S+I+R conserved exactly per settlement every tick.

### M209 — Plague Runs the Graph
- **Intent:** The trade routes that carry grain and cloth also carry death, so the road network gains real cost.
- **Build:** Implement `epidemic.rs` with an SIR compartment per settlement (β, γ from the digest's R₀ 2–3, γ≈0.1 band), diffusing along `trade.rs` routes with distance-scaled latency (Yue's 1.5–5 km/day overland) and instant port-to-port jumps for coastal superspreaders; town size and route betweenness (Boerner & Severgnini) scale local β, and sanitation tech from `society.rs` dampens it.
- **Touches:** new: game/rust/src/epidemic.rs, game/rust/src/trade.rs, game/rust/src/settlements.rs, game/rust/src/society.rs, game/rust/src/systems.rs
- **Gate:** determinism hash includes SIR state; single-seed 150-year run shows R₀ within 2–3 band and infection order correlating with route betweenness rank.

### M210 — Quarantine and Flight
- **Intent:** Courts and towns answer plague with closed roads and empty streets, not passive infection counters.
- **Build:** Add route-closure behavior keyed to infected fraction thresholds (temporarily zeroing `trade.rs` route capacity), and a flight migration channel in `famine.rs`'s migration machinery that pulls population out of high-mortality settlements toward kin towns below the threshold; both decisions are deterministic functions of infected-fraction and settlement policy, no RNG branching beyond derived streams.
- **Touches:** game/rust/src/trade.rs, game/rust/src/famine.rs, game/rust/src/epidemic.rs, game/rust/src/settlements.rs
- **Gate:** closed routes reopen deterministically once infected fraction drops below threshold, and flight migration is conservative — total population before and after a flight event matches to the unit.

### M211 — Pandemic Arcs and Aftermaths
- **Intent:** A plague is a story with a beginning, a dying, and a changed world after, not a stat that resets.
- **Build:** Name pandemic events with onset/peak/recession dates in `chronicle.rs`, and wire post-plague aftermath effects — wage-rate bump and land-price drop in `economy.rs` proportional to mortality, decaying over a multi-decade recovery window matching the digest's logistic-recovery-after-shock model (Fanta et al.).
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/economy.rs, game/rust/src/epidemic.rs, game/rust/src/event.rs
- **Gate:** every pandemic above a mortality floor produces exactly one named chronicle arc with onset/peak/recession dates, and wage/land aftermath decays to baseline within the calibrated recovery window under the determinism hash.

### M212 — The Endemic Burden
- **Intent:** Disease is not only a wave — some belts live with it always, and that steadily presses how many people the land can hold.
- **Build:** Add a background endemic-mortality term (malaria-belt style, keyed to climate wetness/heat bands from `climate.rs`) and an urban-graveyard term scaled by settlement density, both folded as a standing subtraction into the carrying-capacity math already in `agriculture.rs`.
- **Touches:** game/rust/src/climate.rs, game/rust/src/agriculture.rs, game/rust/src/epidemic.rs, game/rust/src/settlements.rs
- **Gate:** endemic belts show sustained population suppression versus disease-free control regions in the same climate band, stable across a 150-year run at the determinism hash.

### M213 — Mortality Against the Black Death
- **Intent:** The great dyings must sit inside the envelope history actually measured, not an arbitrary knob.
- **Build:** Calibrate pandemic severity parameters (β, γ, urban density multiplier) in `epidemic.rs` against Black-Death-class mortality envelopes (30–50% of afflicted regions), tuning the diagnostics harness's disease report to compare simulated peak mortality distributions to that band.
- **Touches:** game/rust/src/epidemic.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose epidemic` reports 150-year sweep peak regional mortality distribution with median inside the 30–50% Black-Death envelope across the 5-seed sweep.

### M214 — Mass Graves and Emptied Quarters
- **Intent:** The plague years must be legible on the map and in the record long after the dying stops.
- **Build:** Stamp mass-grave and emptied-quarter markers into the biome/settlement layer wherever mortality crosses a severity threshold, persisted as map features consumed by `render.rs`, and date every plague year explicitly in `chronicle.rs`'s year index.
- **Touches:** game/rust/src/settlements.rs, game/rust/src/chronicle.rs, game/rust/src/render.rs, game/rust/src/epidemic.rs
- **Gate:** every settlement crossing the mortality threshold carries a persistent grave/emptied-quarter marker in the pack, and chronicle year index lists every plague year exactly once.

### M215 — Sifter Patterns of the Dying
- **Intent:** The telling layer should recognize a plague arc, an emptied town, and the wild's return as namable shapes, not raw numbers.
- **Build:** Extend `telling.rs`'s sifter pattern library with three new motifs — the plague arc (onset-to-recovery), the emptied town (depopulation below a floor followed by re-wilding via M185's succession), and the wild's return (vegetation reclaiming abandoned land) — each keyed off `epidemic.rs` and succession state already tracked.
- **Touches:** game/rust/src/telling.rs, game/rust/src/epidemic.rs, game/rust/src/agriculture.rs, game/rust/src/chronicle.rs
- **Gate:** all three new sifter motifs fire at least once across the 5-seed sweep with no false positives against a hand-checked control seed.

### M216 — Disease Metamorphics
- **Intent:** The claim that dense, connected towns die faster must be proven by the harness, not assumed from the model.
- **Build:** Add a metamorphic property test to the harness that perturbs route connectivity and settlement density in a fixed test world and asserts monotonic response — higher connectivity and higher density strictly non-decreasing in peak infection speed and extent.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/epidemic.rs, game/rust/src/trade.rs
- **Gate:** the connectivity/density metamorphic sweep passes with strictly monotonic (never decreasing) infection-speed response across all tested perturbation steps.

### M217 — Ecology in the Atlas
- **Intent:** Succession, stocks, and disease history deserve first-class map layers, not buried counters.
- **Build:** Add atlas layers for vegetation-succession stage, stock depletion (wildlife/fish/timber), and cumulative plague history, each rendered through `render.rs`'s existing layer-toggle machinery and packed alongside the biome layer already in `pack.rs`.
- **Touches:** game/rust/src/render.rs, game/rust/src/pack.rs, game/rust/src/biomes.rs, game/web/js
- **Gate:** all three new layers toggle independently in the UI and round-trip through the pack with byte-identical values across two consecutive loads of the same snapshot.

### M218 — The Inspector Reads the Land
- **Intent:** Clicking a tile should tell its ecological history in plain language, not just its current numbers.
- **Build:** Extend `explain.rs`'s per-cell inspector text generator to compose a land-history sentence from succession stage, harvest/clearing history, and disease-recency state — e.g. "cleared oakwood, worked out, re-wilding since the plague" — built from the same state feeding the atlas layers of M217.
- **Touches:** game/rust/src/explain.rs, game/rust/src/agriculture.rs, game/rust/src/epidemic.rs, game/rust/src/biomes.rs
- **Gate:** inspector text for every land cell composes deterministically from stored state with no missing clauses across a full-map sweep of a test seed.

### M219 — Ecology at Cadence
- **Intent:** The chronicle should mention the land's turning at a rhythm that feels observed, never silent and never spammy.
- **Build:** Tune emission thresholds in `chronicle.rs` for succession transitions, stock collapses, and disease events against the existing cadence-band machinery, so ecological entries land at a rate comparable to political and economic ones.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/agriculture.rs, game/rust/src/epidemic.rs
- **Gate:** `diagnose chronicle` reports ecological entry rate per century within the same in-band tolerance already enforced for political/economic entries, no seed silent for a full era.

### M220 — Trophic Sanity
- **Intent:** The food web must hold recognizable shape — enough plants for prey, enough prey for predators — across every biome.
- **Build:** Add a trophic-ratio diagnostic comparing plant biomass proxy, herbivore stock, and predator stock per biome against literature-grounded ratio bands (Strange Loop's full-food-web guidance), flagging any biome where predator or herbivore populations run structurally inverted.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/resources.rs, game/rust/src/agriculture.rs
- **Gate:** trophic ratio report shows every biome's plant:herbivore:predator ratio within the calibrated band across the 5-seed sweep, zero structural inversions.

### M221 — Coupling Audit
- **Intent:** Carrying capacity, famine, and migration must actually read the living land, not a frozen snapshot of it.
- **Build:** Audit and wire remaining gaps so `agriculture.rs`'s carrying-capacity term, `famine.rs`'s harvest verdict, and settlement migration all consume live succession stage, stock depletion, and disease state each tick rather than any cached or generation-time value.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/famine.rs, game/rust/src/settlements.rs, game/rust/src/systems.rs
- **Gate:** a targeted test forcing a stock collapse or succession reversion produces a same-tick change in carrying capacity and migration pressure, verified against the determinism hash.

### M222 — Ecology Joins the Registry
- **Intent:** Every field the era added must be declared once, not hand-mirrored across the pack and the hash.
- **Build:** Register succession stage, wildlife/fish/timber stocks, and SIR compartments in the field-registry codegen (ADR-0015) with quantized delta-tick encoding matching pack v2 (ADR-0016), removing any hand-written pack/unpack code for these fields.
- **Touches:** game/rust/src/pack.rs, game/rust/src/agriculture.rs, game/rust/src/epidemic.rs, game/rust/src/resources.rs, docs/adr/0015-registry-codegen-architecture.md
- **Gate:** codegen output contains every ecology field with no hand-written pack code remaining, and pack round-trip hash matches pre-registration determinism hash bit-for-bit.

### M223 — Tick Budgets Hold
- **Intent:** The full ecology system must run inside the tick budget the harness already enforces, not blow past it.
- **Build:** Profile the per-month tick cost of succession, stock dynamics, and SIR diffusion together, and optimize hot paths (grid iteration order, buckets reuse from `util.rs`) until the combined ecology tick cost sits inside the existing budget band.
- **Touches:** game/rust/src/systems.rs, game/rust/src/agriculture.rs, game/rust/src/epidemic.rs, game/rust/src/util.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose perf` reports per-month tick time with full ecology enabled inside the established budget band across all sweep seeds.

### M224 — Five-Century Mosaics
- **Intent:** Across the deep run, the land must never tip to all-farm or all-wild — the balance is the point.
- **Build:** Run a 500-year single-seed sweep tracking farmland/wilderness area ratio over time, tuning clearing (M184), re-wilding (M185), and succession clock parameters until the ratio holds within a historically-plausible band for the full run.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/settlements.rs, game/rust/scripts/report.sh
- **Gate:** 500-year sweep shows farmland/wilderness ratio remaining within the calibrated band at every century checkpoint, no monotonic drift to either extreme.

### M225 — Every Land Its Own
- **Intent:** Two seeds should grow visibly different living lands, proving the ecology is generative, not a fixed script.
- **Build:** Add the ecology oatmeal-IV cross-seed distinctiveness check, comparing succession mosaics, stock trajectories, and plague histories across the sweep seeds for statistically meaningful divergence rather than superficial noise.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** oatmeal-IV report shows pairwise seed divergence above the established distinctiveness floor for succession mosaic, stock trajectory, and plague-history metrics.

### M226 — Press-Sweeps Standing
- **Intent:** The stress tests that prove the land bends but doesn't break must live in the harness permanently, not as one-off runs.
- **Build:** Promote the harvest-press and plague-press stress runs (from M202's press-harvest metamorphics and M216's disease metamorphics) into standing harness lanes invoked by `report.sh`, with their outputs summarized into SUMMARY.txt like every other lane.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** `report.sh` full mode runs both press-sweep lanes every invocation and their pass/fail status appears in SUMMARY.txt.

### M227 — Return-Time Calibration
- **Intent:** Blight, murrain, and pandemic must recur at rates history would recognize, not whatever frequency fell out of the model.
- **Build:** Measure return intervals for crop blights (M205), murrains (M206), and pandemics (M211) across the sweep and tune their trigger-probability constants until interval distributions sit inside historically-grounded envelopes.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/famine.rs, game/rust/src/epidemic.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose` return-time report shows blight, murrain, and pandemic recurrence intervals within calibrated envelopes across the 5-seed sweep.

### M228 — Property Suite for the Living Land
- **Intent:** The invariants the era depends on must be provable, not merely observed to hold in practice.
- **Build:** Add property tests asserting every stock (wildlife, fish, timber) stays non-negative under all presses, that the conservation ledger (M203) balances exactly across harvest and regrowth, and that SIR compartments stay bounded in [0, population] with S+I+R conserved every tick.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/resources.rs, game/rust/src/epidemic.rs, game/rust/src/agriculture.rs
- **Gate:** property suite passes with zero violations of stock non-negativity, exact conservation, and SIR boundedness across randomized fuzz runs and the seed sweep.

### M229 — `diagnose land` Joins the Standing Runners
- **Intent:** The era's diagnostics deserve a single named command that becomes permanent harness furniture.
- **Build:** Consolidate the succession, stock, disease, and trophic reports into one `diagnose land` subcommand producing a unified report file, wired into `report.sh` alongside the existing `terrain`, `economy`, and other standing runners.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` full mode invokes `diagnose land` for every seed and produces a `land-<seed>.txt` report consumed by SUMMARY.txt.

### M230 — Era IV Gate
- **Intent:** The living land must prove itself whole across the long run before the forge re-cuts it.
- **Build:** Run the full 300-year sweep across all seeds with every Era IV system engaged — succession, stocks, plague, and the coupling audit — and confirm every established band (trophic ratios, disease envelopes, farmland/wilderness balance, conservation) holds simultaneously.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** 300-year, 5-seed sweep passes with plague mortality, famine frequency, and wilderness balance all simultaneously in band, zero [FAIL] in SUMMARY.txt.

### M231 — One Stock-State Facility
- **Intent:** Minerals, forests, game, and fish grew four bespoke depletion mechanisms; the era's hindsight demands one shared facility.
- **Build:** Extract a single generic stock-state module (capacity, current level, Ricker/logistic regrowth, press-harvest function) behind `resources.rs`'s mineral deposits, `agriculture.rs`'s forest/timber stock, and the wildlife/fish stocks, replacing four divergent implementations with one parameterized behind an ADR.
- **Touches:** new: game/rust/src/stock.rs, game/rust/src/resources.rs, game/rust/src/agriculture.rs, new: docs/adr (shared stock-state facility ADR, numbered at land time)
- **Gate:** determinism hash unchanged before and after the refactor across the full seed sweep, and the four call sites all route through the shared module with no duplicated regrowth code remaining.

### M232 — Ecology Grids into Registry Codegen
- **Intent:** The shared stock facility must declare itself once through the same machinery every other field uses.
- **Build:** Extend the field-registry codegen (ADR-0015) to generate pack/unpack and quantization code directly from the new `stock.rs` type definitions, removing any remaining hand-written serialization for succession or stock grids.
- **Touches:** game/rust/src/pack.rs, game/rust/src/stock.rs, game/rust/src/agriculture.rs, docs/adr/0015-registry-codegen-architecture.md
- **Gate:** determinism hash unchanged through the codegen migration, and no hand-written pack/unpack function remains for any stock or succession field.

### M233 — Event-Table Families, Declared Once
- **Intent:** Blight, murrain, locust, plague, and stock-collapse events grew as separate ad-hoc event constructors; they belong to declared families.
- **Build:** Consolidate the ecology and disease event constructors scattered across `famine.rs`, `epidemic.rs`, and `agriculture.rs` into declared event-table families in `event.rs`, following the pattern already established for other event categories.
- **Touches:** game/rust/src/event.rs, game/rust/src/famine.rs, game/rust/src/epidemic.rs, game/rust/src/agriculture.rs
- **Gate:** determinism hash unchanged through the consolidation, and every ecology/disease event constructs through the declared table with zero remaining ad-hoc constructors.

### M234 — Budgets Back in Band
- **Intent:** The living land's full weight must sit back inside generation, tick, memory, and payload budgets before the next era builds atop it.
- **Build:** Re-profile generation time, monthly tick cost, resident memory, and pack payload size with every Era IV system engaged post-refactor, tuning grid resolutions and update cadences in `stock.rs` and `epidemic.rs` until all four budgets return to their established bands.
- **Touches:** game/rust/src/stock.rs, game/rust/src/epidemic.rs, game/rust/src/systems.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` full mode reports generation, tick, memory, and payload budgets all in band with the complete living-land system engaged.

### M235 — Harness Speed
- **Intent:** The growing press-sweep suite must stay fast enough to run every change, not slow the loop it's meant to protect.
- **Build:** Parallelize the press-sweep lanes (M226) and the standing seed sweep across available cores in `report.sh` and `diagnose.rs`, restructuring the runner loop so independent seeds and presses execute concurrently without touching simulation determinism.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** full suite wall-clock time returns to the established band with identical determinism hashes and identical SUMMARY.txt contents versus the sequential run.
