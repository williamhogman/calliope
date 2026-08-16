# ADR-0011: Hidden-deposit discovery/depletion economy

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/resources.rs`, `world.rs` (prospecting), `economy.rs` (shocks)

## Context

With all mineral deposits visible from the dawn of the world, the economic
map was static: the best sites were claimed in year one and nothing about
resources ever surprised anyone again. Real resource economies are driven
by *information* — strikes, rushes, exhaustion — and the chronicle needed
those beats.

## Decision

Mineral deposits start mostly hidden (`known: false`, per-kind dawn-
knowledge probabilities); settlements prospect their hinterlands monthly
with odds shaped by distance, rarity and prospecting tech; a far-venture
channel lets distant rare seams be found late. Discovery and exhaustion
fire market shocks (price drop on a major strike, spike on depletion).
Non-renewable deposits carry finite reserves, deplete under active mining,
and close with a chronicle event; depleted mines render hollow-grey.
Known, unworked seams project a price-weighted pull onto colony-site
scoring, so mining camps found in hungry country when the market runs hot.

## Consequences

- The economy has an arc: strikes pace themselves across a century
  (18-29 per 100 y in harness runs), gold rushes move population, exhaustion
  retires districts.
- Tech gating on *discovered* deposits makes prospecting strategically
  meaningful (no bronze age without found copper and tin-class inputs).
- Costs: dawn-knowledge and strike pacing are tuned bands (too fast = the
  static world returns; too slow = tech stalls — guarded by harness checks
  and by resource floors, ADR-0013).

## Alternatives considered

- **All deposits known at dawn** — the static world this replaces.
- **Random event-driven "discoveries" decoupled from geology** — cheaper,
  but strikes would land anywhere, severing the link between geology,
  markets and settlement that makes the system explainable.
