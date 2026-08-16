# Gap Analysis — Calliope vs the Literature

Measures every system in `SYSTEMS.md` against the research corpus
(`research/01-…11-*.md`, distilled in `research/SYNTHESIS.md`). Verdicts:
**AT PAR** (matches current practice), **PARTIAL** (sound core, missing
named techniques), **GAP** (system absent or structurally short).

## 1. Terrain — PARTIAL

At par: domain-warped fbm + primitives, arcs/hotspots/archipelagos, frame
falloff — a hand-rolled Azgaar-template pipeline the literature endorses.
Short: **no erosion of any kind** — the single biggest physical gap
(digests 01/03/10 all converge on it). Fixes in order: talus thermal pass
(S), stream-power incision reusing `filled`/`dirs`/`discharge` (M),
hillslope diffusion (S), secondary-range pass (S).

## 2. Climate — PARTIAL (strong)

At par: lat-band temperature + lapse + EDT continentality + moisture
advection with orographic extraction and subsidence — matches WorldEngine+
class. Short: static ITCZ (no monsoon/wet-dry seasonality — S fix, high
value), no ocean gyre heat transport (M), no upwind/downwind coast
asymmetry (S).

## 3. Hydrology — PARTIAL (strong)

At par: priority-flood + ε, D8, precipitation-weighted accumulation —
textbook (Barnes/RichDEM validated). Short: boolean rivers — no Strahler
order (S) or width ∝ √Q (S); no endorheic salt-lake basins (M); no
sinuosity at render (M); no seasonal discharge (M).

## 4. Biomes, soils & agriculture — GAP

Whittaker classification and fertility scalar are fine as far as they go,
but the literature's key structure is missing: **crop packages**
(wheat/rice/maize/pastoral from T, P, growing period) with per-package
carrying capacity (0.05→120 p/km² range), the <300 mm pastoral boundary,
and land-per-capita falling with tech. All S-cost items with large
downstream effects (trade patterns, culture fit, K).

## 5. Resources — AT PAR

Discovery/depletion economy (ADR-0011) plus floors (ADR-0013) is ahead of
anything surveyed in the game literature. No action.

## 6. Settlements — PARTIAL

At par: suitability scoring + spacing blackout (= Azgaar), delta/resource
pulls. Short: no rank-size (Zipf) validation or Gibrat-style growth (S);
no Bettencourt sub/superlinear scaling in wealth/infrastructure (S-M);
spacing constants never calibrated against the 15-30 km market-town band
(S); no defensibility/ford/harbor-shelter terms (M); **no abandonment/
ruins lifecycle** (M).

## 7. Cultures & language — PARTIAL

At par: k-means culture blocs (Axelrod-justified), per-culture banks
(= Azgaar name bases). Short: uniform draws (S fix: power-law weights);
culture-blind toponym generics (S-M); no etymology glosses feeding
chronicle prose (M); no language relatedness/drift (L); no religion layer
at all (L, pairs with chronicle pantheon).

## 8. Trade & economy — PARTIAL

At par: terrain-priced anisotropic A* (= Galin et al. 2010), damped/
clamped/shock-split relative pricing (ADR-0012) — the stability backbone
the failure literature demands. Short: **single world market** — no
market areas, no arbitrage meaning for routes (L, the highest-value
economic change); terminal goods — no recipes (M); no famine coupling
(S-M); no merchants (L, after market areas); route rendering lacks
smoothing/junction merging (M).

## 9. Society & polities — GAP

Tech tree and polity ladder exist, but the political layer the literature
centers on is absent: wars never move borders (M), no opinion/coalition
graph (S-M), no legitimacy/asabiyyah cycle so polities never fragment (M),
no sieges despite masonry/engineering techs (S), no vassalage tier (S).
Combat needs only aggregate Lanchester strength — no battle sim.

## 10. Chronicle — GAP (structural)

Emission works, but events are pre-rendered strings — actor references are
discarded at birth, which blocks everything the narrative literature
values: sifting, legends browsing, causal chains. Fix order: structured
events (S-M, prerequisite), named non-ruler entities (M), artifacts with
provenance (M), pantheon citation (M), pacing layer (S), sifter (M-L),
legends browser (L).

## 11. Rendering — PARTIAL

Satellite pass done to a high bar; atlas grammar missing: coastal
vignettes (S), Töpfer-law culling (M), letter-spaced area labels (S),
climate-blended hypsometry (M), multi-directional hillshade (M), texture
shading (M), curved river labels (L).

## 12. Ecology — GAP (absent by design so far)

No wildlife, succession, or disease. Ranked entry points: trade-graph SIR
plagues (L — highest drama per system), Markov vegetation succession with
harvest/fire (M), Ricker wildlife with hunting pressure (M).

## 13. Diagnostics — PARTIAL (strong)

Banded multi-seed harness (ADR-0009) already implements the field's core
prescription. Short: seam-invariant property checks (S each), ERA 2D
histograms (M), between-seed distinctiveness metrics — the oatmeal
detector (M-L), metamorphic direction-of-effect properties (S-M).

## Reading

The physical stack (1-3, 5) is near par — its gaps are *named formulas*
away. The human stack (4, 6-10, 12) is where structural gaps live: the
world lacks locality (markets, famine, plague), consequence (borders,
collapse, ruins), and memory (structured events, entities, citation).
The instrument (13) is one property-suite away from ERA-grade. That
ordering is what `ROADMAP.md` encodes.
