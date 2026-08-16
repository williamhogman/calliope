# 12 — Worldbuilding Craft Theory (the literary canon)

Why a generated world *feels* real — the writers' answers, from the people
who defined the problem before procedural generation existed. Companion to
`05-history-narrative.md` (systems) and `15-tellability-prose.md` (prose);
this docket is about doctrine.

## Sources

1. **On Fairy-stories** — J.R.R. Tolkien (1939/1947) — https://ia902307.us.archive.org/32/items/on-fairy-stories_202110/J.%20R.%20R.%20Tolkien%20-%20On%20Fairy-Stories.pdf — READ. *Sub-creation*: a Secondary World with its own internally consistent laws, taken as seriously as engineering. *Secondary Belief* > "suspension of disbelief": the mind believes from inside as long as the world never violates its own logic; belief breaks on self-inconsistency, not impossibility.
2. **Impression of depth in The Lord of the Rings** — scholarship synthesis — https://en.wikipedia.org/wiki/Impression_of_depth_in_The_Lord_of_the_Rings — READ. Depth = (a) vast background apparatus glimpsed not lectured (maps, genealogies), (b) *casual unexplained references* to off-stage events and names, (c) invented history echoing real myth, (d) register shifts signaling age. **Queen Berúthiel**: Tolkien dropped a vivid detail (a wicked queen, nine cats) with zero explanation — he admitted to Auden he didn't know the story himself. The orphaned fragment reads as *more* real, because real archives are full of details nobody remembers the origin of.
3. **Le Guin: The Language of the Night; Steering the Craft** — SKIM, plus Robinson, "Onomaturgy vs. Onomastics" (https://ans-names.pitt.edu/ans/article/view/1926) — ABSTRACT. Names are generated *by* a culture's phonology, cosmology and taboos, never labels applied after. Style is itself worldbuilding: register and rhythm do the work exposition can't. Earthsea's Hardic/Kargish/Old Speech behave like real language families with cognates and sound-shift.
4. **Sanderson's Laws + worldbuilding lectures** — https://www.brandonsanderson.com/blogs/blog/sandersons-first-law (and second, third) — READ. First Law: a system may only *solve* problems in proportion to how well the audience understands it; a system whose unpredictability is the point must stay mysterious and never cash in. Second Law: **limitations > powers** — what a culture cannot do, and what things cost, defines it more than capability. Third Law: expand what exists before adding new. The Iceberg: depth is *implied consistency* — small surface gestures (an idiom, an oath, an offhand price) let the audience infer a submerged mass, and the inference must be *true*.
5. **"The great clomping foot of nerdism"** — M. John Harrison (2007) — http://web.archive.org/web/20080410181840/http:/uzwi.wordpress.com/2007/01/27/very-afraid/ — READ; Marshall, TEXT journal (https://textjournal.scholasticahq.com/article/18569) — SKIM. The anti-thesis: exhaustive systematized setting "numbs the reader's ability to fulfil their part of the bargain" — the lore-dump as foot-fault. Viriconium refuses consistency on purpose to stay uncanny rather than navigable. Marshall's reframe: worldbuilding fails when it seeks *totalizing coherence*; gaps, contradictions and unresolved texture are the charge.
6. **Gardener vs. architect** — G.R.R. Martin interviews — https://www.theguardian.com/books/booksblog/2011/apr/14/more-george-r-r-martin — SKIM. Plant details first, discover what they meant later: "the summer snows" was a throwaway line whose explanation was reverse-engineered afterward and became load-bearing. Meaning found after the fact reads as unforced.
7. **Lived-in world essays** — Alam (wear implies history), Angeline (constraints force specific solutions), "Composting the Glitch" (https://drwedge.uk/composting-the-glitch-why-game-engines-need-more-rot/) — READ/SKIM. Jury-rigged, mismatched, repaired things are evidence of an implied past. Worlds that feel pre-existing are worlds where limitations visibly forced awkward compromises. Most digital worlds are frozen in "spotless stasis": engines need *un-building* — rust, overgrowth, ruin, abandonment — not just growth.

## Synthesis

The canon converges on a contract: **the engine must be perfectly
consistent; the telling must be deliberately incomplete.** Tolkien and
Sanderson own the first half (Secondary Belief, the true iceberg), Harrison
and Berúthiel the second (the withheld fragment as the site of wonder).
Between them: constraints define cultures better than capabilities; decay
is content; meaning is gardened first and framed after; and names/style
carry more world per word than exposition ever does. For a simulator this
maps almost literally — the sim is the iceberg and the gardener, the
chronicle is the architect and must *withhold*.

## Calliope

The engine side of the contract is largely honored: everything is computed,
deterministic, causally traceable (`world.rs` → `chronicle.rs`). The
telling side is not — the chronicle explains everything it emits, nothing
decays, and no fragment is ever orphaned.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Ruins & abandonment: settlements can die (war, depletion, famine), leaving named ruin entities on the map | M | The single strongest "time passed here" signal we lack |
| 2 | Berúthiel emissions: a bounded fraction of chronicle lines reference computed-but-never-explained events/names | S | Free depth — the sim already knows more than it tells |
| 3 | Constraint surfacing: per-culture taboos/limits emitted as proverbs and laws, enforced by the sim (iceberg guarantee) | M | Cultures defined by what they refuse |
| 4 | Disused infrastructure: trade routes fade when traffic dies; roads outlive their reason (path dependence) | S-M | Wear on the map itself |
| 5 | Register shift by era: annal-terse for old events, fuller prose for recent ones | S | Age made audible |
| 6 | Never emit raw internal state in prose; keep the audit trail in the log, the myth in the telling | S (discipline) | Harrison's warning, cheaply honored |
