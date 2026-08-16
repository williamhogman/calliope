# 13 — Feel Doctrines of Shipped World-Simulators

What the games that *solved* the feel problem each discovered.
`05-history-narrative.md` covers the machinery (sifting, storylets, entity
graphs); this docket covers the doctrines — the design stances that make
the same machinery read as alive instead of mechanical.

## Sources

1. **DF talks: Practices in ProcGen (GDC 2016), Villains (Roguelike Celeb 2018), PRACTICE 2016** — SKIM. Bottom-up agent simulation; artifacts as anchors: named objects with creation stories and custody chains thread separate historical arcs together. Simulation fidelity is spent where it generates *story* (personality, motive, secrets), never on atoms.
2. **Generation of Mythic Biographies in Caves of Qud** — Grinblat & Bucklew (FDG 2017) — https://freeholdgames.com/papers/Generation_of_mythic_biographies_in_Cavesofqud.pdf — READ. The counter-lesson to literal simulation: real oral history already compresses and distorts cause→effect (the hero dies of *hubris*, not the mechanical reason). Generate *mythologized* biography — thematically tight, causally loose — not an audit trail. Elemental seeding per sultan (salt, glass, chrome) makes random rolls read as authored symbolism.
3. **Qud: "If you have to ask, it's lore"** — GDC 2018 / wiki sultan histories — SKIM. Lore is dropped in-world as texture (statues, engravings, ruins) and revealed piecemeal via *distributed, sometimes contradictory evidence* the player triangulates — historiography as gameplay. No central codex.
4. **URR: How to generate a religion / culture** — Mark R. Johnson, RPS + GDC Europe 2015 — SKIM. The **permeation principle**: one culture seed read by *every* subsystem (architecture, dress, law, dialect, puzzles), so unrelated systems visibly agree. Hand-authored exceptions hide the procedural seams.
5. **Designing Games ch. 4 + The Simulation Dream** — Tynan Sylvester — https://tynansylvester.com/2013/06/the-simulation-dream/ — READ. Unguided simulation produces statistically probable, dramatically flat outcomes. Two fixes: a **pacing layer** (storyteller reads recent history, shapes valleys and peaks), and **apophenia** — players narrativize correlated coincidences themselves; the designer's job is to maximize the rate of apophenia-triggering coincidence, not to pre-author meaning.
6. **Wildermyth: GDC + EPC 2021 talks** — SKIM. **Visible consequence as memory anchor**: scars, prosthetics and legacy items are rendered on the body/portrait — history is always on-screen, never in a menu. Event selection pays off *existing* relationships rather than rolling fresh.
7. **King of Dragon Pass / Six Ages** — Game Developer interviews with David Dunham — SKIM. **Myth as mechanic**: heroquests re-enact clan myths and change real game state, so lore has stakes and players *care*. Culture-first design: the belief system is the foundation the economy/diplomacy expresses, not a palette swap. Council framing gives every generated event one consistent "as told by our people" voice.
8. **Songs of the Eons devlog; Sapiens devblogs** — https://demiansky.itch.io/songs-of-the-eons/devlog/66412/ — SKIM. The living-world thesis: history generation and live simulation should be *the same system* — history keeps happening during play, so relics of the past stay causally connected to the present. Causality traceable to physical substrate (the war happened *because* the farmland failed) reads as historical materialism, but needs a narrative layer on top or it stays flat.

## Synthesis

Four doctrines, each independently rediscovered:

1. **Mythologize, don't log** (Qud) — keep ground truth in the state,
   compress and distort it in the telling. The audit trail is for the
   harness; the legend is for the reader.
2. **Permeate, don't decorate** (URR, KoDP) — one cultural seed cited by
   every subsystem; myth with mechanical stakes. Coherence across
   unrelated systems *is* the illusion of intentional civilization.
3. **Pace and let them connect the dots** (RimWorld) — a drama layer over
   flat probability, plus enough causal proximity that apophenia does the
   narrating for free.
4. **Anchor memory in things** (DF, Wildermyth) — named artifacts with
   custody chains, consequence rendered visibly on the world, evidence
   scattered as fragments to triangulate rather than a codex to read.

## Calliope

`chronicle.rs` currently logs more than it mythologizes; culture flavors
names but does not permeate; nothing anchors memory in objects or marks
on the map.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Two-layer telling: sim event log (ground truth, harness-checked) vs. rendered legend (compressed, thematically seeded per dynasty) | M | The Qud lesson; builds on M6.1 structured events |
| 2 | History marks the map: battle sites, ruins and war-renamed features feed back into `naming.rs`/render | M | Chronicle → map residue loop; today the map forgets |
| 3 | Culture seed permeation: culture axioms cited by settlement styles, law flavor, omen banks, market quirks | M-L | The URR coherence trick; culture.rs already holds the seed |
| 4 | Myth with stakes: festivals/omens that modify real state (trade, war odds) so lore is load-bearing | M | KoDP lesson; prevents decorative-lore drift |
| 5 | Artifact custody chains (extends M6.3): relics change hands *through* wars and raids, and the chronicle cites the chain | S on top of M6.3 | DF's cheapest deep trick |
| 6 | Elemental/thematic seeding per dynasty coloring its myths, epithets, omens | S | Randomness reads as symbolism |
