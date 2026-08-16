# 05 — History Generation, Myth & Procedural Narrative

## Sources

1-3. **Tarn Adams: DF villains talk (2018), Gamasutra interview (2016), GDC myth-gen (2016, w/ Tanya Short)** — SKIM. Legends mode = centuries of pre-play simulation mined for named entities; cosmogony as its own layer; curation principle: simulate what is *narratively* interesting.
4. **End-to-End Procedural Generation in Caves of Qud** — Grinblat & Bucklew (GDC 2019) — https://media.gdcvault.com/gdc2019/presentations/Grinblat_Jason_End-to-End_Procedural_Generation.pdf — READ. Layered generators: culture → religion → sultan lineage (dogma/era/relics) → village → NPC/quests. Each layer consumes the previous layer's **typed data**; prose is rendered last.
5. **Tapping Qud's potential** — Game Developer (2022) — SKIM. Tolerable "dream logic" in generated myth.
6. **Curating Simulated Storyworlds** — James Ryan, PhD (UCSC 2018) — https://escholarship.org/content/qt1340j5h2/qt1340j5h2.pdf — READ. "Curated emergent narrative": simulation produces events; a curator (human or algorithmic) makes them stories. Names the bridging problem: **story sifting**.
7-9. **Talk of the Town; character-knowledge chapter; Bad News** — SKIM/ABSTRACT. Epistemic state (who believes what, wrongly) as first-class data → gossip, rumor, unreliable narration.
10-12. **Felt (ICIDS 2019); felt repo; Winnow DSL (AIIDE 2021)** — READ/SKIM. Sifting patterns = declarative queries over an event/fact store; Winnow = incremental evaluation for streaming histories.
13. **Storylets design space** — Kreminski & Wardrip-Fruin — READ. Grain size, preconditions, selection — vocabulary that classifies Calliope's current bank-driven emission.
14-15. **Gardening games; storytelling partners** — READ/ABSTRACT. Legible causality + persistent named entities + export are what let players tell stories about a sim.
16-17. **Select the Unexpected (ICIDS 2022); Authoring for Story Sifters** — ABSTRACT. Rank sifted stories by statistical surprise; pattern authoring, not the query engine, is the dominant cost.
18-19. **Epitaph history generation + repo** — Kreminski — READ/SKIM. Era-transition weighted tables conditioned on prior state; deliberately shallow, jam-scale.
20-23. **Emily Short: procedural narrative series** — READ/SKIM. Content selection vs generation vs simulation; raw event sequences rarely read as story without framing.
24-25. **Microscope RPG** — Robbins — SKIM. Fractal timeline: eras → events → scenes, *retroactive* insertion allowed; palette constraints.
26-29. **Ultima Ratio Regum: culture, religion, 10-year retrospective, religion interview** — SKIM/READ/ABSTRACT. Axis-based cultures: generate a few orthogonal axioms once, cite them everywhere (architecture, myth, dialogue) — the coherence trick.
30. **RimWorld storyteller** — Sylvester (GDC 2017) — SKIM. Pacing layer overrides simulated probabilities to serve a tension/relief curve.
31-32. **Wildermyth GDC + RPS making-of** — ABSTRACT/SKIM. Authored storylets grafted onto persistent procedural characters (scars, traits).
33-34. **Propp-grammar generation** — Gervás — SKIM/ABSTRACT. Authored-grammar pole: guaranteed shape, limited variety.
35. **PCG textbook** — Shaker/Togelius/Nelson — SKIM.
36. **LegendsViewer-Next; legendsbrowser2** — SKIM. Community rebuilt DF's entity graph from a flat log — the export should be entity-graph-shaped in the first place.

## Synthesis

Two poles — **simulate-then-sift** (DF, Talk of the Town, Felt/Winnow) and **authored grammars** (Propp, storylets) — with shipping games in the hybrid middle (Qud's layered typed-data pipeline; RimWorld's authored pacing over real simulation; Wildermyth's authored scenes on procedural casts). The universal precondition is a **persistent entity web**: named people, artifacts with provenance, sites and factions accumulating relations. Sifting is powerful but its cost is authoring the patterns, not running the queries. Myth works as cosmogony generated once plus downstream citation; nobody has shipped mechanized *myth drift* — flagged as genuinely novel territory.

## Calliope

`chronicle.rs` is template-emission over static banks with a minimal entity graph (rulers, wars, two booleans): the storylet pole in its simplest form. Events are pre-rendered strings — actor references are lost at emission.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Structured `Event` fields (actor/entity ids beside rendered text) | S-M | Prerequisite for everything below |
| 2 | Minimal story sifter (3-5 Felt-style patterns over the log: orphaned heirs, overlapping wars, avenged raids) | M-L | The highest-leverage narrative addition per the literature |
| 3 | Persistent named non-ruler entities (generals, prospectors, founders) | M | Richer sifting substrate |
| 4 | Named artifacts with provenance ("the crown carried off in the Salt War") | M | Classic DF/Qud flavor |
| 5 | Pantheon layer: named gods cited across omens/festivals/wars (URR axis pattern) | M | Coherent myth instead of independent flavor lines |
| 6 | Drama-pacing modifier over event probabilities (RimWorld lesson) | S | Better felt rhythm, no new state |
| 7 | Legends browser (entity-graph UI) | L | Do after 1/3/4 exist |
| 8 | Myth drift / retelling corruption | S shallow, L deep | Novel; lowest urgency |
