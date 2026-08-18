# ADR-0020: Overstretch as span of control, not population mass

- **Status:** Accepted (supersedes the overstretch formula of ADR-0019)
- **Date:** 2026-08-17
- **Touches:** `game/rust/src/civ.rs` (`CAP_TOWNS`, `D_SPAN`,
  `COLLAPSE_STRAIN`, `STRAIN_RELIEF`, `DARK_AGE_YEARS`, the metrics
  loop), `bin/diagnose.rs` (driver + span rows)

## Context

ADR-0019 defined the M13 overstretch index as `Σ pop^0.85 / capacity`
with a fixed per-realm capacity (Bettencourt reuse). Measured against
the M13.4 gate it fails structurally: on seed 12345 at 512², stretch
sat at 5–28 from year 80 onward — never below the 0.95 golden gate —
so in 300 years no golden age dawned and no arc completed. Population
grows ~870× over the run while capacity is static, so no constant is
right on both sides of the growth curve. Population mass is also the
wrong load: treasuries scale with headcount, but what breaks empires
is administering *places* at *distance*.

Two further failures surfaced during calibration, both measured:

1. **Morale collapse can never bind.** Court-rot nudges applied by the
   yearly civ pass (−0.012 asab) are erased by the monthly political
   tide, which restores legitimacy 12× as often. The
   asab/legit collapse gate alone produced 0 falls in 300 years.
2. **The strain clock only ran under Golden.** A family that sprawled
   past its writ while still Rising never waned — Kalliopia sat
   "rising" at stretch 2–15 for 150 years.

## Decision

Administrative load is **span of control**: every member town costs one
court plus a remoteness surcharge, `Σ_towns (1 + d/D_SPAN)` where `d` is
distance to the civilization's anchor seat and `D_SPAN` = 96 cells
(≈384 km). Capacity is courts the family can staff: `CAP_TOWNS(12) ×
crowns × (1 + 0.20·era) × (0.70 + 0.60·asabiyyah)`. Calibrated on seed
12345 @512: a compact 2-crown civ (avg remoteness ~55 cells) clears the
golden gate; a 1-crown sprawler over 44 towns at ~150 cells breaks it.

The **Tainter clause**: 34 net strained years end the arc regardless of
morale, with the strain clock running in Rising, Golden and Waning
alike. Relief pays the clock down double (`STRAIN_RELIEF` = 2/yr), so
collapse only lands on families whose fragmentation never actually
shortens the writ — the tail outcome, not the default. Court-rot under
Waning scales with stretch (`×min(stretch,5)`) so decline is legible in
the political numbers even though it is not what kills.

`DARK_AGE_YEARS` (55): a fallen family is not re-counted as a
civilization while its peoples lately belonged to a closed one; without
it the interregnum was cosmetic — the same closure re-minted next year.

Consequences designed in, then verified by the harness:

- **Scale-free over growth** — town count plateaus with the map, so a
  mature world is not permanently overstretched by exponential pop.
- **Fragmentation relieves stretch** — successor realms raise `crowns`
  over the same towns; the cycle, not a one-way ratchet. Renaissance
  (Waning → Golden) stays reachable when relief lands in time.
- **Distance is the killer** — far conquests push stretch faster than
  dense heartland growth (the Tainter/Turchin shape in the corpus).

Measured at 512²/300y: arcs completed 5 / 3 / 2 on seeds 12345 / 777 /
90210 (band sweet 1–4, hard 0.5–8), golden dawns 2–4, lineage
succession visible (Weal of Hrimgard → Circle of Thorstad).

The pop^0.85 term remains correct where ADR-0019 borrowed it from
(M2.4 infrastructure upkeep); superseded is only its use as the
civilizational overstretch load.

## Alternatives considered

- **Recalibrate `CAP_BASE` upward** — fails structurally: any constant
  is wrong on one side of an 870× growth curve.
- **Capacity ∝ population** — scale-free but inert: the gate never
  binds and M13.3 becomes decoration.
- **Stronger court-rot instead of the strain clause** — fighting the
  monthly tide with yearly nudges needs rot so violent it deletes the
  renaissance path; the clock is deterministic and self-relieving.
