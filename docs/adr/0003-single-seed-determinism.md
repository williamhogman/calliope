# ADR-0003: Single-seed determinism with derived RNG streams

- **Status:** Accepted (backfilled) · reaffirmed at the Era I gate
  (M65): 300 years replayed under two tick chunkings to one state and
  one town ledger (`diagnose gate` structural leg), cross-runtime
  ledger identity per ADR-0025
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/util.rs`, every generation and tick module

## Context

A world must be reproducible: the same seed must give the same terrain, the
same dawn towns, the same century of history — natively, in WASM, today and
after refactors. At the same time, subsystems must be able to evolve without
scrambling each other's randomness (adding a noise octave to terrain must not
rename every king).

## Decision

We derive every random stream from the single world seed with fixed integer
offsets/multipliers per subsystem (`util::rng(seed * k + c)` with per-module
constants), using PCG (`rand_pcg`) for portable, high-quality streams.
Iteration orders that feed RNG are fixed (row-major scans, sorted keys —
`BTreeMap` over `HashMap` wherever order reaches output). Wall-clock time
never influences state; the packed payload's stage timings are the one
sanctioned nondeterminism and live only in the header (excluded from state
hashing, see `diagnose.rs::hash_state`).

## Consequences

- Bit-identical regeneration across native and WASM; the determinism
  diagnostic (`diagnose determinism`) is a hard gate: generation, simulation,
  and chunking invariance must all hash equal.
- Subsystem streams are independent: changes stay contained.
- Costs: no `HashMap` iteration into anything user-visible; parallelism must
  be deterministic-reduction only; float math must avoid
  platform-divergent operations.

## Alternatives considered

- **One shared RNG threaded through generation** — any new draw anywhere
  reshuffles everything downstream; unmaintainable.
- **Hash-based per-cell randomness everywhere** — right for field noise
  (we do use it there), wrong for sequential simulation events.
- **Tolerating cross-platform drift** — would fork native diagnostics from
  the shipped WASM world; the harness would test a different world than the
  one users see.
