# ADR-0015: Hand-rolled system lattice, not bevy_ecs

- **Status:** Accepted
- **Date:** 2026-08
- **Touches:** `game/rust/src/systems.rs`, `game/rust/src/world.rs::tick`, `game/rust/src/bin/diagnose.rs::cmd_systems`

## Context

E11.4/E11.5 rebuilt the tick as an ordered lattice of 21 `System` impls
driven by a thin loop. The open question (E11.7) was whether a real ECS
scheduler — bevy_ecs — would earn its place: automatic parallelism from
declared access sets is the whole pitch. The rule for this decision was
numbers on real workloads, not taste. `diagnose systems` now profiles every
system across a grown world (seed 12345, 512², 150 years, 104 towns).

## Decision

Keep the hand-rolled lattice. bevy_ecs is rejected on three measurements:

1. **The serial fraction is 80.5%.** Determinism law (ADR-0003) routes all
   randomness through one PCG stream, and most systems mutate `Peoples`.
   Any scheduler must run those in total order. Amdahl's ceiling on the
   measured profile is **1.243×** — before paying any scheduler cost.
2. **The dispatch we would replace costs 0.27%.** The driver (cadence
   check + dyn call, 21 per month) is noise; there is no overhead for
   bevy_ecs to win back.
3. **The target is wasm without threads.** No SharedArrayBuffer/atomics
   setup is planned, so in the shipped browser build even the theoretical
   1.243× collapses to 1.0× — while the dependency would grow the binary
   against the E6 budget (2.94 of 3.2 MiB sweet) and put a scheduler
   between the determinism gate and the code it gates.

The two real hotspots the profile exposed — `colonize` at 38.6% and
`prospect` at 23.8% — are algorithmic (full-map suitability scans), not
scheduling. They are tracked as roadmap items, where the time actually is.

## Consequences

- `world.rs::tick_profiled` (native-only) and `diagnose systems` keep the
  measurement alive; the suite carries a `[WARN]` tripwire — if the Amdahl
  ceiling ever rises above 1.5×, this ADR's premise has shifted and the
  question reopens with new numbers.
- The access table in `cmd_systems` (`ACCESS`) is the declared write-set of
  every system, checked against the lattice by name at run time — the same
  information an ECS would demand, kept as documentation with a gate.
- Costs: ordering stays a convention enforced by one list in `systems.rs`
  ("reordering is a balance change"), and disjoint-borrow discipline stays
  manual (the wall structs of E11.3 make it tractable).

## Alternatives considered

- **bevy_ecs, parallel schedule** — bounded at 1.243× by the measured
  serial fraction; 1.0× on the threadless wasm target; nondeterministic
  system interleaving would need pinning back to a total order anyway.
- **bevy_ecs, single-threaded schedule** — all of the dependency and
  migration cost, none of the parallelism; strictly worse than the loop.
- **Splitting the RNG into per-system streams** to shrink the serial
  fraction — changes every downstream draw, breaking byte-compatibility
  with the entire report corpus for at best ~24%; not worth it now, and
  orthogonal to the ECS question (revisit only if the tripwire fires).
