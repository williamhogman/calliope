# Era VI — The Unseen Order (M291–M345)

Full four-field specs for Era VI of `../ROADMAP-500.md`: faiths with
tenets and mechanical stakes, sacred ground and temple economies,
conversion along the roads, schisms and holy wars, myth generation and
bounded drift over the two-layer telling, saints, relics, and unbelief
— closed by Forge VI (M341–M345), which recasts belief and culture
state. The one-liners in the parent file are binding; these specs
expand them.

### M291 — The Faiths Given Tenets
- **Intent:** Belief stops being palette flavor and becomes an entity with stakes, growing the M3.5 pantheon seeds into faiths that can act.
- **Build:** Introduce a `Faith` entity registered in `entity.rs` (`EntityKind::Faith`) carrying a tenet set drawn from the existing `God` domains in `culture.rs`, a founding culture link, and a doctrine vector (asceticism, orthodoxy, syncretism-tolerance) sampled with the power-law weighting already used in `make_word`; write the religion ADR documenting the tenet-vector design and its relation to the M3.5 pantheon.
- **Touches:** game/rust/src/culture.rs, game/rust/src/entity.rs, game/rust/src/event.rs, new: docs/adr (faiths-as-entities ADR, numbered at land time), new: game/rust/src/religion.rs
- **Gate:** `diagnose culture` reports at least one faith per surviving culture by tick 100 and every faith's tenet vector round-trips through the determinism hash unchanged across identical seeds.

### M292 — The Sacred Ground
- **Intent:** Faiths need a place to point to; holy sites root belief in the same terrain the geologists already generated.
- **Build:** For each faith, select one to three holy sites from existing landform data (peaks from `geo.rs` elevation extrema, springs from `hydrology.rs` source cells, groves from `biomes.rs` forest patches) weighted by proximity to the founding culture's territory, and record them as `Entity` sites with a `sacred_since` tick.
- **Touches:** game/rust/src/religion.rs, game/rust/src/geo.rs, game/rust/src/hydrology.rs, game/rust/src/biomes.rs, game/rust/src/entity.rs
- **Gate:** every faith with a founding culture owns at least one holy site whose coordinates match a real extremum or patch in the generated terrain, verified by `diagnose culture --sites`.

### M293 — The Belief Ledger
- **Intent:** Faith state must be as trustworthy as terrain state before anything is built atop it.
- **Build:** Fold faith and holy-site tables into the world determinism hash alongside the existing culture and society hashes, and add a monthly cadence check so faith mutation events (founding, tenet drift) fire only within their designed bands rather than every tick.
- **Touches:** game/rust/src/religion.rs, game/rust/src/state.rs, game/rust/src/systems.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `report.sh` shows faith-state hash identical across three repeated runs of the same seed and mutation-event cadence within the configured band on a 50-seed sweep.

### M294 — Rite and Taboo
- **Intent:** Belief must touch the calendar, the table, and the battlefield, not sit inert beside them.
- **Build:** Attach mechanical hooks to each faith's tenet vector: festival days that shift the existing calendar cadence, dietary taboos that modify `agriculture.rs` consumption weights, and war taboos that adjust `politics.rs` casus-belli odds; each hook reads the tenet vector so effect magnitude is derived, not hardcoded per faith.
- **Touches:** game/rust/src/religion.rs, game/rust/src/agriculture.rs, game/rust/src/politics.rs, game/rust/src/systems.rs
- **Gate:** `diagnose culture --rites` shows measurable deltas in consumption and war-odds correlated with tenet strength on at least three faiths per sweep, reproducible byte-for-byte on reseed.

### M295 — Treasuries of the Faithful
- **Intent:** Sacred sites must accumulate wealth and staff the way market towns accumulate trade, so religion has an economy worth fighting over.
- **Build:** Extend holy sites into temples with a treasury ledger fed by tithe flows drawn from nearby settlement wealth (mirroring `economy.rs` flow accounting), and staff them with clergy counts that scale with temple treasury via a logistic saturation curve consistent with the settlement population models.
- **Touches:** game/rust/src/religion.rs, game/rust/src/economy.rs, game/rust/src/settlements.rs
- **Gate:** temple treasuries never go negative, clergy counts track treasury within the calibrated logistic band, and the delta-clean payload for temple state matches `pack.rs` codegen expectations.

### M296 — The High Priests Take Their Seats
- **Intent:** Belief needs faces; clergy join generals and merchants as notables the chronicle can name and track.
- **Build:** Register high-priest notables as `Entity` persons tied to their temple, drawn into the Era III notable-cast machinery already used for generals and magnates, granting them epithet eligibility and mention tracking via `Registry::earn_epithet` and `Registry::mention`.
- **Touches:** game/rust/src/entity.rs, game/rust/src/religion.rs, game/rust/src/chronicle.rs
- **Gate:** every staffed temple has exactly one living high-priest entity at all times, succession on death is immediate and hash-stable, and `diagnose culture --clergy` confirms zero orphaned temples across a 100-seed sweep.

### M297 — The Word Travels the Road
- **Intent:** Faith should diffuse the way culture already diffuses, so conversion looks like geography, not dice.
- **Build:** Model conversion probability along trade roads and court contacts using the Axelrod local-imitation-with-homophily rule already cited for culture assignment in `culture.rs`, with settlement-level faith-share state updated monthly and weighted by road distance from `trade.rs` route costs.
- **Touches:** game/rust/src/culture.rs, game/rust/src/religion.rs, game/rust/src/trade.rs, game/rust/src/systems.rs
- **Gate:** faith-share fields sum to one per settlement at every tick and conversion rate falls monotonically with road distance from the nearest temple across a 50-seed sample.

### M298 — Gods Borrowed and Merged
- **Intent:** Where faiths meet at length they should blend, echoing the URR permeation lesson that coherent contact produces shared symbols.
- **Build:** When two faiths' settlement-level shares both exceed a contact threshold for a sustained duration, merge shared tenets into a syncretic offshoot faith that inherits holy sites proportionally, using the same tenet-vector averaging math introduced in M291.
- **Touches:** game/rust/src/religion.rs, game/rust/src/culture.rs, game/rust/src/event.rs
- **Gate:** syncretic faiths never exceed the sum of their parents' holy-site counts and syncretism events are deterministic and logged once per qualifying contact pair per sweep.

### M299 — Reading the Faith Map
- **Intent:** The diagnostics harness must be able to see belief spread the way it already sees trade and culture spread.
- **Build:** Add faith-map diagnostics that trace conversion routes and conquest-driven faith changes over the sweep window, banding expected conversion velocity against road density and war frequency the way existing culture-spread diagnostics band against Axelrod zone counts.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/religion.rs
- **Gate:** `diagnose faith --spread` reports conversion velocity within its calibrated band on 50 seeds with zero unexplained faith-share jumps outside conquest or syncretism events.

### M300 — The Rite of Coronation
- **Intent:** Kingship should need blessing, tying the crown's legitimacy to the temple's favor.
- **Build:** Add a coronation-rite modifier feeding the existing M4/M11 legitimacy calculation, weighted by the ruling faith's tenet alignment with the crown and by whether the coronation used the culturally sanctioned holy site.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/src/chronicle.rs
- **Gate:** legitimacy deltas from coronation rites stay within the existing legitimacy band and reproduce identically on reseed with the same faith-crown alignment inputs.

### M301 — Crown Against Cassock
- **Intent:** Wealthy temples and threatened crowns must be able to collide, giving investiture its own conflict class.
- **Build:** Introduce an investiture-conflict casus belli in `politics.rs` triggered when temple treasury growth outpaces treasury income for the ruling dynasty, with outcomes redistributing legitimacy and treasury between crown and temple according to the conflict's resolution.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/src/economy.rs, game/rust/src/event.rs
- **Gate:** investiture conflicts fire only when the treasury-divergence threshold is crossed and resolve with conserved total wealth across crown and temple ledgers, verified by `diagnose culture --investiture`.

### M302 — The Coupling Holds
- **Intent:** Religion and politics must move together without either destabilizing the other across the full sweep.
- **Build:** Run the combined legitimacy-investiture-conversion loop through the standing 300-year sweep and tune coupling coefficients until legitimacy variance and investiture-conflict frequency stay within the bands established since M4/M11.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` shows legitimacy variance and investiture-conflict frequency within band on a 100-seed, 300-year sweep with no diverging seeds.

### M303 — Houses That Keep the Word
- **Intent:** Monastic orders give faith an institution that preserves knowledge, joining Era V's literacy work to the temple economy.
- **Build:** Add monastic-house entities attached to temples that consume clergy and treasury to raise a local literacy modifier (reusing Era V's literacy fields) and to clear adjacent land via the existing agriculture expansion rules, with houses tracked as long-lived `Entity` sites that copy texts into the chronicle's record.
- **Touches:** game/rust/src/religion.rs, game/rust/src/agriculture.rs, game/rust/src/chronicle.rs, game/rust/src/entity.rs
- **Gate:** monastic-house literacy contribution matches the calibrated Era V literacy band and land cleared by houses is bounded by their treasury-funded labor allotment, hash-stable across reseed.

### M304 — The Pilgrim's Road
- **Intent:** Holy sites should pull traffic, letting pilgrimage carry both trade and plague as Era IV's disease model predicts.
- **Build:** Generate pilgrimage routes to holy sites layered onto existing `trade.rs` road infrastructure, adding a pilgrim-flow volume that boosts trade goods along the route and feeds the Era IV contagion model's contact-rate term at pilgrimage waypoints.
- **Touches:** game/rust/src/trade.rs, game/rust/src/religion.rs, game/rust/src/famine.rs
- **Gate:** pilgrim-flow volume correlates positively with holy-site prestige and contagion contact-rate spikes measurably at pilgrimage waypoints without breaking existing plague-band calibration.

### M305 — Orders in Cadence
- **Intent:** The new institutions must not destabilize tick pacing or drift outside their designed rhythm.
- **Build:** Diagnose monastic-house growth and pilgrimage-flow cadence across the sweep, banding house founding rate against clergy population and pilgrimage volume against holy-site prestige growth.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/religion.rs
- **Gate:** `diagnose faith --orders` shows house-founding and pilgrimage-volume cadence within band on a 100-seed sweep with zero runaway growth seeds.

### M306 — The Doctrine Cracks
- **Intent:** Faiths under strain should split the way real churches split, driven by distance, politics, and the memory of plague.
- **Build:** Implement schism mechanics where doctrinal distance accumulates from geographic separation, ruling-dynasty interference, and unresolved plague-theodicy tension (deaths attributed to the faith's inaction), triggering a schism event that spawns a daughter faith inheriting a tenet-vector perturbation once accumulated distance crosses a threshold.
- **Touches:** game/rust/src/religion.rs, game/rust/src/politics.rs, game/rust/src/famine.rs, game/rust/src/event.rs
- **Gate:** schism events fire only after threshold crossing, daughter-faith tenet vectors differ from the parent by a bounded perturbation, and the schism log is byte-identical across reseeds.

### M307 — Heresy Kept or Crushed
- **Intent:** A schism is not the end of the story; the daughter faith must live, die, or be stamped out with consequences.
- **Build:** Give daughter faiths a suppression track resolved by the ruling dynasty's investiture posture and military capacity from `politics.rs`, with two resolutions — suppression that folds believers back to the parent, or triumph that lets the heresy stand as an independent faith with its own holy sites.
- **Touches:** game/rust/src/religion.rs, game/rust/src/politics.rs, game/rust/src/chronicle.rs
- **Gate:** every heresy resolves to exactly one of suppression or triumph within its designed window, never left indefinitely pending, verified on a full sweep.

### M308 — The Schism Diagnosed
- **Intent:** Splits must be visible, calibrated, and provably reversible before they're trusted to run unattended.
- **Build:** Add schism diagnostics tracking split frequency, threshold-crossing distribution, and suppression-versus-triumph ratio, banding all three against historical schism-rate envelopes noted in the research digests.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/religion.rs
- **Gate:** `diagnose faith --schism` shows split frequency and resolution ratio within the calibrated band on 100 seeds with both suppression and triumph outcomes observed.

### M309 — The Line Between Gods
- **Intent:** Belief needs its own axis in the diplomatic graph so faith itself can be a reason nations go to war.
- **Build:** Add a faith-distance edge to the existing opinion matrix alongside culture and dynasty edges, and introduce a crusade-class coalition-war type in `politics.rs` triggered when faith-distance exceeds a threshold between neighboring polities with a shared border tension.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/src/event.rs
- **Gate:** crusade-class wars only trigger above the faith-distance threshold and coalition membership matches faith alignment for at least ninety percent of participants across a 100-seed sweep.

### M310 — Convert by Sword or by Road
- **Intent:** Conquest should carry a faith policy choice with visible aftershocks, not silently overwrite the losing side's belief.
- **Build:** Give conquering polities a forced-conversion-versus-tolerance policy option resolved at annexation, where forced conversion spikes unrest and rebellion odds in `politics.rs` while tolerance slows conversion but preserves stability, both consuming the same faith-share update machinery from M297.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/src/entity.rs
- **Gate:** forced conversion measurably raises rebellion probability relative to tolerance policy on annexed provinces across a 100-seed sweep, reproducibly under the same policy choice.

### M311 — The Holy War Calibrated
- **Intent:** Crusades must occur at a rate that reads as history, not as spam or as silence.
- **Build:** Tune crusade-trigger thresholds and faith-distance decay against the historical religious-war frequency envelope from the history-narrative digest, running the full sweep to converge on a stable frequency band.
- **Touches:** game/rust/src/politics.rs, game/rust/src/religion.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` shows crusade-class war frequency within the calibrated historical envelope across a 100-seed, 300-year sweep.

### M312 — The Sacred Year
- **Intent:** The calendar itself should belong to the gods, with feast cycles and leap corrections structuring the civil year.
- **Build:** Build a feast-cycle calendar per faith with intercalation rules correcting drift against the solar year already tracked by `climate.rs`, exposing feast days as scheduling anchors consumed by the M294 rite hooks.
- **Touches:** game/rust/src/religion.rs, game/rust/src/climate.rs, game/rust/src/systems.rs
- **Gate:** feast-cycle drift against the solar year stays within one day per century after intercalation correction, verified deterministically across a 300-year sweep.

### M313 — Omens Read Into Power
- **Intent:** The heavens and the plagues should speak through priesthoods, turning natural events into political ones.
- **Build:** Institutionalize omen readings where eclipses, comets, and plague onsets recorded by `climate.rs` and `famine.rs` are interpreted by the nearest temple's clergy, producing a legitimacy or unrest modifier scaled by the temple's prestige and the omen's rarity.
- **Touches:** game/rust/src/religion.rs, game/rust/src/climate.rs, game/rust/src/famine.rs, game/rust/src/politics.rs
- **Gate:** omen-driven legitimacy or unrest deltas scale monotonically with temple prestige and omen rarity, reproducing identically across reseed for the same event sequence.

### M314 — Prophecy Kept a Record
- **Intent:** Priesthoods should make claims the world can later judge, applying the Berúthiel discipline of tracked prediction to divination.
- **Build:** Generate priest prophecies tied to specific future conditions (harvest, war outcome, ruler's fate) and record each prediction's eventual truth-value against the ground-truth log, feeding an accuracy score per temple that the chronicle can cite.
- **Touches:** game/rust/src/religion.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** every issued prophecy resolves to a recorded true or false outcome by its due tick with zero unresolved prophecies at sweep end, hash-stable across reseed.

### M315 — Myths Out of History
- **Intent:** Every faith deserves its own origin, flood, and founder tales, grown from what actually happened in the world.
- **Build:** Generate a myth corpus per faith mining the chronicle's structured event log for origin candidates (founding culture's earliest events), flood or catastrophe candidates (famine and climate extremes), and founder candidates (the faith's first named clergy), rendering them through a mythologizing template layer distinct from the ground-truth log per the Qud lesson.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/religion.rs, new: game/rust/src/myth.rs
- **Gate:** every faith has exactly one origin, flood, and founder myth citing a real ground-truth event id, and the myth corpus is byte-identical across reseed of the same world.

### M316 — The Telling Drifts
- **Intent:** Myths should mutate the way oral history really does, mechanically, on a generational clock rather than at random.
- **Build:** Add a myth-drift process that mutates retellings on a generational cadence tied to clergy succession, applying bounded rewrites to non-core myth fields (embellishment, attributed cause) while leaving the ground-truth ledger untouched.
- **Touches:** game/rust/src/myth.rs, game/rust/src/religion.rs, game/rust/src/entity.rs
- **Gate:** myth drift fires only at clergy-succession ticks, each drift step changes at most the designed field subset, and drift history is deterministic and hash-covered.

### M317 — The Motif Holds
- **Intent:** Drift must never erase what makes a myth recognizable; the core has to survive while the edges wander.
- **Build:** Define a core-motif invariant per myth type (the founder's name, the catastrophe's location) that drift steps are forbidden to alter, and add a bounds check comparing drifted variant distance from the original against a calibrated band.
- **Touches:** game/rust/src/myth.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose faith --myth-drift` confirms zero core-motif violations and variant distance stays within band across a 100-seed, multi-generation sweep.

### M318 — Two Tellings, Full Depth
- **Intent:** The ground truth and the legend must now stand as fully separate, comparable layers, completing M6.9's promise.
- **Build:** Finalize the two-layer telling architecture so every myth and chronicle entry carries both a ground-truth event reference and a rendered legend string, with a divergence metric quantifying how far the legend has drifted from the ground truth at any tick.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/myth.rs, game/rust/src/event.rs, game/rust/src/pack.rs
- **Gate:** every rendered legend resolves to a valid ground-truth reference, the divergence metric is computable and hash-stable, and `diagnose faith --two-layer` passes on a full 300-year sweep.

### M319 — Saints and Heroes
- **Intent:** The dead notables of Era III become mythic figures whose remembered deeds swell measurably past their recorded ones.
- **Build:** Canonize eligible dead notables (rulers, clergy, generals) per faith tenet thresholds into a `Saint` record referencing the ground-truth `Ruler`/notable id, then run `telling.rs`'s `legendize` over their event chain to produce an inflation delta scored against the base `eventfulness` weight; canonization triggers on death, martyrdom, or miracle-attribution events drawn from the existing event log.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/culture.rs, game/rust/src/event.rs, new: game/rust/src/canon.rs
- **Gate:** every canonized saint's legend-layer deed count exceeds its ground-truth count by a bounded, logged multiplier and the determinism hash covers the canon table across a full run.

### M320 — Relic Cults
- **Intent:** Era III relics accrue pilgrimage traffic once claimed by a cult, and false relics enter circulation alongside the real ones.
- **Build:** Extend the artifact custody chain (per 05-history-narrative's provenance model) with a `RelicCult` link binding artifact id, temple, and pilgrimage-route weight, plus a forgery generator that mints duplicate relic claims scored by distance-decayed plausibility against the true custody chain; forged and true relics both feed festival and pilgrimage mechanics identically until audited.
- **Touches:** game/rust/src/artifact.rs, game/rust/src/culture.rs, game/rust/src/chronicle.rs, new: game/rust/src/relic.rs
- **Gate:** forged-relic count stays within a fixed ratio band of true-relic count per sweep and custody-chain plus forgery state hash-stable across reruns.

### M321 — Sacred Toponymy
- **Intent:** The map itself remembers its gods, layering god-names and saint-names over the physical strata the naming system already tracks.
- **Build:** Extend the M9 naming-strata pipeline with a sacred-name layer that assigns god or saint epithets to holy mountains, springs, groves, and temple sites chosen in M292, respecting existing settlement and biome name conventions; naming draws from the `God` struct's identity fields so citation stays coherent per the URR permeation principle.
- **Touches:** game/rust/src/naming.rs, game/rust/src/culture.rs, game/rust/src/geo.rs, new: game/rust/src/sacred_names.rs
- **Gate:** every holy site carries a stable sacred name traceable to its patron deity or saint and the name table is included unchanged in the determinism hash across reruns.

### M322 — Temple Archaeology
- **Intent:** A faith's fall leaves stone the next faith inherits, so sanctuaries carry a stacked history of belief the way real ruins do.
- **Build:** On temple abandonment or schism-driven desecration, demote the site to a `RuinedSanctuary` record retaining its sacred-name layer and prior patron, and let successor faiths reconsecrate the same site with a reuse bonus over building fresh, mirroring the layered-strata pattern from settlements.rs's ruin handling.
- **Touches:** game/rust/src/culture.rs, game/rust/src/settlements.rs, game/rust/src/geo.rs, game/rust/src/sacred_names.rs
- **Gate:** reconsecrated sites show measurably lower founding cost than fresh temples and the ruin-to-reuse chain is deterministic and hash-stable across seeds.

### M323 — Myth-Map Coherence
- **Intent:** Every sacred site's told story must still answer to the ground it stands on, closing the loop between legend and terrain.
- **Build:** A coherence checker cross-references each sacred site's myth text (origin tale, saint deed, relic claim) against its geo record — elevation, biome, founding date, custody chain — flagging and auto-repairing drifted claims before they enter the telling layer, following the two-layer discipline from M6.9/M318.
- **Touches:** game/rust/src/telling.rs, game/rust/src/geo.rs, game/rust/src/culture.rs, game/rust/src/sacred_names.rs
- **Gate:** the diagnostics harness reports zero unresolved myth-map contradictions across a full sweep and every repair is logged and hash-covered.

### M324 — Unbelief
- **Intent:** Faith is not a ratchet; late-tech societies can doubt their gods and let temples empty.
- **Build:** Introduce a skepticism variable driven by literacy, urbanization, and famine-theodicy strain (per Era V literacy and Era IV plague joins), producing decline arcs in temple attendance and clergy headcount that can partially reverse under crisis-driven revival, modeled as a bounded stochastic process seeded from the existing culture RNG stream.
- **Touches:** game/rust/src/culture.rs, game/rust/src/society.rs, game/rust/src/economy.rs, new: game/rust/src/unbelief.rs
- **Gate:** temple attendance trends downward in high-literacy, low-crisis regions within a calibrated band and skepticism state is folded into the determinism hash.

### M325 — Faith-Count Trajectory
- **Intent:** The pantheon of faiths must breathe — born, split, and extinguished — not merely accumulate.
- **Build:** Track a per-seed faith-count time series across birth (M291), schism (M306), and death (temple collapse from M324) events, exposing the trajectory through the diagnostics harness alongside the existing population and culture-count series already tracked in systems.rs.
- **Touches:** game/rust/src/systems.rs, game/rust/src/culture.rs, game/rust/src/unbelief.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose` reports faith-count time series with both net growth and net decline present across a 300-year sweep, hash-stable.

### M326 — Bounded Monofaith
- **Intent:** History must resist the single flattening story of one god conquering the whole map.
- **Build:** Calibrate conversion, schism, and unbelief rates jointly so that no single faith saturates the seed's population beyond the historical envelope, tuning the Axelrod-gradient conversion constants from M297 against the new unbelief and schism counter-pressures.
- **Touches:** game/rust/src/culture.rs, game/rust/src/unbelief.rs, game/rust/src/constants.rs, game/rust/scripts/report.sh
- **Gate:** across a swept batch of seeds, no more than eighty percent show a single faith exceeding half the population by run's end.

### M327 — Faith Layers in the Atlas
- **Intent:** The observer needs to see belief the way they already see terrain and trade — as a map layer, not a table.
- **Build:** Add religion-map, pilgrimage-route, and holy-site-marker render layers to the wgpu fullscreen-shader renderer, driven by the culture and relic state already computed, following the existing atlas layer-toggle pattern.
- **Touches:** game/rust/src/render.rs, game/web/js, game/rust/src/culture.rs, game/rust/src/relic.rs
- **Gate:** all three new atlas layers render at the existing frame-budget target with pixel-stable output for a fixed seed and camera.

### M328 — The Inspector Reads Belief
- **Intent:** Clicking a town should surface its faith the way it already surfaces its ruler and trade goods.
- **Build:** Extend the town inspector card with faith affiliation, temple presence, feast-day calendar entry, and patron saint/god fields, sourced directly from the culture and canon state without duplicating it, per ADR-0016's single-source pack discipline.
- **Touches:** game/web/js, game/rust/src/explain.rs, game/rust/src/culture.rs, game/rust/src/canon.rs
- **Gate:** every settlement's inspector card renders its faith fields consistently with the underlying culture state and the explain payload is byte-identical across reruns of the same seed.

### M329 — The Chronicle Weaves the Unseen
- **Intent:** Omens, festivals, and schisms deserve prose telling, not silent state changes.
- **Build:** Extend `chronicle.rs`'s monthly event emission with omen-reading, festival, and schism-class templates scored by the eventfulness metric from 15-tellability-chronicle-prose.md, feeding the sift patterns in `telling.rs` so these events can be lifted into microstories alongside wars and successions.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/culture.rs, game/rust/src/event.rs
- **Gate:** omen, festival, and schism prose appears at the calibrated cadence per era-year and the chronicle log stays hash-stable across reruns.

### M330 — Legends Browser
- **Intent:** An observer should be able to compare a faith's saints, myth variants, and schisms the way they already browse dynasties.
- **Build:** Build a legends-browser UI panel listing faith trees, canonized saints, and myth-variant lineages side by side, reading the entity-graph shape recommended by 05-history-narrative.md rather than a flat log, reusing the existing browser component scaffolding.
- **Touches:** game/web/js, game/rust/src/canon.rs, game/rust/src/culture.rs, game/rust/src/telling.rs
- **Gate:** the browser resolves every displayed saint and myth variant back to its ground-truth entity id with zero orphaned references across a test seed.

### M331 — Sifter Patterns for the Unseen
- **Intent:** The schism, the holy war, and the prophecy kept or broken deserve their own sift patterns, not generic war/succession ones.
- **Build:** Add three new Felt-style query patterns to `telling.rs`'s `sift` function — schism-arc, holy-war-coalition, and prophecy-outcome — each scored by the existing weight/fortune functions and surfaced in the legends browser and chronicle feed.
- **Touches:** game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/web/js
- **Gate:** each of the three new patterns fires at least once across a calibrated multi-seed batch and sift output remains deterministic for a fixed seed.

### M332 — Belief Joins the Registry
- **Intent:** Faith state must be a first-class citizen of the pack pipeline, not a hand-mirrored side table.
- **Build:** Move `God`, `Culture` faith fields, `Saint`, `RelicCult`, and unbelief state into the field registry with generated pack v2 layout and delta-tick coverage, following ADR-0016's quantized-CRC payload scheme and eliminating any hand-written mirror structs.
- **Touches:** game/rust/src/pack.rs, game/rust/src/culture.rs, game/rust/src/canon.rs, game/rust/src/relic.rs, game/rust/src/unbelief.rs
- **Gate:** registry codegen produces zero hand-mirrored belief fields and pack round-trip tests reproduce identical state across delta and full-snapshot paths.

### M333 — Tick Budgets Hold With Living Faiths
- **Intent:** The era's new belief machinery must not blow the tick budget that ADR-0009 gates every change against.
- **Build:** Profile and optimize the faith, canon, relic, and unbelief update passes added since M291 against the existing tick-budget bands, hoisting per-frame recomputation into cached deltas where the harness flags regressions.
- **Touches:** game/rust/src/systems.rs, game/rust/src/culture.rs, game/rust/src/canon.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` shows tick time in-band with the full belief stack active across the standard benchmark seed set.

### M334 — Metamorphic Belief
- **Intent:** The doctrine that contact breeds syncretism and isolation breeds divergence must hold as a provable invariant, not a hoped-for tendency.
- **Build:** Write metamorphic property tests pairing seed variants — one with roads/trade opened between two faiths, one sealed — asserting the opened pair's syncretism score (M298) exceeds the sealed pair's, and the sealed pair's rite-divergence score exceeds the opened pair's.
- **Touches:** game/rust/src/culture.rs, game/rust/src/bin/diagnose.rs, new: game/rust/tests/belief_metamorphic.rs
- **Gate:** the metamorphic property suite passes across all sampled seed pairs with no exceptions in either direction.

### M335 — Myth-Drift Properties
- **Intent:** The drift bounds promised in M317 must be proven properties, exercised across every faith's generational retelling.
- **Build:** Extend the property suite with generational myth-drift tests asserting core motifs (per M317's conserved-motif set) survive N generations of `legendize`-driven retelling while surface details vary within the calibrated band, running across a sampled batch of faiths and seeds.
- **Touches:** game/rust/src/telling.rs, game/rust/src/culture.rs, new: game/rust/tests/myth_drift_properties.rs
- **Gate:** every sampled myth retains its conserved motif set after the maximum drift horizon and surface variance stays within the calibrated band across all sampled runs.

### M336 — Calibration Against History
- **Intent:** Conversion speed, schism rate, and temple-economy flows must answer to historical envelopes, not house intuition.
- **Build:** Tune conversion-gradient constants (M297), schism-trigger thresholds (M306), and temple treasury flow rates (M295) against the historical-envelope bands established for prior eras' calibration phases, using the same envelope-fitting method as M311's religious-war calibration.
- **Touches:** game/rust/src/culture.rs, game/rust/src/constants.rs, game/rust/scripts/report.sh
- **Gate:** conversion rate, schism frequency, and temple-wealth growth all land inside their historical envelope bands across the calibration seed batch.

### M337 — Every Faith Its Own
- **Intent:** Two seeds' faiths must read as distinct religions, not palette swaps of one template.
- **Build:** Run the oatmeal-VI cross-seed distinctiveness audit over tenet sets, sacred-name choices, myth motifs, and schism histories, scoring pairwise seed similarity and flagging any pair whose faiths converge past the era's oatmeal threshold.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/culture.rs, game/rust/src/sacred_names.rs, game/rust/scripts/report.sh
- **Gate:** pairwise faith-similarity scores across the oatmeal-VI seed batch stay below the era's convergence threshold for every pair.

### M338 — Property Suite Across the Belief Lanes
- **Intent:** The era's belief mechanics need one consolidated property suite standing guard, not scattered ad hoc tests.
- **Build:** Consolidate the M334/M335 metamorphic and drift properties with new invariants for canonization eligibility, relic-forgery ratio bounds, and unbelief monotonicity-under-crisis into a single belief property lane runnable from the harness.
- **Touches:** game/rust/tests/belief_metamorphic.rs, game/rust/tests/myth_drift_properties.rs, new: game/rust/tests/belief_suite.rs
- **Gate:** the consolidated belief property suite runs green as one harness lane with no test moved outside its era-scoped bounds.

### M339 — `diagnose faith` Joins the Standing Runners
- **Intent:** Belief needs its own permanent diagnostic lens the way geo, climate, and politics already have theirs.
- **Build:** Add a `diagnose faith` subcommand reporting faith counts, conversion/schism rates, canonization tallies, relic-forgery ratios, and unbelief trends, wiring it into `report.sh` as a standing runner alongside the existing era diagnostics.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/culture.rs
- **Gate:** `report.sh` invokes `diagnose faith` on every run and its output stays byte-stable for a fixed seed.

### M340 — Era VI Gate
- **Intent:** The Unseen Order closes only once belief holds across a full historical sweep with both tellings proven sound.
- **Build:** Run the full 300-year sweep with every Era VI system live — faiths, schisms, holy wars, myth drift, saints, relics, unbelief — validating the M6.9/M318 two-layer telling stays coherent throughout and every calibration and property gate from the era holds simultaneously.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/culture.rs, game/rust/src/telling.rs
- **Gate:** the 300-year sweep completes green across all Era VI diagnostics, property suites, and calibration bands with the determinism hash stable.

### M341 — One Lattice of the Inner Life
- **Intent:** Peoples, tongues, and faiths grew as three loosely-coupled systems across Eras V and VI; the forge re-cuts them as one reference-linked lattice.
- **Build:** Unify culture, language, and faith identity under one reference-linked registry lattice, replacing ad hoc cross-references between `culture.rs`'s `Culture`/`God` structs and the language state with typed foreign keys, landing the shape change behind hindsight ADRs per the forge charter's Recast step.
- **Touches:** game/rust/src/culture.rs, game/rust/src/society.rs, docs/adr, new: game/rust/src/inner_life.rs
- **Gate:** determinism hash is unchanged before and after the refactor on the full regression seed set, and the full suite stays green.

### M342 — Myth-Layer Storage as Deltas
- **Intent:** Myth-variant text stored in full per generation bloats the pack; it should compress against the ground truth it drifted from.
- **Build:** Re-store myth variant texts as compact deltas against the ground-truth event log rather than full retelling snapshots, reducing per-faith myth storage while preserving exact reconstruction via the existing `legendize` drift function run in reverse.
- **Touches:** game/rust/src/telling.rs, game/rust/src/pack.rs, game/rust/src/culture.rs
- **Gate:** myth-layer payload size drops against the pre-forge baseline while every stored variant reconstructs byte-identical to its pre-delta form.

### M343 — Belief Tables Into Registry Codegen
- **Intent:** The belief tables built ad hoc across Q15 and the era must be fully declared, not hand-wired, per the forge's Declare step.
- **Build:** Move all remaining hand-written belief tables — canon, relic-cult, unbelief, sacred-name — into the field-registry codegen pipeline with generated delta-tick coverage, and extend the event-family enum to cover omen, schism, and canonization event classes cleanly.
- **Touches:** game/rust/src/pack.rs, game/rust/src/event.rs, game/rust/src/canon.rs, game/rust/src/relic.rs, game/rust/src/unbelief.rs, game/rust/src/sacred_names.rs
- **Gate:** codegen output contains zero hand-mirrored belief structs and event-family coverage includes the three new classes with stable serialization.

### M344 — Budgets in Band With the Full Inner Life
- **Intent:** Generation, tick, memory, and payload costs must re-settle into their bands now that the entire inner-life stack — peoples, tongues, faiths — runs together.
- **Build:** Profile generation time, tick time, resident memory, and pack payload size with the full unified inner-life lattice active, and rebalance caching and delta-tick granularity in `systems.rs` and `pack.rs` wherever the forge's Rehold step finds a band violation.
- **Touches:** game/rust/src/systems.rs, game/rust/src/pack.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` shows generation, tick, memory, and payload metrics all inside their bands with the complete inner-life stack active.

### M345 — Suite Refit: Belief Lanes Fast
- **Intent:** The growing belief-property suite must run fast enough to stay in the loop, and its audits must run themselves.
- **Build:** Consolidate the belief property and oatmeal lanes from M338/M337 into a faster harness path, automating the cross-seed distinctiveness and myth-drift audits so they run unattended as part of `report.sh` rather than by manual invocation.
- **Touches:** game/rust/tests/belief_suite.rs, game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** the belief lanes run within the forge's suite-speed budget and audits execute automatically on every `report.sh` invocation with full-suite green and determinism hash unchanged.
