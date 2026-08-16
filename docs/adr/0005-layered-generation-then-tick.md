# ADR-0005: Layered one-shot generation, then monthly ticks

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/world.rs` (generate/tick split), all subsystem modules

## Context

World simulators choose where "geology time" ends and "human time" begins.
Fully co-simulated worlds (erosion running alongside kingdoms) are
scientifically pleasing but explode the state space, wreck performance
budgets, and make tuning nearly impossible — every layer's bugs alias into
every other's. The original game and its strongest references (Dwarf
Fortress, WorldEngine) generate physical geography once, then simulate
history on top of it.

## Decision

We generate the physical world in one deterministic pass pipeline —
terrain → climate → hydrology → biomes → fertility → naming → resources →
settlements → cultures → trade — then advance only the human layer in
monthly ticks (`World::tick`): population, prospecting, markets, tech,
chronicle. Physical fields are immutable after generation; the simulation
reads them but never rewrites them.

## Consequences

- Each layer is testable in isolation against its own diagnostics; tuning
  one band does not move the ground under another.
- Ticks are cheap (thousands of months per second natively) because they
  touch entity lists, not fields.
- Costs: no in-history physical change — rivers don't shift, volcanoes
  don't build islands mid-chronicle. If we ever want scarring events
  (ADR-worthy future decision), they must be explicit exceptions with their
  own determinism story.

## Alternatives considered

- **Continuous co-simulation of geology + history** — 100-1000× the tick
  cost for changes nobody sees at 4 km/cell and decade scale.
- **Re-running generation stages during history** (e.g. reflowing rivers
  after quakes) — breaks the layered determinism contract and every cached
  view; rejected until a concrete feature demands it.
