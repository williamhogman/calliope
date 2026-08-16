# ADR-0013: Resource floor guarantees ("the floor of fate")

- **Status:** Accepted (backfilled)
- **Date:** 2026-08-16
- **Touches:** `game/rust/src/resources.rs` (guarantee pass after noise placement)

## Context

Resource placement is noise-thresholded over biome/height masks. Across
seeds this frequently produced worlds with **zero** gold or mithril at 512²
(and near-zero stone), silently deleting the late-game arc: no coinage
rushes, no rare-strike chronicle beats, and tech paths gated on missing
inputs. The multi-seed sweep made the failure rate visible.

## Decision

After noise placement, a deterministic guarantee pass enforces per-mineral
minimum seam counts (stone/coal/copper/iron ≥ 4, silver/gold ≥ 2,
mithril ≥ 1), placing shortfalls into the highest fitting ground with a
relaxing height floor for flat worlds, minimum spacing from same-kind
seams, and seeded RNG for richness/knowledge/reserves. Placement order is
sorted and stable, preserving bit-determinism (ADR-0003).

## Consequences

- Every world has a complete economic skeleton; the missing-resource
  harness check went WARN → PASS across all sweep seeds.
- Guaranteed seams still enter play through the discovery system
  (ADR-0011) — a floor on existence, not on knowledge.
- Costs: pure noise-purists lose a little "some worlds are simply poor"
  flavor; judged worth it because poverty of a *kind* (few, remote, late)
  still varies enormously while zero-seam worlds do not tell that story.

## Alternatives considered

- **Retune noise thresholds down** — raises averages but cannot bound the
  tail; some seeds still roll zero.
- **Reroll worlds failing a census** — breaks the "every seed is a valid
  world" contract and makes seed → world non-total.
- **Gate tech on substitutes instead** — hides the problem; the map still
  lacks the seams the chronicle wants to talk about.
