# 09 — Politics, War & Diplomacy

## Sources

1. **Kicking Butt by the Numbers: Lanchester's Laws** — Ernest Adams (2004) — https://www.gamedeveloper.com/design/the-designer-s-notebook-kicking-butt-by-the-numbers-lanchester-s-laws — READ. Linear law (melee, attrition ∝ enemy count) vs square law (concentration quadratically dominant); the chosen law encodes whether mass or quality wins.
2. **Lanchester & RTS design** — evizaer (2010) — SKIM. Square law ⇒ doom-stacking unless terrain/counters break it.
3-4. **Lanchester fighting strength papers** — Intech 2023/24 — ABSTRACT. Heterogeneous/ternary extensions.
5-7. **CK3: claims/CB chains; schemes; CB wiki** — SKIM/ABSTRACT. Conquest gated behind fabricated legitimacy; covert actions with discovery risk.
8. **Old World Designer Notes #10: Diplomacy** — Soren Johnson (2021) — https://www.designer-notes.com/old-world-designer-notes-10-diplomacy/ — READ. Freeform bargaining AIs get exploited; rigid trust meters feel dead; root diplomacy in concrete legitimacy structures.
9. **Playing to Lose** — Johnson (GDC 2008) — SKIM. Fun AI ≠ optimal AI.
10-13. **EU4: aggressive expansion; coalitions; CBs; AE management** — READ/SKIM. Opinion −200..+200; per-province AE decaying over decades; grievance threshold → joint coalition war. A self-limiting expansion loop.
14-16. **Game AI Pro 2 ch. 29-31: influence maps (Mark, Dill, Lewis)** — READ/SKIM. Layered decaying scalar fields from point sources; point-based falloff variant fits settlement-as-source worlds.
17. **Toward Cliodynamics** — Turchin (2011) — https://escholarship.org/content/qt82s3p5hj/qt82s3p5hj.pdf — READ. Structural-demographic cycles: elite overproduction + immiseration + fiscal distress → instability waves (~50-100 y).
18. **A Theory for Formation of Large Empires** — Turchin (JGH 2009) — READ. Imperiogenesis at metaethnic frontiers; spatial asabiyyah model — cohesion forged by frontier conflict, diffusing to neighbors.
19-20. **Cliodynamics site; Secular Cycles** — SKIM/ABSTRACT. 2-3 century growth→stagflation→crisis→depression cycles across eight societies.
21. **Asabiyyah** — Wikipedia — SKIM. Khaldun: solidarity decays over 3-4 generations (~120 y) of settled comfort.
22. **Devereaux: Teaching Paradox CK3** — SKIM. Caution: one culture's legitimacy math misrepresents others.
23-25. **Humankind "frothy" diplomacy; vassals; Stellaris Overlord retrospective** — READ/SKIM. Legible grievance/war-support meters beat deep hidden sims; vassalage/tribute as intermediate war outcomes.
26-28. **Castle economics** — Brauer & van Tuyll (READ: Caernarfon 12 y, £16-27k); Bachrach (ABSTRACT); Danish ring-forts (SKIM). Fortification as opportunity-cost treasury sink.
29. **Turchin spatial model** (dual-use of #18) — reaction-diffusion border formation driven by conflict intensity.
30. **Deriving game mechanics from history** — Game Developer (2022) — SKIM.

Honestly flagged: Voronoi-game/cross-diffusion math papers and DF civ-claim internals were only visible as abstracts/snippets — not cited as read.

## Synthesis

Wars matter narratively only if they **move the map**, and polities feel alive only if cohesion visibly rises and falls. Combat: aggregate Lanchester (strength = pop × tech × fortification) — no battle sim needed. Territory: EU4's triad (war score → named peace terms; aggressive-expansion grievance; coalition threshold) is a self-limiting expansion loop that generates causal stories. Influence maps make borders live: settlement-sourced decaying fields, weight dropping under siege. Pacing: Turchin/Khaldun cycles — an asabiyyah/legitimacy stat surging at conflict frontiers and decaying ~3-4 generations gives collapse and renewal a beat. Diplomacy: keep it legible and eventful (grievances, war support), never a hidden optimizer.

## Calliope

`society.rs`/`chronicle.rs`: wars drain treasuries and raid but never transfer settlements; territory is static culture paint; no opinion graph, legitimacy, rebellion, sieges, or vassalage; polities only ascend.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | War resolution = territorial change (war-score accumulator → settlement transfer at peace) | M | Wars leave marks; the single biggest political payoff |
| 2 | Legitimacy/asabiyyah stat → rebellion/fragmentation rolls (Turchin/Khaldun curve, per-culture-style decay) | M | Collapse & renewal cycles; polities can descend |
| 3 | Opinion matrix + coalition threshold (EU4 pattern) | S-M | Emergent grudges and multi-culture wars |
| 4 | Vassalage/tribute as alternative peace outcome | S | A whole political tier for one field + one transfer |
| 5 | Siege state machine gated on masonry/engineering; fortification as treasury sink (real cost ratios) | S | Makes defense techs mechanically real |
| 6 | Influence-map dynamic territory (pop/tech/war-weighted falloff kernels) | M | Prerequisite for #1/#5 being visible |
