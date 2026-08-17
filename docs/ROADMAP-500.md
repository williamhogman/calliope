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
A **quarter** holds 2–3 phases under one goal. An **era** is ~55 phases,
~20 quarters, one great theme. Phases here are one-line sketches —
each phase gets its full item breakdown and gates only when its era
comes into range, era by era, interview by interview.

Status: skeleton + Era I drafted. Eras II–IX are themes awaiting their
own drafting rounds.

## The nine eras

| Era | Name | Phases | Theme |
|---|---|---|---|
| I | The Named Lives | M16–M70 | Notable persons: kinship, courts, deeds, plots, biography |
| II | The Deep Earth | M71–M125 | Physical stack II: tectonic prehistory, glaciation, ocean circulation, GPU erosion |
| III | The Long Sky | M126–M180 | Climate variability: century drift, oscillations, disasters, the weather of history |
| IV | The Living Land | M181–M235 | Ecology: wildlife, succession, fisheries, trade-graph plagues |
| V | The Tongues | M236–M290 | Language families, sound change, dialect continua, script and literacy |
| VI | The Unseen Order | M291–M345 | Religion with mechanical stakes: cults, schisms, holy sites, myth drift |
| VII | The Wide World | M346–M400 | Scale: exploration, distant continents, world-systems trade, hegemonic cycles |
| VIII | The Proof | M401–M455 | Calibration against historical datasets; property proofs everywhere; ERA-grade evaluation |
| IX | The Sealed Instrument | M456–M515 | Observability: causal query over history, explain-everything, archival export, the final gate |

Eras I–VIII: 55 phases each. Era IX: 60. Total: 500.

---

## Era I — The Named Lives (M16–M70)

The chronicle stops being about abstractions. Notables only — rulers,
generals, merchants, sages, founders — hundreds of tracked persons per
world with kin, deeds, deaths; demography stays aggregate. Builds
directly on the prologue's M10–M13 (peoples/realms/dynasties) and M6.2
(persistent named entities). Five roadmap-years, twenty quarters.

### Year 1 — The Person Record

**Q1 — Flesh and bone.** A person exists, ages, and dies, deterministically.
- M16 Person entity + registry: ids, birth/death, ties to people and realm
- M17 Lifecycle clock: aging, deterministic mortality curves by era and station
- M18 Person determinism: registry folded into the state hash, sweep gates

**Q2 — Blood and name.** Persons connect into houses.
- M19 Marriage and household formation
- M20 Children, descent, name inheritance in the people's tongue
- M21 Kinship-graph properties in the harness: acyclic descent, spouse symmetry, no time paradoxes

**Q3 — The court.** Power has a room and chairs in it.
- M22 Offices: marshal, chancellor, steward — appointed, held, lost
- M23 Courts sited at seats (M10.4); office-holders live where the crown is
- M24 Favor and standing: a per-court ledger the plots of Year 3 will read

**Q4 — Deeds.** What a person did is recorded, and echoes.
- M25 Deed ledger per person, registry-linked to events
- M26 Earned epithets from deed patterns (extends M6.8)
- M27 Epitaphs and tombs as artifacts with provenance

### Year 2 — The Persons of Power

**Q5 — The crowned.** Rulers become real persons.
- M28 Succession resolved from actual kin trees, not synthetic house state
- M29 Regencies, minorities, and the danger of a child on the throne
- M30 Dynasty trees derived from descent; M10.3 house state retired into it

**Q6 — The sworded.** Wars are commanded by someone.
- M31 Generals hold command: appointment, victory, disgrace
- M32 Battlefield fates: death, capture, ransom
- M33 War chronicle cites commanders on both sides

**Q7 — The monied.** Wealth has owners.
- M34 Merchants as persons riding the M5.5 agent lane
- M35 Prospectors and founders retro-wired from M6.2 entities
- M36 Personal fortunes beside realm treasuries

**Q8 — The learned.** Knowledge has authors.
- M37 Sages and engineers tied to tech unlocks
- M38 Works: named inventions and buildings carry their maker
- M39 Masters and apprentices: knowledge lineages across generations

### Year 3 — The Play of Wills

**Q9 — Character.** Persons differ, boundedly and deterministically.
- M40 Trait vector fixed at birth: bold/cautious, cruel/just, open/grasping
- M41 Traits bias decisions the sim already makes: war, building, exploration
- M42 Trait heredity with drift, held in bands

**Q10 — Faction.** Courts split.
- M43 Factions form from favor, kinship, and grievance
- M44 Faction pressure feeds the M11 unrest ladder
- M45 Purges and exiles

**Q11 — The plot.** The usurper is a person with backers.
- M46 Conspiracies: recruitment, secrecy, resolve
- M47 Assassination attempts with consequences either way
- M48 Plot discovery, trials, and the chronicle beats they earn

**Q12 — The match.** Marriage is statecraft.
- M49 Inter-realm matches move the opinion matrix
- M50 Dowries and claims travel with brides and grooms
- M51 Claim wars: succession claims through blood, wired to M11.3

### Year 4 — The Peopled Telling

**Q13 — Voices.** Every event names its people.
- M52 Chronicle events cite persons, always
- M53 Prose knows kinship: "his uncle's slayer", "her father's city"
- M54 Mention-aware person callbacks (extends M6.8)

**Q14 — The lives sifted.** Biography becomes a story form.
- M55 Biography arcs as sifter patterns: rise, fall, revenge, exile-and-return
- M56 Eventfulness scored per person
- M57 Cast discipline: bounded notables per century, dedup, no name soup

**Q15 — The remembered.** The dead stay browsable.
- M58 Legends browser: genealogies and house trees
- M59 Person cards in the inspector: deeds, kin, works, tomb
- M60 Mythologization of dead notables (M6.9 legend layer over lives)

**Q16 — The graven.** Lives mark the map.
- M61 Tombs, monuments, and person-derived name strata (M9 machinery)
- M62 Relics: person-owned artifacts with provenance chains
- M63 Sites of memory: where the famous fell, marked and named

### Year 5 — The Honest Census

**Q17 — The bands.** The cast is demographically honest.
- M64 Notable-population bands vs town sizes and eras across the sweep
- M65 Lifespan and mortality calibrated against medieval demography

**Q18 — The proofs.** Lives join the property suite.
- M66 Kinship and reference integrity at ERA grade: no orphans, no paradoxes
- M67 Metamorphic checks: more war years ⇒ commander deaths not fewer

**Q19 — The cost.** Persons stay cheap.
- M68 Registry performance bands: memory and tick cost at 500 years
- M69 Pack and UI lanes for persons: delta ticks, browser at full cast

**Q20 — The seal.**
- M70 Era I gate: `diagnose lives` runner, 300-year sweep, biographies in band, full suite green

---

## Era II — The Deep Earth (M71–M125) — to be drafted

The world beneath earns its history: tectonic prehistory as a
generative layer (superseding ADR required), glaciation cycles carving
fjords and moraines, ocean circulation driving real coastal climates,
GPU erosion at scale, soils and aquifers, quakes and eruptions entering
the chronicle. Reopened items pass the "genuinely deeper" bar or stay dead.

## Era III — The Long Sky (M126–M180) — to be drafted

Climate stops being a constant: century-scale drift, oscillation modes,
storm and drought as events with dates, little-ice-age arcs that move
peoples — the weather of history, feeding famine, migration, and war.

## Era IV — The Living Land (M181–M235) — to be drafted

The deferred frontier, first half: wildlife with hunting pressure,
vegetation succession under axe and fire, fisheries and forests as
collapsible stocks, and trade-graph plagues that give the road network
teeth.

## Era V — The Tongues (M236–M290) — to be drafted

The deferred frontier, second half, part one: language families with
sound change, dialect continua along the roads, script and literacy as
tech with consequences, toponymy as true archaeology.

## Era VI — The Unseen Order (M291–M345) — to be drafted

Part two: religion with mechanical stakes — cults, schisms, holy
sites, pilgrimage, monastic knowledge-keeping, myth drift against the
ground-truth log.

## Era VII — The Wide World (M346–M400) — to be drafted

Scale as a subject: exploration and the fog of the far, distant
continents and late contact, world-systems trade, hegemonic cycles at
the civilization tier (M13 machinery at full reach).

## Era VIII — The Proof (M401–M455) — to be drafted

The instrument earns the word definitive: calibration against
historical datasets (demography, price series, war frequency,
rank-size), property proofs across every subsystem, ERA-grade
evaluation as a standing lane, the oatmeal detector at full strength.

## Era IX — The Sealed Instrument (M456–M515) — to be drafted

The closing era: causal query over the whole of history, an
explain-everything inspector, archival-grade export of worlds and runs,
cross-run statistics, and the final gate that proves the Five Hundred
closed.
