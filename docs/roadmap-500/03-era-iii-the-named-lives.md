# Era III — The Named Lives (M126–M180)

Full four-field specs for Era III of `../ROADMAP-500.md`: the person
record and its lifecycle, kinship and households, courts, offices and
factions, plots and assassinations, the peopled telling, legends
browsing, and the honest census — closed by Forge III (M176–M180),
which recasts the entity and chronicle systems around persons. The
one-liners in the parent file are binding; these specs expand them.

### M126 — The Person Record
- **Intent:** Individual lives become the chronicle's unit of account, not just realms and cultures, so history has faces.
- **Build:** Add `EntityKind::Person`-backed persons as a dedicated `Person` struct in a new `person.rs` module — id, name, birth month, culture, home realm, kin slots reserved for M130 — registered through `entity::Registry` exactly as rulers already are, with a bounded-population registry sized off town counts so the cast never explodes ahead of demand.
- **Touches:** game/rust/src/entity.rs, game/rust/src/ids.rs, new: game/rust/src/person.rs, game/rust/src/state.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose lives` (new, gated) shows person count scaling monotonically with settled population across three seeds, zero duplicate ids, zero orphaned culture references.

### M127 — Lifecycle Clock
- **Intent:** Persons age and die on a curve, giving the cast a rhythm instead of frozen immortality.
- **Build:** Implement a monthly aging tick in `person.rs` driving deterministic mortality draws from era- and station-conditioned survival curves (child mortality high pre-Bronze, tapering with tech age per society.rs's era ladder; nobles get a modest longevity edge over commoners), each draw pulled from a dedicated PCG stream per ADR-0003; mortality bands are stored as small lookup tables keyed by `society::Era` and station, consumed once per person per tick rather than recomputed, keeping the tick cost bounded as cast size grows.
- **Touches:** game/rust/src/person.rs, game/rust/src/society.rs, game/rust/src/systems.rs, game/rust/src/util.rs
- **Gate:** `diagnose lives` reports median lifespan and infant mortality within the calibration bands per era across a 100-year sweep, stable across three runs of the same seed.

### M128 — Person Determinism
- **Intent:** The new cast must be provably part of the single deterministic world, not a side channel.
- **Build:** Fold the person registry — ids, birth/death months, culture and station fields — into `diagnose.rs::hash_state`, and add a sweep gate that regenerates a seed twice and diffs every person field, not just counts.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/person.rs, game/rust/src/state.rs
- **Gate:** `diagnose determinism` hashes identical across native/WASM and repeated native runs with the person registry included, zero field-level diffs across two seed-identical runs.

### M129 — Marriage and Household Formation
- **Intent:** Persons stop existing in isolation and start forming the households that houses and dynasties will stand on.
- **Build:** Add a monthly matchmaking pass pairing eligible unmarried adults within culture and station bounds (or across allied cultures, rare), creating a `Household` record and, on first union between two named lines, a `House` alliance flag consumed later by court and faction systems; marriage rate tuned against era-appropriate age-at-first-marriage bands.
- **Touches:** game/rust/src/person.rs, new: game/rust/src/household.rs, game/rust/src/society.rs, game/rust/src/culture.rs
- **Gate:** `diagnose lives` shows marriage rate and age-at-marriage within calibration bands, no person married twice concurrently, no self-marriage, stable across reruns.

### M130 — Children, Descent, Name Inheritance
- **Intent:** Households produce the next generation, and names carry the weight of lineage in the people's own tongue.
- **Build:** Households roll deterministic fertility per married year against era-appropriate fertility bands, spawning child `Person` records with a parent-link pair and a name drawn from `naming.rs`'s per-culture generator with a patronymic or house-name suffix rule keyed to that culture's naming convention.
- **Touches:** game/rust/src/household.rs, game/rust/src/person.rs, game/rust/src/naming.rs, game/rust/src/culture.rs
- **Gate:** `diagnose lives` confirms every child resolves to exactly two registered parents, birth months fall inside the parents' fertile window, and naming conventions match the parent culture in a sampled century.

### M131 — Kinship-Graph Properties
- **Intent:** The kinship web earns the same rigor as the terrain and economy graphs before anything is built atop it.
- **Build:** Add a kinship-integrity pass to the harness checking descent-graph acyclicity (no person is their own ancestor), spousal symmetry (every marriage link resolves both directions), and temporal soundness (no child born before a parent's birth or after a parent's death, no marriage after death).
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/person.rs, game/rust/src/household.rs
- **Gate:** `diagnose lives` runs the kinship-property suite across a 300-year sweep on three seeds with zero cycle, symmetry, or paradox violations.

### M132 — Offices
- **Intent:** Power in a realm gets named seats — marshal, chancellor, steward — that persons can win and lose.
- **Build:** Add an `Office` enum and per-realm office table in a new `court.rs`, with deterministic appointment rolls favoring eligible adult persons of the ruler's culture and station, term lengths bounded by the holder's life or dismissal, and a chronicle event on every appointment and vacating.
- **Touches:** new: game/rust/src/court.rs, game/rust/src/politics.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose statecraft` shows every realm's three offices filled or explicitly vacant at all times, no person holding the same office twice concurrently, hash-stable across reruns.

### M133 — Courts at Seats
- **Intent:** Office-holders live somewhere real, tying power to the geography the realm already claims.
- **Build:** Site each realm's court at its capital settlement per M10.4's seat record, relocating office-holders' residence field when the seat moves (conquest, succession, city founding), and expose the court's location to the settlement inspector.
- **Touches:** game/rust/src/court.rs, game/rust/src/settlements.rs, game/rust/src/politics.rs, game/web/js
- **Gate:** `diagnose statecraft` confirms every office-holder's residence equals their realm's current seat settlement id at all sampled ticks.

### M134 — Favor and Standing
- **Intent:** Court life needs a ledger of who is rising and falling, feeding the plots the third year will read.
- **Build:** Add a per-court favor ledger — a bounded map from person id to a decaying standing score, nudged by deed outcomes, office tenure, and marriage alliances — stored in `court.rs` and folded into the determinism hash; no consumer yet, this phase only accrues the ledger honestly.
- **Touches:** game/rust/src/court.rs, game/rust/src/politics.rs, game/rust/src/state.rs
- **Gate:** `diagnose statecraft` shows favor scores bounded within their configured range for every tracked person, decaying toward zero absent events, hash-stable across reruns.

### M135 — Deed Ledger
- **Intent:** What a person did becomes queryable history, not a fact lost in the prose.
- **Build:** Add a per-person `Vec<DeedRef>` in `person.rs` linking to the existing `Event` log via `EntityId`, populated at emission time wherever chronicle.rs already writes an event citing a person, with no new event kinds — only the back-reference.
- **Touches:** game/rust/src/person.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose lives` confirms every deed reference resolves to a real event in the log and every person-citing event is reachable from its person's ledger, both directions.

### M136 — Earned Epithets from Deeds
- **Intent:** A name should carry what its bearer did, extending the epithet system from titles alone to a life's pattern of deeds.
- **Build:** Extend M6.8's epithet mechanism to scan a person's deed ledger for recurring patterns (repeat victories, repeat foundings, a death in battle) and append a matching epithet from a per-pattern bank, deduplicated against epithets already held.
- **Touches:** game/rust/src/entity.rs, game/rust/src/person.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose chronicle` shows epithet-earning rate within band per century, no person holding contradictory epithets, hash-stable across reruns.

### M137 — Epitaphs and Tombs
- **Intent:** Death leaves a physical, ownable trace instead of vanishing from the map.
- **Build:** On a tracked person's death, emit a tomb `Artifact` (extending the existing `EntityKind::Artifact` machinery in `artifact.rs`) sited at the person's home settlement, carrying a one-line epitaph composed from the deed ledger and a provenance chain rooted at the death event.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/person.rs, game/rust/src/chronicle.rs, game/rust/src/entity.rs
- **Gate:** `diagnose relics` shows every tracked person's death producing exactly one tomb artifact with a non-empty provenance chain, stable hash across reruns.

### M138 — Succession from Kin Trees
- **Intent:** Who inherits the crown finally comes from the family the sim actually simulated, not a synthetic house counter.
- **Build:** Rewrite `politics.rs`'s succession resolver to walk the deceased ruler's kinship graph (eldest eligible child, then sibling, then nearest agnate) via `household.rs` links, retiring the interim house-state successor picker while keeping the same event vocabulary.
- **Touches:** game/rust/src/politics.rs, game/rust/src/person.rs, game/rust/src/household.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose statecraft` shows every succession resolves to a real kin-graph descendant or a documented fallback, zero synthetic successors remain, hash-stable across reruns.

### M139 — Regencies and Minorities
- **Intent:** A child on the throne is a real danger, not a rules gap papered over.
- **Build:** When succession lands on a person below an era-appropriate majority age, install a regent (the highest-favor office-holder or nearest adult kin) who governs in the ruler's name until majority, with elevated instability risk feeding the existing unrest ladder while the regency holds.
- **Touches:** game/rust/src/politics.rs, game/rust/src/court.rs, game/rust/src/person.rs
- **Gate:** `diagnose statecraft` shows every minor ruler under an active regency with a resolvable regent id, regency ending exactly at majority age, hash-stable across reruns.

### M140 — Dynasty Trees from Descent
- **Intent:** The dynasty becomes a derived fact of who bore whom, retiring the synthetic house record it grew up beside.
- **Build:** Derive dynasty membership and generational depth directly from the kinship graph built in M130/M131, retire M10.3's standalone house-state struct in favor of a query over `household.rs`, and preserve existing dynasty-name continuity so realm history doesn't visibly jump.
- **Touches:** game/rust/src/politics.rs, game/rust/src/household.rs, game/rust/src/person.rs, game/rust/src/state.rs
- **Gate:** `diagnose statecraft` shows dynasty trees derived and cross-checked against the kin graph with zero mismatches, the retired house-state field removed and the state hash unchanged in structure elsewhere.

### M141 — Generals Hold Command
- **Intent:** Armies answer to a named person whose career can rise or end on the field.
- **Build:** Add a `General` office (extending `court.rs`'s office table) appointed per active war from eligible adults, tracked through the war's duration with victory/defeat tallies feeding favor, and disgrace on repeated defeat removing the appointment before natural term end.
- **Touches:** game/rust/src/court.rs, game/rust/src/politics.rs, game/rust/src/person.rs
- **Gate:** `diagnose statecraft` shows every active war citing a resolvable general on each side, disgrace events matching the defeat-streak rule exactly, hash-stable across reruns.

### M142 — Battlefield Fates
- **Intent:** War risks the commander's body, not just the treasury and the map.
- **Build:** Add a per-battle-resolution roll (scaled by war score swing per M9 politics' Lanchester-derived combat) sending a general to death in the line, capture, or unharmed retreat, with captured generals entering a ransom mechanic that drains the losing realm's treasury or ends in death if unpaid within a bounded term.
- **Touches:** game/rust/src/politics.rs, game/rust/src/court.rs, game/rust/src/person.rs, game/rust/src/economy.rs
- **Gate:** `diagnose statecraft` shows battlefield-fate rates within calibrated bands per war-score magnitude, every ransom resolving to payment or death within its term, hash-stable across reruns.

### M143 — War Chronicle Cites Commanders
- **Intent:** The written history of a war finally names who fought it, not just which realms.
- **Build:** Extend chronicle.rs's war narration templates to interpolate the appointed general's name and title on both sides at declaration, key battles, and peace, reading directly from the `Office` table rather than the realm alone.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/court.rs, game/rust/src/event.rs
- **Gate:** `diagnose chronicle` shows every war-kind event emitted after M141 citing at least one resolvable commander id, zero unresolved name slots.

### M144 — Merchants as Persons
- **Intent:** Wealth gets a face by riding the trade agent lane the economy already runs.
- **Build:** Attach a `Person` record to each long-lived trade agent from M5.5's agent lane, carrying accumulated personal profit as a new fortune field distinct from realm treasury, with agent death (via the M127 mortality clock) passing the fortune to an heir or, absent one, into the settlement's coffers.
- **Touches:** game/rust/src/trade.rs, game/rust/src/person.rs, game/rust/src/economy.rs
- **Gate:** `diagnose merchants` shows every long-lived agent bound to a resolvable person id with a fortune ledger, inheritance resolving to an heir or settlement in one hundred percent of death events.

### M145 — Prospectors and Founders Retro-Wired
- **Intent:** The prospectors and founders M6.2 already named finally live inside the full person system instead of beside it.
- **Build:** Migrate the standalone prospector/founder entity records into `person.rs`'s `Person` struct with role tags, backfilling their existing registry entries so entity ids and history are preserved, and wire their existing deed events into the M135 deed ledger.
- **Touches:** game/rust/src/prospecting.rs, game/rust/src/person.rs, game/rust/src/entity.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose prospect` and `diagnose lives` show zero entity id changes across the migration on a fixed seed, every prospector/founder record queryable as a full person with a non-empty deed ledger.

### M146 — Personal Fortunes Beside Treasuries
- **Intent:** A realm's wealth and a notable's wealth are visibly different things that can diverge.
- **Build:** Generalize M144's fortune field to every person with an economic role (merchants, prospectors, founders, office-holders drawing a stipend), expose fortunes in the person inspector, and add a fortune-to-treasury ratio diagnostic so personal wealth never silently dwarfs the realm's.
- **Touches:** game/rust/src/person.rs, game/rust/src/economy.rs, game/rust/src/court.rs, game/web/js
- **Gate:** `diagnose economy` shows the fortune-to-treasury ratio staying within a calibrated band across a 200-year sweep on three seeds.

### M147 — Sages and Engineers
- **Intent:** Knowledge stops being anonymous and gets tied to the people who unlocked it.
- **Build:** On each tech unlock in `society.rs`, attribute it to a deterministically chosen eligible sage or engineer person (existing adult of appropriate station in the unlocking culture, promoted to that role if none exists), recording the attribution in the tech-unlock event and the person's deed ledger.
- **Touches:** game/rust/src/society.rs, game/rust/src/person.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** `diagnose society` shows every tech unlock after this phase citing a resolvable sage id, hash-stable attribution across reruns of the same seed.

### M148 — Works Carry Their Maker
- **Intent:** Named inventions and buildings remember whose hands or mind made them, closing the provenance loop artifacts already have.
- **Build:** Extend `artifact.rs`'s provenance chain to include a maker field populated from the M147 sage/engineer or a founder/builder person, surfaced in the artifact inspector and cited in the wonder/discovery chronicle events that already exist.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/person.rs, game/rust/src/chronicle.rs, game/web/js
- **Gate:** `diagnose relics` shows every work-class artifact created after this phase carrying a resolvable maker id, zero maker-less works among the newly created.

### M149 — Masters and Apprentices
- **Intent:** Knowledge doesn't just appear once — it passes hand to hand across generations, and the sim should show the line.
- **Build:** Add an apprenticeship link (master person id, apprentice person id, craft/tech domain) formed when an eligible young adult is deterministically bound to a same-culture sage or engineer, with the apprentice inheriting eligibility to future tech attribution in that domain on the master's death or retirement.
- **Touches:** game/rust/src/person.rs, game/rust/src/society.rs, new: game/rust/src/lineage.rs
- **Gate:** `diagnose society` shows every knowledge-lineage chain acyclic and terminating in a living or dead master, hash-stable across a 300-year sweep.

### M150 — Trait Vector at Birth
- **Intent:** Persons start to differ from each other in ways the sim can act on, deterministically bounded.
- **Build:** Add a fixed-length trait vector (bold/cautious, cruel/just, open/grasping, and reserved slots for M152 heredity) rolled once at birth in `person.rs` from a per-axis normal-ish distribution clamped to a bounded range, seeded from the same derived-RNG discipline as every other draw.
- **Touches:** game/rust/src/person.rs, game/rust/src/util.rs
- **Gate:** `diagnose lives` shows trait values for every person within their clamped bounds and the population distribution matching the configured curve within tolerance across three seeds.

### M151 — Traits Bias Decisions
- **Intent:** Character stops being decorative and starts nudging the choices the sim already makes.
- **Build:** Thread the acting person's trait vector as a bounded multiplier into existing decision rolls the sim already performs — a ruler's war-declaration threshold, a founder's colonize threshold, an explorer's risk tolerance — capping each trait's influence so no single axis can override the base probability outside a documented band.
- **Touches:** game/rust/src/politics.rs, game/rust/src/prospecting.rs, game/rust/src/person.rs
- **Gate:** `diagnose statecraft` and `diagnose prospect` show decision-rate deltas attributable to trait bias staying within the documented cap across a 200-year sweep, hash-stable across reruns.

### M152 — Trait Heredity with Drift
- **Intent:** Character runs in families, loosely, the way real temperament does.
- **Build:** At child birth, blend both parents' trait vectors with a bounded random drift term per axis, keeping the population-level distribution within the same calibrated band M150 established rather than drifting to extremes over generations.
- **Touches:** game/rust/src/person.rs, game/rust/src/household.rs
- **Gate:** `diagnose lives` shows trait-band stability across a 300-year, multi-generation sweep on three seeds, parent-child trait correlation positive but bounded within the documented drift range.

### M153 — Factions Form
- **Intent:** A court stops being one voice and splits along the favor, blood, and grudge lines it has been quietly accumulating.
- **Build:** Add a `Faction` record in a new `faction.rs` clustering court members by correlated favor trajectories, shared kinship, and standing grudges (opinion-matrix entries below a threshold, per politics.rs's existing opinion web), each faction tracking a leader, membership list, and a grievance score that persists across ticks.
- **Touches:** new: game/rust/src/faction.rs, game/rust/src/court.rs, game/rust/src/politics.rs, game/rust/src/person.rs
- **Gate:** `diagnose statecraft` shows every court with more than a threshold of members partitioned into resolvable factions with no person in two factions at once, hash-stable across reruns.

### M154 — Faction Pressure on Unrest
- **Intent:** Court factions stop being scenery and start bending the streets — a losing faction breeds real unrest, not flavor text.
- **Build:** Wire the M153 faction ledger into the M11 unrest ladder as an additive pressure term keyed to faction strength deficit and grievance age, decayed on the same monthly cadence as `OPINION_DECAY`; unrest events cite the pressuring faction and its leading person by id.
- **Touches:** game/rust/src/politics.rs, game/rust/src/event.rs, game/rust/src/state.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose civ` shows unrest correlating with faction-pressure magnitude (Spearman ≥ 0.4 across a 10-seed sweep) and determinism hash includes faction-pressure fields unchanged run to run.

### M155 — Purge and Exile
- **Intent:** Losing a faction fight has teeth: the defeated pay in banishment, and the realm remembers who it cast out.
- **Build:** Add purge/exile resolution triggered when faction pressure crosses a threshold, stripping offices (M132) from losers, relocating or removing them from the court roster, and logging an `Exile` deed on the person's ledger (M135) that later mention-callbacks (M164) can cite; exiles retain their entity id and may resurface in claim wars (M161).
- **Touches:** game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** every purge produces exactly one exile deed and one office vacancy in the same tick, no dangling office-holder ids, hash-stable across reruns at fixed seed.

### M156 — Conspiracy
- **Intent:** The usurper needs backers before a knife: plots are recruited, strained, and resolved deterministically, never scripted.
- **Build:** Model conspiracies as a recruitment graph over kin, faction, and favor ties (extending the Q10 faction substrate) with a secrecy score that decays as backers join, resolved by a bounded random walk against a discovery/success threshold each month; a `Conspiracy` struct records instigator, target, and backer ids in the registry.
- **Touches:** game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/state.rs, new: game/rust/src/conspiracy.rs
- **Gate:** conspiracies never recruit a backer twice or an already-dead person, secrecy monotonically trends downward with backer count across a 5-seed sample, and the state hash folds in conspiracy fields.

### M157 — Assassination
- **Intent:** A plot that reaches the knife's edge has to cut — or fail — with consequences that ripple through court and kin alike.
- **Build:** Resolve mature conspiracies (secrecy exhausted or backer quorum met) into an assassination roll against the target's guard/favor standing, producing either a death (folding into M127 mortality bookkeeping) or a survived attempt that raises the target's dread and burns the instigator's standing; both outcomes emit chronicle events citing every named backer.
- **Touches:** game/rust/src/conspiracy.rs, game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs
- **Gate:** across a 10-seed, 100-year sweep assassination success rate stays within a 15-40% band and every resolved conspiracy leaves no orphaned backer references.

### M158 — Discovery and Trial
- **Intent:** Not every plot ends in blood — some are caught first, and the court's justice is itself a chronicle beat.
- **Build:** Add a discovery branch to conspiracy resolution that can fire before the knife, routing caught plots through a trial resolving guilt by backer count and evidence weight, sentencing losers to the M155 exile machinery or execution, and emitting trial-specific chronicle prose distinct from the assassination beats.
- **Touches:** game/rust/src/conspiracy.rs, game/rust/src/politics.rs, game/rust/src/chronicle.rs, game/rust/src/telling.rs
- **Gate:** discovered-vs-executed conspiracies are mutually exclusive per plot instance in the harness's conspiracy property check, and trial outcomes are deterministic and hash-stable at fixed seed.

### M159 — The Marriage Match
- **Intent:** Marriage between realms is not romance, it is diplomacy, and it should move the numbers that diplomacy moves.
- **Build:** Extend the M129 marriage machinery so inter-realm matches write into the existing opinion matrix (politics.rs) with a magnitude scaled by the match's rank (ruling house vs minor house), and log a `Match` deed on both spouses' person records.
- **Touches:** game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs, game/rust/src/state.rs
- **Gate:** inter-realm matches shift the relevant opinion-matrix cell by a bounded, deterministic amount and the sweep shows no unmatched deed without a corresponding opinion delta.

### M160 — Dowry and Claim
- **Intent:** Brides and grooms carry more than a name across a border — they carry property and a stake in someone else's throne.
- **Build:** Attach a dowry ledger (goods or treasury share, per M146 personal fortunes) and a latent succession claim to each inter-realm match, both traveling with the person entity and surfacing in later inheritance and claim-war resolution.
- **Touches:** game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/economy.rs, game/rust/src/state.rs
- **Gate:** every dowry transfer balances (source treasury debit equals recipient credit within rounding) and claims persist through save/hash round-trips unchanged.

### M161 — Claim War
- **Intent:** A succession claim through blood is a casus belli the sim can actually fight over, not a flavor note.
- **Build:** Wire M160 claims into the M11.3 war-declaration machinery as a distinct war cause with its own legitimacy weighting, resolving a won claim war into an actual succession change via the M138 kin-tree resolver rather than a synthetic transfer.
- **Touches:** game/rust/src/politics.rs, game/rust/src/entity.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** claim wars only fire when a live, unexpired claim exists, and a won claim war always produces a kin-tree-consistent succession event in the same or following tick.

### M162 — Every Event Names Its People
- **Intent:** The chronicle finally speaks in names instead of abstractions — no event happens to no one.
- **Build:** Audit and extend the `event_table!` (world.rs, ADR-0015) and every emission site in chronicle.rs so each event kind that plausibly involves a person carries a mandatory actor/target id field, backfilling war, succession, plot, and deed events that currently omit them.
- **Touches:** game/rust/src/event.rs, game/rust/src/chronicle.rs, game/rust/src/world.rs, game/rust/src/entity.rs
- **Gate:** `diagnose telling` reports zero person-eligible events missing an actor id across a 300-year sweep, and hash_state is unaffected since actor ids were already registry fields.

### M163 — Prose Knows Kinship
- **Intent:** The telling should speak the way people actually do — "his uncle's slayer," "her father's city" — because it knows the family tree.
- **Build:** Add kinship-relative phrase generation to telling.rs that queries the M130/M131 descent graph at render time to produce relational epithets and clauses, selected from a kernel/satellite split (per the tellability digest) so causal clauses stay constrained while relational flavor varies.
- **Touches:** game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/rust/src/politics.rs
- **Gate:** generated kinship phrases match the live descent graph with zero false relations across a property-test corpus of 1000 sampled events, and prose output stays deterministic per seed.

### M164 — Mention-Aware Person Callbacks
- **Intent:** A person met once should never be re-introduced like a stranger — the telling remembers who it has already named.
- **Build:** Extend the M6.8 epithet/callback machinery in telling.rs with a per-person mention-count ledger (folded into narration state, not world state) so first mentions introduce fully and later mentions use earned epithets or kinship callbacks, per the narration-memory model in the tellability digest.
- **Touches:** game/rust/src/telling.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs
- **Gate:** no person entity is described with its full introduction clause twice within a single chronicle rendering, verified by a repetition-detector property test across a 300-year run.

### M165 — Biography Arcs
- **Intent:** A life read end to end should read as a story, not a list — rise, fall, revenge, exile-and-return are shapes the sifter can find.
- **Build:** Implement biography arc detection as Felt-style sifter patterns over each person's deed ledger (rise, fall, revenge, exile-and-return), reusing the M6.5 sifting substrate, and attach a matched-arc tag to the person entity for downstream mythologization and cards.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/entity.rs, new: game/rust/src/biography.rs
- **Gate:** the arc sifter finds at least one instance of each of the four canonical patterns across a 10-seed, 300-year corpus, and matched arcs are reproducible byte-for-byte at fixed seed.

### M166 — Eventfulness by the Weight of Days
- **Intent:** The notable dead deserve ranking by the weight of what they did, not by the order the sim happened to log them.
- **Build:** Score each person's eventfulness as stakes × norm-violation × reversal-flag (per the tellability digest's formula), aggregated from their deed ledger and biography arc tags, and rank the notable dead by this score for downstream cast selection.
- **Touches:** game/rust/src/biography.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs
- **Gate:** eventfulness scores are monotonic under a synthetic deed-injection test (adding a reversal-tagged deed strictly raises score) and stable under reruns at fixed seed.

### M167 — Cast Discipline
- **Intent:** A century should have a cast, not a crowd — named without dedup or name soup drowning the telling in strangers.
- **Build:** Enforce a bounded-notables-per-century cap in chronicle.rs driven by the M166 eventfulness ranking, with deduplication against existing entity names and a fallback demotion path for persons who fall below the cast line into background aggregate demography.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/entity.rs, game/rust/src/biography.rs
- **Gate:** `diagnose telling` enforces a per-century notable-cast band (checked via BANDS) and confirms zero duplicate names among live notables in any single century across a 10-seed sweep.

### M168 — Legends Browser
- **Intent:** The dead stay reachable: a browser lets the entity graph of houses and generations be walked, not just read once in the feed.
- **Build:** Ship a Solid.js legends browser panel rendering genealogies and house trees from the registry's person/entity graph, following the LegendsViewer precedent of rebuilding relations from a flat log rather than a bespoke structure, wired through the existing pack/registry lane (ADR-0015) with no hand-mirrored data.
- **Touches:** game/web/js/gen/, new: game/web/js/legends.js, game/rust/src/entity.rs, game/rust/src/pack.rs
- **Gate:** the browser renders a complete acyclic tree for every dynasty in a 300-year world with no missing or duplicated nodes, checked against the registry by a headless diff script.

### M169 — Person Cards
- **Intent:** Click a name, get a life — deeds, kin, works, and tomb in one legible card, the inspector's answer to a person.
- **Build:** Add a person-card component to the inspector UI pulling deed ledger (M135), kinship (M131), authored works (M148), and tomb/epitaph (M137) from the registry into one composed view, reusing the generated types from `genjs` rather than hand-typed shapes.
- **Touches:** game/web/js/legends.js, game/web/js/gen/, game/rust/src/bin/genjs.rs, game/rust/src/entity.rs
- **Gate:** every live person entity in a sampled world renders a card with no missing required field and no console error, checked by a headless render script over a 5-seed sample.

### M170 — Mythologization of the Dead
- **Intent:** Time blurs the notable dead into legend, the same drift M6.9 gave to places now reaching lives.
- **Build:** Extend the M6.9 legend-layer mechanism to person entities, retelling old deeds with accreted embellishment proportional to years-since-death and eventfulness score, replacing precise deed text with mythologized variants in the chronicle feed while the registry keeps the ground truth untouched.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/entity.rs
- **Gate:** mythologized retellings never alter the underlying deed record (registry diff is empty) and drift magnitude increases monotonically with elapsed time in a synthetic aging test.

### M171 — Tombs, Monuments, and Name Strata
- **Intent:** A famous life leaves a mark on the land itself — the map should carry the names the dead earned.
- **Build:** Extend the M9 naming-strata machinery so tombs and monuments (M137) seed place-name candidates keyed to the interred person's epithet, feeding the existing naming layers with a new person-derived stratum ranked below settlement-founding names.
- **Touches:** game/rust/src/naming.rs, game/rust/src/entity.rs, game/rust/src/artifact.rs
- **Gate:** every tomb/monument entity produces exactly one candidate name in its stratum, deterministic at fixed seed, with no collision against existing settlement names in a 10-seed sweep.

### M172 — Relics with Provenance
- **Intent:** An object a notable once carried should carry their story forward — from hand to hand, war to tomb.
- **Build:** Add person-owned relic artifacts to artifact.rs with a provenance chain (owner history as an ordered list of person/event ids) updated on transfer, gift, theft, or burial, feeding both the chronicle ("the crown carried off in the Salt War") and person cards.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/entity.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** every relic's provenance chain is strictly time-ordered and references only living-at-the-time owners, verified by a property test over a 300-year run.

### M173 — Sites of Memory
- **Intent:** Where the famous fell should be marked and named, so the map remembers battles the way it remembers birthplaces.
- **Build:** Generate site-of-memory features at the location of notable deaths (battlefield fates from M142, assassinations from M157) using the existing feature/ruin entity kind, named through the M171 person-derived naming stratum and cross-linked from the person's card.
- **Touches:** game/rust/src/entity.rs, game/rust/src/naming.rs, game/rust/src/chronicle.rs
- **Gate:** every notable death that qualifies (battlefield or assassination) produces exactly one sited, named feature entity, reproducible byte-for-byte at fixed seed.

### M174 — The Honest Census
- **Intent:** The cast must answer to demography — a world of a thousand souls cannot field a hundred generals.
- **Build:** Calibrate notable-population bands against town sizes and era, and lifespan/mortality curves (M127) against medieval demographic literature, adding `BANDS` entries in the diagnostics harness for notable-density-per-capita and life-expectancy-by-station.
- **Touches:** game/rust/src/entity.rs, game/rust/src/bin/diagnose.rs, game/rust/src/politics.rs
- **Gate:** `diagnose civ` reports notable density and life expectancy within calibrated bands across a 10-seed, 300-year sweep with zero FAIL and at most WARN at the tails.

### M175 — Era III Gate
- **Intent:** The Named Lives closes only when centuries of persons hold together end to end, provably, not just plausibly.
- **Build:** Add a `diagnose lives` runner performing a 300-year sweep that checks person registry integrity, biography arc coverage, cast discipline bands, and kinship acyclicity in one pass, matching the era-gate pattern of prior eras (`diagnose era`).
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/entity.rs, game/rust/src/politics.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose lives` passes across a 10-seed, 300-year sweep with biography counts inside the M174 bands and the full suite (`report.sh`) green.

### M176 — Recast the Person Registry
- **Intent:** Fifty phases of persons strained `entity.rs` past its original shape — the forge re-cuts it with what the era taught.
- **Build:** Refactor `entity.rs` to separate person-specific fields (kin refs, offices, deeds, traits) from the generic entity envelope into a dedicated person table, landing as pure structural change with an ADR documenting the split and its rejected alternatives.
- **Touches:** game/rust/src/entity.rs, game/rust/src/politics.rs, game/rust/src/chronicle.rs, new: docs/adr (person-table split ADR, numbered at land time)
- **Gate:** determinism hash is bit-identical before and after the refactor across a 5-seed sweep, and full suite is green with no behavior change.

### M177 — Person Tables into Registry Codegen
- **Intent:** Person kinds, deeds, and offices stop being hand-mirrored and join the single-declaration discipline the rest of the engine already lives by.
- **Build:** Declare person kinds, deed kinds, and office kinds in registry-style tables beside `field_registry!`/`event_table!` (ADR-0015) so `genjs` derives the JS-side vocabulary (kind lists, filter options) from the same source, deleting any hand-kept UI copies.
- **Touches:** game/rust/src/world.rs, game/rust/src/entity.rs, game/rust/src/bin/genjs.rs, game/web/js/gen/
- **Gate:** a grep-based drift check finds zero hand-written duplicates of person/deed/office vocabulary in game/web/js, and genjs output is byte-stable across repeated runs.

### M178 — Person Pack and UI Lanes
- **Intent:** A full cast of hundreds of persons has to reach the browser without breaking the pack budget or the delta-tick discipline.
- **Build:** Extend the pack protocol (ADR-0007) with a person-section delta lane so only changed persons transmit per tick rather than the full roster, and update the legends browser (M168) and person cards (M169) to consume incremental updates.
- **Touches:** game/rust/src/pack.rs, game/web/js/net.js, game/web/js/legends.js, game/web/js/worker.js
- **Gate:** at 300+ tracked persons the per-tick payload for unchanged-cast months stays within the existing pack budget band, verified by `diagnose perf`.

### M179 — Kinship and Reference Integrity at Era Grade
- **Intent:** Every kin edge and every id reference in the person graph must hold at the scale the era actually reached, not just in small tests.
- **Build:** Add ERA-grade property checks for kinship acyclicity, spouse symmetry, and reference integrity (no orphaned offices, deeds, or claims) plus a metamorphic lane asserting that war-heavy years never produce fewer commander deaths than peace years of comparable length.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/politics.rs, game/rust/src/entity.rs
- **Gate:** the property suite finds zero kinship or reference-integrity violations and the war-years metamorphic check holds across a 10-seed, 500-year sweep.

### M180 — Registry Performance Bands at Five Centuries
- **Intent:** The person registry must stay cheap in memory and tick time even after five hundred years pile names onto the world.
- **Build:** Profile and band memory footprint and per-tick cost of the person registry at a 500-year, seed-swept run, extending `cmd_perf`/`cmd_systems` with person-specific measurements and setting `BANDS` thresholds that gate future person-adding phases.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/entity.rs, game/rust/src/systems.rs
- **Gate:** person-registry memory and tick-cost stay within newly declared sweet bands at 500 simulated years across a 5-seed sweep, with full suite green and determinism hash unchanged.
