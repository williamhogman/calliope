# ADR-0010: Terrain-priced trade with sea/land asymmetry

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/trade.rs`, `naming.rs` (passes/fords), route rendering

## Context

Early trade routes were shortest-path lines with mild terrain costs; they
crossed mountains casually and ignored water. Historically, pre-modern sea
freight was roughly an order of magnitude cheaper than land carriage, and
geography — passes, fords, navigable rivers — decided where wealth flowed.
The design goal: geography should *price* the human world, so that ports,
pass towns and river cities emerge instead of being scripted.

## Decision

We route trade over a cost grid where sea travel costs ~1/9 of base land
movement, slope/altitude/biome multiply land costs (desert +3×, ice +9×,
forests elevated), navigable high-discharge rivers act as barge highways
(1.1×), and crossing the shoreline pays a harbour fee. A* finds routes;
each leg records its mode (road / sea lane / barge) for rendering; route
squeeze points become named Passes and Fords; coastal route towns become
harbours with trade-income and growth bonuses. Towns stranded by the
viability cap get one rescue lifeline route at whatever price terrain asks.

## Consequences

- Ports, strait towns and pass towns emerge unscripted; sea lanes dominate
  long hauls, exactly as intended.
- Route mode drives rendering (amber road / dashed blue lane / teal barge),
  making the economics legible on the map.
- Costs: cost constants are a believability surface needing bands in the
  harness; pathfinding is the priciest generation stage after terrain.

## Alternatives considered

- **Euclidean or mildly-weighted routes** — produced geography-blind lines;
  the map read as decoration, not economics.
- **Full gravity-model trade assignment** — better flows, but needs local
  markets first; staged as roadmap work, not a blocker for pricing terrain.
