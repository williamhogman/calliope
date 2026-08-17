# Era V — The Tongues (M236–M290)

Full four-field specs for Era V of `../ROADMAP-500.md`: generated
phonologies replace static name banks, proto-languages descend through
regular sound change into family trees, dialect continua follow roads
and split at mountains, loanwords ride the trade routes, persons and
dynasties get real onomastics, scripts and literacy change how history
is recorded, and every name becomes walkable back to its proto-root —
closed by Forge V (M286–M290), which unifies the lexicon engine. The
one-liners in the parent file are binding; these specs expand them.

### M236 — The Sound of a People
- **Intent:** Every culture's speech gets a real acoustic skeleton instead of a fixed morpheme bank, so names finally originate from a language rather than a lookup table.
- **Build:** Replace the five static `Bank` tables in naming.rs with a generated `Phonology` struct per culture — consonant and vowel inventories drawn power-law-weighted per Rosenfelder's gen model, a syllable template obeying the sonority sequencing principle (onset rises, coda falls), and a `make_word` rewritten to sample syllables from the template instead of concatenating fixed pre/mid/end fragments; the six existing styles seed six starter phonologies and retire OLD/HELLENIC/NORDIC/ARID/SYLVAN/STEPPE as literal banks.
- **Touches:** game/rust/src/naming.rs, game/rust/src/culture.rs, game/rust/src/constants.rs, new: game/rust/src/language.rs
- **Gate:** `diagnose civ` regenerates all settlement and culture names with zero raw-bank fragments remaining, every emitted word parses back through its syllable template, and the determinism hash for a fixed seed is stable across two consecutive runs.

### M237 — The First Tongues
- **Intent:** Founding peoples each speak a distinct proto-language from the moment they exist, giving every later divergence a documented common ancestor.
- **Build:** Add a `Language` entity kind carrying an id, a phonology, and a founding culture link; assign exactly one proto-language per founding culture at world genesis in the order cultures are created, threading the assignment through the existing culture-creation RNG stream; write the language-layer ADR recording the decision (one language object per lineage root, phonology owned by the language not the culture) and the rejected alternative of embedding phonology directly on `Culture`.
- **Touches:** game/rust/src/language.rs, game/rust/src/culture.rs, game/rust/src/entity.rs, game/rust/src/ids.rs, new: docs/adr (language-layer ADR, numbered at land time)
- **Gate:** every founding culture in `diagnose civ` resolves to exactly one language with a non-empty phonology, the language registry entry count equals the founding-culture count, and the determinism hash folds in the new `Language` entity table unchanged run-to-run.

### M238 — Words From Roots
- **Intent:** A lexicon of meaning-bearing morphemes replaces arbitrary syllables, so every generated word is a compound of things that mean something.
- **Build:** Build a per-language root table of ~150–300 semantic roots (following the DF precedent of large translatable root sets) each tagged with a gloss category (place-feature, virtue, color, animal, element), generate lexemes by combining one or more roots under the language's syllable template with power-law category weighting, and thread the existing M3.3 gloss field through so every emitted name still resolves to a gloss string built from its constituent roots' meanings.
- **Touches:** game/rust/src/language.rs, game/rust/src/naming.rs, new: game/rust/src/lexicon.rs
- **Gate:** every toponym gloss in `diagnose civ` output decomposes into a concatenation of root glosses with zero unglossed fragments, and root-table size stays within the 150–300 band per language across all seeds tested.

### M239 — The Family Tree
- **Intent:** Languages descend from one another the way the peoples who speak them do, so linguistic kinship mirrors lineage rather than running independently of it.
- **Build:** Attach a `parent_language` edge to `Language`, derived deterministically whenever a culture splits or a new culture is founded by migration from an existing one, forming a tree rooted at each Era II founding tongue; the tree walk reuses the culture-split events already recorded in politics.rs and chronicle.rs so no new split logic is introduced, only a language edge alongside the existing culture edge.
- **Touches:** game/rust/src/language.rs, game/rust/src/culture.rs, game/rust/src/politics.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose civ` reports a language forest with exactly one root per founding proto-language, no cycles, and every non-root language's parent culture split event timestamp precedes the child language's creation month.

### M240 — Regular Sound Change
- **Intent:** Sister tongues drift apart by rule rather than by re-roll, giving daughter languages the family resemblance real philology predicts.
- **Build:** Implement an SCA²-style ordered rewrite engine: a deterministic list of context-sensitive rules (per language-tree edge, seeded from the edge's hash) applied in order to every root and lexeme inherited from the parent, covering common shift classes — lenition, vowel raising/lowering, final-consonant loss — with rule selection power-law weighted so most languages take one or two shifts, not a cascade.
- **Touches:** game/rust/src/language.rs, new: game/rust/src/soundchange.rs
- **Gate:** applying a language's full rule chain to its parent's root table is idempotent and order-stable across two runs, and `diagnose civ` shows zero root collisions where two distinct parent roots rewrite to an identical daughter form without a merge record.

### M241 — Cognates
- **Intent:** The same ancestral root should be visibly recognizable across sister tongues, letting a player spot family resemblance in the raw text.
- **Build:** Track a `cognate_of: RootId` back-pointer on every derived root produced by soundchange.rs, expose a lookup that, given any lexeme, returns its full cognate set across the language tree, and surface this in explain.rs so the inspector can answer "what does this word share ancestry with."
- **Touches:** game/rust/src/lexicon.rs, game/rust/src/soundchange.rs, game/rust/src/explain.rs
- **Gate:** for a sampled 5% of roots across a 200-year `diagnose civ` run, cognate-set lookup returns every sister-language reflex with the correct shared proto-root, and no cognate chain crosses two unrelated language trees.

### M242 — Toponyms Re-Derived
- **Intent:** Old place-names finally obey the sound laws of the tongue that coined them instead of being drawn fresh from the current culture's bank.
- **Build:** Rewrite toponym generation so a name is produced once, at the founding language's stage in the tree, then carried forward unchanged in the record while its rendered surface form is recomputed by walking the sound-change chain from coining language to the settlement's current-culture language; this replaces the M9.3 direct-generation call with a lookup against the stored coining event.
- **Touches:** game/rust/src/naming.rs, game/rust/src/settlements.rs, game/rust/src/soundchange.rs, game/rust/src/chronicle.rs
- **Gate:** every settlement's displayed name in `diagnose civ` matches the deterministic output of replaying its coining root through the recorded language chain, and re-running generation twice at the same seed yields byte-identical surface forms.

### M243 — Hydronym Conservatism
- **Intent:** Rivers keep the words of vanished tongues, the single most authentic toponymic signal real geography offers.
- **Build:** Mark river and mountain features as naming-locked to the oldest language attested in their region at generation time, so later conquest or culture replacement renames settlements but never these features; implement the lock as a `coined_by: LanguageId` field set once at hydrology-driven feature naming and never overwritten by subsequent culture assignment passes.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/naming.rs, game/rust/src/geo.rs
- **Gate:** across a 300-year `diagnose civ` sweep, zero river or mountain-range names change their `coined_by` language after their founding month even as the surrounding settlement culture changes at least once per region.

### M244 — Sound-Change Properties
- **Intent:** The philology engine needs proof it behaves like a language, not a noise generator, before more history is layered on it.
- **Build:** Add a property-check suite to diagnose.rs verifying three invariants: every rule chain is regular (same input context always yields same output), collision rate between distinct roots after N shift generations stays bounded below a fixed ceiling, and gloss coverage is total (no lexeme loses its root gloss after any number of sound-change applications).
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/soundchange.rs, game/rust/src/lexicon.rs
- **Gate:** `diagnose properties` reports rule-chain regularity at 100%, root-collision rate under 5% at ten generations of drift, and gloss coverage at 100% across all sampled seeds.

### M245 — Dialect Continua
- **Intent:** Speech should grade smoothly across a people's territory instead of snapping between discrete culture-wide tongues.
- **Build:** Introduce a `Dialect` layer beneath `Language`: each settlement gets a dialect vector computed as a distance-weighted blend of neighboring settlements' sound-change parameters, decaying with grid distance and further attenuated across recorded geographic barriers (mountains, unsettled gaps), so speech drifts continuously rather than in blocks.
- **Touches:** game/rust/src/language.rs, game/rust/src/settlements.rs, new: game/rust/src/dialect.rs
- **Gate:** in `diagnose civ`, pairwise dialect distance between settlements correlates with grid travel distance at Pearson r above 0.6, and distance is monotonic non-decreasing along any unbroken land corridor.

### M246 — The Road Levels, The Mountain Splits
- **Intent:** Trade routes and terrain barriers should visibly shape where dialects blend and where they fracture, tying speech to the map's real geometry.
- **Build:** Feed the trade.rs route graph into dialect.rs as a distance-reducing channel (settlements linked by an active route treat their travel distance as the route cost, not raw grid distance) and feed unrouted mountain/sea barriers as distance-inflating multipliers, so the continuum levels along roads and splits sharply across unconnected ranges.
- **Touches:** game/rust/src/dialect.rs, game/rust/src/trade.rs, game/rust/src/geo.rs
- **Gate:** `diagnose civ` shows dialect distance between route-linked settlement pairs averaging at least 30% lower than unlinked pairs at equal grid distance, and distance across an unrouted mountain barrier at least doubles versus open plain.

### M247 — Dialect Diagnostics
- **Intent:** The continuum needs a measurable correlation to geography before the next quarter builds loanwords on top of it.
- **Build:** Add a `diagnose dialect` report computing the correlation band between dialect distance and effective travel distance (route-adjusted) across the full settlement graph, plus a barrier-crossing multiplier check, both logged as banded pass/fail ranges alongside the existing civ diagnostics.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/dialect.rs
- **Gate:** `diagnose dialect` reports correlation r in [0.55, 0.85] and barrier multiplier in [1.8, 4.0] on every seed in the standard diagnostic seed set.

### M248 — The Borrowed Word
- **Intent:** Goods should carry their names down the trade routes they travel, giving loanwords a literal economic cause.
- **Build:** For each active high-volume trade route in trade.rs, sample a small set of traded-good lexemes from the origin settlement's dialect and insert them into the destination dialect's lexicon as marked loans (`origin_language`, `loan_month` fields), with insertion probability scaled by the route's carried trade volume already tracked in economy.rs.
- **Touches:** game/rust/src/lexicon.rs, game/rust/src/trade.rs, game/rust/src/economy.rs, new: game/rust/src/loanword.rs
- **Gate:** in a 200-year `diagnose civ` run, loanword count per settlement correlates positively with its cumulative trade volume at r above 0.5, and every loan entry retains a traceable `origin_language` distinct from its host.

### M249 — Substrate Strata
- **Intent:** Conquered tongues should leave a fingerprint behind, not vanish cleanly when a people is absorbed.
- **Build:** When politics.rs records a culture absorption or conquest event, retain the losing language as a `substrate` layer attached to the surviving dialect region: a fixed-size sample of its roots persists in place-names and a small residue set of grammar markers (chosen syllable-final patterns) biases the surviving dialect's future sound-change rule selection.
- **Touches:** game/rust/src/politics.rs, game/rust/src/dialect.rs, game/rust/src/language.rs, game/rust/src/soundchange.rs
- **Gate:** across a 300-year sweep, every recorded conquest leaves at least one surviving substrate toponym in the conquered region, and substrate residue never appears outside the historically conquered polygon.

### M250 — Loan Diagnostics
- **Intent:** Borrowing needs to be shown as caused by trade, not merely present alongside it, before the era leans on it further.
- **Build:** Add a `diagnose loans` report correlating per-settlement loanword density against cumulative trade volume and against route centrality, banded against expected ranges, plus a check that substrate residue frequency decays with time since conquest as assimilation (M12.4-linked) proceeds.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/loanword.rs, game/rust/src/dialect.rs
- **Gate:** `diagnose loans` reports trade-loanword correlation r above 0.5 and substrate-residue half-life within a fixed multi-decade band on every standard seed.

### M251 — The Named Person
- **Intent:** People should be named the way their culture actually names people, not stamped from the toponym generator.
- **Build:** Add a per-culture naming-custom descriptor (patronymic, matronymic, epithet-first, house-surname) drawn from the culture's lexicon roots at culture creation, and rewrite the person-name generation path used by chronicle.rs's ruler/notable creation to compose names from this custom instead of a shared generic-name pool.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/naming.rs, game/rust/src/lexicon.rs, new: game/rust/src/onomastics.rs
- **Gate:** in `diagnose civ`, every generated person name matches its culture's assigned naming-custom pattern with zero cross-culture pattern leakage, verified across all cultures in a 200-year run.

### M252 — Dynastic Onomastics
- **Intent:** Ruling houses should favor a small recognizable name stock and number their kings, the way real dynasties read.
- **Build:** Give each ruling house (already tracked via `chronicle::new_ruler` and the rulers list in politics.rs) a fixed favored-name pool of 4–8 names drawn once at house founding from its culture's onomastics, and append an ordinal (numbered by same-name precedence within the house) whenever a ruler's drawn name repeats.
- **Touches:** game/rust/src/politics.rs, game/rust/src/chronicle.rs, game/rust/src/onomastics.rs
- **Gate:** in a 300-year `diagnose civ` run, at least 70% of rulers within any house draw from that house's favored-name pool, and every repeated name carries a correctly incremented ordinal.

### M253 — Names Age With The Tongue
- **Intent:** The Era III cast should not be frozen in amber — their names must drift down the generations exactly as the language does.
- **Build:** Wire onomastics.rs into soundchange.rs so a person name recorded in an early era, when displayed or referenced generations later, is passed through the intervening sound-change chain of its bearer's lineage language, matching the treatment already given to toponyms in M242.
- **Touches:** game/rust/src/onomastics.rs, game/rust/src/soundchange.rs, game/rust/src/chronicle.rs
- **Gate:** for a sampled Era III figure carried into a 300-year sweep, the displayed name at generation N matches deterministic replay of the sound-change chain, identical across repeated runs at the same seed.

### M254 — The Drifting Meaning
- **Intent:** Words should not mean the same thing forever — glosses need to date themselves as sense shifts with the era.
- **Build:** Attach an era-clock to each lexeme gloss in lexicon.rs; on a fixed low-frequency schedule (tied to the era boundaries already used elsewhere in the roadmap), a gloss may shift to an adjacent semantic-category sense via a small deterministic drift table, with the prior gloss retained as a dated history entry rather than overwritten.
- **Touches:** game/rust/src/lexicon.rs, game/rust/src/constants.rs
- **Gate:** every lexeme's gloss history in a 500-year `diagnose properties` run is monotonically dated with no gaps or duplicate timestamps, and current gloss always matches the most recent history entry.

### M255 — Etymology Walkable
- **Intent:** Any name in the world should be traceable back to the proto-root it began as, on demand, in the inspector.
- **Build:** Extend explain.rs with an etymology-walk function that, given any lexeme or name, reconstructs its full derivation chain — proto-root, each sound-change rule applied, each loan or substrate insertion, each semantic drift — as an ordered list ready for UI display.
- **Touches:** game/rust/src/explain.rs, game/rust/src/lexicon.rs, game/rust/src/soundchange.rs, game/web/js
- **Gate:** for a 5% sample of all live names in a 300-year run, the etymology walk terminates at a founding proto-root with no missing derivation step, verified deterministically across repeated runs.

### M256 — Lexicon Bounds
- **Intent:** Vocabularies must stay finite and clean at depth — no name soup where roots multiply without limit.
- **Build:** Add a deduplication and cap pass to lexicon.rs that merges lexemes whose surface form and gloss category collide after sound change, and enforces a per-language maximum root-and-derived-lexeme count by retiring the least-referenced entries (by usage count tracked since M238) once the cap is hit.
- **Touches:** game/rust/src/lexicon.rs, game/rust/src/soundchange.rs
- **Gate:** `diagnose properties` shows every language's lexicon size staying under its fixed cap through a 500-year sweep with zero duplicate surface-form/gloss pairs.

### M257 — Language Is Not People
- **Intent:** Speech communities should be free to diverge from political and cultural boundaries, the way real ports and trade routes produce their own tongues.
- **Build:** Introduce a `SpeechCommunity` layer distinct from `Culture`: ports and high-traffic route junctions above a trade-volume threshold spawn a creole language blending the dialects of their top trading partners via a fixed root-mixing ratio, and long, stable trade corridors accumulate a lingua-franca language used for exchange without displacing local household tongues.
- **Touches:** game/rust/src/language.rs, game/rust/src/dialect.rs, game/rust/src/trade.rs, new: game/rust/src/speechcommunity.rs
- **Gate:** in `diagnose civ`, every settlement above the port trade-volume threshold shows a distinct creole SpeechCommunity whose root mix traces to at least two parent dialects, and creole formation timing is stable across repeated runs at the same seed.

### M258 — Bilingual Belts
- **Intent:** Borders should read as belts of dual speech, and what one people calls a place versus what another calls it should come from real tongues.
- **Build:** Mark settlements within a fixed distance of a culture-territory boundary as bilingual, holding both neighboring SpeechCommunity tongues; derive M9.2's exonym/endonym pairing directly from these two tongues' independent name-generation passes over the same feature rather than from a generic exonym table.
- **Touches:** game/rust/src/speechcommunity.rs, game/rust/src/naming.rs, game/rust/src/geo.rs
- **Gate:** every border settlement within the fixed belt distance in `diagnose civ` carries a distinct endonym and exonym each traceable to its own tongue's lexicon, with zero belt settlement left monolingual.

### M259 — Community Diagnostics
- **Intent:** Language and people must be shown as genuinely separable maps, not two views of the same partition.
- **Build:** Add a `diagnose language-map` report computing the divergence between the culture-territory partition and the SpeechCommunity partition (measured as fraction of settlements where the two disagree), banded against an expected range reflecting real-world creole/lingua-franca prevalence.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/speechcommunity.rs
- **Gate:** `diagnose language-map` reports culture/language divergence within a fixed 10–30% band across all standard seeds, never collapsing to zero divergence.

### M260 — Scripts
- **Intent:** Writing systems should exist as objects with their own histories, invented or borrowed the way real scripts spread along trade and conquest.
- **Build:** Add a `Script` entity, invented once per language family root when the owning culture crosses a fixed tech-tree literacy threshold, and inherited or borrowed by daughter/contact languages via the same edges already used for language descent and loanwords; script drift applies a lightweight glyph-shift analogous to sound change, keyed to the same rule-selection RNG pattern as soundchange.rs.
- **Touches:** game/rust/src/language.rs, game/rust/src/society.rs, new: game/rust/src/script.rs
- **Gate:** in `diagnose civ`, every language above the literacy tech threshold resolves to exactly one script tracing to an invention or borrowing event, and script family trees mirror language family trees with zero orphan scripts.

### M261 — Literacy
- **Intent:** Reading and writing should be rare where they are rare, common where tech and town size warrant, not uniform across the map.
- **Build:** Compute a per-settlement literacy rate as a function of society.rs's existing tech level and settlement population tier, following a logistic curve gated by the script-invention threshold from M260, and expose the rate as a tracked settlement field feeding the chronicle's future written-record weighting.
- **Touches:** game/rust/src/society.rs, game/rust/src/settlements.rs, new: game/rust/src/literacy.rs
- **Gate:** `diagnose civ` shows literacy rate monotonically increasing with tech tier at fixed population, staying within a 0–100% logistic band with no settlement exceeding its tech-tier ceiling.

### M262 — Writing Changes The Telling
- **Intent:** The chronicle itself should shift from campfire memory to written record as literacy rises, changing how history is preserved.
- **Build:** Add a `source_mode` field to chronicle entries (oral or written), assigned at event-recording time by sampling the recording settlement's literacy rate from literacy.rs, and feed source_mode into telling.rs's `legendize` path so written-sourced events skip the mythologizing vagueness pass and retain exact figures (feeding forward to M6.9).
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/literacy.rs
- **Gate:** in a 300-year `diagnose telling` run, the fraction of written-sourced events tracks the era's mean literacy rate within 5 percentage points, and written events never pass through the digit-vaguing step.

### M263 — The Written Realm Remembers
- **Intent:** Detail surviving into the chronicle should visibly track how literate the recording realm was at the time.
- **Build:** Weight chronicle event retention and detail granularity (number of preserved fields, precision of dates and counts) by the recording settlement's literacy rate at event time, so high-literacy realms leave dense, exact records and low-literacy ones leave sparse, rounded ones, replacing the flat retention model telling.rs currently uses.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/literacy.rs
- **Gate:** `diagnose telling` shows chronicle detail density (fields retained per event) correlating with recording-settlement literacy at r above 0.6 across a 300-year sweep, stable across repeated runs at the same seed.

### M264 — The Lost Texts
- **Intent:** History has silences too; burned libraries and vanished chronicles must leave a dated, provable absence rather than a quiet gap.
- **Build:** Add a `RecordGap` event kind in `event.rs` fired when a library, scriptorium, or archive settlement is destroyed or abandoned, recording the span of years and the language/culture whose written record it held; `telling.rs` surfaces the gap as a chronicle entry ("the annals of Vethmar fall silent for forty years") and downstream literacy/record-density queries (M263) treat the span as withheld rather than missing-by-omission.
- **Touches:** game/rust/src/event.rs, game/rust/src/telling.rs, game/rust/src/chronicle.rs, game/rust/src/state.rs, game/rust/src/bin/diagnose.rs
- **Gate:** every archive destruction event in a 300-year run produces exactly one `RecordGap` with correct start/end years, and the gap's hash contributes to the determinism hash reproducibly across reruns of the same seed.

### M265 — Stone and Coin Speak
- **Intent:** Inscriptions give the world physical, dated fragments of language independent of the oral chronicle, the archaeologist's evidence layer.
- **Build:** Generate `Inscription` records — steles at battle sites, coin legends at mints, tomb texts at burials — each carrying a short lexicon-derived phrase in its language's current phonology and orthography plus a mint/carve year; store them keyed to the settlement or event that produced them and expose them to the inspector as datable language samples distinct from spoken record.
- **Touches:** game/rust/src/naming.rs, game/rust/src/culture.rs, game/rust/src/artifact.rs, game/rust/src/pack.rs, new: game/rust/src/inscription.rs
- **Gate:** every settlement with a mint or necropolis older than one century holds at least one inscription whose year, language id, and text regenerate identically from the seed, folded into the determinism hash.

### M266 — Tongues That Outlive Speakers
- **Intent:** Liturgical and chancery registers freeze older grammar and vocabulary even as the vernacular moves on, the classic diglossia of dead-but-official languages.
- **Build:** Introduce a `RegisterLanguage` layer that snapshots a proto- or ancestor-language's phonology at the founding of a temple or chancery institution and keeps it static while descendant vernaculars keep shifting under M240's sound-change laws; chronicle and inscription text generation (M265) picks the register language for formal contexts and the vernacular for informal ones.
- **Touches:** game/rust/src/culture.rs, game/rust/src/naming.rs, game/rust/src/telling.rs, game/rust/src/language.rs
- **Gate:** register languages remain byte-identical across ticks after their founding while sibling vernaculars diverge, verified by a diagnose check comparing register-vs-vernacular phoneme drift over a 200-year window.

### M267 — The Scribe's Error
- **Intent:** Cross-tongue diplomacy is lossy; treaties translated between languages should sometimes mean slightly different things, seeding real narrative friction.
- **Build:** When a treaty or truce event (politics.rs) crosses a language boundary, run the clause text through a deterministic translation pass that can flip a term to its nearest cognate or false-friend with a probability keyed to translator literacy and language distance; log a `MistranslationIncident` event when a flip occurs, and let telling.rs narrate the resulting dispute.
- **Touches:** game/rust/src/politics.rs, game/rust/src/event.rs, game/rust/src/telling.rs, game/rust/src/language.rs
- **Gate:** mistranslation rate scales monotonically with language distance across a swept parameter set, and incident selection is bit-stable for a fixed seed across two consecutive runs.

### M268 — The Written-Word Bands
- **Intent:** The archive layer needs a provable relationship between literacy and how much of history actually gets written down.
- **Build:** Add a `diagnose written-word` report correlating per-settlement literacy rate (M261) against chronicle entry density, inscription count, and record-gap frequency across the run, and assert the correlation falls within a literature-grounded band rather than drifting arbitrarily with scale.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/telling.rs, game/rust/src/chronicle.rs
- **Gate:** literacy-to-record-density Pearson correlation lands in the 0.6–0.9 band across three seeds and the diagnose report exits non-zero outside it.

### M269 — The Dying Word
- **Intent:** Languages, like the peoples who speak them, can go extinct — assimilation should leave linguistic strata behind rather than erasing history.
- **Build:** Extend the assimilation model (M12.4) so a culture absorbed by another marks its language `Dying`, its speaker count decaying on a half-life curve until zero, at which point its lexicon and sound-change branch freeze into a substrate stratum (M249) still readable in place-names and loanwords.
- **Touches:** game/rust/src/society.rs, game/rust/src/culture.rs, game/rust/src/language.rs, game/rust/src/naming.rs
- **Gate:** a language's speaker-count decay matches its declared half-life within 2 percent over a 300-year sweep, and dead languages never regenerate new words after their extinction tick.

### M270 — The Standard Tongue
- **Intent:** Late-tech states flatten dialect diversity into an official standard, mirroring the real history of chancery-driven standardization.
- **Build:** At a tech-tree threshold, a polity's capital dialect is promoted to `Standard`, pulling nearby dialects' phoneme distributions toward it at a rate keyed to administrative reach and literacy, implemented as a weighted convergence pass over the dialect-continuum grid from M245.
- **Touches:** game/rust/src/culture.rs, game/rust/src/language.rs, game/rust/src/geo.rs
- **Gate:** dialect-continuum variance within a standardized polity shrinks monotonically over the century following standardization, checked by diagnose across a 300-year run.

### M271 — The Sweep of Tongues
- **Intent:** Across five centuries the count of living languages should tell an honest story of both birth and death, never a monotone climb or crash.
- **Build:** Add a `diagnose language-trajectory` report plotting language-count over time against founding events (M237), splits (M239), and deaths (M269), and bound the net trajectory to a plausible band derived from the founding-people count and assimilation rate.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/language.rs, game/rust/scripts/report.sh
- **Gate:** language count across a 500-year run stays within a band of 0.3x–3x the founding count at every century mark, for three seeds.

### M272 — The Linguistic Atlas
- **Intent:** Language, dialect, and script are three distinct map layers, and the atlas should let an observer see all three at once.
- **Build:** Add renderer layers for language-family color fields, dialect-continuum gradient shading, and script-boundary isoglosses, each drawing from the pack-declared language grids; isoglosses render as contour lines computed from the dialect distance field rather than hard polygon borders.
- **Touches:** game/web/js/render.js, game/web/js/render, game/rust/src/render.rs, game/rust/src/pack.rs
- **Gate:** all three layers toggle independently in the UI and isogloss contours regenerate pixel-identically for a fixed seed and camera state across two runs.

### M273 — The Inspector Speaks
- **Intent:** Every name in the world should be an object you can interrogate — its sound, its meaning, its age — not a static label.
- **Build:** Extend `explain.rs` and the inspector UI to parse any selected name back through its language's phonology and sound-change history, rendering a pronunciation guide, a compositional gloss (specific + generic, per M14's toponymy synthesis), and the founding date of its oldest attested layer.
- **Touches:** game/rust/src/explain.rs, game/web/js/inspect.js, game/rust/src/naming.rs, game/rust/src/language.rs
- **Gate:** every settled, river, and mountain name in a test world resolves a non-empty pronunciation, gloss, and founding date, verified by a diagnose pass over the full feature list.

### M274 — Prose in the Tongues
- **Intent:** The chronicle should speak in the world's own words sometimes, not just describe them from outside.
- **Build:** Weave short sayings, proverbs, and name-meaning asides into `telling.rs` output, drawn from the per-culture lexicon and gloss data, triggered probabilistically at chronicle-worthy events tied to a place or dynasty whose name has a known etymology.
- **Touches:** game/rust/src/telling.rs, game/rust/src/language.rs, game/rust/src/naming.rs
- **Gate:** at least 5 percent of chronicle entries referencing a named place or house include a woven saying or gloss, stable and reproducible for a fixed seed.

### M275 — Philology Properties
- **Intent:** The whole language-family apparatus needs machine-checked invariants, not just plausible output.
- **Build:** Write property tests asserting descent-tree consistency (every language has exactly one parent except proto-roots), sound-change regularity (a given rule applies identically to every word matching its context), and total gloss coverage (every generated word traces to at least one root morpheme).
- **Touches:** game/rust/src/language.rs, game/rust/src/naming.rs, new: game/rust/tests/philology_properties.rs
- **Gate:** the property suite runs 10,000 generated cases per invariant with zero failures and completes inside the existing property-lane time budget.

### M276 — Metamorphic Philology
- **Intent:** Contact and isolation should visibly and provably steer language change in opposite directions.
- **Build:** Add metamorphic tests: doubling trade-route contact between two cultures must not decrease their measured loanword rate (M250), and doubling geographic isolation must not decrease their measured phonological divergence (M240), both checked by re-running world generation under perturbed inputs and comparing summary statistics.
- **Touches:** game/rust/src/trade.rs, game/rust/src/language.rs, new: game/rust/tests/philology_metamorphic.rs
- **Gate:** both metamorphic relations hold across 50 seeded perturbation pairs with zero monotonicity violations.

### M277 — The Lexicon in the Registry
- **Intent:** Language state has grown ad hoc through the era; it must join the field registry and pack lanes like every other piece of world truth (ADR-0015).
- **Build:** Declare language, dialect, script, and lexicon grids and tables through the `field_registry!` macro in `pack.rs`, re-measure string-interning costs against real generated lexicons at full-era depth (E3.8), and update codegen so the JS side receives generated accessors instead of hand-mirrored constants.
- **Touches:** game/rust/src/pack.rs, game/rust/src/language.rs, game/rust/src/bin/genjs.rs, game/web/js/gen, docs/adr/0015-registry-codegen-architecture.md
- **Gate:** `cargo build` regenerates JS bindings with zero hand-edited drift, and interning benchmarks on a full-depth world stay within the recorded E3.8 baseline band or better.

### M278 — Name-Generation Budgets
- **Intent:** The full philology stack must not blow the world's generation-time or memory envelope at depth.
- **Build:** Profile name and lexicon generation across the entire pipeline — proto-root synthesis, sound-change application, per-settlement toponym derivation, inscription and register-language text — and bring the total inside the existing generation-time budget bands, trimming or memoizing the costliest passes.
- **Touches:** game/rust/src/naming.rs, game/rust/src/language.rs, game/rust/src/culture.rs, game/rust/scripts/report.sh
- **Gate:** full-depth world generation time and peak memory both stay within the ADR-0009 budget bands on the reference machine across three seeds.

### M279 — Every Tongue Its Own
- **Intent:** Cross-seed variety must extend to language itself: two worlds should not sound alike, the oatmeal test for philology.
- **Build:** Add an oatmeal-suite check comparing phoneme-inventory shape, sound-change rule sets, and lexicon root stock across a batch of seeds, flagging convergent or degenerate outputs the way earlier oatmeal phases (M8-era, M108-era) did for terrain and culture.
- **Touches:** game/rust/src/naming.rs, game/rust/src/language.rs, new: game/rust/tests/oatmeal_philology.rs
- **Gate:** pairwise phoneme-inventory and lexicon-overlap similarity across 30 seeds stays below the oatmeal ceiling used by prior eras, with zero seed pairs flagged as near-duplicate.

### M280 — Five Centuries of Coherence
- **Intent:** A tree that only makes sense at generation time is not a tree; philology's invariants must survive the whole simulated span.
- **Build:** Run the philology property and metamorphic suites (M275/M276) embedded inside a full 500-year `diagnose sweep`, sampling descent-tree consistency, sound-law regularity, and gloss coverage at century marks rather than only at world-genesis.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/language.rs, game/rust/scripts/report.sh
- **Gate:** all philology invariants hold at every century checkpoint across a 500-year run for three seeds with zero violations logged.

### M281 — Calibrated to Real Tongues
- **Intent:** Invented languages should sit inside the shape of real ones, not wander into alien statistics.
- **Build:** Compare generated phoneme-inventory sizes, syllable-template complexity, and loanword-rate distributions against the typological envelopes cited in the culture-language digest (Rosenfelder's LCK, WALS-style natural-language ranges), adjusting `make_word` weighting and sound-change rule generation where outputs fall outside them.
- **Touches:** game/rust/src/naming.rs, game/rust/src/language.rs, docs/research/06-culture-language.md
- **Gate:** 95 percent of generated languages across 30 seeds have phoneme-inventory size and loanword rate inside the documented natural-language envelope bands.

### M282 — The Label Audit at Depth
- **Intent:** After all this machinery, a sampled name must actually classify back to the tongue that made it, or the archaeology is theater.
- **Build:** Build a classifier that, given a toponym's surface form, predicts its source language from phonotactic signature alone, then audit it against a large sample of ground-truth generated names across every culture and era layer.
- **Touches:** game/rust/src/naming.rs, game/rust/src/language.rs, new: game/rust/src/bin/label_audit.rs
- **Gate:** the classifier correctly attributes sampled toponyms to their true source language at 95 percent accuracy or better across 5,000 samples from three seeds.

### M283 — The Property Suite, Consolidated
- **Intent:** Eighteen quarters of language work has scattered checks across many files; the era needs one coherent lane before it closes.
- **Build:** Consolidate the philology property, metamorphic, oatmeal, and calibration tests (M275, M276, M279, M281) into a single `philology` test module with shared fixtures and a documented invariant list, removing duplicated seed-world setup.
- **Touches:** game/rust/tests/philology_properties.rs, game/rust/tests/philology_metamorphic.rs, game/rust/tests/oatmeal_philology.rs, new: game/rust/tests/philology/mod.rs
- **Gate:** the consolidated lane runs in under the prior combined wall time and every invariant from the four source files still executes and passes.

### M284 — `diagnose tongues`
- **Intent:** Language needs its own standing diagnostic voice alongside terrain, climate, and politics, run on every change from here on.
- **Build:** Add a `tongues` subcommand to `diagnose.rs` that reports language count, family-tree depth, sound-change rule counts, loanword rate, script distribution, and label-audit accuracy in one pass, and wire it into `report.sh` as a standing runner.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` invokes `diagnose tongues` on every run and fails the build if any of its bundled bands are breached.

### M285 — Era V Gate: Every Name Walkable
- **Intent:** The era closes only when the promise of the opening quarter is real: any name, traced to its root, across three centuries, without exception.
- **Build:** Run the full 300-year sweep with `diagnose tongues`, the consolidated philology suite, and the label audit all green simultaneously, and confirm the inspector (M273) can walk every settled, river, mountain, and dynastic name in the sweep back to a founding proto-root with a dated history.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, game/rust/src/explain.rs
- **Gate:** a 300-year sweep passes `diagnose tongues`, the philology suite, and a 100-percent name-walkability audit with zero unresolved names, on three seeds.

### M286 — One Lexicon Engine
- **Intent:** Toponyms, person names, and glosses grew their own generation paths across the era; hindsight demands one engine behind all three.
- **Build:** Recast `naming.rs`, `culture.rs`, and `language.rs`'s word-generation code paths into a single lexicon engine with one entry point for root selection, sound-change application, and gloss lookup, landing the consolidation as an ADR that supersedes the prior ad hoc split; no new naming behavior is introduced.
- **Touches:** game/rust/src/naming.rs, game/rust/src/culture.rs, game/rust/src/language.rs, new: docs/adr (unified-lexicon-engine ADR, numbered at land time)
- **Gate:** the determinism hash over a 300-year sweep is byte-identical before and after the refactor on three seeds.

### M287 — Compact Name-Bank Formats
- **Intent:** Root tables and sound-change tries have grown organically; the forge recuts them into registry-declared, codegen-friendly formats.
- **Build:** Replace ad hoc root-list vectors with compact trie-structured name-bank tables declared through `field_registry!`, with codegen in `genjs.rs` emitting matching JS-side lookup structures instead of hand-written banks.
- **Touches:** game/rust/src/naming.rs, game/rust/src/pack.rs, game/rust/src/bin/genjs.rs, game/web/js/gen
- **Gate:** generated JS name-bank lookups match Rust-side output byte-for-byte on a full sample sweep, and the determinism hash is unchanged from M286's baseline.

### M288 — Names as IDs, Reconsidered
- **Intent:** String interning across the WASM boundary was settled once (E3.8); the lexicon engine's real costs now argue the question again.
- **Build:** Re-benchmark string versus integer-id name references across the Rust/JS boundary using the M286 lexicon engine's actual traffic patterns, and either reaffirm E3.8 in writing or supersede it with an ADR switching the wire format to interned ids.
- **Touches:** game/rust/src/pack.rs, game/rust/src/naming.rs, game/web/js/wasm-load.js, docs/adr/0015-no-sab-field-mirror.md
- **Gate:** the benchmark comparison is reproducible within 5 percent variance across three runs and the chosen format's determinism hash matches the pre-forge baseline.

### M289 — Budgets With Full Philology
- **Intent:** The forge's rehold quarter must prove the era's weight fits inside the instrument's standing budgets, not just at genesis but through the tick.
- **Build:** Re-measure generation-time, per-tick, and payload budgets with the full philology stack (M236–M282) running at era-depth, tuning the lexicon engine's memoization and pack-lane quantization until all three land back in band.
- **Touches:** game/rust/src/pack.rs, game/rust/src/language.rs, game/rust/scripts/report.sh
- **Gate:** generation time, tick time, and pack payload size all sit within their ADR-0009 budget bands on the reference machine across three seeds with full philology active.

### M290 — Suite Refit, Label Audits Standing
- **Intent:** The forge closes by making the growing suite fast and the label audit a routine check rather than a special-occasion script.
- **Build:** Fold `label_audit.rs` into the standard `report.sh` run at a reduced sample size for routine checks and a full sample size for release gates, and consolidate the philology property/metamorphic/oatmeal lanes further so the whole suite's wall time drops relative to the pre-forge baseline.
- **Touches:** game/rust/src/bin/label_audit.rs, game/rust/scripts/report.sh, game/rust/tests/philology/mod.rs
- **Gate:** full suite wall time is at or below the pre-forge (M285) baseline, budgets stay green, and the determinism hash is unchanged through every forge refactor across three seeds.
