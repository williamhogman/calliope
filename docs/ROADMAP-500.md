# Calliope — The Five Hundred

The long arc: 500 milestone-sized phases, M16 through M515, rolled up
into rough quarterly goals. This is the successor horizon to
`ROADMAP.md` (M1–M15, the prologue, being closed under its own gate)
and `ROADMAP-ENGINE.md` (E1–E11). Prologue numbers are kept; the Five
Hundred begins where they end.

End state: **the definitive instrument** — the deepest deterministic
world-simulator, every subsystem research-grounded, machine-provable,
observer-only. No play layer, no product pivot.

Rules carry over unchanged: determinism is law (ADR-0003), the harness
is the gate (ADR-0009), architecture lands with an ADR, and rejected
approaches stay rejected unless a superseding ADR argues the world gets
genuinely deeper (per-item reopening is allowed on that standard).

Grain: a **phase** is milestone-sized (an M2 or an M9, not an M2.3).
A **quarter** holds 2–3 phases under one goal. An **era** is 50 world
phases (~18 quarters) followed by a **forge interlude** — 5 engine
phases (2 quarters) of systems restructuring, refactoring, data-format
and performance work that re-cuts what the era just strained. Engine
work is first-class in the Five Hundred: the E-track discipline
(registry, codegen, pack lanes, budgets) threads through every forge.
Each phase gets its full item breakdown and gates when its era comes
into range; the specs here are the binding sketch.

Status: fully specced — every phase M16–M515 carries its line.
`scripts/roadmap-500-check.sh` is the spec gate: it exits non-zero
unless all 500 phases are present, unique, in order, and properly
worded. Era-by-era steering interviews continue on top of the
complete draft.

Progress: M16–M26 landed (plate history, orogeny ages, rock
provinces, deposits re-seated, regional stone, geologic legibility,
fault seams + dated earthquakes, live volcanism, disaster wiring,
sea-level history, drowned and raised coasts) — each closed on its
`diagnose` gate across the seed sweep. M22's replay gate is the
first cross-runtime check: the seismic ledger is byte-identical
native vs WASM (ADR-0025), enforced by the `earth-wasm` report lane.
M23 reads cone ages straight off summit height (the generator writes
age into height by construction) and lays permanent ash fertility
under a distance-decay gate. M24 wires both hazards into one shared
kill path (`fell_settlement`): every destroying-magnitude strike
yields exactly one chronicle beat and one ruin, damage marks of a
twelfth or worse open forty-year rebuild arcs (target capped just
under carrying capacity; sqrt near-field falloff), and the `civ`
lane gates 1:1 fall-to-ruin correspondence plus median arc closure
inside the window. M25 gives every world a sea-level history: a
seeded freeze point on the glacial sawtooth sets the eustatic stand
while a post-glacial isostatic field (rebound above the old ice
edge, a sinking forebulge collar below it) tilts the coasts — all
applied before erosion, all IEEE-exact, hashed into `hash_state`,
gated on the coastline holding the datum. The cone-age law it
disturbed was recalibrated to height rank (the affine map had
saturated: 2/3 of all cones read age 0.00 and the M23 cadence gate
was measuring noise — now 2.19× young/old). M26 reads the coasts
back out of that history: a pure classifier (`landform.rs`) tags
raised beaches where the land outran the sea and rias/skerries where
the sea won, naming mints firths, skerry fields and strands off the
grid, and the terrain lane bands landform frequency per unit of
waterline offset (raised 91–121, drowned ~92–99 across seeds and
both stand signs — the amplitude law) plus a within-world gate that
the rebound belt out-raises the forebulge collar. The classifier
joins `hash_state` as the F-lane. Landed alongside: six settlement
events (storms, golden harvests, caravans) now carry their ground
coords, closing the one orphan-id path the telling lane could hit
when a town is renamed the same tick. M27 closes Year 1: every
deep-earth layer already rode `hash_state` (P/Q/V/L/F lines plus the
rock grid in the field hash), so the phase's real work was widening
the cross-runtime gate — a labeled deep-earth identity line
(plates·rock·seismic·volcanism·sealevel·landform) computed by both
`diagnose earth-hash` and a new `earth_hash()` wasm export, compared
by the report lane. Measurement beat assumption: the terrain-
downstream layers (rock, volcanism, landform) were presumed hostage
to transcendental drift, but three seeds × 240 months came back
byte-identical native vs wasm, so the gate covers all six layers
instead of the seismic ledger alone. Generation budget held green
(512² median 1371 ms) with the suite at 440 pass · 0 fail. M28
gives the world its ice age: `ice.rs` cuts the LGM footprint from an
equilibrium-line-altitude law — a parabola from 0.62 height units at
the equator to sea level at 62°, the same edge the M25 rebound belt
remembers — with SplitMix64 margin wobble and a Vialov √distance
thickness profile (BFS from the margin, 4 km cells, 4000 m cap).
Frozen prehistory per ADR-0024: computed at the dawn, I-lane in
`hash_state`, `ice=` in the deep-earth identity, never ticked. The
earth lane grew four bands (share of land 39–41 % — twin polar
landmasses run it above Earth's ~25 —, lowland margin ~60°, ELA
poleward march 100 % monotone, dome ~2.3 km) plus a purity check;
cross-runtime replay stays byte-identical with ice included. Suite:
445 pass · 0 fail.

## The forge charter

Every forge interlude answers five standing questions, two quarters long:

1. **Recast** — which modules did the era strain? Re-cut them with
   hindsight; shape changes land with ADRs.
2. **Declare** — all new state into the field registry, pack lanes,
   delta ticks, generated constants. Nothing hand-mirrored.
3. **Rehold** — generation, tick, memory, and payload budgets back in
   bands with the era's systems included.
4. **Refit the instrument** — the growing suite stays fast; property
   and oatmeal lanes consolidated.
5. **Clear the ledger** — refactors queued during the era land now or
   are rejected in writing.

Forge gates: determinism hash unchanged through pure refactors, budgets
green, full suite green. A forge never adds world behavior.

## The nine eras

Deep systems first: the stage is settled before it is peopled.

| Era | Name | Phases | Theme |
|---|---|---|---|
| I | The Deep Earth | M16–M65 | Physical stack II: tectonic prehistory, ice, ocean circulation, soils, GPU erosion |
| ⚒ | Forge I | M66–M70 | Recast geo/hydrology, compute lanes, new grids into pack v3 |
| II | The Long Sky | M71–M120 | Climate variability: century drift, oscillations, disasters, the weather of history |
| ⚒ | Forge II | M121–M125 | Recast climate stack, time-series state, event lanes |
| III | The Named Lives | M126–M175 | Notable persons: kinship, courts, deeds, plots, biography |
| ⚒ | Forge III | M176–M180 | Recast entity/chronicle systems around persons; registry codegen extended |
| IV | The Living Land | M181–M230 | Ecology: wildlife, succession, fisheries, trade-graph plagues |
| ⚒ | Forge IV | M231–M235 | Recast stocks/flows, ecology grids, tick budgets |
| V | The Tongues | M236–M285 | Language families, sound change, dialect continua, script and literacy |
| ⚒ | Forge V | M286–M290 | Recast naming/lexicon machinery, name-bank formats |
| VI | The Unseen Order | M291–M340 | Religion with mechanical stakes: cults, schisms, holy sites, myth drift |
| ⚒ | Forge VI | M341–M345 | Recast belief/culture state, myth-layer storage |
| VII | The Wide World | M346–M395 | Scale: exploration, distant continents, world-systems trade, hegemonic cycles |
| ⚒ | Forge VII | M396–M400 | Recast for world size: memory, streaming, render scale |
| VIII | The Proof | M401–M450 | Calibration against historical datasets; property proofs everywhere; ERA-grade evaluation |
| ⚒ | Forge VIII | M451–M455 | Recast the harness itself: suite speed, report formats, CI-grade lanes |
| IX | The Sealed Instrument | M456–M515 | Observability: causal query over history, explain-everything, archival export, the final gate |

Eight era+forge blocks of 55, then Era IX at 60: 500 exactly.

---

## Era I — The Deep Earth (M16–M65)

The world beneath earns its history. Tectonic prehistory enters as a
*generative sketch* — plate polygons, drift ages, collision seams as
inputs to generation — never as a plate simulation (the ADR-level
rejection is superseded only that far). Ice, ocean, soil, and
GPU-resolution erosion follow, each passing the "genuinely deeper"
bar or staying dead. Eighteen quarters, then Forge I.

### Year 1 — The Buried History

**Q1 — The plates remembered.** Prehistory as sketch, not simulation.
- M16 Superseding ADR + plate-history layer: polygons, drift ages, collision seams as generation inputs
- M17 Orogeny ages: ranges carry birth dates — young ranges sharp, old ranges worn by deep time
- M18 Rock provinces: basement geology grid — shields, basins, fold belts, volcanic terranes

**Q2 — The mineral truth.** Ore stops being noise.
- M19 Deposits re-seated on rock provinces (gold in shields, coal in basins, tin in granites); ADR-0013 floors preserved
- M20 Regional stone: granite, marble, limestone quarried by province (feeds the M14.5 goods)
- M21 Geologic legibility: harness checks that province maps read true

**Q3 — The restless ground.** The deep earth acts in sim time.
- M22 Fault seams + earthquakes as dated events with magnitudes and epicenters
- M23 Live volcanism: arc and hotspot eruptions — ash plumes, fertile slopes, buried towns
- M24 Disaster wiring: quakes and eruptions feed chronicle, ruins (M9), and rebuild arcs

**Q4 — The slow breath.** Coasts remember ice and time.
- M25 Sea-level history: eustatic curve + post-glacial rebound reshaping coastlines
- M26 Drowned and raised coasts: rias, skerries, raised beaches as landform vocabulary
- M27 Deep-earth determinism: new layers join the hash; native gen budget holds

### Year 2 — The Ice

**Q5 — The ages of ice.**
- M28 Ice-sheet extent model from latitude, elevation, and the glacial cycle
- M29 Ice-carved relief: U-valleys, fjords, cirques, hanging valleys where the sheets sat
- M30 Depositional legacy: moraines, drumlins, eskers; till-plain fertility

**Q6 — The melt.**
- M31 Proglacial lakes and spillways: great-lake chains, giant abandoned channels
- M32 Outwash plains and braided meltwater rivers below the old ice line
- M33 Permafrost and patterned ground on the cold rim

**Q7 — The living ice.**
- M34 Mountain glaciers at the modern snowline, climate-responsive
- M35 Glacier-fed discharge: meltwater seasonality extends M1.7's regime types
- M36 Ice diagnostics: fjord/lake/moraine cadence bands by latitude belt

**Q8 — The frozen sea.**
- M37 Sea ice: seasonal pack extent; frozen straits close sea lanes in winter
- M38 Tundra honesty: biome refinement under permafrost, treeline discipline
- M39 Glacial calibration vs Earth: fjord latitudes, lake densities in bands

### Year 3 — The Circling Sea

**Q9 — The great gyres.**
- M40 Wind-driven gyres: basin-scale surface currents from the existing wind and pressure fields
- M41 Heat transport: warm and cold currents bend coastal climate — Gulf-Stream coasts, cold-current deserts
- M42 Current-aware climate re-derivation: deserts and rain belts recomputed, bands re-tuned

**Q10 — The worked shore.**
- M43 Tides: range from basin geometry; tidal flats and estuaries
- M44 Longshore drift: spits, barrier islands, lagoons; harbors gained and silted
- M45 Harbor-shelter scoring: settlements re-read the new coasts (GAP §6 term lands)

**Q11 — The sailor's sea.**
- M46 Currents and prevailing winds price sea lanes: with-current passages fast, doldrums real
- M47 Upwelling zones: cold nutrient coasts marked — the fisheries Era IV will harvest
- M48 Sea-route seasonality: monsoon sailing windows; winter closures join pack ice

**Q12 — The ocean proven.**
- M49 Ocean diagnostics: gyre topology, current-coast temperature deltas in bands
- M50 Metamorphic checks: remove a warm current ⇒ its coast cools; route times respond to currents

### Year 4 — The Ground That Feeds

**Q13 — The true soil.**
- M51 Soil genesis: parent rock × climate × vegetation × slope → soil classes; the scalar fertility grid retires
- M52 Alluvium and loess: floodplain and wind-blown fertility where rivers and ice left it
- M53 Agriculture re-based on soils: crop packages (M2.1) read soil class; bands re-tuned

**Q14 — The hidden water.**
- M54 Aquifers and water tables from rock and rainfall
- M55 Springs, wells, oases: dry-land settlement stops cheating; well techs gate deep water
- M56 Karst: limestone country — sinkholes, caves, disappearing rivers

**Q15 — The sharpened knife.**
- M57 GPU erosion compute pass (reopened from Later/research): stream-power at full resolution inside budget
- M58 River forms II: meanders, oxbows, terraces, braids from valley slope and sediment load
- M59 Sediment budget: deltas grow where the load lands; estuaries silt

**Q16 — The explained cell.**
- M60 Landform vocabulary: every cell classifies (fjord, terrace, moraine, karst…) for inspector and naming
- M61 "Why is this here": inspector provenance chain — rock → ice → water → soil in one card

### Year 5 — The Honest Earth

**Q17 — The named earth.**
- M62 Geomorphic toponymy: per-culture landform generics name what is truly there (-dale, -fjord, -fell)
- M63 The atlas learns: hillshade and hypsometry read the new relief; geology and soils as map layers

**Q18 — The seal.**
- M64 Calibration vs Earth: hypsometry, drainage density, floodplain share, coast-type frequencies; oatmeal II over landforms
- M65 Era I gate: `diagnose earth` joins `report.sh`; 300-year sweep green; superseded ADRs recorded

### Forge I — after the earth (M66–M70)

**FQ1 — Recast the ground.**
- M66 Geo/erosion/hydrology modules re-cut around the landform pipeline; hindsight ADRs
- M67 Compute lane hardened: the GPU erosion pass becomes a general engine facility
- M68 New grids into the registry: quantized pack fields, delta lanes, generated constants

**FQ2 — Rehold the budgets.**
- M69 Generation, memory, and payload budgets back in bands with the full deep-earth stack
- M70 Property and round-trip lanes consolidated: no-NaN, byte-identity, descent invariants in one suite

---

## Era II — The Long Sky (M71–M120)

Climate stops being a constant. The sky gains a history — anomalous
years, oscillation modes, storms and droughts with dates, cold ages
and warm optima that move margins and peoples — all bounded, banded,
and deterministic. Eighteen quarters, then Forge II.

### Year 1 — The Turning Year

**Q1 — The year stops repeating.**
- M71 Interannual variability: deterministic anomalies over temperature and rain, latitude-shaped
- M72 Anomaly propagation: harvests, discharge, and pasture read the year that was, not the mean
- M73 Variability determinism: anomaly variance by latitude held in bands, hash-covered

**Q2 — The seesaw seas.**
- M74 Ocean-atmosphere oscillation modes: an ENSO-class seesaw with period and amplitude bands
- M75 Teleconnections: the mode's phase tilts rain belts a hemisphere away
- M76 Oscillation diagnostics: spectra and phase statistics inside envelopes

**Q3 — The storm.**
- M77 Storm tracks: mid-latitude cyclone corridors riding the westerlies
- M78 Tropical cyclones: warm-sea genesis, curving landfall tracks, dated disasters
- M79 Storm consequence: fleets scattered, harbors wrecked, coasts that remember — chronicle and ruins wired

**Q4 — The failed year.**
- M80 Drought as an event with a shape: multi-year, mapped extent, a name in the chronicle
- M81 Flood years: river spates that drown the levees and gift the silt in the same stroke
- M82 Dry/wet calibration: return times against paleoclimate envelopes

### Year 2 — The Ages

**Q5 — The slow drift.**
- M83 Century-scale temperature drift: each run carries its own bounded secular curve
- M84 Wandering belts: rain and storm tracks shift with the drift
- M85 Drift discipline: bounded excursions, no runaway, means stationary over the millennium

**Q6 — The cold ages.**
- M86 Cold-age arcs: multidecadal winters with dated onsets and slow releases
- M87 Warm optima: the generous centuries, when the uplands open
- M88 The named ages: chronicles christen them — the Long Winter, the Wine Years

**Q7 — The moving margins.**
- M89 Margins respond: treeline, snowline, and pack ice track the age
- M90 Marginal farms: upland and northern fields open in optima and fail in the cold
- M91 Glaciers in sim time: Era I's ice advances and retreats with the ages

**Q8 — The failing rains.**
- M92 Monsoon fortune: strength rides drift and mode; failed-monsoon years strike the paddies
- M93 Lakes that breathe: endorheic shores rise and shrink, leaving dated strandlines
- M94 The dry edge: steppe encroachment and oasis failure as events with extents

### Year 3 — The Human Weather

**Q9 — Hunger from the sky.**
- M95 Famine re-grounded: M2.6 reads real anomalies, not dice — hunger has a meteorological cause
- M96 Granaries: towns bank fat years against lean, gated by storage techs
- M97 Famine cadence recalibrated against the historical record

**Q10 — The moving peoples.**
- M98 Climate migration: failed decades push peoples off the margins in dated pulses
- M99 Steppe pressure: pastoral ranges shift with the grass; nomad and farmer collide
- M100 Migration diagnostics: pulses fire in cold ages, within bands, on most seeds

**Q11 — War and weather.**
- M101 Campaign seasons: mud and winter pause the wars; bad years starve the sieges
- M102 The hungry sword: lean years harden opinion and feed the unrest ladder
- M103 Weather turns battles: storms scatter fleets, winters break encampments — and the chronicle says so

**Q12 — The remembered sky.**
- M104 Weather enters the record: dated events with extents, sifter-visible
- M105 The calendar's omens: eclipses and comets fall deterministically, and are read
- M106 Living memory: "the worst winter in memory" is checked against actual memory windows

### Year 4 — The Instrumented Sky

**Q13 — The climate ledger.**
- M107 The weather archive: per-region climate series stored compact for the run's whole life
- M108 Sky layers in the atlas: anomaly maps, age timelines, event overlays
- M109 The explained year: the inspector says why — mode east, age cold, monsoon failed

**Q14 — The proven sky.**
- M110 Metamorphic weather: a colder age shortens the growing season, never lengthens it
- M111 Return-time honesty: drought, storm, and flood frequencies inside Earth envelopes
- M112 Spectral honesty: variance by timescale — year, decade, century — each in its band

**Q15 — The kept sky.**
- M113 Tick budgets hold with the living sky running every month
- M114 Weather state joins the registry: packed, quantized, delta-clean

**Q16 — The atlas of ages.**
- M115 Climate-history plates: age timelines and anomaly atlases in the reports
- M116 ERA plots gain weather axes: expressive range over climate histories

### Year 5 — The Long-Run Truth

**Q17 — The bounded sky.**
- M117 Five-century runs: drift, ages, and events stay bounded; the means do not wander
- M118 Every sky its own: cross-seed climate-history distinctiveness (oatmeal III)

**Q18 — The seal.**
- M119 `diagnose sky` joins `report.sh` as the era's standing runner
- M120 Era II gate: 300-year sweep green; famine, migration, and war couplings in band

### Forge II — after the sky (M121–M125)

**FQ1 — Recast the sky.**
- M121 Climate stack re-cut: generation-time climate and sim-time weather split cleanly, ADR'd
- M122 Time-series state: ring buffers and run archives as registry-declared formats
- M123 Weather and disaster events fold into the event-table discipline

**FQ2 — Rehold the budgets.**
- M124 Generation, tick, memory, and payload budgets back in band with the live sky
- M125 Suite refit: weather lanes fast; the sweep's wall-clock held

---

## Era III — The Named Lives (M126–M175)

The chronicle stops being about abstractions. Notables only — rulers,
generals, merchants, sages, founders — hundreds of tracked persons per
world with kin, deeds, deaths; demography stays aggregate. Builds
directly on the prologue's M10–M13 (peoples/realms/dynasties) and M6.2
(persistent named entities). Eighteen quarters, then Forge III.

### Year 1 — The Person Record

**Q1 — Flesh and bone.** A person exists, ages, and dies, deterministically.
- M126 Person entity + registry: ids, birth/death, ties to people and realm
- M127 Lifecycle clock: aging, deterministic mortality curves by era and station
- M128 Person determinism: registry folded into the state hash, sweep gates

**Q2 — Blood and name.** Persons connect into houses.
- M129 Marriage and household formation: unions recorded, households founded, houses allied
- M130 Children, descent, name inheritance in the people's tongue
- M131 Kinship-graph properties in the harness: acyclic descent, spouse symmetry, no time paradoxes

**Q3 — The court.** Power has a room and chairs in it.
- M132 Offices: marshal, chancellor, steward — appointed, held, lost
- M133 Courts sited at seats (M10.4); office-holders live where the crown is
- M134 Favor and standing: a per-court ledger the plots of Year 3 will read

**Q4 — Deeds.** What a person did is recorded, and echoes.
- M135 Deed ledger per person, registry-linked to events
- M136 Earned epithets from deed patterns (extends M6.8)
- M137 Epitaphs and tombs as artifacts with provenance

### Year 2 — The Persons of Power

**Q5 — The crowned.** Rulers become real persons.
- M138 Succession resolved from actual kin trees, not synthetic house state
- M139 Regencies, minorities, and the danger of a child on the throne
- M140 Dynasty trees derived from descent; M10.3 house state retired into it

**Q6 — The sworded.** Wars are commanded by someone.
- M141 Generals hold command: appointment, victory, disgrace
- M142 Battlefield fates: death in the line, capture, ransom home
- M143 War chronicle cites commanders on both sides

**Q7 — The monied.** Wealth has owners.
- M144 Merchants as persons riding the M5.5 agent lane
- M145 Prospectors and founders retro-wired from M6.2 entities
- M146 Personal fortunes beside realm treasuries

**Q8 — The learned.** Knowledge has authors.
- M147 Sages and engineers tied to tech unlocks
- M148 Works: named inventions and buildings carry their maker
- M149 Masters and apprentices: knowledge lineages across generations

### Year 3 — The Play of Wills

**Q9 — Character.** Persons differ, boundedly and deterministically.
- M150 Trait vector fixed at birth: bold/cautious, cruel/just, open/grasping
- M151 Traits bias decisions the sim already makes: war, building, exploration
- M152 Trait heredity with drift, held in bands

**Q10 — Faction.** Courts split.
- M153 Factions form from favor, kinship, and grievance
- M154 Faction pressure feeds the M11 unrest ladder
- M155 Purges and exiles: the losing faction pays in banishment, and remembers

**Q11 — The plot.** The usurper is a person with backers.
- M156 Conspiracies: backers recruited, secrecy strained, resolve tested to the knife's edge
- M157 Assassination attempts with consequences either way
- M158 Plot discovery, trials, and the chronicle beats they earn

**Q12 — The match.** Marriage is statecraft.
- M159 Inter-realm matches move the opinion matrix
- M160 Dowries and claims travel with brides and grooms
- M161 Claim wars: succession claims through blood, wired to M11.3

### Year 4 — The Peopled Telling

**Q13 — Voices.** Every event names its people.
- M162 Chronicle events cite persons, always
- M163 Prose knows kinship: "his uncle's slayer", "her father's city"
- M164 Mention-aware person callbacks (extends M6.8)

**Q14 — The lives sifted.** Biography becomes a story form.
- M165 Biography arcs as sifter patterns: rise, fall, revenge, exile-and-return
- M166 Eventfulness scored per person; the notable dead ranked by the weight of their days
- M167 Cast discipline: bounded notables per century, dedup, no name soup

**Q15 — The remembered.** The dead stay browsable.
- M168 Legends browser: genealogies and house trees
- M169 Person cards in the inspector: deeds, kin, works, tomb
- M170 Mythologization of dead notables (M6.9 legend layer over lives)

**Q16 — The graven.** Lives mark the map.
- M171 Tombs, monuments, and person-derived name strata (M9 machinery)
- M172 Relics: person-owned artifacts with provenance chains
- M173 Sites of memory: where the famous fell, marked and named

### Year 5 — The Honest Census

**Q17 — The bands.** The cast is demographically honest.
- M174 Notable-population bands vs town sizes and eras; lifespan and mortality calibrated against medieval demography
- M175 Era III gate: `diagnose lives` runner, 300-year sweep, biographies in band, full suite green

### Forge III — after the lives (M176–M180)

**FQ1 — Recast the registry.**
- M176 Entity/chronicle systems re-cut around persons; `entity.rs` re-shaped with hindsight ADRs
- M177 Person tables into registry codegen: kinds, deeds, offices declared once (E2 discipline holds)
- M178 Person pack and UI lanes: delta ticks, browser at full cast

**FQ2 — Rehold the proofs.**
- M179 Kinship and reference integrity at ERA grade: no orphans, no paradoxes; metamorphic lane (war years ⇒ commander deaths not fewer)
- M180 Registry performance bands: memory and tick cost at 500 years

---

## Era IV — The Living Land (M181–M230)

The deferred frontier, first half: succession under axe and fire,
wildlife under the hunt, fisheries and forests as collapsible stocks,
and trade-graph plagues that give the road network teeth. The land
becomes a party to history. Eighteen quarters, then Forge IV.

### Year 1 — The Green Succession

**Q1 — The growing land.**
- M181 Vegetation succession grid: grass to shrub to young wood to old forest, on deterministic clocks
- M182 Disturbance: fire, windthrow, and the axe reset the clock
- M183 Succession bands and determinism: mosaics stable, hash-covered

**Q2 — The axe.**
- M184 Clearing: settlements take land for field and fuel by population and tech
- M185 The green return: abandoned land re-wilds; ruins grow over (M9 joined)
- M186 The map remembers harvest: deforestation legible in the biome layer (M14.8 deepened)

**Q3 — The wild stocks.**
- M187 Wildlife populations: herbivore and predator stocks per region, boundedly dynamic
- M188 The hunt: game taken for food and furs draws the stocks down
- M189 Collapse and refuge: hunted-out regions, last herds in the deep wood

**Q4 — The wild map.**
- M190 Wilderness layers: game richness, old forest, and the untouched, mapped
- M191 Beasts in the telling: wolf winters, the last aurochs, the named hunt
- M192 Ecology determinism and the generation budget hold together

### Year 2 — The Harvested Wealth

**Q5 — The harvested sea.**
- M193 Fish stocks on the shelves and upwellings Era I marked
- M194 The fleets: coastal towns work the banks; the catch feeds carrying capacity
- M195 Collapse and shift: overfished banks empty; stocks move with the climate (herring years)

**Q6 — The forest ledger.**
- M196 Timber as stock: shipyards and hearths draw it down; regrowth is slow and mapped
- M197 Charcoal and the smelters: metal eats forest, measurably
- M198 Naval stores: mast trees and pitch as strategic goods (M14 catalogue extended)

**Q7 — The worked land.**
- M199 Soil exhaustion: monoculture presses yields down; fallow and rotation techs restore
- M200 Pasture degradation: overgrazing turns marginal steppe to waste
- M201 Land-care calibration: exhaustion and recovery cadences against historical envelopes

**Q8 — The commons proven.**
- M202 Press-harvest metamorphics across every stock: harder press, lower stock, never higher
- M203 Conservation ledger extended to the living: nothing eaten that did not grow (M15.6 widened)
- M204 Renewable diagnostics: collapse fires under press, stays rare in band otherwise

### Year 3 — The Pest and the Plague

**Q9 — The blighted year.**
- M205 Crop blights: dated regional harvest-killers that test the granaries
- M206 Murrains: herd collapses that starve the pastoral belts
- M207 Locust years on the dry margins, moving with the wind

**Q10 — The pestilence.**
- M208 Trade-graph SIR: plague rides the routes from the ports inward, latency and all
- M209 Plague dynamics: town size, connectivity, and sanitation shape the dying
- M210 Quarantine and flight: closed routes, fleeing courts, emptied cities

**Q11 — The great dyings.**
- M211 Pandemic arcs: named plagues with aftermaths — wages up, land cheap, the survivors' boom
- M212 The endemic burden: malaria belts and city graveyards press carrying capacity
- M213 Mortality calibration against Black-Death-class envelopes

**Q12 — The plague remembered.**
- M214 Mass graves and emptied quarters mark the map; plague years date the chronicle
- M215 Sifter patterns: the plague arc, the emptied town, the wild's return
- M216 Disease metamorphics: more connectivity, faster spread — proven, not assumed

### Year 4 — The Balance Instrumented

**Q13 — The land explained.**
- M217 Ecology in the atlas: succession, stocks, and disease history as layers
- M218 The inspector reads the land: "cleared oakwood, worked out, re-wilding since the plague"
- M219 Ecology at cadence: chronicle presence in band — no spam, no silence

**Q14 — The balance audited.**
- M220 Trophic sanity: plant, prey, and predator ratios in band across biomes
- M221 Coupling audit: carrying capacity, famine, and migration all read the living land now

**Q15 — The kept land.**
- M222 Ecology state joins the registry: packed, quantized, delta-clean
- M223 Tick budgets hold with the full ecology running

**Q16 — The long wildness.**
- M224 Five-century mosaics: farmland and wilderness balance holds — no all-farm, no all-wild
- M225 Every land its own: cross-seed ecological distinctiveness (oatmeal IV)

### Year 5 — The Reckoning

**Q17 — The stress lanes.**
- M226 Press-sweeps standing: harvest and plague stress runs join the harness
- M227 Return-time calibration: blight, murrain, and pandemic frequencies in envelopes

**Q18 — The seal.**
- M228 Property suite: stocks non-negative, conservation exact, SIR bounded
- M229 `diagnose land` joins `report.sh` as the era's standing runner
- M230 Era IV gate: 300-year sweep green; plague, famine, and wilderness in band

### Forge IV — after the land (M231–M235)

**FQ1 — Recast the stocks.**
- M231 One stock-state facility behind minerals, forests, game, and fish — re-cut with an ADR
- M232 Ecology grids into registry codegen and pack quantization
- M233 Event-table families for ecology and disease, declared once

**FQ2 — Rehold the budgets.**
- M234 Generation, tick, memory, and payload budgets back in band with the living land
- M235 Harness speed: press-sweeps parallelized; suite wall-clock in band

---

## Era V — The Tongues (M236–M285)

The deferred frontier, second half, part one: languages become real
objects with histories — phonologies, families, sound laws, loans,
scripts — and every name in the world becomes a datable artifact of
them. Toponymy turns into true archaeology. Eighteen quarters, then
Forge V.

### Year 1 — The Proto-Tongues

**Q1 — The sound of a people.**
- M236 Phoneme inventories and syllable grammars per language; raw name banks retire
- M237 Proto-languages assigned to founding peoples; the language-layer ADR lands
- M238 Words from roots: meaning-bearing morphemes generate the lexicon (M3.3 glosses grounded)

**Q2 — The family tree.**
- M239 Language families: descent trees mirror the lineage of peoples (M12 kinship read)
- M240 Regular sound change: deterministic shift laws applied down the tree
- M241 Cognates: the same root wears different faces in sister tongues

**Q3 — The old names.**
- M242 Toponyms re-derived through language history: old names obey old sound laws (M9.3 becomes linguistics)
- M243 Hydronym conservatism deepened: rivers keep the words of vanished tongues
- M244 Sound-change properties: shifts regular, collisions bounded, glosses preserved

**Q4 — The graded speech.**
- M245 Dialect continua: distance and barriers grade speech within a people
- M246 The road levels, the mountain splits: dialect geography follows the routes
- M247 Dialect diagnostics: continua correlate with geography in bands

### Year 2 — The Words That Travel

**Q5 — The borrowed word.**
- M248 Loanwords: goods carry their names down the trade routes
- M249 Substrate strata: conquered tongues leave residue in grammar and place-names
- M250 Loan diagnostics: borrowing tracks trade intensity, provably

**Q6 — The named person.**
- M251 Personal names from the lexicon: naming customs per people — patronymics, epithets, house styles
- M252 Dynastic onomastics: houses favor name stocks and number their kings
- M253 Names age with the tongue: sound change works on the Era III cast across generations

**Q7 — The drifting meaning.**
- M254 Semantic drift: words shift sense on era clocks; glosses date themselves
- M255 Etymology walkable: the inspector traces any name to its proto-root
- M256 Lexicon bounds: vocabularies bounded and deduplicated — no name soup at depth

**Q8 — The speech communities.**
- M257 Language is not people: creoles at the ports, lingua francas on the routes
- M258 Bilingual belts on the borders; exonym and endonym from actual tongues (M9.2 grounded)
- M259 Community diagnostics: language map vs people map divergence in bands

### Year 3 — The Written Word

**Q9 — The letters.**
- M260 Scripts: invented and borrowed on the tech tree; script families drift like tongues
- M261 Literacy: rates by tech and town — the lettered and the unlettered
- M262 Writing changes the telling: chronicle sources shift from oral to written (feeds M6.9)

**Q10 — The archive.**
- M263 The written realm remembers: chronicle detail tracks literacy
- M264 Lost texts and burned libraries: record gaps as dated events (the withheld, grounded)
- M265 Inscriptions: steles, coins, and tomb texts carry dated language samples

**Q11 — The learned tongue.**
- M266 Liturgical and chancery languages outlive their speakers
- M267 Translation and the scribes: cross-tongue treaties, mistranslation incidents
- M268 Written-word diagnostics: record density tracks literacy in bands

**Q12 — The dying word.**
- M269 Language death: tongues fade under assimilation (M12.4), leaving strata behind
- M270 Standard tongues: late-tech standardization gathers the dialects
- M271 Language-count trajectory: births and deaths both, across the sweep, in bands

### Year 4 — The Tongues Instrumented

**Q13 — The linguistic atlas.**
- M272 Language, dialect, and script layers with isogloss rendering
- M273 The inspector speaks: any name parsed, pronounced, glossed, and dated
- M274 Prose in the tongues: sayings, quotes, and name-meanings woven into the chronicle

**Q14 — The proven word.**
- M275 Philology properties: tree consistency, shift regularity, total gloss coverage
- M276 Metamorphic philology: contact breeds loans; isolation breeds divergence

**Q15 — The kept word.**
- M277 Lexicon state into registry and pack lanes; interning re-measured on real lexicons (E3.8 revisited)
- M278 Name-generation budgets in band at full depth

**Q16 — The distinct word.**
- M279 Every tongue its own: cross-seed linguistic distinctiveness (oatmeal V)
- M280 Five-century philology: trees stay coherent; the invariants do not drift

### Year 5 — The Seal of Speech

**Q17 — The calibrated tongue.**
- M281 Typological calibration: inventories and borrowing rates inside natural-language envelopes
- M282 The label audit at depth: sampled toponyms classify to their true tongue at 95 percent

**Q18 — The seal.**
- M283 Property suite consolidation across the language lanes
- M284 `diagnose tongues` joins `report.sh` as the era's standing runner
- M285 Era V gate: 300-year sweep green; every name's history walkable end to end

### Forge V — after the tongues (M286–M290)

**FQ1 — Recast the lexicon.**
- M286 One lexicon engine behind toponyms, person names, and glosses — re-cut with hindsight ADRs
- M287 Name-bank formats: compact root tables and tries, registry-declared, codegen to JS
- M288 Names as ids across the boundary: string interning re-decided on new evidence (E3.8 superseded if it now wins)

**FQ2 — Rehold the budgets.**
- M289 Generation and tick budgets in band with full philology running
- M290 Suite refit: philology lanes fast; label audits automated

---

## Era VI — The Unseen Order (M291–M340)

Part two of the inner life: belief becomes mechanical. Faiths with
tenets and temples, conversion along the roads, schisms under strain,
holy wars, and a myth corpus that drifts measurably against the
ground-truth log. Eighteen quarters, then Forge VI.

### Year 1 — The Gods Given Form

**Q1 — The faiths.**
- M291 Belief systems as entities: pantheons become faiths with tenets (M3.5 seeds grown); the religion ADR lands
- M292 Sacred geography: holy mountains, springs, and groves chosen from real landforms
- M293 Belief determinism: faith state hash-covered, cadence in bands

**Q2 — The practice.**
- M294 Cult practice: festivals, sacrifices, taboos with mechanical hooks — calendar, diet, war
- M295 Temples and shrines: built wealth, staffed clergy, treasury flows
- M296 Clergy as notables: high priests join the Era III cast

**Q3 — The spreading word.**
- M297 Conversion along roads and courts: belief flows down the Axelrod gradients
- M298 Syncretism: gods merge and borrow where faiths meet
- M299 Spread diagnostics: faith maps track routes and conquests in bands

**Q4 — The crown and the altar.**
- M300 Divine legitimacy: coronation rites feed the M4/M11 legitimacy machinery
- M301 Priest against crown: investiture-class conflicts; temple wealth versus treasury
- M302 Religion-politics coupling held in bands across the sweep

### Year 2 — The Orders and the Schisms

**Q5 — The orders.**
- M303 Monastic orders: houses that keep records, copy texts, and clear land (Era V literacy joined)
- M304 Pilgrimage: routes to holy sites carry trade and plague alike (Era IV joined)
- M305 Order and pilgrimage cadence bands across the sweep

**Q6 — The schism.**
- M306 Schism mechanics: doctrine splits under distance, politics, and plague theodicy
- M307 Heresies: suppressed or triumphant, never ignored
- M308 Schism diagnostics: splits fire in band and resolve both ways

**Q7 — The holy war.**
- M309 Faith edges in the opinion matrix; crusade-class coalition wars
- M310 Conversion by sword or by road: conquest faith policy with consequences
- M311 Religious-war frequency calibrated against historical envelopes

**Q8 — The sacred calendar.**
- M312 Feast cycles and intercalation structure the year
- M313 Omens institutionalized: priesthoods read eclipses, comets, and plagues into politics
- M314 Prophecy with tracked outcomes: the Berúthiel discipline applied to divination

### Year 3 — The Myth That Drifts

**Q9 — The myth corpus.**
- M315 Myths generated from world events: origin, flood, and founder tales per faith
- M316 Myth drift: retellings mutate on generational clocks, mechanically
- M317 Drift bounds: core motifs conserved, details drift within bands

**Q10 — The two tellings.**
- M318 Ground truth versus legend at full depth: M6.9's two layers completed
- M319 Saints and heroes: dead notables canonized, their deeds inflated measurably
- M320 Relic cults: Era III relics gather pilgrimages — and forgeries

**Q11 — The sacred map.**
- M321 Sacred toponymy: god-names and saint-names layer onto the map (M9 strata)
- M322 Temple archaeology: ruined faiths leave sanctuaries the next faith reuses
- M323 Myth-map coherence: every sacred site's story checks against its ground

**Q12 — The doubted god.**
- M324 Unbelief: late-tech skeptic currents and temple decline arcs
- M325 Faith-count trajectory: births, schisms, and deaths in both directions
- M326 No runaway monofaith on more than eighty percent of seeds

### Year 4 — The Order Instrumented

**Q13 — The faith explained.**
- M327 Faith layers in the atlas: religion map, pilgrimage routes, holy-site markers
- M328 The inspector reads belief: faith, temple, feast days, patron on every town card
- M329 The chronicle weaves the unseen: omen, festival, and schism prose at cadence

**Q14 — The faith browsed.**
- M330 Legends browser: faith trees, saint cults, and myth variants compared side by side
- M331 Sifter patterns: the schism arc, the holy war, the prophecy kept or broken

**Q15 — The kept faith.**
- M332 Belief state joins the registry: packed, delta-clean, codegen'd
- M333 Tick budgets hold with the living faiths

**Q16 — The proven faith.**
- M334 Metamorphic belief: contact breeds syncretism; isolation breeds divergence of rite
- M335 Myth-drift properties: core motifs conserved, variants bounded

### Year 5 — The Seal of the Unseen

**Q17 — The calibrated faith.**
- M336 Calibration: conversion, schism, and temple-economy rates inside historical envelopes
- M337 Every faith its own: cross-seed distinctiveness of belief (oatmeal VI)

**Q18 — The seal.**
- M338 Property suite across the belief lanes
- M339 `diagnose faith` joins `report.sh` as the era's standing runner
- M340 Era VI gate: 300-year sweep green; the two-layer telling proven

### Forge VI — after the unseen (M341–M345)

**FQ1 — Recast the inner life.**
- M341 Peoples, tongues, and faiths re-cut as one lattice of reference-linked registries; hindsight ADRs
- M342 Myth-layer storage: variant texts as deltas against ground truth, compact
- M343 Belief tables into registry codegen; event families extended

**FQ2 — Rehold the budgets.**
- M344 Generation, tick, memory, and payload budgets in band with the full inner life
- M345 Suite refit: belief lanes fast; audits automated

---

## Era VII — The Wide World (M346–M395)

Scale becomes a subject. The stage grows to continents unknown to each
other, exploration earns the meeting of worlds, trade becomes a world
system, and the whole stack holds at 1000 years and 1024-plus grids.
Eighteen quarters, then Forge VII.

### Year 1 — The Larger Stage

**Q1 — The wide stage.**
- M346 World scale-up: larger grids and multi-continent templates; budgets scale honestly
- M347 The far side: distant continents generated whole, unknown to all peoples at dawn
- M348 Scale determinism: hashes and budgets proven at the new size

**Q2 — The known world.**
- M349 Knowledge per people: known-world maps with terra incognita as real state
- M350 The fog of the far: rumor, distance decay, and mythical geographies at the edge of knowledge
- M351 Map-knowledge diagnostics: known-world growth cadence in bands

**Q3 — The explorers.**
- M352 Expeditions: named captains coast-crawl, then cross blue water (Era III cast at sea)
- M353 The navigation ladder: stars, compass, and ships gate the reach
- M354 Landfall in the telling: discovery chronicles and the naming of new coasts (Era V tongues)

**Q4 — The meeting of worlds.**
- M355 First contact between hemispheres: trade, disease, and war arrive together
- M356 Contact asymmetries: tech and immunity gradients play out measurably
- M357 Contact-outcome bands: no scripted conquest; the envelopes hold both ways

### Year 2 — The World Economy

**Q5 — The long routes.**
- M358 Blue-water trade joins the network; entrepôts rise at the crossings
- M359 World goods: the M14 catalogue crosses hemispheres
- M360 Convergence: connected worlds' price gaps narrow within bands

**Q6 — The colonies.**
- M361 Overseas outposts: mining and plantation colonies at distance
- M362 Metropole ties: extraction, tribute, and control strained by distance
- M363 Colonial secession: distance and creole identity feed the M11 gates

**Q7 — The world system.**
- M364 Core and periphery emerge from the flows — measured, never decreed
- M365 Hegemonic cycles: sea-power hegemons rise and pass (M13 arcs at world scale)
- M366 World-system diagnostics: core-periphery and hegemony metrics in bands

**Q8 — The wide war.**
- M367 Naval power projection: blockades and colonial theaters
- M368 World wars: coalition wars spanning continents in the late eras
- M369 War-reach calibration: conflict distance tracks navigation tech

### Year 3 — The Whole Earth Seen

**Q9 — The maps within the map.**
- M370 Cartography in-world: peoples draw their own maps — wrong, then better
- M371 The atlas of atlases: a people's map compared to the truth, in the UI
- M372 Map provenance: surveys and explorers' journals as artifact sources

**Q10 — The globe rendered.**
- M373 Globe-scale views: graticule, great circles, the curve of the world
- M374 Streaming render: only the viewed region resident at full detail
- M375 Render budgets banded at world scale, frame-time proven

**Q11 — The long history.**
- M376 Thousand-year runs across the full stage stay bounded and eventful
- M377 Era pacing at scale: dawn-to-late arcs calibrated per continent
- M378 Long-run diagnostics: no heat-death, no runaway, cadence bands hold at a millennium

**Q12 — The peopled far.**
- M379 Every era's systems hold on every continent: lives, tongues, faiths, ecology
- M380 Hemispheres diverge until contact: cross-continent distinctiveness (oatmeal VII)
- M381 Full-stack sweep at world scale, green

### Year 4 — The World Instrumented

**Q13 — The world surfaces.**
- M382 World-scale UI: outliner, search, and legends at ten times the entity counts
- M383 Interaction performance: pan, pick, and search stay instant at world scale
- M384 Payload discipline: delta lanes hold at world size

**Q14 — The world proven.**
- M385 Contact metamorphics: earlier navigation, earlier convergence; closed seas, divergence
- M386 World-economy properties: no arbitrage loops; conservation at world scale

**Q15 — The world calibrated.**
- M387 Exploration and contact calibrated against the age-of-sail envelope
- M388 Colonial arcs calibrated: rise, strain, and secession cadences

**Q16 — The world told.**
- M389 World-history sifter: contact arcs, hegemonic passings, the world war told whole
- M390 Chronicle at scale: cast discipline and cadence bands at ten times the events

### Year 5 — The World Kept

**Q17 — The world archive.**
- M391 Full-run snapshots: worlds archived, replayable, hash-stable
- M392 Replay determinism: the archive reruns bit-for-bit per platform

**Q18 — The seal.**
- M393 Property suite at world scale, green across the sweep
- M394 `diagnose world` joins `report.sh` as the era's standing runner
- M395 Era VII gate: thousand-year world-scale sweep green

### Forge VII — after the wide world (M396–M400)

**FQ1 — Recast for scale.**
- M396 Memory architecture for world scale: arenas, tiling, cache honesty; hindsight ADRs
- M397 Streaming and residency: pack v4 — tiled, demand-loaded world state
- M398 Worker and render pipeline re-cut for the globe: upload paths, damage tracking at scale

**FQ2 — Rehold the budgets.**
- M399 Generation, tick, memory, and payload bands at 1024-plus
- M400 Suite refit: world-scale sweeps parallel; wall-clock in band

---

## Era VIII — The Proof (M401–M450)

The instrument earns the word definitive. Every claim the simulator
makes is checked against curated historical data, every invariant
becomes a machine check, every system proves it matters. Eighteen
quarters, then Forge VIII.

### Year 1 — The Historical Envelopes

**Q1 — The corpus of record.**
- M401 The calibration corpus: curated historical datasets as versioned fixtures; the evidence-standards ADR
- M402 Envelope framework: every calibration a named check against a sourced envelope
- M403 Provenance discipline: each dataset cited, licensed, and frozen

**Q2 — The demographic proof.**
- M404 Life tables, age structure, and growth rates against medieval and early-modern data
- M405 Urban proof: rank-size, urbanization shares, city growth against Bairoch-class series
- M406 Famine and plague return-times against the historical record

**Q3 — The economic proof.**
- M407 Price ratios, wages, and trade gradients against the Hodges/Goucher lists and beyond
- M408 Zipf, Gibrat, and Bettencourt at full depth across all seeds and eras
- M409 Market integration: price convergence tracks connectivity as the literature says it must

**Q4 — The political proof.**
- M410 War frequency, duration, and casualty distributions against Correlates-of-War-class envelopes
- M411 Polity survival curves and imperial-cycle periods against the Turchin data
- M412 Succession, coup, and secession shares against the historical ledgers

### Year 2 — The Property Lattice

**Q5 — The lattice laid.**
- M413 Property lanes over every subsystem: the M15 proptest program extended to all eras
- M414 Invariant census: every documented invariant becomes a machine check — no prose-only guarantees
- M415 Coverage metric: the share of systems under proof, driven to one hundred

**Q6 — The directions of effect.**
- M416 Metamorphic lattice: direction-of-effect checks across every coupling in the causal table
- M417 Counterfactual harness: single-input perturbations with bounded expected divergence
- M418 Metamorphic coverage held in bands

**Q7 — The hostile lanes.**
- M419 Fuzz lanes: hostile unpack, hostile config, truncated archives — never a panic
- M420 Numeric honesty: NaN, inf, and overflow sweeps; the float-determinism question settled by ADR
- M421 Findings ledger: every fuzz find becomes a permanent regression check

**Q8 — The statistical rigor.**
- M422 Bands earn confidence intervals; multiple-comparison discipline lands
- M423 Sweep design: seed counts and run lengths sized for statistical power
- M424 The honest report: PASS, WARN, and FAIL formalized; flake rate zero

### Year 3 — The Distinct Worlds

**Q9 — The oatmeal front.**
- M425 The oatmeal detector at full strength: structural distinctiveness across every layer
- M426 Typicality and novelty as axes: worlds plot in a portfolio; extremes flagged, not failed
- M427 Distinctiveness floors: minimum and mean divergence across the sweep

**Q10 — The expressive range.**
- M428 The ERA program complete: expressive-range plates for every era's systems
- M429 Cross-era ERA: coupled histograms — climate against economy, faith against war
- M430 ERA regression: plates diffed between engine versions; drift flagged

**Q11 — The ablation proof.**
- M431 Ablation harness: every system toggleable, its fingerprint measured
- M432 No dead systems: every ablation moves measured outputs, provably
- M433 The interaction map: which systems couple, measured not asserted

**Q12 — The null defeated.**
- M434 Noise-world baselines: Calliope's structure beats matched noise on every metric
- M435 Anti-oatmeal at world scale: hemisphere, continent, and realm distinctiveness floors
- M436 Novelty cadence: the marvelous stays rare but present, in bands

### Year 4 — The Ledger of Proof

**Q13 — The proof ledger.**
- M437 Every check numbered, sourced, and versioned: the instrument's own registry
- M438 Coverage gates: no system ships without envelope, property, metamorphic, and ERA entries
- M439 The ledger browsable: a live check registry with status in the UI

**Q14 — The replicable claim.**
- M440 Replication kit: one command reproduces any reported number from seed and version
- M441 Format stability: archives from older engines still verify

**Q15 — The standing proof.**
- M442 Calibration re-runs as standing lanes: envelopes re-checked on every landing
- M443 Performance proof: the budgets themselves become banded checks with history

**Q16 — The falsification pass.**
- M444 Hunt the unfalsifiable: checks that cannot fail are strengthened or removed
- M445 Red-team sweeps: adversarial seeds and configs hunting silent failure

### Year 5 — The Proof Written

**Q17 — The documented proof.**
- M446 Every check's method documented to replication grade
- M447 The proof synthesis: what is proven, to what strength, on what evidence — in one document

**Q18 — The seal.**
- M448 Full-suite wall-clock in band: the proof stays runnable
- M449 `diagnose proof` — the check of checks — joins `report.sh`
- M450 Era VIII gate: full proof-lattice coverage; every envelope green across the sweep

### Forge VIII — after the proof (M451–M455)

**FQ1 — Recast the harness.**
- M451 Harness architecture re-cut: the check registry as code, runners generated; hindsight ADRs
- M452 Report formats: machine-readable results with a historical trend store
- M453 Incremental checking: only affected lanes re-run per change

**FQ2 — Rehold the discipline.**
- M454 CI-grade gating: every landing checked by its affected lanes; budgets enforced
- M455 The ledger cleared: all queued harness debt landed or rejected in writing

---

## Era IX — The Sealed Instrument (M456–M515)

The closing era, sixty phases: history becomes fully queryable, every
number explains itself, worlds archive to specification grade, and the
Five Hundred closes under a gate that prints its own evidence. Twenty
quarters; the era and its final hardening are one.

### Year 1 — The Causal Thread

**Q1 — The why-chain.**
- M456 The causal graph: every event records its causes as edges, as data
- M457 Cause propagation: system couplings annotate influence — drought to famine to rising
- M458 Causal determinism: the graph is seed-stable and hash-covered

**Q2 — The question asked.**
- M459 The why query: any event walked back through its chain to first causes
- M460 Counterfactuals against the archive: branch a run at a cause, diff the histories
- M461 Causal-graph properties: acyclic in time, connected, bounded fan-in

**Q3 — The history queried.**
- M462 A query language over history: filter, join, and aggregate events, entities, and series
- M463 Query surfaces: inspector, browser, and command line all speak it
- M464 Query performance: full-history questions answered in interactive time

**Q4 — The explained world.**
- M465 Explain-everything: every surfaced number traceable to its computation
- M466 Provenance cards: any value's derivation chain rendered on demand
- M467 Explanation coverage: one hundred percent of surfaced values, audited

### Year 2 — The Archive Eternal

**Q5 — The archival form.**
- M468 The archival world format: self-describing, versioned, documented to specification grade
- M469 The run archive: complete histories stored compact, indexed, queryable cold
- M470 Archive integrity: checksums, migration lanes, decade-stable readability

**Q6 — The exported world.**
- M471 Export surfaces: atlases, chronicles, gazetteers, and genealogies as archival documents
- M472 The printed world: publication-grade plate rendering — the atlas as artifact
- M473 Export fidelity: documents regenerate byte-identical from archive and version

**Q7 — The portfolio.**
- M474 Cross-run statistics: the worlds as a dataset — distributions over history itself
- M475 The comparative atlas: worlds compared structurally, side by side
- M476 Portfolio distinctiveness: the oatmeal program's final form

**Q8 — The observatory.**
- M477 The observatory UI: checks, budgets, and portfolios live on one surface
- M478 The run notebook: annotated observation sessions with bookmarkable moments
- M479 Observatory performance held in bands

### Year 3 — The Instrument Complete

**Q9 — The one fabric.**
- M480 Every layer explains: geology to weather to lives to faiths, one causal fabric
- M481 The grand inspector: any entity's whole story — causes, effects, kin, works — on one card
- M482 Fabric integrity: cross-layer reference audit at one hundred percent

**Q10 — Determinism sealed.**
- M483 The ADR-0003 program's final audit: every lane, every platform, documented
- M484 The float question closed: bit-identity scope decided and enforced by ADR
- M485 Determinism regression lanes: permanent, fast, unskippable

**Q11 — Performance sealed.**
- M486 All budgets at final bands with recorded history
- M487 Memory sealed: no growth across thousand-year runs, proven
- M488 Payload sealed: boundary formats at final specification, documented

**Q12 — The code sealed.**
- M489 The module map final: ADR index complete, no undocumented architecture
- M490 Debt zero: nothing queued, nothing orphaned, nothing dead
- M491 The build sealed: clean checkout to green suite in one command, timed and banded

### Year 4 — The Documentation of Record

**Q13 — The systems written.**
- M492 SYSTEMS.md at full depth: every system documented to its research sources
- M493 The corpus closed out: every digest's implications marked landed or rejected
- M494 GAP-ANALYSIS final revision: zero structural gaps; residuals ADR'd

**Q14 — The manuals.**
- M495 The operator's manual: running, sweeping, querying, and extending the instrument
- M496 The theory of the instrument: why it is built as it is, as a document
- M497 Doc-truth audit: every documented claim carries the id of its proving check

**Q15 — The chronicle of the making.**
- M498 The Five Hundred's own history: eras, decisions, and reversals recorded
- M499 The ADR corpus complete: every architecture question asked has its indexed answer
- M500 The five-hundredth number: an audit at the M500 mark — everything before it green, twice over

**Q16 — The stranger's eye.**
- M501 External-grade review: the instrument examined as a stranger would examine it
- M502 Findings landed: the review's issues fixed or rejected in writing
- M503 Strangeness audits made standing and repeatable

### Year 5 — The Seal

**Q17 — The closing worlds.**
- M504 The final sweep design: seeds, sizes, and lengths for the closing proof
- M505 The closing portfolio: the definitive world-set generated, archived, published to the observatory
- M506 Portfolio verification: every closing world passes every check

**Q18 — The soak.**
- M507 The long soak: continuous long-run stability at final specification
- M508 The cold start: archives re-verified from clean environments
- M509 Soak findings closed: every anomaly root-caused, checked, and banded

**Q19 — The whole gate.**
- M510 The whole-suite gate: every check of every era green in one run, wall-clock in band
- M511 The meta-gate: roadmap check, proof ledger, and doc-truth audit green together
- M512 The instrument's statement: the machine prints its own completion evidence

**Q20 — The seal.**
- M513 The last ledger: nothing queued, nothing deferred, nothing withheld
- M514 The seal ADR: the Five Hundred closed by decision of record
- M515 The Sealed Instrument: the final gate — the script that began this arc exits zero
