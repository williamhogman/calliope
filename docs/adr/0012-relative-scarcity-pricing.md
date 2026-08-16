# ADR-0012: Relative-scarcity market pricing

- **Status:** Accepted (backfilled)
- **Date:** 2026-08-16
- **Touches:** `game/rust/src/economy.rs::update_prices`

## Context

The first market model scaled demand for every good with total world
population while supply scaled only with local producing workforces. As the
world grew, *every* price rose toward the clamp: by year 150, ~99 % of goods
sat pinned at 5× base value (caught by the economy diagnostic). A price
that never moves relative to its neighbours carries no information; rushes,
shocks and trade all lost meaning.

## Decision

We price goods by **relative** scarcity: per-good demand pressure
`(demand_weight + ε) / (supply + ε)` is normalized by the geometric mean of
all goods' pressures, so the market has an internal zero. Price targets are
`base · (pressure/gm)^0.55`, clamped to `0.3×…5×` base, smoothed 75/25 per
month. A growing world no longer inflates everything at once; only goods
scarcer than the market average rise.

## Consequences

- Prices spread across the band instead of pinning (harness: pinned-share
  check went FAIL → PASS); staples sink, scarce metals stay dear but mobile.
- Discovery/depletion shocks (ADR-0011) read clearly against a stable
  baseline.
- Costs: absolute price levels are no longer meaningful across worlds —
  only ratios are; anything consuming prices (wonder gates, treasury flows)
  is calibrated against ratios and re-tuned via the harness.

## Alternatives considered

- **Scale demand by population, per good** (the original) — structurally
  pins all prices in any growing world.
- **True agent bazaar (BazaarBot-style belief updating)** — richer, but a
  large jump in state and tick cost; staged as roadmap work on top of this
  stable baseline, not instead of it.
- **Hard price caps per good class** — treats the symptom; information
  content of prices stays dead.
