# Era VII — The Wide World (M346–M400)

Full four-field specs for Era VII of `../ROADMAP-500.md`: the stage
widens past 1024 cells to a multi-continent globe, knowledge becomes
per-people state, expeditions cross blue water, hemispheres meet with
all their asymmetries, the world economy sorts into core and
periphery, hegemons rise and pass, and the whole instrument is proven
across a thousand-year run — closed by Forge VII (M396–M400), which
re-cuts memory, pack, and pipeline for the scale the era reached. The
one-liners in the parent file are binding; these specs expand them.

### M346 — The Wide Stage
- **Intent:** Scale is the raw material Era VII trades in, so the map itself must grow before any world-system behavior can exist.
- **Build:** Raise the grid ceiling past 1024 cells per side with multi-continent template generation in `GenBuilder`, spacing landmasses across an ocean-frame world per ADR-0014, and rescale every generation-time budget (memory, wall-clock, cache tiling) proportionally rather than by fixed constant.
- **Touches:** game/rust/src/world.rs, game/rust/src/geo.rs, game/rust/src/constants.rs, game/rust/src/bin/worldgen.rs, docs/adr/0014-ocean-frame-falloff.md
- **Gate:** `diagnose terrain` and `worldgen` at size 1024 and 2048 complete within budget bands scaled linearly with cell count, no landmass clipped at the frame edge.

### M347 — The Far Side
- **Intent:** A world worth discovering needs continents that exist fully formed and utterly unknown before anyone sails toward them.
- **Build:** Extend the multi-continent template so distant landmasses generate whole — terrain, climate, hydrology, biomes, resources, and settlements complete — while every people's knowledge state starts blank of them, keeping generation and knowledge as separate concerns in `World`.
- **Touches:** game/rust/src/world.rs, game/rust/src/geo.rs, game/rust/src/biomes.rs, game/rust/src/settlements.rs, new: game/rust/src/knowledge.rs
- **Gate:** `diagnose civ` shows full field completeness on every continent at generation while the known-world mask for every people covers zero far-continent cells at dawn.

### M348 — Scale Determinism
- **Intent:** Doubling the stage is worthless if the hash that proves the world repeatable breaks under the new size.
- **Build:** Extend the determinism hash and `diagnose determinism` to run at 1024-plus grids and multi-continent templates, verifying bit-identical output across platforms and thread counts at the new scale, and fold the wide-stage RNG streams into the seed-derivation scheme of ADR-0003.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/world.rs, game/rust/src/state.rs, docs/adr/0003-single-seed-determinism.md
- **Gate:** `diagnose determinism` at size 1024 and 2048 produces an identical hash across three consecutive runs and across at least two thread-count configurations.

### M349 — Knowledge Per People
- **Intent:** The world a people can act on is the world they know, not the world that exists, so knowledge must become simulated state.
- **Build:** Give every people a known-world map — a per-cell knowledge grade (unknown, rumored, mapped) tracked in the new knowledge module, seeded from home continent and updated by exploration, trade contact, and diplomacy, with terra incognita cells excluded from that people's route and settlement AI.
- **Touches:** game/rust/src/knowledge.rs, game/rust/src/world.rs, game/rust/src/society.rs, game/rust/src/trade.rs, game/rust/src/pack.rs
- **Gate:** `diagnose civ` confirms every people's known-cell count only grows monotonically and never exceeds the world's total revealed cells for that people.

### M350 — The Fog of the Far
- **Intent:** Knowledge thins with distance long before it vanishes, and what fills the gap is rumor, not silence.
- **Build:** Add distance-decayed rumor propagation along trade and diplomatic contact graphs that degrades knowledge grade with hops and time, and generate mythical-geography artifacts (sea-monster coasts, phantom islands) for cells beyond the mapped frontier, stored as fictions distinct from the true terrain layer.
- **Touches:** game/rust/src/knowledge.rs, game/rust/src/artifact.rs, game/rust/src/telling.rs, game/rust/src/naming.rs
- **Gate:** rumor-grade knowledge decays with an exponential-in-distance falloff matching a fixed half-distance band, and mythical-geography records never overwrite true terrain in the pack.

### M351 — Map-Knowledge Diagnostics
- **Intent:** Knowledge growth needs a measured cadence, not an unbounded rush to omniscience.
- **Build:** Add a `diagnose knowledge` command reporting known-world fraction over time per people, rumor half-life, and mythical-geography density, banding the growth curve against the exploration-era pacing established for Era III.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/knowledge.rs
- **Gate:** known-world growth per people stays within the report.sh cadence band across a full test run, flagged red on either stagnation or instant omniscience.

### M352 — Expeditions
- **Intent:** Someone has to go look, and the going is a story worth naming.
- **Build:** Revive Era III's captain-and-crew cast for sea expeditions that coast-crawl before attempting blue-water crossings, using the existing entity and event systems to track named captains, ship state, provisioning, and loss, with expedition routes feeding directly into the knowledge module's reveal calls.
- **Touches:** game/rust/src/entity.rs, game/rust/src/event.rs, game/rust/src/knowledge.rs, game/rust/src/trade.rs, game/rust/src/society.rs
- **Gate:** `diagnose civ` shows named expedition entities persisting across ticks with survival and knowledge-reveal outcomes reproducible under determinism replay.

### M353 — The Navigation Ladder
- **Intent:** Reaching the horizon should cost real technology, not a flat distance cap.
- **Build:** Gate expedition range by a navigation-tech ladder — dead reckoning, stellar navigation, magnetic compass, then instrument-assisted longitude — each rung unlocking longer blue-water legs and lower loss rates, tied into the existing culture/tech progression rather than a bespoke counter.
- **Touches:** game/rust/src/culture.rs, game/rust/src/entity.rs, game/rust/src/knowledge.rs, game/rust/src/constants.rs
- **Gate:** expedition max safe range and loss probability shift in lockstep with navigation-tech rung transitions, verified in `diagnose civ` output across a full tech-progression run.

### M354 — Landfall in the Telling
- **Intent:** A new coast is only real to a culture once it has been named and told.
- **Build:** Generate discovery chronicles for first landfall on unmapped coasts, using Era V's naming-generation machinery to coin place-names in the discovering people's tongue and feeding the event into `telling.rs`'s chronicle pipeline as a first-class narrative beat.
- **Touches:** game/rust/src/telling.rs, game/rust/src/naming.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** every first-landfall event produces exactly one chronicle entry with a tongue-consistent generated place-name, stable under determinism replay.

### M355 — First Contact Between Hemispheres
- **Intent:** Two worlds meeting is the era's central event, and it must arrive as a bundle of consequences, not a flag flip.
- **Build:** Trigger first-contact events when an expedition's knowledge reveal intersects an occupied foreign continent, simultaneously opening a trade route, seeding disease-exposure state per Era II's epidemiology model, and enabling war eligibility between the contacted peoples in one atomic transition.
- **Touches:** game/rust/src/society.rs, game/rust/src/trade.rs, game/rust/src/event.rs, game/rust/src/politics.rs, game/rust/src/famine.rs
- **Gate:** every first-contact event simultaneously stamps trade-eligible, disease-exposed, and war-eligible flags in the same tick, reproducible bit-for-bit under replay.

### M356 — Contact Asymmetries
- **Intent:** History's contacts were never symmetric, and the gap between peoples should be a measurable quantity, not a script.
- **Build:** Derive a tech-gradient and immunity-gradient score for each contact pair from existing culture-tech and disease-history state, and let those gradients scale disease mortality, war outcome odds, and trade-term advantage without any hardcoded winner.
- **Touches:** game/rust/src/politics.rs, game/rust/src/culture.rs, game/rust/src/famine.rs, game/rust/src/trade.rs
- **Gate:** `diagnose civ` reports gradient scores per contact pair whose sign correlates with measured mortality and war-outcome asymmetry across a batch of seeds.

### M357 — Contact-Outcome Bands
- **Intent:** The meeting of worlds must stay an open envelope, never a rigged conquest narrative.
- **Build:** Add contact-outcome diagnostics tracking the distribution of who conquers, trades, or is displaced across many seeds and gradient magnitudes, banding the outcome mix so neither side wins deterministically at any gradient level tested.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/politics.rs
- **Gate:** across a 50-seed sweep at matched gradient magnitude, both contact directions produce conquest, coexistence, and displacement outcomes within the banded mix, none pinned to zero or one.

### M358 — The Long Routes
- **Intent:** The trade network isn't whole until ships can cross oceans and profit from the crossing.
- **Build:** Extend `TradeGrid` and `astar` with a blue-water routing mode using the open-sea cost constant already defined, connect it to the navigation-tech ladder for reach, and let high-throughput crossing points spawn entrepôt settlements via `found_settlements` when connectivity and volume thresholds are crossed.
- **Touches:** game/rust/src/trade.rs, game/rust/src/settlements.rs, game/rust/src/constants.rs
- **Gate:** `diagnose economy` at world scale shows blue-water routes carrying nonzero flow and at least one entrepôt settlement founded at a crossing point in a standard test run.

### M359 — World Goods
- **Intent:** A world market is only a world market once every people's goods can reach every other people's shelves.
- **Build:** Extend the M14 goods catalogue's terminal-goods list across hemisphere boundaries, ensuring `goods_for` and `assign_goods` treat cross-hemisphere deposits and crops as tradeable once a route exists, with no special-cased goods restricted to one continent.
- **Touches:** game/rust/src/trade.rs, game/rust/src/resources.rs, game/rust/src/agriculture.rs
- **Gate:** `diagnose economy` shows every catalogue good traded on both hemispheres once blue-water connectivity exists, with zero goods hard-locked to a single continent.

### M360 — Convergence
- **Intent:** Trade should visibly narrow the gap between rich and poor worlds once they're connected, the way real long routes did.
- **Build:** Track per-good price variance across connected market regions and verify it narrows after blue-water connection relative to the pre-contact baseline, tuning route-cost decay so convergence happens over a plausible number of ticks rather than instantly.
- **Touches:** game/rust/src/economy.rs, game/rust/src/trade.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose economy` shows cross-hemisphere price-gap variance for connected goods falling into a banded convergence-half-life range after first blue-water contact.

### M361 — The Colonies
- **Intent:** Distant resources are worthless until someone plants a settlement to pull them out.
- **Build:** Add overseas outpost founding — mining and plantation colony variants of `found_settlements` sited by deposit and crop suitability at distance from the founding people's home continent, extending the existing `colony_site` machinery in `settlements.rs`, with reduced initial capacity per `capacity_at` reflecting the isolation.
- **Touches:** game/rust/src/settlements.rs, game/rust/src/resources.rs, game/rust/src/trade.rs
- **Gate:** `diagnose civ` shows overseas colonies founded only on foreign continents with a deposit or crop match, and their capacity curve starts measurably below equivalent home-continent settlements.

### M362 — Metropole Ties
- **Intent:** A colony's bond to home should visibly strain the farther and longer it sits across the ocean.
- **Build:** Model extraction and tribute flow from colony to metropole as a distance-and-time-weighted trade term in `economy.rs`, with control strength decaying by route length and elapsed time since founding, feeding directly into the colonial-secession gates already defined by M11.
- **Touches:** game/rust/src/economy.rs, game/rust/src/trade.rs, game/rust/src/politics.rs
- **Gate:** `diagnose economy` shows metropole control strength decaying monotonically with route distance and colony age across the standard colony-founding test set.

### M363 — Colonial Secession
- **Intent:** A colony that drifts far enough from its metropole in blood and identity should be able to walk away.
- **Build:** Feed distance-decayed control strength and a new creole-identity divergence measure — tracked alongside existing culture-drift state — into the M11 secession gate conditions, so colonies secede only when both control weakness and identity divergence cross their bands together.
- **Touches:** game/rust/src/politics.rs, game/rust/src/culture.rs, game/rust/src/economy.rs
- **Gate:** `diagnose civ` shows secession events occurring only where control strength and creole-divergence both cross their respective thresholds, zero secessions triggered by either alone.

### M364 — Core and Periphery
- **Intent:** A world economy should sort itself into cores and peripheries from the flows alone, never by decree.
- **Build:** Derive a core-periphery classification per settlement or market region from measured trade-flow centrality and terms-of-trade asymmetry in `economy.rs`, computed purely from existing route and price data with no hardcoded region tags.
- **Touches:** game/rust/src/economy.rs, game/rust/src/trade.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose economy` classifies every connected settlement as core or periphery from measured centrality alone, and the classification changes when trade-flow topology changes across reruns.

### M365 — Hegemonic Cycles
- **Intent:** Sea power should rise and pass at world scale the way M13's political arcs already do at continental scale.
- **Build:** Extend the M13 hegemonic-arc model to sea-power dominance measured by controlled trade-route share and naval settlement count, letting a hegemon's rise strain its rivals and its eventual decline reopen the core-periphery sort from M364.
- **Touches:** game/rust/src/politics.rs, game/rust/src/economy.rs, game/rust/src/trade.rs
- **Gate:** `diagnose civ` shows at least one full hegemonic rise-and-decline cycle in trade-route-share dominance over a standard millennium test run, reproducible under determinism replay.

### M366 — World-System Diagnostics
- **Intent:** Core-periphery structure and hegemonic cycles need measured bands, not eyeballed shape.
- **Build:** Add a `diagnose worldsystem` command reporting core-periphery ratio stability, hegemon dominance-share time series, and cycle-length distribution, banding each metric against the qualitative Wallerstein-style pattern the era targets.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose worldsystem` output stays within its banded ranges for core-periphery ratio and hegemon cycle length across a 20-seed batch run.

### M367 — Naval Power Projection
- **Intent:** Wars at sea need their own logic once fleets can carry armies across oceans.
- **Build:** Add naval blockade and colonial-theater mechanics to `politics.rs`'s war model — blockades throttling a settlement's trade-route throughput, colonial theaters resolving separately from the metropole conflict but feeding its outcome — gated by the navigation-tech ladder from M353.
- **Touches:** game/rust/src/politics.rs, game/rust/src/trade.rs, game/rust/src/event.rs
- **Gate:** `diagnose civ` shows blockaded settlements' trade throughput dropping to a banded fraction of baseline for the blockade's duration, restoring fully once lifted.

### M368 — World Wars
- **Intent:** The late eras deserve wars that span continents, not just neighboring provinces.
- **Build:** Extend coalition-war formation in `politics.rs` to permit cross-continent alliance blocs once contact and navigation thresholds are met, letting a single conflict's theater list span multiple landmasses with naval projection carrying troops between them.
- **Touches:** game/rust/src/politics.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose civ` shows at least one coalition war per late-era test run spanning two or more continents, with theater membership consistent under determinism replay.

### M369 — War-Reach Calibration
- **Intent:** How far war can reach should track how far ships can sail, not outrun it.
- **Build:** Calibrate maximum war-theater distance and coalition cross-continent eligibility directly against the navigation-tech rung reached, adding a diagnostics check that flags any conflict whose reach exceeds what the era's navigation tech should permit.
- **Touches:** game/rust/src/politics.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose civ` finds zero conflicts whose theater distance exceeds the navigation-tech-derived reach ceiling across the full test battery.

### M370 — Cartography In-World
- **Intent:** Peoples should draw their own maps, wrong at first and improving with study, distinct from the truth the engine holds.
- **Build:** Generate an in-world map artifact per people from their knowledge-grade grid, distorting unsurveyed regions by rumor uncertainty and projecting the whole through a simplified coordinate scheme that improves as surveying and navigation tech advance, stored as artifact records rather than derived on demand.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/knowledge.rs, game/rust/src/culture.rs
- **Gate:** `diagnose civ` shows a people's in-world map's positional error against true coordinates shrinking monotonically as navigation tech advances, never reaching zero before instrument-grade tech.

### M371 — The Atlas of Atlases
- **Intent:** Seeing a people's wrong map beside the true one is where the era's knowledge gap becomes visible and legible.
- **Build:** Add a UI comparison view in the outliner/inspector that overlays a selected people's in-world map artifact against the true terrain layer, with a toggle and a numeric error readout drawn from the M370 distortion measure.
- **Touches:** game/web/js/ui/inspector.js, game/web/js/ui/outliner.js, game/web/js/render/overlays.js
- **Gate:** the comparison view renders both layers with the error readout matching the backend's M370 distortion value within display rounding, verified against a fixed test snapshot.

### M372 — Map Provenance
- **Intent:** A map's authority should trace to who drew it and how, not appear ex nihilo.
- **Build:** Attach provenance metadata to each map artifact — the surveying expedition or explorer's journal that sourced each revealed region, drawn from the M352 expedition and M354 chronicle records — so the atlas view can cite its sources.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/entity.rs, game/rust/src/telling.rs, game/web/js/ui/inspector.js
- **Gate:** every revealed region on a people's map artifact carries a non-empty provenance record traceable to a specific expedition or journal entity, checked in `diagnose civ`.

### M373 — Globe-Scale Views
- **Intent:** A world this wide deserves to be seen whole, curved and true, not stretched flat forever.
- **Build:** Add a globe-projection render mode to the wgpu compositor with a graticule overlay, great-circle route rendering for blue-water trade and expedition paths, and a visible curve-of-the-world horizon effect at low zoom, per ADR-0006's fullscreen-shader architecture.
- **Touches:** game/web/js/render/compositor.js, game/web/js/gpu.js, game/web/js/view.js, docs/adr/0006-wgpu-fullscreen-shader-renderer.md
- **Gate:** the globe view renders the graticule and at least one great-circle route with curvature error under one pixel at reference resolution, verified against a fixed test snapshot.

### M374 — Streaming Render
- **Intent:** The viewport must not pay the memory or upload cost of continents nobody is looking at.
- **Build:** Add a tiled residency layer over `Orbital`'s GPU textures keyed by viewport cell-bounds (`cam`), demand-loading full-detail height/climate/misc/tint tiles from the pack cache and evicting tiles outside a margin around the visible rect via LRU; off-screen continents stay resident only at a coarse mip generated at pack time.
- **Touches:** game/rust/src/render.rs, game/rust/src/pack.rs, game/web/js, new: game/rust/src/tiling.rs
- **Gate:** GPU-resident texture memory stays under the per-tile budget regardless of world size, tile fetch/evict counts are logged and stable for a fixed camera path across three runs.

### M375 — Render Budgets at World Scale
- **Intent:** The streaming renderer earns its keep only if frame time survives contact with a full-size world.
- **Build:** Extend the diagnostics harness with a render-budget probe that drives the streaming layer through pan, zoom, and jump-cut camera scripts at 1024-plus grid size, recording frame time, tile-load latency, and eviction churn against banded thresholds.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/render.rs, game/rust/src/tiling.rs
- **Gate:** p95 frame time and tile-load latency stay within the report.sh render-budget band across the pan/zoom/jump-cut scripts at 1024-plus size, three runs in a row.

### M376 — The Thousand-Year Run
- **Intent:** A world this wide has to survive a millennium of its own history without drift or stall.
- **Build:** Run the full tick pipeline across the wide-world stage for 1000 simulated years, tracking population, settlement count, faction count, and event-rate envelopes over time; add guard rails in `systems.rs` that flag runaway growth or total collapse as diagnostic failures rather than silent states.
- **Touches:** game/rust/src/systems.rs, game/rust/src/world.rs, game/rust/src/bin/diagnose.rs
- **Gate:** a 1000-year run at world scale completes with population, settlement, and event-rate curves staying inside the guard-rail bands with zero heat-death or runaway flags.

### M377 — Era Pacing at Scale
- **Intent:** Dawn-to-late historical arcs must land at the same beats on every continent, not just the home one.
- **Build:** Calibrate per-continent era-transition timers (agricultural, urban, industrial thresholds already in `systems.rs`) against isolation and contact state so an unmet continent still paces its own arc correctly, then verify against the M13/M14 era-arc gates per continent.
- **Touches:** game/rust/src/systems.rs, game/rust/src/society.rs, game/rust/src/politics.rs, game/rust/src/bin/diagnose.rs
- **Gate:** era-transition timings for every continent fall within the established per-era pacing bands across a five-seed sweep, isolated or contacted.

### M378 — Long-Run Diagnostics
- **Intent:** The harness needs a standing witness that a millennium at world scale never quietly breaks.
- **Build:** Add a long-run diagnostic mode to `diagnose.rs` that samples population, price, event, and cadence metrics at fixed intervals across the full 1000-year world-scale run and renders them as banded time series in the report output.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** the long-run report shows every sampled metric inside its band for the full 1000 years with no heat-death or runaway flag across three seeds.

### M379 — Every Era Holds Everywhere
- **Intent:** The accumulated depth of eras I through VI must not thin out on continents that were never the "first" one.
- **Build:** Audit and, where needed, extend the per-continent initialization paths in `world.rs`, `culture.rs`, `society.rs`, and `agriculture.rs` so ecology, lives, tongues, and faiths all generate and tick correctly regardless of which continent or hemisphere a settlement began on.
- **Touches:** game/rust/src/world.rs, game/rust/src/culture.rs, game/rust/src/society.rs, game/rust/src/agriculture.rs, game/rust/src/bin/diagnose.rs
- **Gate:** a full-stack diagnostic sweep confirms every subsystem (ecology, demography, language, faith) produces non-degenerate output on every generated continent, five-seed sweep green.

### M380 — Hemispheres Diverge
- **Intent:** Two unmet continents should tell visibly different stories until the day they meet.
- **Build:** Add cross-continent distinctiveness metrics (lexical divergence, faith-tree divergence, tech-tree divergence, biome-driven economy divergence) to the oatmeal-lane diagnostics, comparing pre-contact hemispheres against each other and against post-contact convergence trend lines.
- **Touches:** game/rust/src/culture.rs, game/rust/src/naming.rs, game/rust/src/economy.rs, game/rust/src/bin/diagnose.rs
- **Gate:** pre-contact hemisphere-pair divergence scores exceed the oatmeal-VII floor while post-contact pairs trend downward, five-seed sweep green.

### M381 — Full-Stack Sweep at World Scale
- **Intent:** Year 3's close demands proof that every system from every era still coheres at full continental scale.
- **Build:** Run the complete property and oatmeal suite against the world-scale stage end to end, closing any gaps M379 and M380 surfaced and folding the results into the standing `report.sh` full-suite invocation.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** `report.sh` full mode runs clean at world scale with zero WARN or FAIL entries in SUMMARY.txt across a five-seed sweep.

### M382 — World-Scale UI
- **Intent:** The outliner and search surfaces must stay legible when the entity count multiplies by an order of magnitude.
- **Build:** Rework the Solid.js outliner, search index, and legend components to paginate, virtualize, and lazily hydrate entries so ten times the settlements, factions, and expeditions render without blocking the main thread.
- **Touches:** game/web/js/ui/outliner.js, game/web/js/ui/search.js, game/web/js/ui/list.js, game/rust/src/pack.rs, game/rust/src/snapshot.rs
- **Gate:** outliner and search render and respond to input in under the interaction budget with ten times M14-era entity counts loaded, measured via the browser probe in report.sh.

### M383 — Interaction Performance
- **Intent:** Panning, picking, and searching a continent-spanning world can't feel different from panning a single valley.
- **Build:** Profile and optimize the pick-ray and search-index code paths against the streaming tile layer from M374, adding spatial indices (grid-bucketed or quadtree) so lookups stay near-constant time regardless of world size.
- **Touches:** game/web/js/picking.js, game/web/js/ui/search.js, game/rust/src/render.rs, game/rust/src/tiling.rs, game/rust/src/bin/diagnose.rs
- **Gate:** pan, pick, and search latencies stay within the E9/E10 interaction-budget bands at 1024-plus grid size across the browser-probe sweep.

### M384 — Payload Discipline at Scale
- **Intent:** The delta-tick wire format must not balloon just because the world got wider.
- **Build:** Audit `pack.rs` and `snapshot.rs` delta lanes for per-tick payload growth at world scale, adding tile-scoped delta batching so only changed, resident tiles serialize, matching the pack v2 quantized/checksummed contract.
- **Touches:** game/rust/src/pack.rs, game/rust/src/snapshot.rs, game/rust/src/tiling.rs
- **Gate:** per-tick delta payload size stays within the pack-v2 payload band at world scale across a five-seed, thousand-tick sweep.

### M385 — Contact Metamorphics
- **Intent:** The world-system model must answer counterfactuals, not just replay the one history it generated.
- **Build:** Add harness-driven metamorphic tests that vary navigation-tech onset timing and sea-closure geometry (closed inland seas versus open blue water) and assert the expected directional shifts in contact date, price convergence rate, and hemisphere divergence.
- **Touches:** game/rust/src/trade.rs, game/rust/src/geo.rs, game/rust/src/bin/diagnose.rs
- **Gate:** earlier navigation onset yields earlier contact and faster convergence, and closed-sea geometry yields sustained divergence, both by statistically significant margins across a five-seed sweep.

### M386 — World-Economy Properties
- **Intent:** A market this wide is worthless if it lets value appear or vanish between connected regions.
- **Build:** Extend the property suite with conservation checks across the full trade network — total goods produced versus consumed versus stored, and a no-arbitrage-loop invariant over the market-area graph from `trade.rs`'s route and connection-component structures.
- **Touches:** game/rust/src/trade.rs, game/rust/src/economy.rs, game/rust/src/bin/diagnose.rs
- **Gate:** the property suite finds zero arbitrage loops and closes the goods conservation ledger within tolerance across a five-seed, world-scale sweep.

### M387 — Exploration Calibrated
- **Intent:** The age of coast-crawling into blue water should feel like the real one, not an arbitrary clock.
- **Build:** Calibrate expedition speed, navigation-ladder unlock timing, and first-contact dates against the historical age-of-sail envelope (Polynesian/Viking coastal reach through Iberian blue-water crossing), tuning constants in `trade.rs` and the expedition system rather than hardcoding dates.
- **Touches:** game/rust/src/trade.rs, game/rust/src/geo.rs, game/rust/src/constants.rs, game/rust/src/bin/diagnose.rs
- **Gate:** expedition reach and contact timing land inside the age-of-sail calibration band across a five-seed sweep, reported by the diagnostics harness.

### M388 — Colonial Arcs Calibrated
- **Intent:** Overseas empires should rise, strain, and break on a cadence that matches recorded colonial history.
- **Build:** Calibrate the M361–M363 colonial-outpost, metropole-tie, and secession timers against historical distance-decay envelopes for control and identity divergence, tuning the constants that gate the M11 secession thresholds for distant colonies.
- **Touches:** game/rust/src/politics.rs, game/rust/src/society.rs, game/rust/src/constants.rs, game/rust/src/bin/diagnose.rs
- **Gate:** colonial rise-to-secession cadences fall within the historical calibration band as a function of metropole distance across a five-seed sweep.

### M389 — World-History Sifter
- **Intent:** A reader should be able to follow the meeting of worlds and the passing of hegemons as one continuous telling.
- **Build:** Extend `telling.rs`'s chronicle sifter with world-scale narrative threads — contact arcs, hegemonic rise-and-fall, and the coalition world war — selecting and ordering events by the same cast-discipline rules already governing M13/M14 chronicles.
- **Touches:** game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** the sifter produces a coherent contact-to-hegemony-to-war thread with cast size and event density inside the chronicle bands across a five-seed sweep.

### M390 — Chronicle at Scale
- **Intent:** Telling a world's history can't mean flooding the reader with ten times the noise.
- **Build:** Verify and tune the chronicle's cast-discipline and cadence-selection thresholds against a tenfold rise in raw event volume from the wide-world stage, ensuring selection stays proportional rather than linear in output length.
- **Touches:** game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/rust/src/bin/diagnose.rs
- **Gate:** chronicle output length and cast size stay within the established bands even as raw event volume rises tenfold, five-seed sweep green.

### M391 — Full-Run Snapshots
- **Intent:** A world worth simulating for a thousand years is worth keeping, not just watching once.
- **Build:** Add a full-run archive format built on `snapshot.rs`'s bootstrap and pack machinery, capturing periodic whole-world snapshots plus the tick log needed to reconstruct any intermediate state, hash-stamped per snapshot.
- **Touches:** game/rust/src/snapshot.rs, game/rust/src/pack.rs, new: game/rust/src/archive.rs
- **Gate:** an archived world reloads and replays to the same determinism hash as the live run at every stored checkpoint, verified across three seeds.

### M392 — Replay Determinism
- **Intent:** An archive that can't be trusted bit-for-bit on another machine isn't an archive.
- **Build:** Run the archive replay path from M391 across the platform matrix already exercised by ADR-0003's determinism gate, comparing per-checkpoint hashes byte-for-byte across operating system and CPU architecture combinations.
- **Touches:** game/rust/src/archive.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** every archived checkpoint reruns to an identical determinism hash across all platforms in the CI matrix, zero mismatches across three seeds.

### M393 — Property Suite at World Scale
- **Intent:** Year 5's close needs every invariant the simulator has ever earned proven again at full size.
- **Build:** Run the complete accumulated property suite — conservation, no-arbitrage, cadence, and cast-discipline invariants — against the world-scale stage in one pass, closing any residual gaps from M381 and M386.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** the full property sweep passes with zero violations at world scale across a five-seed run.

### M394 — The `diagnose world` Runner
- **Intent:** An era this wide earns its own standing diagnostic command rather than borrowing another era's.
- **Build:** Add a `diagnose world` subcommand bundling the streaming-render, long-run, world-economy, contact-metamorphic, and archive-replay probes into one invocation, wired into `report.sh` as a permanent lane alongside the existing full and quick modes.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh full` invokes `diagnose world` and SUMMARY.txt reports zero WARN or FAIL from the world lane across three seeds.

### M395 — Era VII Gate
- **Intent:** The wide world only counts once it has survived its own sweep at full length and size.
- **Build:** Run the complete Era VII acceptance sweep — thousand-year duration, world-scale grid, full property and oatmeal suites, archive replay — as the closing gate before Forge VII begins.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** a thousand-year, world-scale run passes the full report.sh suite with zero WARN or FAIL across a five-seed sweep, sealing Era VII.

### M396 — Memory Architecture for Scale
- **Intent:** The era strained allocation patterns that only a deliberate re-cut, not another patch, can fix.
- **Build:** Recast `world.rs` and `render.rs`'s allocation strategy into arena-backed storage with explicit tiling boundaries matching the M374 streaming layer, replacing ad hoc per-tick allocation with cache-honest reuse; land the shape change behind a hindsight ADR.
- **Touches:** game/rust/src/world.rs, game/rust/src/render.rs, game/rust/src/tiling.rs, new: docs/adr (memory-arenas-for-world-scale ADR, numbered at land time)
- **Gate:** determinism hash is unchanged before and after the refactor across three seeds, and peak resident memory drops or holds against the pre-refactor baseline.

### M397 — Pack v4: Tiled Residency
- **Intent:** The pack format itself must speak tiles, not just the renderer that consumes them.
- **Build:** Design pack v4 as a tiled, demand-loaded extension of the pack v2 quantized/checksummed contract (ADR-0016), versioning the loader to refuse mismatched clients and folding tile metadata into the field registry per the declare discipline.
- **Touches:** game/rust/src/pack.rs, game/rust/src/tiling.rs, game/web/js/wasm-load.js, new: docs/adr (pack-v4-tiled-residency ADR, numbered at land time)
- **Gate:** pack v4 round-trips every registry field through a tiled load with byte-identical decoded state versus the pre-v4 pack, determinism hash unchanged across three seeds.

### M398 — Pipeline Re-Cut for the Globe
- **Intent:** The worker and render pipeline built for one continent needs a deliberate re-cut to carry ten.
- **Build:** Rework the upload paths and damage-tracking logic in `render.rs` and the web worker glue so only changed, resident tiles cross the wasm-to-GPU boundary each frame, replacing whole-buffer uploads with tile-scoped dirty regions.
- **Touches:** game/rust/src/render.rs, game/rust/src/tiling.rs, game/web/js/worker.js, game/web/js/gpu.js
- **Gate:** frame upload bandwidth scales with damaged-tile count rather than world size, and determinism hash is unchanged across three seeds through the refactor.

### M399 — Budgets Rehold at 1024-Plus
- **Intent:** Every budget the instrument enforces has to be re-proven honest at the size the era actually reached.
- **Build:** Recompute generation, tick, memory, and payload budget bands in the diagnostics harness against 1024-plus grid sizes with the arena, pack-v4, and pipeline re-cuts from M396–M398 in place, replacing any band still calibrated to pre-forge scale.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** generation, tick, memory, and payload metrics all land inside their newly-set 1024-plus bands across a five-seed sweep.

### M400 — Suite Refit: Parallel Sweeps
- **Intent:** The growing suite has to stay fast even as the world it tests grows wider.
- **Build:** Parallelize the world-scale property and oatmeal sweeps in `report.sh` across available cores, consolidating redundant lanes accumulated during Era VII per the forge charter's refit-the-instrument mandate.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** full-suite wall-clock time at world scale lands within the forge's wall-clock band, determinism hash unchanged and all suites green across three seeds.
