# 11 — PCG Theory, Evaluation & Orchestration

## Sources

1. **Procedural Content Generation in Games** — Togelius, Shaker, Nelson (2016) — https://www.pcgbook.com/ — SKIM. The textbook: search-based, constructive, grammar, constraint methods, evaluation.
2. **Expressive Range Analysis** — Smith & Whitehead (2010) — ABSTRACT. Run the generator across many seeds, plot metric-pair histograms → coverage, gaps, bias clusters. The founding evaluation method.
3-5. **Launchpad; Smith dissertation; Tanagra** — SKIM/ABSTRACT. ERA's origin systems; designer-facing evaluation tools.
6-7. **Sentient Sketchbook; Mixed-Initiative Co-Creativity** — Yannakakis, Liapis et al. — SKIM. Live metric feedback + constrained novelty search.
8. **Orchestrating Game Generation** — Liapis, Yannakakis, Nelson, Preuss, Bidarra (IEEE ToG 2018) — https://repository.falmouth.ac.uk/2977/1/OrchestratingGenerators_IEEEToG.pdf — READ. Multi-generator coordination: pipelines, hierarchical planners, feedback loops; coherence needs explicit contracts or a mediating layer.
9. **Compositional PCG** — Togelius et al. (FDG 2012) — SKIM. The canonical failure: stage B assumes an invariant stage A never promised; bug surfaces in stage C.
10-11. **So You Want to Build a Generator (oatmeal problem)** — Kate Compton (2015) — https://www.tumblr.com/galaxykate0/139774965871/so-you-want-to-build-a-generator — READ. Combinatorial variety ≠ perceptual variety; budget weirdness; measure surprise, not entropy.
12. **Tracery** — Compton et al. — SKIM. Author-facing grammars need tracing tools.
13. **Danesh** — Cook, Gow, Smith, Colton — SKIM. Productized ERA: auto metric logging, extremes browsing, near-duplicate detection.
14. **ANGELINA** — Cook et al. — ABSTRACT. Whole-game orchestration under one fitness loop.
15-17. **WFC is Constraint Solving in the Wild (Karth & Smith 2017); Merrell comparison; ToG WFC** — READ/SKIM/ABSTRACT. WFC = AC-3 propagation over adjacency CSPs; coherence by construction, not validation.
18-19. **ASP for PCG; Mechanizing Exploratory Game Design** — Adam Smith & Mateas — SKIM/ABSTRACT. Declare the design space; solver guarantees invariants.
20. **ERaCA** — Kreminski et al. (2022) — SKIM. Realized diversity vs theoretical capacity.
21. **Generators that Read** — Kreminski et al. (2020) — ABSTRACT. Later stages should *interpret* earlier outputs, not treat them as opaque grids.
22-24. **PCG RNG family** — O'Neill (2014); Lemire commentary; Vigna critique — SKIM. Splittable per-subsystem streams; verify the specific variant, not the family name.
25-26. **Simplex noise demystified** — Gustavson (2005); noise comparison (2020) — SKIM/ABSTRACT. Directional-artifact differences only visible in aggregate stats.
27. **PCG Benchmark** — Khalifa et al. (2025) — https://arxiv.org/html/2503.21474v1 — SKIM. Community consensus: report quality, diversity, controllability.
28. **WFC repo** — Gumin — SKIM.

## Synthesis

A generator is a **distribution**, not an artifact; rigor means characterizing the distribution. ERA (multi-seed metric clouds) answers range/gaps/bias; Compton's oatmeal problem warns that numeric variance ≠ felt variety — measure player-legible qualities and structural distinctiveness between seeds. Constraint methods (WFC/ASP) buy per-construction guarantees at the seams where imperative pipelines silently break (Togelius's stage-B-assumes bug class). Orchestration wants explicit, checkable inter-stage contracts. Determinism engineering (split PCG streams per subsystem — Calliope's derived-stream discipline, ADR-0003) prevents cross-layer correlations only ERA would catch.

## Calliope

Note: this draft's tree predates the diagnostics harness; **mainline already has** `diagnose` with banded PASS/WARN/FAIL checks, multi-seed sweeps, determinism/state-hash gates and `report.sh` aggregation (ADR-0009) — which covers techniques 1-2 below. Remaining gaps:

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Multi-seed sweep → percentile bands | done (mainline) | — |
| 2 | PASS/WARN/FAIL banding | done (mainline) | — |
| 3 | Seam invariants as explicit checks (rivers monotonically descend `filled`; every settlement route-reachable or rescued; pack/unpack byte-identity) — grow the property list | S each | Converts silent seam bugs into failures |
| 4 | ERA proper: 2D metric-pair histograms (e.g. land % × biome entropy; settlement count × route length) exported per sweep | M | Sees coverage gaps 1D bands cannot |
| 5 | Oatmeal detector: structural distinctiveness metrics between seeds (biome patch fragmentation, settlement-spacing entropy, feature-type Jaccard between seed pairs) | M-L | Measures whether seeds *feel* different |
| 6 | Metamorphic properties (rainfall↑ ⇒ river cells not↓, across seeds) | S-M | Catches direction-of-effect regressions |
| 7 | Constraint solver at one high-risk seam | L | Only if a seam keeps breaking |
