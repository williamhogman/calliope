# 06 — Culture, Language & Naming

## Sources

1. **Language Construction Kit** — Rosenfelder — https://www.zompist.com/kit.html — READ. The canonical pipeline: phoneme inventory → phonotactics → morphology → orthography; warns against kitchen-sink phonologies.
2-4. **gen, phono, SCA²** — Rosenfelder — https://www.zompist.com/gen.html, /phono.html, /sca2.html — READ/SKIM. gen: power-law weighting over choices ("flat random is highly unnaturalistic"). SCA²: ordered context-sensitive rewrite rules derive daughter languages from a proto-language.
5. **gen/SCA commentary** — Isaac Karth — SKIM.
6-7. **Lexifer + online port** — Annis — https://github.com/wmannis/lexifer — READ/SKIM. Weighted classes, assimilation filters, declarative vowel-harmony-like rules.
8. **Vulgarlang** — https://www.vulgarlang.com/how-it-works/ — SKIM. Irregularity as a design goal.
9-10. **Awkwords; Kozuka (Rust/WASM)** — READ/SKIM. Pattern-language generators; Kozuka is a Rust precedent.
11-13. **Sonority Sequencing Principle; hierarchy; phonotactics** — Wikipedia — READ/SKIM. Sonority must rise to the nucleus and fall after: why "pl-"/"tr-" exist and "lp-" onsets don't.
14. **Vowel harmony** — READ. First vowel picks a class; later vowels conform (Turkish/Finnish texture) — a cheap one-word post-process.
15-17. **O'Leary naming-language (via Karth); mewo2 repos** — READ/SKIM/ABSTRACT. One phonology, per-culture orthographies = distinct "scripts"; toponym morpheme grammar.
18-20. **Azgaar FMG: name bases, culture sets, religions** — READ/SKIM. Word-list name bases per culture (≈ Calliope's banks — parity confirmed); religions spread by seeded diffusion with expansionism + schism thresholds.
21-23. **Ultima Ratio Regum: cultures, religions, retrospective** — READ/SKIM. Slot-grammar deities (domains, symbols, commandments) whose consequences propagate downstream.
24-25. **Dwarf Fortress language + tokens** — READ/SKIM. ~1700 semantic roots per language; names carry *meaning* ("translatable") — the contrast case to pure syllable banks.
26-28. **Onomastics: specific+generic structure; toponymy; Blair & Tent typology** — READ/SKIM/ABSTRACT. Toponym = specific (descriptive/commemorative/incident/transferred) + feature-typed generic ("-ford", "-by", "Mount-").
29. **UNGEGN exonyms** — SKIM. Endonym vs exonym.
30. **Axelrod culture dissemination** — JCR 1997 — https://web.mit.edu/curhan/www/docs/Articles/15341_Readings/Culture_and_Identity/Axelrod-1997.pdf — READ. Local imitation-with-homophily self-organizes into few large sharply-bordered culture zones — the theoretical justification for k-means-as-proxy.
31-32. **Axelrod successors; cultural-distance metrics** — SKIM.

## Synthesis

Every serious naming generator converges on the four-stage pipeline (inventory → phonotactics → weighting/filters → orthography); believability comes from **restriction and skew**, not variety. Sound-change rules (SCA²) are the standard mechanism for making sibling cultures sound *related* — regular, exceptionless rewrites preserve family resemblance where re-rolled banks cannot. Toponyms are generic+specific compounds with culture-specific formation strategies. DF shows the value of semantic roots: names that mean something feed prose. Axelrod explains why a handful of coherent culture blocs is the *right* emergent shape.

## Calliope

`naming.rs`/`culture.rs`: five fixed banks, uniform draws, culture-blind generics, no semantics, no relatedness between cultures, no religion.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Culture-styled toponym generics + per-culture formation strategy (nordic "-vik/-fjell", hellenic "Cape …") | S-M | Every label instantly reads as belonging to a people |
| 2 | Power-law weighting in `make_word` draws | S | One-line naturalism fix |
| 3 | Etymology glosses on fragments + chronicle hooks ("Frostvik — 'cold bay'") | M | Names that mean something in the prose |
| 4 | Vowel-harmony post-process (needs phoneme templating) | M | Strongest per-line texture cue |
| 5 | SCA²-style drift: derive sibling banks from proto-banks | L | Language *families* matching culture lineage |
| 6 | Exonym/endonym doubling on straddling features | S | Border texture |
| 7 | Religion as culture-conditioned slot grammar | L | New subsystem; pairs with digest 05 pantheon |
