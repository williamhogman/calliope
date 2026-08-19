# ADR-0024: The plate-history sketch — deep past as input, never simulation

- **Status:** Accepted
- **Date:** 2026-08-18
- **Touches:** `game/rust/src/plates.rs` (new), `game/rust/src/geo.rs`
  (`heightmap` consumes the sketch), `game/rust/src/world.rs` (the sketch
  rides the `World`, widened with the ocean margins), the diagnose
  harness (`cmd_terrain` plate census and byte-identity checks).

## Context

Calliope's terrain has always been noise-first: continental bulges,
domain-warped fbm, ridged orogeny — a pipeline the literature endorses
for its speed and control (ADR-0005: layered generation, then tick).
Live plate simulation was rejected in that era and stays rejected: a
tectonic integrator is a second simulation loop with its own timestep,
stability problems, and no observer-facing payoff at our cell size.

But the noise-first stance leaves the deep past unexplained. Mountains
rise where ridged noise happened to roll high; no range has an age; the
rock under a mine is a dice roll (the M18–M21 arc has nothing to stand
on). The research corpus (McDonald's clustered convection; the
WorldEngine pipeline) argues noise-as-initial-topography is the weak
link, and Era I of the Five Hundred opens by superseding the rejection
*only as far as a generative sketch*.

## Decision

A **plate-history sketch**, frozen at world-genesis (M16):

- `plates::generate(seed, size)` deals 9–13 plates: greedy max–min
  Voronoi seeds, warped by low-frequency noise so seams wander
  organically. Each plate carries a drift vector, a drift-age in
  megayears, and a continental flag.
- Every plate pair is classified **once** from relative drift —
  convergent, divergent, or transform — and every convergent pair gets
  a collision age keyed to its younger partner.
- Derived grids (owning plate, boundary kind, distance-to-boundary,
  distance-to-convergent-seam, seam age) are inputs to
  `geo::heightmap`: collision seams gate where the orogeny belts rise,
  plate interiors bias coastlines continental-up / oceanic-down.
- **Nothing advances in tick time.** The sketch is state, hashed and
  reproducible, but no system ever writes to it. It is prehistory, not
  history.

## Consequences

- Mountain belts align with collision seams; M17 can age them, M18 can
  hang rock provinces off plate interiors and seams, M22 can read fault
  lines straight from transform boundaries.
- The sketch joins `hash_state`; regenerating a seed must reproduce the
  polygons byte-for-byte (gated in `diagnose terrain`).
- Terrain bands (land fraction, mountain share, landmass census) keep
  their existing sweet ranges — the sketch biases the same passes, it
  does not add relief mass.
- The rejection of *live* tectonics stands. Any future phase wanting
  plates to move in sim time needs its own superseding ADR.

## Alternatives considered

- **Full plate simulation** (PyPlatec-style rigid plates, or clustered
  convection) — rejected again: a second integrator, nondeterminism
  hazards under refactoring, and the observable gain over a frozen
  sketch is nil at 4 km cells.
- **Pure-noise status quo** — rejected: Era I's geology arc (provinces,
  deposits, quakes) needs seams and ages that noise cannot name.
- **Plates as a render-only overlay** — rejected: the point is that
  generation *consumes* the sketch; an overlay explains nothing.
