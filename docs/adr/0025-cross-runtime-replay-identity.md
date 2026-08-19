# ADR-0025: Cross-runtime replay identity for gated ledgers

- **Status:** Accepted
- **Date:** 2026-08-18
- **Touches:** `game/rust/src/seismic.rs` (rational ramp, reciprocal
  tail), `game/rust/src/plates.rs` (squared distances, √-normalized
  drift vectors), `game/rust/src/noisegen.rs` (fixed-width shuffle),
  `game/rust/src/bin/diagnose.rs` (`seismic-hash`, `seismic-debug`),
  `scripts/wasm-replay.mjs`, `game/rust/scripts/report.sh`
  (earth-wasm lane).

## Context

M22's gate demands the seismic event log replay **byte-identical from a
fixed seed across native and WASM** — the first check in the suite that
compares state across runtimes rather than across runs. Two independent
divergence classes surfaced immediately, both invisible to the
native-only determinism lane (ADR-0003 guarantees one runtime, not
two):

1. **libm transcendentals.** `sin`, `cos`, `exp`, `hypot` are not
   required to be correctly rounded; glibc's answers differ in the last
   ulp from the Rust `libm` crate the wasm32 target compiles in. One
   ulp in a plate drift vector flips a convergent/transform
   classification near the 0.35·rel threshold; one ulp in a hazard
   ramp flips an `rng < p` rupture draw. Diverged in the field: plate
   drift (`dir.cos()`), pair classification (`hypot`), the seismic
   ramp (`exp`).
2. **Pointer-width-dependent RNG consumption.** `Uniform<usize>`
   samples at native width: 32-bit draws on wasm32, 64-bit natively —
   a different number of PCG words consumed, so every draw after the
   first range sample sees a different stream. Diverged in the field:
   the Fisher–Yates shuffle in `Perlin3::new`, which silently gave the
   two runtimes *different permutation tables* — every noise field in
   the generator differed across runtimes, and had since the first
   wasm build. Nothing noticed because nothing compared.

## Decision

Any state a gate replays across runtimes must be computed under two
disciplines, enforced by construction on the whole dependency path:

- **IEEE-exact ops only**: `+ − × ÷ √` (and integer/bit ops) are
  correctly rounded per IEEE 754 and bit-identical on every target;
  libm transcendentals are banned. Where a curve shape wants `exp` or
  `ln`, substitute an algebraic stand-in: the rational saturation
  `t/(t+τ)` for `1−e^(−t/τ)` (seismic reload ramp), a reciprocal draw
  `a/(b−u)−c` for the exponential Gutenberg–Richter tail (magnitudes).
  Comparisons prefer squared distances over `hypot`; unit vectors come
  from √-normalized coordinate draws, not `sin/cos` of an angle.
- **Fixed-width RNG draws**: never sample `Uniform<usize>` or any
  pointer-width type on a gated path. Ranged integer draws use an
  explicit u64 multiply-shift (`(u64 as u128 * n) >> 64`), one PCG
  word per draw on every target.

The instrument for the invariant: `diagnose seismic-hash` /
`seismic-debug` print the native leg, `scripts/wasm-replay.mjs` runs
the shipped wasm-bindgen module under bun and prints the same, and the
`earth-wasm` report lane fails the suite on any mismatch. The debug
form prints per-layer sub-hashes (plate table, ownership grid,
boundary grid, fault table, clocks, log) so a future divergence names
the layer it lives in.

Scope: the discipline binds the *gated* paths — today the plate
sketch → fault seams → seismic ledger chain. The rest of the
generator (heightmap, climate, erosion) keeps its transcendentals;
its cross-runtime identity is not claimed by any gate. Any future
phase whose gate says "across native and WASM" inherits both rules
for its whole input cone, and extends the replay script rather than
minting a new one.

## Consequences

- The seismic ledger — seams, clocks, epicenters, magnitudes, months —
  is provably one object in both runtimes; the browser's quake record
  *is* the harness's quake record.
- The `Perlin3` shuffle fix changed every permutation table, which
  re-rolled every noise field and thus every world. A one-time full
  rebaseline of the report suite rode the M22 landing; native runs
  now also match what the shipped wasm generates.
- Curve substitutions are approximations of shape, not of values —
  bands were re-tuned against the rational forms (τ as half-load age
  rather than 1/e age).
- Cost of the discipline is negligible: the substitutes are cheaper
  than the libm calls they replace.

## Alternatives considered

- **Ship `libm` (the crate) on both targets** for identical software
  transcendentals — rejected: slower on native, still leaves the
  `usize` RNG trap, and quietly re-invites divergence with every new
  math call; the exact-ops discipline is checkable by reading a diff.
- **Fixed-point arithmetic on gated paths** — rejected: heavier
  rewrite for no additional guarantee once ops are IEEE-exact.
- **Relax the gate to native-only replay** — rejected: the browser is
  the shipped artifact; a ledger that only replays in the harness
  proves the wrong thing.
