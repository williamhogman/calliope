# ADR-0009: Native diagnostics harness as the tuning gate

- **Status:** Accepted (backfilled)
- **Date:** 2026-08-16
- **Touches:** `game/rust/src/bin/diagnose.rs`, `game/rust/scripts/report.sh`, `game/reports/`

## Context

Balancing a layered simulator by eyeballing the map does not scale: desert
share, price pinning, growth pacing and river density all drifted out of
believable ranges at various points, and each was caught late, by a human,
on a single seed. The Rust core builds natively, so the whole simulation can
be measured headless at ~150 ms per world.

## Decision

We maintain a native diagnostics binary (`diagnose`) with subcommands for
terrain, climate, hydrology, resources, civilization, economy, determinism,
benchmarks and multi-seed sweeps. Every metric is evaluated by a `Checks`
framework against two bands: a sweet range (PASS) and hard limits
(WARN/FAIL). `scripts/report.sh` runs the suite across seeds and aggregates
`game/reports/SUMMARY.txt`. The rule: **no balance or systems change lands
without a clean run**, and every new system ships with new checks. State
comparison uses `hash_state` (fields + entities), never the packed payload,
whose header holds wall-clock timings (ADR-0007).

## Consequences

- Tuning is a read-report → adjust → re-run loop with regressions caught in
  minutes across seeds — this loop found and fixed price pinning, growth
  saturation, river overdensity, missing gold/mithril, and stranded towns.
- The bands encode design intent (e.g. desert 12-28 % of land, pop still
  growing at half-run) as executable documentation.
- Costs: bands need recalibration when design intent changes — which is by
  design, since that forces the change to be stated.

## Alternatives considered

- **Unit tests only** — verify code paths, not world quality; "is 35 %
  desert too much" is a band question, not an assert.
- **Playwright-based visual checks** — orders of magnitude slower, and
  measures the renderer, not the simulation.
- **Tuning by inspection** — where we started; repeatedly missed multi-seed
  regressions.
