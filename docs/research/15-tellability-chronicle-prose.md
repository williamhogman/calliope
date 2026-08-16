# 15 — Tellability & Chronicle Prose

Which events deserve telling, and how to phrase generated history so it
survives its hundredth sentence. The narratology and prose-craft companion
to `05-history-narrative.md` (which covers the sifting machinery).

## Sources

1. **Tellability** — Baroni, Living Handbook of Narratology — http://lhn.sub.uni-hamburg.de/index.php/Tellability.html — READ. An event is worth reporting iff it violates a norm against a backdrop of expected order. Labov's sociolinguistics formalized: no disruption, no story.
2. **Scripts, Sequences, and Stories** — David Herman (PMLA) — ABSTRACT. A sequence of script-conforming actions ("woke, ate, worked") is not a story; story requires deviation from the script that demands resolution. Maps directly onto log-vs-skip.
3. **Event and Eventfulness** — Hühn, LHN — READ. Eventfulness is *scalar*: from mundane change to the turning point that recontextualizes a trajectory. Systems should rank, not filter binarily.
4. **Narrative reversals and story success** — *Science Advances* 2024 — https://pmc.ncbi.nlm.nih.gov/articles/PMC11421681/ — ABSTRACT. Empirical: stories with more reversals of fortune are rated more compelling and are shared more — with an optimum; too many reversals harm coherence.
5. **Emily Short: procedural text series** — https://emshort.blog/2014/11/18/procedural-text-generation-in-if/ ; "Bowls of Oatmeal" — READ. Three axes: **salience** (how much world-state the sentence reflects), **variety** (surface realizations per slot), **coherence** (one voice). Mad-libs slot-filling = variety without depth; template fatigue sets in fast. Oatmeal law: generated detail that never *matters later* teaches the reader to stop reading it — vary significance, not just surface.
6. **Aaron Reed: Intentional Collapse; satellite sentences** — https://medium.com/@aareed/intentional-collapse-plausibly-human-randomized-text-e901220cbc3d — READ. Kernel sentences (plot-load-bearing) stay constrained and hand-tuned; satellite sentences (mood, texture) absorb the heavy randomness, where errors are cheap. Motivate the chosen branch (foreshadow, narrator aside) so selection feels authored.
7. **Narrative Legos** — Ken Levine (GDC 2014) — https://www.gdcvault.com/play/1020211/Narrative — SKIM. Narrative as small context-tagged recombination units checked against a running state ledger; the hard part is consistency across combinations, not variety.
8. **Ryan, Talk of the Town / Bad News** — READ (cross-ref 05). Sifting needs relational substrate — grudges, debts, oaths, rumors — not just timestamps; and the richest presentation is *investigative* (query the town), not a feed.
9. **Short/Reed: anti-repetition craft** — Exercises in Generated Prose; Sharing Authoring with Algorithms — SKIM. Track what has been said: second mentions get epithets and callbacks ("the same walls that saw his coronation…"), never the introduction template again.

## Synthesis

A chronicle earns belief through **selection, memory and consequence**:

- **Selection** — score events for eventfulness (norm-violation × stakes ×
  reversal) and promote only the top band into the default telling; keep
  the full log behind a toggle. Detect reversals explicitly: rise-fall,
  defeat-vengeance, prodigal return are the premium patterns.
- **Memory** — per-entity narration state: first mention introduces,
  later mentions use epithets and callbacks; no entity is ever
  re-described identically. Epithets accrete from deeds.
- **Consequence** — every specific detail is tagged consequential
  (recurs, is load-bearing) or atmospheric (satellite). Consequential
  details must pay off; atmospheric ones carry the variation budget.
- **Silence** — some entries stay unresolved ("the granary burned;
  arson was suspected, never proven") and some causes stay disputed
  between sources. Full transparency forecloses apophenia (cross-ref 12/13).

## Calliope

`chronicle.rs` emits every event with equal weight, re-introduces entities
identically each mention, and resolves everything it raises.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Eventfulness score per event (stakes × norm-violation × reversal flag); default feed shows top band, toggle shows all | S-M | Turns the feed from log into telling; slots into M6.5 sifter |
| 2 | Reversal detector over dynasty/settlement fortune curves (rise-fall, avenged-defeat) feeding sifter rank | M | The empirically-validated premium material |
| 3 | Narration memory: per-entity mention count + earned epithets ("Oathbreaker", "the Old") + callback clauses | M | Kills template fatigue at its root |
| 4 | Kernel/satellite split in template banks: causal clauses constrained, flavor clauses free-varying | S | Structural variety without incoherence |
| 5 | Consequential/atmospheric tagging: consequential details guaranteed to recur (the relic, the omen animal) | M | The oatmeal fix — specificity becomes signal |
| 6 | Disputed/unresolved event status rendered as competing accounts | S | The cheapest mystery we can buy (pairs with 12.#2) |
