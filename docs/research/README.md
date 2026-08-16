# Research Corpus

A structured reading program across the procedural generation and
worldbuilding literature — blogs, papers, GDC talks, textbooks, postmortems
— distilled into per-domain digests and one cross-cutting synthesis.

## Method

Fifteen themed dockets were researched in parallel waves — 01-11 covering
the technical literature, 12-15 the craft/feel literature — each hunting a
seeded list of primary sources and then following citations outward. Every
digest:

- lists its sources with **URL** and a **depth marker** —
  `READ` (studied), `SKIM` (surveyed for the relevant sections),
  `ABSTRACT` (only the abstract/summary was accessible);
- extracts the techniques and models that matter, with enough math or
  algorithmic sketch to implement from;
- ends with **implications for Calliope** — where our engine already
  matches the state of the art, and where it falls short.

Honesty rule: nothing is cited that was not actually retrieved. Dead links
and paywalled papers are marked as such. No fabricated page numbers, no
invented quotes.

## Digests

| File | Docket |
|---|---|
| `01-terrain-erosion.md` | Terrain, tectonics & erosion |
| `02-climate.md` | Climate & weather simulation |
| `03-hydrology.md` | Hydrology & rivers |
| `04-settlements-roads.md` | Settlements, roads & urbanism |
| `05-history-narrative.md` | History generation, myth & procedural narrative |
| `06-culture-language.md` | Culture, language & naming |
| `07-economy-trade.md` | Economy, trade & production |
| `08-ecology-agriculture.md` | Ecology, wildlife & agriculture |
| `09-politics-war.md` | Politics, war & diplomacy |
| `10-cartography.md` | Cartography & map rendering |
| `11-pcg-theory.md` | PCG theory, evaluation & orchestration |
| `12-worldbuilding-craft.md` | Worldbuilding craft theory (the literary canon) |
| `13-worldsim-feel-doctrines.md` | Feel doctrines of shipped world-simulators |
| `14-toponymy-layered-history.md` | Toponymy as archaeology & geography of situation |
| `15-tellability-chronicle-prose.md` | Tellability & chronicle prose |

## Synthesis

`SYNTHESIS.md` — the cross-domain distillation of the technical corpus
(01-11): recurring principles, the techniques multiple dockets
independently converged on, and the ranked gap list that feeds
`../GAP-ANALYSIS.md` and `../ROADMAP.md`.

`SYNTHESIS-FEEL.md` — the craft-corpus distillation (12-15): the five
feel-laws and the ranked feel-gap list that feeds M6/M9 in
`../ROADMAP.md`.
