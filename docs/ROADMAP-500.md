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
Phases here are one-line sketches — each phase gets its full item
breakdown and gates only when its era comes into range, era by era,
interview by interview.

Status: skeleton + Era I, Forge I, Era III, Forge III drafted.
Remaining eras are themes awaiting their own drafting rounds.

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
| V | The Tongues | M236–M290 | Language families, sound change, dialect continua, script and literacy |
| ⚒ | Forge V | M286–M290 | Recast naming/lexicon machinery, string interning, name-bank formats |
| VI | The Unseen Order | M291–M340 | Religion with mechanical stakes: cults, schisms, holy sites, myth drift |
| ⚒ | Forge VI | M341–M345 | Recast belief/culture state, myth-layer storage |
| VII | The Wide World | M346–M395 | Scale: exploration, distant continents, world-systems trade, hegemonic cycles |
| ⚒ | Forge VII | M396–M400 | Recast for world size: memory, streaming, render scale |
| VIII | The Proof | M401–M450 | Calibration against historical datasets; property proofs everywhere; ERA-grade evaluation |
| ⚒ | Forge VIII | M451–M455 | Recast the harness itself: suite speed, report formats, CI-grade lanes |
| IX | The Sealed Instrument | M456–M515 | Observability: causal query over history, explain-everything, archival export, the final gate |

Note: Era V spans M236–M285 with Forge V at M286–M290. Eight
era+forge blocks of 55 plus Era IX at 60: 500 exactly.

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
- M32 Outwash plains and braided meltwater rivers
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

## Era II — The Long Sky (M71–M120) — to be drafted

Climate stops being a constant: century-scale drift, oscillation modes,
storm and drought as events with dates, little-ice-age arcs that move
peoples — the weather of history, feeding famine, migration, and war.
Forge II (M121–M125) recasts the climate stack, time-series state, and
event lanes behind it.

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
- M129 Marriage and household formation
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
- M142 Battlefield fates: death, capture, ransom
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
- M155 Purges and exiles

**Q11 — The plot.** The usurper is a person with backers.
- M156 Conspiracies: recruitment, secrecy, resolve
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
- M166 Eventfulness scored per person
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

## Era IV — The Living Land (M181–M230) — to be drafted

The deferred frontier, first half: wildlife with hunting pressure,
vegetation succession under axe and fire, fisheries and forests as
collapsible stocks, and trade-graph plagues that give the road network
teeth. Forge IV (M231–M235) recasts stocks/flows state and tick budgets.

## Era V — The Tongues (M236–M285) — to be drafted

The deferred frontier, second half, part one: language families with
sound change, dialect continua along the roads, script and literacy as
tech with consequences, toponymy as true archaeology. Forge V
(M286–M290) recasts the naming/lexicon machinery and name-bank formats.

## Era VI — The Unseen Order (M291–M340) — to be drafted

Part two: religion with mechanical stakes — cults, schisms, holy
sites, pilgrimage, monastic knowledge-keeping, myth drift against the
ground-truth log. Forge VI (M341–M345) recasts belief/culture state and
myth-layer storage.

## Era VII — The Wide World (M346–M395) — to be drafted

Scale as a subject: exploration and the fog of the far, distant
continents and late contact, world-systems trade, hegemonic cycles at
the civilization tier (M13 machinery at full reach). Forge VII
(M396–M400) recasts for world size: memory, streaming, render scale.

## Era VIII — The Proof (M401–M450) — to be drafted

The instrument earns the word definitive: calibration against
historical datasets (demography, price series, war frequency,
rank-size), property proofs across every subsystem, ERA-grade
evaluation as a standing lane, the oatmeal detector at full strength.
Forge VIII (M451–M455) recasts the harness itself: suite speed, report
formats, CI-grade lanes.

## Era IX — The Sealed Instrument (M456–M515) — to be drafted

The closing era: causal query over the whole of history, an
explain-everything inspector, archival-grade export of worlds and runs,
cross-run statistics, and the final gate that proves the Five Hundred
closed. Sixty phases — the era and its final hardening are one.
