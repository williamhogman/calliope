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

- [x] M10.1 ADR + type split: `People` in `culture.rs`, `Realm` in `politics.rs`; settlements carry `people` and `realm` ids; `secede` no longer creates a people (L) — ADR-0018
- [x] M10.2 Move all M4 state to the realm axis: opinion matrix, AE dread, coalitions, wars, sieges, tribute, vassalage, asabiyyah, legitimacy (L)
- [x] M10.3 Dynasties first-class: house name coined in the people's tongue, founder, seat; succession stays in-house; chronicle ruler events become dynasty state, registry-linked (M)
- [x] M10.4 Seat and court: the capital is a named seat; losing it (war, cession, or silence) is a legitimacy shock; the crown re-homes the same month; quiet court translations when a town outshines the seat; `diagnose civ` holds the seat-integrity invariant (S-M)
- [x] M10.5 Ownership sort: lore/tech travels with the people, treasury/armies with the realm; markets and territory kernels read realm (M)
- [x] M10.6 Pack + UI: political layer colours by realm, culture layer by people; inspector reads "a town of the Norrfolk, under the crown of Vessmark" (M)

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

- [x] M11.1 Unrest stat per realm — fed by low legitimacy, guttering asabiyyah, famine (M2.6), war weariness, and over-extension (towns beyond the tier's administrative reach); replaces the raw rebellion roll (M)
- [x] M11.2 Palace coup: a named usurper (general from M6.2, or a courtier) takes the circlet — same realm, same borders, new house; legitimacy resets low; chronicle beats + earned epithets ("the Usurper", "the Kingslayer") (M)
- [x] M11.3 Succession crisis: a death on a low-legitimacy throne raises 2-3 pretenders; a months-long war of the circlet resolves *internally* — the winner's house rules, no border moves (M-L)
- [x] M11.4 Secession gate: a rising escalates to secession only when the rebel towns are of another people, or geographically detached (over-sea / beyond admin reach), *and* the realm is hollow; otherwise it resolves lower on the ladder (S-M)
- [x] M11.5 Charters: realms holding law-codes may answer unrest with concessions — treasury down, legitimacy up, no blood; makes the law tech mechanically real (S)
- [x] M11.6 Rising cooldown + hysteresis so one realm cannot convulse monthly; sifter patterns for usurpation and restoration arcs (S-M)

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

- [x] M12.1 Kinship metric between peoples: shared style family, secession lineage (child remembers parent), pantheon overlap, years spent under one realm (S-M)
- [x] M12.2 Assimilation: towns of people A under a realm of kindred people B drift toward B over ~3-4 generations — faster along roads and shared market areas, slower across straits and ranges (M)
- [x] M12.3 Peaceful union: two realms of kindred peoples with high mutual opinion and a shared threat merge under one crown by compact or marriage; the lesser house persists as a named vassal line (M-L)
- [x] M12.4 People merging: kindred peoples long under one realm fuse — the dominant tongue keeps the name bank, the minority leaves loanword strata in toponyms (M9.3 machinery) and its gods enter the shared pantheon (L)
- [x] M12.5 Minorities and memory: unassimilated towns remember their people across conquests — the standing fuel M11.4 reads; exonym/endonym doubling marks the seam (S)
- [x] M12.6 Diagnostics: people-count trajectory must fall as well as rise over 300 y; assimilation cadence band; unions fire in sweeps (S)

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

- [x] M13.1 Civilization as a derived entity: the kinship-closure of peoples, named, registry-tracked, browsable in the legends UI (M)
- [x] M13.2 Golden ages: sustained legitimacy + asabiyyah + wealth open an era of building — monument artifacts, tech pace up, chronicle set-pieces (M)
- [x] M13.3 Overstretch and decadence: administrative upkeep superlinear in realm span (Bettencourt ∝ pop^0.85 reuse); the Khaldun decay already in `ASAB_DECAY` surfaced as court-rot events (S-M)
- [x] M13.4 Collapse and succession: a failing empire fragments into kindred successor realms through an interregnum — never into new peoples; ruins and name strata mark the fall (M)
- [x] M13.5 Hegemony: tribute/vassal edges compose into a named paramount tier; decisive peaces can build it, collapse dissolves it (S-M)
- [x] M13.6 Sifter arc: the rise and fall of a civilization detected and told as one multi-century story (M6.7 eventfulness reuse) (M)

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

- [x] M14.1 Ontology as data: one declarative GOODS table in `resources.rs` (ISA/REQUIRES/ABUNDANCE/FOUNDIN + placement rule, transport class, perishability, color) replacing the scattered match arms; `resource_meta()` derives from it so engine and client cannot drift. ADR on landing (M) — ADR-0021; placement byte-identical across seeds 12345/777/90210; `ontology_lint` wired into `diagnose resources`
- [x] M14.2 Salt: coastal salt-pan and rock-salt seam placement; PRESERVES relation — salted fish/meat gain trade range; salt towns and salt roads emerge (M) — `Good::Salt` appended to the GOODS table (earlier noise planes untouched); `Place::CoastOrBand` rule: renewing pans on arid shores (known at dawn, `left = -1`) + hidden finite rock seams in dry basins; placement floor guarantees ≥1 pan and ≥2 sources per world; PRESERVES (`salt_cured`) gates perishables out of all three cross-area lanes (border equalizer, gravity lane, merchants) unless a market area holds salt; demand 1.00 flat (under food, off luxury); salt/grain band 0.8–10× sweet on the Hodges list; checks in `diagnose resources` (pan exists / ≥2 sources / pans known, all 3 seeds) and `diagnose economy` (salt towns worked, price band). Landing exposed a latent M12 breach — drift decayed 0.02/mo instead of dying when a pair fell non-kindred, leaving stale drift up to 50 months after a conquest; the leaning now zeroes at once in `culture.rs`. Full gate after: 346 pass · 0 fail.
- [x] M14.3 Animal secondaries: wool (sheep country), hides→leather (cattle/game), furs (boreal/tundra trapping — a luxury pull that colonizes the cold the way ore colonizes the dry) (M) — wool and hides are derived secondaries in `goods_for` (the flock is the deposit: sheep ⇒ wool, cattle/deer/elk ⇒ hides; appended behind their animals, first dropped when the list fills); furs are placed grounds (`Place::Biomes` boreal/tundra, Rare, Precious, renewing, known at dawn) and join the colonist pull in `resource_pull` — the one renewable that calls camps into the waste; new `is_luxury` closure flag + lint row + demand arm (0.25 + 1.6·luxury: taste, nothing else); `luxury` category lands in the ontology. Wool/grain band 1–12× sweet. Evidence: 100 y seed 12345 — 11 wool towns · 5 hide towns · wool/grain 5.1×; 300 y — 2 fur towns (12345), 5 (777) incl. Kharagan, pop 10 852, a harbour exporting furs. Full gate: 352 pass · 0 fail. Leather/cloth recipes stay M14.6.
- [x] M14.4 Cultivated luxuries: wine (warm-temperate belt), spices (tropical coasts), dyes (coastal murex / madder belts) — tight biome bands so they concentrate and long routes have a reason to exist (M) — three GOODS rows: grapes (`Place::BiomesAndBand` woodland/seasonal-rain-forest on the 0.12–0.5 hill band, farming-gated), spices (`Place::Coast` tropical shores, Rare, Precious — joins furs in the `resource_pull` colonist call: the fever coast colonizes like the cold), dyes (`Place::Coast` temperate shores, Rare, Precious); all three under `is_luxury` so demand is taste alone. `Place::Coast` places by within-mask maxima with a best-cell fallback so 1-cell shore strips still take a ground. Placement rows in `diagnose resources` (grapes/spices/dyes 100% across 3 seeds). Evidence: seed 777, 300 y — Mossathorn, pop 21.6k, a harbour exporting spices. Full gate: 356 pass · 0 fail. Wine as a pressed recipe stays M14.6.
- [x] M14.5 Earth and workshop goods: clay→pottery/brick; marble as luxury stone; gem seams (jewelry input beside gold/silver) (S-M) — five GOODS rows: clay (`Place::RiverBanks(0.35)`, new rule shape: low land touching river/lake — the alluvial margins; Common, Bulk, renews, plain to see), marble (the luxury stone, the one Bulk luxury: `is_luxury` + Rare + `Place::Above(0.55)` quarry seams, 45% known at dawn, ADR-0013 floor 2; it crosses the world only where water carries it once M14.7 prices that), gems (`Place::Above(0.6)`, Rare, Precious, 4% known — the jeweler's third ore: joins gold/silver in the M5.1 jewelry recipe; floor 2), pottery and brick (crafts, `Place::None`) on two new kiln RECIPES (pottery: era-0 Pottery art, pop ≥800; brick: Masonry, pop ≥1500; both burn fuel, both draw clay off the area market) with kiln-voiced chronicle lines. `GoodSet` widened u32→u64 (35 goods). Structural price fix found by the gate: the scarcity ratio now saturates below the 0.3×/5× clamp (0.148–16 → 0.35×–4.6×base), so a single-supplier good settles dear but off the pin — max pinned share fell 55.9%→0% on seed 12345 while gold/grain held 25.9×. Evidence: 100 y seed 12345 — 20 clay towns, 3 pottery kilns lit, marble worked by 2 towns. Full gate: 356 pass · 0 fail.
- [x] M14.6 Secondary recipes on the M5.1 engine: wool→cloth, hides→leather, clay→pottery, grapes→wine — processing towns distinct from extraction camps, workforce- and tech-gated (M) — three RECIPES on the existing engine (no new machinery: the M5.1 niche cap, area-market sourcing and monthly lighting roll carry the whole feature): cloth (Loom, pop ≥1000, wool), leather (herb-lore — tan-bark, not fire, pop ≥1000, hides), wine (the Pottery art — amphorae are the wine trade, pop ≥1200, grapes); none burn fuel. Chronicle voices split per craft: `craft_voice(out)` is now the one lookup for (works, feedstock, line-set) — forges/kilns/looms/tan-pits/presses each speak their own idiom, and the cold lines name what stopped coming. Two new gate rows: "soft trades lit" (≥1 kind by 100 y) and "processing splits from extraction" (≥1 workshop town holding no ground for its own feedstock — the towns divide by role because sourcing is area-wide, not decreed). Evidence: seed 12345 — 100 y: leather + wine lit, 2 split shops; 200 y: 3 cloth · 1 leather · 1 wine, 3 split shops (the loom is era-0, the lag is pop and niche, as designed). Full gate: 359 pass · 0 fail.
- [x] M14.7 Transport classes: value-density tiers (bulk/ordinary/precious) + perishable flag priced into route viability — bulk moves short or by water, precious crosses the map; von Thünen rings become emergent, not painted (M) — one function, `carriage(g, cost, c0)`: the fraction of a price gap a cargo class can profitably haul, reach quoted in units of the median leg cost (scale-free like M5.4; bulk 0.75·c0, ordinary 3·c0, precious 24·c0; fresh fruit ×0.15 — no salt cures it). Sea legs are already ~9× cheaper in `Route::cost` (ADR-0010), so "bulk moves by water" falls out of the same number. Applied at all four border crossings: route-equalization rate, route-flow gap earnings, merchant run selection (linked pairs now remember their cheapest leg), and — the decisive one — the world-anchor pull in `update_area_prices` is now a class fact, not a constant (bulk 0.12, ordinary 0.35, precious 0.60, fresh 0.05): a "world price" only exists for goods that actually cross the world. That last change is where the rings live — with uniform openness 0.5 the ordering came out INVERTED (precious ×1.98 > bulk ×1.68, rarity noise beating freight); class-split openness flipped it decisively. New gate row "von Thünen ordering" (bulk spread ≥ precious spread across areas): seed 12345 — bulk ×4.31 · ordinary ×3.63 · precious ×1.59; seed 777 — ×4.42/×3.29/×1.91. Divergence band re-based 3.0→4.5 sweet (bulk's dispersion is now deliberate); hard wall unchanged. Full gate: 361 pass · 0 fail.
- [x] M14.8 Renewable stocks with memory: timber/game/fish regenerate logistically and thin under overharvest; deforestation marks the biome map; collapse events feed chronicle and M9 ruins (M-L) — `Deposit` gains `stock` (logistic: r·s·(1−s) − 0.0025·crews monthly; timber r=0.020, game 0.030, fish 0.040, so max sustainable harvest r/4 means a lone hamlet logs forever and a crowded coast strips its woods — forests fail before fisheries) and `phase`, a hysteresis latch (healthy → thinned at 0.35 → collapsed at 0.06; recovery re-arms at 0.50) so the chronicle speaks at thresholds, not monthly. One new method `Deposit::live()` replaced all five hand-kept qualification sites (goods_for, resource_pull, mining rush, tech reach, crews) — collapse withdraws the good everywhere by construction. Timber collapse scars the biome map (forest→grassland in a radius, originals remembered in `World::scars`; recovery restores them). Gate rows: "wild stocks breathe", "the axe bites somewhere", "wild collapse share" band (0–25% sweet), and the withdrawal invariant "wild goods trace to living grounds" (subsistence nets exempt: a coastal town whose only natural good is the fallback fish keeps netting minnows — it never suppresses recovery since collapsed grounds draw no crews). Seed 12345 · 100y: 10 thin · 9 collapse · 6 recover, 107 scar cells, collapse share 12.5%. Full gate: 363 pass · 0 fail.
- [x] M14.9 Per-culture tastes: small data-driven demand modifiers (steppe prizes horses, coasts prize wine and marble) declared once in `culture::taste`, folded into the demand side of `compute_prices` as a pop-weighted mix per market; explain.rs mirrors the mix; diagnose proves the wiring with a same-supply A/B through the public API

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

- [x] M15.1 `proptest` dev-dependency + `cargo test` property lane wired into `report.sh` (S)
- [x] M15.2 Ontology properties: ISA graph is an acyclic forest rooted in {food, material, fuel, craft}; every good has category, abundance, color, `base_value` > 0; every recipe input/output exists in the ontology; every REQUIRES names a real tech in `society.rs` (S)
- [x] M15.3 Placement properties: every deposit sits on its own suitability mask; per-resource minimum spacing holds; `rich` ∈ [0.35, 1]; reserves positive for minerals and −1 for renewables; hidden-start only for buried seams; ADR-0013 floors met on arbitrary seeds (M)
- [x] M15.4 Market properties: all prices within [0.3, 5.0]×base; no NaN/inf in any book; `shock` respects clamps; geometric-mean renormalization keeps mean log-price drift-free over 500 y (S-M)
- [x] M15.5 Metamorphic economy checks: more supply of g ⇒ p(g) not↑; exhausting g everywhere ⇒ p(g)↑; opening a route between two areas ⇒ their spread on shared goods not↑ (M)
- [x] M15.6 Conservation ledger: per-area stock/flow accounting — nothing consumed that was not produced or imported; ledger printed by `diagnose economy`, balances within rounding over 200 y (M)
- [x] M15.7 Hostile unpack: extend the M8.1 round-trip gate — truncated/corrupt pack buffers must error, never panic (fuzz lane) (S)

Gates: property lanes green in `report.sh` across the sweep; new
`diagnose resources` sections (mask, spacing, floors) PASS; metamorphic
trio green on 3 seeds; ledger zero-balance within rounding; full
existing suite stays green throughout.

## Ready (calibration queue)

Findings from the M15 assay landing (staged, not gate failures):

- `validate_pack` guards the pack natively only: the worker's unpacker
  still trusts the header at face value. Export the validator through
  `WasmWorld` (or run it in `worker.js` before unpack) so a corrupted
  cached pack fails loud in the client too — the proptest lane already
  proves the checker itself never panics on hostile bytes.
- `scripts/build.sh` decides wasm staleness by mtime, and checkout
  normalization can stamp sources and binary within milliseconds of
  each other — this run silently skipped a needed rebuild until forced.
  Replace the `-newer` probe with a content hash of `src/ + Cargo.toml`
  stored beside `version.js`.
- The M15.5 route-spread law tests only the dearest route as bridge;
  a sweep variant over every area-bridging route (and over the sweep
  seeds) would turn one witness into a quantifier.

Findings from the M14.2/M14.3 landings (staged, not gate failures):

- Fur-country telling: fur camps founded by the pull emit the generic
  colony line today. Give the chronicle a trapping-camp flavor at the
  emission site and let `telling.rs` sift a "fur road" microstory when
  a fur town's exports cross an area border — Kharagan (seed 777, 300 y,
  pop 10.8k, exports furs) is exactly the town whose story goes untold.
- `salt_cured` gates only fish until meats perish: cattle/pig/sheep and
  game are `perishable: false` in the ontology, so salt's preservation
  rule bites one good. When M14.7/M14.8 touch herd goods, revisit
  whether fresh meat should perish (and salt widen its franchise).

- Rock-salt seams should seek the dead seas: bias the
  `Place::CoastOrBand` bed mask toward `CellFlags::SALT` endorheic
  basins so the patina's salt lakes and the economy's salt roads agree
  — today beds pick any arid cell in the 0.15–0.5 height band; the
  hydrology already knows where the desert keeps its brine.
- Assimilation cadence still reads 0.0 flips/century on 150 y runs
  (WARN since M12): drift ~1.6 %/yr means the first flips land around
  y60–80 only where a crown holds the same kindred town unbroken;
  conquests reset the clock (correctly, and now instantly per the M12
  gate fix). Re-measure on 300 y patina runs before touching the rate.

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

Findings from the M12 closure run (325 pass · 8 warn · 0 fail):

- Assimilation cadence 0.7 flips/century vs the "few per century" M12.2
  band. Candidates: soften the 0.20 kinship drift floor in `culture.rs`
  (co-residence has to climb a long way before drift may begin), or let
  road/market adjacency raise the drift rate. Re-measure at 150 y.
- People roster saturates: patina shows exactly 20 sunderings per seed
  over 300 y — divergence is pinned at the `CULTURE_COLORS.len() * 2`
  cap with fusion as the only release valve. Consider whether the cap
  should breathe (dead slots reusable) or the divergence odds should
  fall as the roster fills; add a check that the roster spends < 80 %
  of the run at the ceiling.

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
