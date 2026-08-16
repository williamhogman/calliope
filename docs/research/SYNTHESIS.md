# Synthesis — What ~280 Sources Agree On

Eleven dockets, ~280 sources spanning geomorphology papers, GDC talks,
generator devlogs, economics, cliodynamics, onomastics and cartography.
Below: the principles that recurred independently across dockets, then the
consolidated gap list that feeds `../GAP-ANALYSIS.md` and `../ROADMAP.md`.

## The ten convergent principles

**1. A generator is a distribution, not an artifact.** (PCG theory, and
implicitly every docket.) Evaluation means characterizing the distribution
across many seeds — coverage, gaps, bias clusters — never eyeballing one
world. Calliope's banded multi-seed harness (ADR-0009) is the right spine;
the missing limbs are 2D expressive-range views, seam invariants, and
distinctiveness-between-seeds metrics.

**2. Erosion is the load-bearing absence.** Three dockets independently
converge on it: terrain (valleys should be carved by the stream-power law,
using drainage area we already compute), hydrology (rivers that "don't look
like they carved anything" is the classic artifact), cartography (texture
shading exists to fake the tactile relief erosion creates). The implicit
O(n) stream-power solver (Braun & Willett) plus a cheap talus pass is the
literature-validated fix, and Calliope's D8/discharge machinery already
provides 90 % of the inputs.

**3. The square-root laws of believability.** The literature is full of
sublinear scaling laws that cost one exponent each and buy proportion:
river width ∝ √discharge; Hack's law L ∝ A^0.57; Zipf rank-size for
settlements; Bettencourt's pop^0.85 infrastructure / pop^1.15 output;
Töpfer's √-law for label density under zoom. Every one of these is a
harness-checkable band. Worlds read as real when their *proportions* are
real.

**4. Locality is where stories live.** A single world market, a static
territory map, a global climate mean — pooling erases narrative. The
converging prescription: market areas with price divergence (Victoria 3,
X4, EVE), famine as a *local* subsistence failure, plagues traveling the
actual trade graph, wars that transfer actual settlements, seasonal rains
that arrive somewhere. Every system that goes local mints chronicle events
for free.

**5. Typed data first, prose last.** (Qud's pipeline, DF legends tooling,
the sifting literature.) Events must carry entity references, not
pre-rendered strings; persistent named entities (people, artifacts, sites)
and their relations are the substrate every downstream feature — sifting,
legends browsing, causal chains — depends on. Retrofitting structure onto
prose is the expensive direction.

**6. Coherence through citation, not repetition.** (URR's axis cultures,
DF's semantic name roots, pantheon design.) Generate a small set of
cultural axioms once — gods, values, name-roots with glosses — then *cite*
them everywhere: omens, festivals, toponym generics, war names. Independent
randomization per surface is what makes generated culture feel incoherent.

**7. One seasonal parameter, outsized returns.** The ITCZ migrating with
the seasons produces monsoons, wet/dry seasons and seasonal deserts for
~five lines. Seasonal discharge produces wadis, flood-pulse agriculture and
trading seasons. Cheap dynamics in the right variable beat expensive
dynamics in the wrong one.

**8. Legibility beats fidelity.** (Banished cut crop rotation; Humankind
chose "frothy" visible grievance meters; RimWorld overrides probability for
pacing.) Simulation depth that no observer can read is cost without value.
Every system should surface its state as explainable map/inspector/
chronicle output — Calliope's "explainable world" instinct is the right
one and should gate new systems.

**9. Self-limiting loops give history a beat.** EU4's aggressive-expansion
→ coalition loop; Turchin/Khaldun's asabiyyah surge-and-decay; price
renormalization against the geometric mean (ADR-0012). Systems need
counterpressures that turn monotonic growth into cycles — otherwise the
first mover snowballs and the chronicle flatlines after the early game.

**10. The atlas is a grammar, not a style.** Imhof's label rules, aerial
perspective, climate-blended hypsometry, coastal vignettes, Töpfer culling
— cartographic authority is a set of learnable filters over data we already
have. The satellite pass is done; the atlas grammar is the remaining half
of "beautiful".

## Consolidated gap list (ranked by value ÷ cost)

| Rank | Gap | Docket(s) | Cost |
|---|---|---|---|
| 1 | No erosion (stream-power incision + talus pass) | 01, 03, 10 | M |
| 2 | Boolean rivers (Strahler order, width ∝ √Q) | 03 | S |
| 3 | Static ITCZ (seasonal shift → monsoons) | 02 | S |
| 4 | Pre-rendered chronicle events (structured actors) | 05 | S-M |
| 5 | No rank-size / scaling laws on settlements | 04 | S |
| 6 | Culture-blind toponym generics; uniform draws | 06 | S-M |
| 7 | One fertility scalar (crop packages, pastoral boundary) | 08 | S |
| 8 | No famine coupling (local subsistence failure) | 07, 08 | S-M |
| 9 | Atlas grammar (vignettes, Töpfer culling, letter-spacing) | 10 | S-M |
| 10 | Wars never move borders (war score → transfer; influence territory) | 09 | M |
| 11 | No legitimacy/rebellion cycle (asabiyyah) | 09 | M |
| 12 | No endorheic basins / salt lakes | 03 | M |
| 13 | Terminal goods (production recipes ore→metal→tools) | 07 | M |
| 14 | No named non-ruler entities / artifacts | 05 | M |
| 15 | Seam invariants + ERA histograms in harness | 11 | S-M |
| 16 | Route rendering: smoothing, junction merge | 04 | M |
| 17 | No pantheon/religion layer | 05, 06 | M-L |
| 18 | Single world market (market areas, arbitrage) | 07 | L |
| 19 | No plagues on the trade graph | 08 | L |
| 20 | Story sifter over the event log | 05 | M-L |
| 21 | Vegetation succession; wildlife layer | 08 | M |
| 22 | Sound-change language families | 06 | L |
| 23 | Merchant agents | 07 | L |
| 24 | Curved river labels | 10 | L |

Deliberately rejected for now (with reasons recorded in the digests):
plate-tectonic simulation (competes with the tuned primitive stack, ADR-0014
identity), D-infinity flow (breaks the DAG), hydrology-first terrain
synthesis (inverts the whole pipeline), full particle erosion in the default
path (determinism + budget risk), freeform AI diplomacy (exploit surface,
Old World lesson).
