# Calliope Roadmap

Sequenced from `GAP-ANALYSIS.md`. Every milestone lands with new
diagnostics checks (ADR-0009) and, where architecture is set, an ADR.

Items are checkboxes; `scripts/roadmap-check.sh` is the stopping
criterion — it exits non-zero while any milestone item is unchecked.
"Later / research" and "Rejected" are out of scope for the check.

The platform track (engine optimization, boundary data formats, macro
discipline, UI/render surfaces) lives in `ROADMAP-ENGINE.md` with its own
gate, `scripts/roadmap-engine-check.sh`.

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

- [x] M2.1 Crop packages (wheat/rice/maize/pastoral) from T/P/growing-period; per-package K; <300 mm pastoral boundary (S)
- [x] M2.2 Carrying capacity from package + tech (Kaplan T^−0.5); keep logistic growth (S)
- [x] M2.3 Zipf rank-size validation + Gibrat growth correction (S)
- [x] M2.4 Bettencourt scaling: output ∝ pop^1.15, infrastructure/upkeep ∝ pop^0.85 (S-M)
- [x] M2.5 Spacing calibration vs 15-30 km market-town band (S)
- [x] M2.6 Famine coupling: local subsistence failure → demand spike, pop loss, migration, chronicle events (S-M)
- [x] M2.7 Price-ratio calibration vs medieval price lists (harness check) (S)

Gates: rank-size slope in [−1.3, −0.8]; crop-belt maps legible (wheat
belts, rice deltas, pastoral steppes); famine events fire in dry-shock
sweeps but < X/century in bands; price ratios inside historical envelope.

## M3 — Words and Ways (culture, naming, coherence)

- [x] M3.1 Culture-styled toponym generics + per-culture formation strategies (S-M)
- [x] M3.2 Power-law weighted draws in `make_word` (S)
- [x] M3.3 Etymology glosses on fragments; chronicle cites meanings (M)
- [x] M3.4 Exonym/endonym doubling on border features (S)
- [x] M3.5 Pantheon layer: per-culture named gods cited in omens/festivals/war names (M)

Gates: label audit — sampled toponyms classify to their culture ≥ 90 %;
gloss coverage 100 % of fragments; no name-collision regressions.

## M4 — The Great Game (politics with consequences)

- [x] M4.1 Influence-map territory (pop/tech/war-weighted kernels; live borders) (M)
- [x] M4.2 War score → peace terms: settlement transfer / tribute / vassalage (M)
- [x] M4.3 Opinion matrix + aggressive-expansion decay + coalition threshold (S-M)
- [x] M4.4 Siege state machine; fortification as treasury sink (S)
- [x] M4.5 Legitimacy/asabiyyah: surge at frontiers, ~3-4-generation decay, rebellion/fragmentation rolls (M)

Gates: ≥ 1 border change per major war in sweeps; polity count rises *and
falls* over 300 y; coalition wars occur; no runaway single-empire outcome
in > 80 % of seeds.

## M5 — Iron and Coin (the local economy)

- [x] M5.1 Production recipes: ore→metal→tools/weapons, workforce-gated (M)
- [x] M5.2 Market areas from route connectivity; per-area prices (L)
- [x] M5.3 Trade income from price gaps (arbitrage) instead of geography-only (with M5.2)
- [x] M5.4 Gravity-model flow as harness cross-check (S)
- [x] M5.5 Merchant agents (L — only after M5.2 proves out)

Gates: inter-area price divergence within band; recipe towns emerge;
pinned-price share stays PASS; arbitrage income correlates with price gaps.

## M6 — The Telling (chronicle with memory)

- [x] M6.1 Structured events (actor/entity ids beside text) (S-M)
- [x] M6.2 Persistent named non-rulers: generals, prospectors, founders (M)
- [x] M6.3 Artifacts with provenance (M)
- [x] M6.4 Drama-pacing modifier layer (S)
- [x] M6.5 Story sifter: 5-8 Felt-style patterns over the log (M-L)
- [x] M6.6 Legends browser UI over the entity graph (L)
- [x] M6.7 Eventfulness scoring + reversal detection ranking the sifter's output (S-M, research/15)
- [x] M6.8 Narration memory: earned epithets, callbacks, mention-aware templates (M, research/15)
- [x] M6.9 Two-layer telling: ground-truth log vs. mythologized legend rendering (M, research/13)

Gates: every event carries ≥ 1 entity id; sifter yields ≥ N microstories
per century within dedup bounds; browser renders full cross-link graph.

## M7 — The Atlas Plate (cartographic grammar)

- [x] M7.1 Coastal vignettes from coast-distance (S)
- [x] M7.2 Töpfer-law label/settlement culling by zoom (M)
- [x] M7.3 Letter-spaced area labels (S)
- [x] M7.4 Climate-blended hypsometric ramp for elevation mode (M)
- [x] M7.5 Multi-directional hillshade + precomputed texture shading (M)
- [x] M7.6 Route smoothing + junction merging (M)
- [x] M7.7 Curved river labels (L)

Gates: label-overlap count 0 at all zooms; density ≈ Töpfer prediction;
screenshot review at 3 zooms × 2 seeds. Gate runner:
`python3 scripts/atlas-check.py` (live Playwright; plates land in
`game/reports/atlas/`).

## M8 — The Instrument (harness to ERA grade)

- [x] M8.1 Seam-invariant properties: rivers descend `filled`; settlements route-reachable; pack/unpack byte-identity (S each)
- [x] M8.2 Metamorphic checks: rainfall↑ ⇒ river cells not↓ across seeds (S-M)
- [x] M8.3 ERA 2D histograms exported per sweep (M)
- [x] M8.4 Oatmeal detector: between-seed structural distinctiveness metrics (M-L)

Gates: property suite green across sweep; ERA plots reviewed per milestone.
Runners: `diagnose properties` (rect priority-flood descent, route graph,
pack round-trip, rain-metamorphic) and `diagnose era` (four ERA plates,
JSD oatmeal matrix with min/mean collapse alarm) — both in `report.sh`.

## M9 — The Patina (residue, strata & the withheld)

Feel milestone from `research/SYNTHESIS-FEEL.md` (dockets 12-15): let
things die and leave marks, let names carry time, let the telling withhold.

- [x] M9.1 Settlement death & ruins: abandonment (war, depletion, famine) leaves named ruin entities on the map (M)
- [x] M9.2 Hydronym conservatism + bounded conquest name-layers: rivers keep the oldest culture's names; conquest renames settlements inside the conquered polygon only (S-M)
- [x] M9.3 Age-keyed name erosion with stored compositional etymology, glossed in the inspector (S)
- [x] M9.4 History marks the map: battle sites, war-renamed features, disused routes fading (M)
- [x] M9.5 Berúthiel emissions + disputed/unresolved chronicle entries, bounded share (S)

Gates: mature worlds carry ruins across the sweep (≥ 1 per century after
year 100); river names stable through 100 % of border changes; every
emitted name carries an etymology; withheld/disputed entries within a
2-8 % band; full diagnostics suite stays green.
Runner: `diagnose patina` (ruin cadence, hydronym stability, etymology
coverage, veiled share, death-spiral invariants) — in `report.sh`.
Live evidence: 300 y browser run at seed 777 — 25 ruins with strata-glossed
cards, failing-town notices, faded old ways, veiled entries in the feed.

## M10 — Peoples and Thrones (culture ≠ realm)

The structural prerequisite for everything below. Today one `Culture`
is simultaneously a *people* (tongue, gods, demonym, name bank) and a
*state* (treasury, ruler, wars, opinion row) — so the only way a rising
can resolve is by minting an entire new people (`culture::secede`),
realms can only multiply, and the map churns. Split the axes: a
**People** changes on a generational clock (style, pantheon, kinship);
a **Realm** changes on a political clock (dynasty, seat, tier,
treasury, legitimacy, diplomacy). Realms map N:1 onto peoples, and one
realm may hold towns of several peoples. This is the ADR of the arc.

- [ ] M10.1 ADR + type split: `People` in `culture.rs`, `Realm` in `politics.rs`; settlements carry `people` and `realm` ids; `secede` no longer creates a people (L)
- [ ] M10.2 Move all M4 state to the realm axis: opinion matrix, AE dread, coalitions, wars, sieges, tribute, vassalage, asabiyyah, legitimacy (L)
- [ ] M10.3 Dynasties first-class: house name coined in the people's tongue, founder, seat; succession stays in-house; chronicle ruler events become dynasty state, registry-linked (M)
- [ ] M10.4 Seat and court: the capital is a named seat; losing it in war is a legitimacy shock; seat-moved events (S-M)
- [ ] M10.5 Ownership sort: lore/tech travels with the people, treasury/armies with the realm; markets and territory kernels read realm (M)
- [ ] M10.6 Pack + UI: political layer colours by realm, culture layer by people; inspector reads "a town of the Norrfolk, under the crown of Vessmark" (M)

Gates: full suite green after the split with physical-layer hashes
unchanged; every settlement resolves to exactly one people and one
realm across the sweep; no orphan realms or peoples; chronicle entity
links survive the migration; determinism hash stable per ADR-0003.

## M11 — The Crown Endures (the unrest ladder: coup before cleavage)

`rebellion_pass` currently has one answer to a hollow realm: carve out
a new state. Replace the single roll with a severity ladder resolved
by *who* is angry: same-people rebels want the throne, not a border —
only alien or detached peripheries want out. Turchin/Khaldun pacing
(research/09) drives when; kinship (M12) drives which rung.

- [ ] M11.1 Unrest stat per realm — fed by low legitimacy, guttering asabiyyah, famine (M2.6), war weariness, and over-extension (towns beyond the tier's administrative reach); replaces the raw rebellion roll (M)
- [ ] M11.2 Palace coup: a named usurper (general from M6.2, or a courtier) takes the circlet — same realm, same borders, new house; legitimacy resets low; chronicle beats + earned epithets ("the Usurper", "the Kingslayer") (M)
- [ ] M11.3 Succession crisis: a death on a low-legitimacy throne raises 2-3 pretenders; a months-long war of the circlet resolves *internally* — the winner's house rules, no border moves (M-L)
- [ ] M11.4 Secession gate: a rising escalates to secession only when the rebel towns are of another people, or geographically detached (over-sea / beyond admin reach), *and* the realm is hollow; otherwise it resolves lower on the ladder (S-M)
- [ ] M11.5 Charters: realms holding law-codes may answer unrest with concessions — treasury down, legitimacy up, no blood; makes the law tech mechanically real (S)
- [ ] M11.6 Rising cooldown + hysteresis so one realm cannot convulse monthly; sifter patterns for usurpation and restoration arcs (S-M)

Gates: over 300 y sweeps, internal resolutions (coup + crisis + charter)
outnumber secessions ≥ 3:1; a secession never mints a new people when
the rebels share the parent's people; realm count rises *and falls*
within band; no realm posts two risings within the cooldown window;
`diagnose civ` grows an unrest-ledger check.

## M12 — Kindred and Crown (kinship, assimilation, union)

Peoples currently never converge — every historical split adds one and
nothing ever subtracts, so old worlds fragment into a babel and every
minority is forever revolt fuel. Give peoples a kinship metric and let
proximity under one crown do what Axelrod dissemination (research/06)
says it does: pull neighbours together. Merging is the counterweight
that keeps the revolt ledger of M11 balanced.

- [ ] M12.1 Kinship metric between peoples: shared style family, secession lineage (child remembers parent), pantheon overlap, years spent under one realm (S-M)
- [ ] M12.2 Assimilation: towns of people A under a realm of kindred people B drift toward B over ~3-4 generations — faster along roads and shared market areas, slower across straits and ranges (M)
- [ ] M12.3 Peaceful union: two realms of kindred peoples with high mutual opinion and a shared threat merge under one crown by compact or marriage; the lesser house persists as a named vassal line (M-L)
- [ ] M12.4 People merging: kindred peoples long under one realm fuse — the dominant tongue keeps the name bank, the minority leaves loanword strata in toponyms (M9.3 machinery) and its gods enter the shared pantheon (L)
- [ ] M12.5 Minorities and memory: unassimilated towns remember their people across conquests — the standing fuel M11.4 reads; exonym/endonym doubling marks the seam (S)
- [ ] M12.6 Diagnostics: people-count trajectory must fall as well as rise over 300 y; assimilation cadence band; unions fire in sweeps (S)

Gates: people count moves in both directions on ≥ 60 % of sweep seeds;
≥ 1 union or merge per long run in band; hydronym conservatism (M9.2)
survives merges; no assimilation of towns across non-kindred pairs;
suite green.

## M13 — The Arc of Empires (civilizations that rise and fall whole)

With realms, houses, kinship and mergers in place, name the emergent
tier: a **civilization** is a family of kindred peoples plus the realms
that carry them. Give the arc a shape — golden ages, overstretch,
collapse into *successor realms* rather than deletion — so the telling
can narrate rise-and-fall whole instead of a flicker of secessions.

- [ ] M13.1 Civilization as a derived entity: the kinship-closure of peoples, named, registry-tracked, browsable in the legends UI (M)
- [ ] M13.2 Golden ages: sustained legitimacy + asabiyyah + wealth open an era of building — monument artifacts, tech pace up, chronicle set-pieces (M)
- [ ] M13.3 Overstretch and decadence: administrative upkeep superlinear in realm span (Bettencourt ∝ pop^0.85 reuse); the Khaldun decay already in `ASAB_DECAY` surfaced as court-rot events (S-M)
- [ ] M13.4 Collapse and succession: a failing empire fragments into kindred successor realms through an interregnum — never into new peoples; ruins and name strata mark the fall (M)
- [ ] M13.5 Hegemony: tribute/vassal edges compose into a named paramount tier; decisive peaces can build it, collapse dissolves it (S-M)
- [ ] M13.6 Sifter arc: the rise and fall of a civilization detected and told as one multi-century story (M6.7 eventfulness reuse) (M)

Gates: ≥ 1 full rise-and-fall arc per 300 y on most sweep seeds;
successor states inherit their peoples (zero people-minting on
collapse); no runaway single-civilization ending on > 80 % of seeds;
polity count oscillates across the run; suite green.

## M14 — Wool and Wine (the full catalogue of goods)

The resource milestone: from 19 raw deposits + grain + 3 crafts to a
complete goods economy where every classic world-trade good exists for a
mechanical reason — bulk goods hug home, luxuries cross the world, and
the land itself remembers being harvested. Sequenced so the data model
lands first and everything after is table rows.

- [ ] M14.1 Ontology as data: one declarative GOODS table in `resources.rs` (ISA/REQUIRES/ABUNDANCE/FOUNDIN + placement rule, transport class, perishability, color) replacing the scattered match arms; `resource_meta()` derives from it so engine and client cannot drift. ADR on landing (M)
- [ ] M14.2 Salt: coastal salt-pan and rock-salt seam placement; PRESERVES relation — salted fish/meat gain trade range; salt towns and salt roads emerge (M)
- [ ] M14.3 Animal secondaries: wool (sheep country), hides→leather (cattle/game), furs (boreal/tundra trapping — a luxury pull that colonizes the cold the way ore colonizes the dry) (M)
- [ ] M14.4 Cultivated luxuries: wine (warm-temperate belt), spices (tropical coasts), dyes (coastal murex / madder belts) — tight biome bands so they concentrate and long routes have a reason to exist (M)
- [ ] M14.5 Earth and workshop goods: clay→pottery/brick; marble as luxury stone; gem seams (jewelry input beside gold/silver) (S-M)
- [ ] M14.6 Secondary recipes on the M5.1 engine: wool→cloth, hides→leather, clay→pottery, grapes→wine — processing towns distinct from extraction camps, workforce- and tech-gated (M)
- [ ] M14.7 Transport classes: value-density tiers (bulk/ordinary/precious) + perishable flag priced into route viability — bulk moves short or by water, precious crosses the map; von Thünen rings become emergent, not painted (M)
- [ ] M14.8 Renewable stocks with memory: timber/game/fish regenerate logistically and thin under overharvest; deforestation marks the biome map; collapse events feed chronicle and M9 ruins (M-L)
- [ ] M14.9 Per-culture tastes: small data-driven demand modifiers (steppe prizes horses, coasts prize amber-class luxuries) folded into `demand_weight` (S-M)

Gates: every catalogued good is reachable end-to-end across the sweep —
placed or produced, priced in ≥ 1 area book, carried on ≥ 1 route, named
in ≥ 1 chronicle event; median haul distance precious ≥ 3× bulk; salt
towns on suitable coasts ≥ 1 per seed; renewable collapse fires under
press-harvest sweeps but stays < band in normal runs; M2.7 price-ratio
check extended with wool/wine/salt rows from the Hodges/Goucher lists;
determinism hash stable; native gen budget holds. Runner: extended
`diagnose resources` + `diagnose economy` sections in `report.sh`.

## M15 — The Assay (property-proofs for the resource economy)

Property-based testing over the whole resource path: pure-function
properties via `proptest`, world-scale invariants in the harness. The
assay is what lets M14's catalogue grow without fear.

- [ ] M15.1 `proptest` dev-dependency + `cargo test` property lane wired into `report.sh` (S)
- [ ] M15.2 Ontology properties: ISA graph is an acyclic forest rooted in {food, material, fuel, craft}; every good has category, abundance, color, `base_value` > 0; every recipe input/output exists in the ontology; every REQUIRES names a real tech in `society.rs` (S)
- [ ] M15.3 Placement properties: every deposit sits on its own suitability mask; per-resource minimum spacing holds; `rich` ∈ [0.35, 1]; reserves positive for minerals and −1 for renewables; hidden-start only for buried seams; ADR-0013 floors met on arbitrary seeds (M)
- [ ] M15.4 Market properties: all prices within [0.3, 5.0]×base; no NaN/inf in any book; `shock` respects clamps; geometric-mean renormalization keeps mean log-price drift-free over 500 y (S-M)
- [ ] M15.5 Metamorphic economy checks: more supply of g ⇒ p(g) not↑; exhausting g everywhere ⇒ p(g)↑; opening a route between two areas ⇒ their spread on shared goods not↑ (M)
- [ ] M15.6 Conservation ledger: per-area stock/flow accounting — nothing consumed that was not produced or imported; ledger printed by `diagnose economy`, balances within rounding over 200 y (M)
- [ ] M15.7 Hostile unpack: extend the M8.1 round-trip gate — truncated/corrupt pack buffers must error, never panic (fuzz lane) (S)

Gates: property lanes green in `report.sh` across the sweep; new
`diagnose resources` sections (mask, spacing, floors) PASS; metamorphic
trio green on 3 seeds; ledger zero-balance within rounding; full
existing suite stays green throughout.

## Ready (calibration queue)

Near-band findings from the M3 closure run, staged for the next tuning
pass (each is a WARN, not a gate failure):

- Zipf rank-size slope hugs the shallow edge (−0.79 vs gate −1.3…−0.8);

## Ready (calibration queue)

Near-band findings from the M3 closure run, staged for the next tuning
pass (each is a WARN, not a gate failure):

- Zipf rank-size slope hugs the shallow edge (−0.79 vs gate −1.3…−0.8);
  revisit Gibrat noise σ or agglomeration pull after M4 redraws borders.
- Forest share 22.6 % sits under the 25–60 % sweet band across seeds;
  candidate: nudge tree-line moisture threshold in `biomes.rs`.
- On 60 y quick runs only: median town spacing ~67 km (band 15–30) and
  wealth~pop β ~0.73 (target ≈1.15) — re-measure after M5 market areas
  before tuning; long runs pass.

Findings from the M9 closure evidence (live 300 y run, seed 777):

- Tier-change chatter: towns straddling a size threshold emit
  grew/dwindled events repeatedly within a year (Kalliikos, Koropia at
  y300). Add hysteresis (or a per-town cooldown) to tier-transition
  events in `world.rs`; new check: no town posts opposing tier events
  within N months.
- Duplicate myth beats: the same temple-raising myth fired twice for
  Koropia in one year. Add a per-settlement recent-beat dedup in
  `chronicle.rs`; check: no identical (subject, template) pair within
  a 24-month window.
- Native↔WASM trajectory drift: 300 y at seed 777 yields 21 ruins
  native vs 25 in the browser. Per-platform determinism holds
  (ADR-0003 gates native reruns), but libm float differences diverge
  long trajectories across platforms. Decide: pin math to a software
  libm for bit-identity, or write an ADR scoping determinism to
  per-platform reruns.

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
