# ADR-0026: One lattice law — the shared grid module

- **Status:** Accepted
- **Date:** 2026-08-22
- **Touches:** `game/rust/src/grid.rs` (new), `game/rust/src/hydrology.rs`
  (re-export face), `game/rust/src/erosion.rs`, `game/rust/src/ice.rs`,
  `game/rust/src/trade.rs`, `game/rust/src/world.rs` (stage boundaries),
  `game/rust/Cargo.toml` (wasm-opt no-inline), `scripts/wasm-audit.sh`.

## Context

By M65 the crate held **three copies of the flow law**. Hydrology owned
the original: the N8 offset table, priority-flood fill, first-wins
steepest descent, the high-to-low index-tied drainage sort, and the
donor-before-receiver accumulation walk. Erosion re-implemented the
sort and the area walk locally (same comparator, unstable sort); ice
called hydrology's functions but carried three private `N4` constants
and five hand-rolled offset-table literals; trade imported the tables
by path. Every copy was behaviorally identical — and that identity was
*unchecked*: nothing stopped the next edit from bending one copy and
silently forking the lattice.

The stakes are total. The lattice law's **order is load-bearing**:
float sums walk the drainage order, ties break by N8 index, and
`hash_state` remembers every bit. Two copies that differ by one
comparator are two different worlds from the same seed.

A second, related finding: the wasm audit (E6.6) named
`GenBuilder::step` the heaviest item in the shipped binary — 9.3%,
~363 KB, the whole generation pipeline merged into one body. Measured
root cause: **binaryen, not LLVM**. rustc emits the ten stage functions
separately (native `nm` shows all ten; `step` itself is 242 B in raw
LLVM output); `wasm-opt -O` then collapses every single-caller function
up the call tree. Rust-side `#[inline(never)]` holds through LLVM and
is invisible to binaryen.

## Decision

- **The lattice speaks once.** `grid.rs` owns N4, N8, DIST,
  `fill_depressions`, `flow_directions`, `drainage_order`,
  `accumulate`, `flow_accumulation` — moved verbatim from hydrology,
  same neighbour order, same tie-breaks. The drainage sort is the
  unstable one (total-order comparator ⇒ identical permutation,
  E5.11); erosion's local copies are deleted in favour of the shared
  calls; ice and trade walk `grid::` tables; every hand-rolled offset
  literal in the four terrain modules is replaced by the named
  constant.
- **The historical face stays.** `hydrology` re-exports the whole
  grid vocabulary, so `hydrology::N8` and friends remain valid for
  the bins and any external reader; library internals say `grid::`.
- **Stage boundaries are binary-visible.** The ten `stage_*` functions
  carry `#[inline(never)]`, and the wasm-opt invocations carry
  `--no-inline=*stage_*` — **before** `-Oz`/`-O`, because binaryen
  applies options positionally and a no-inline mark set after the pass
  pipeline has run marks nothing (measured: flag-after-`-O` is a
  byte-identical no-op).
- **Identity is the gate for the refactor itself.** The move landed
  against a frozen baseline: 11-layer `earth-hash` plus seismic ledger
  on five seeds, and the determinism lane on two — all bit-identical
  before and after.

## Consequences

- One edit point for the flow law; a fork now requires changing the
  one module every consumer names.
- The audit reads the pipeline in its own vocabulary: heaviest item
  fell from `step` 9.31% to `stage_dawn` 3.44%, and each stage's
  weight is its own row — a future regression names the stage that
  grew. Shipped binary shrank ~6 KB in the bargain.
- The doc headers of geo/erosion/hydrology/landform now state their
  place in the fixed pipeline (rock → ice → water → soil → landform);
  the import DAG lint (E11.8) continues to hold them leaf-shaped.
- `grid.rs` inherits hydrology's square-grid assumption
  (`idx / size`, `idx % size` with `size = rows`); the worlds are
  square today, and any future non-square grid must fix the one
  module rather than three.

## Alternatives considered

- **Leave the copies, add an equivalence test** — rejected: a proptest
  can witness agreement on sampled worlds but cannot make three copies
  one object; the next edit still forks silently until the test runs.
- **Generic grid abstraction (traits over neighbourhoods)** — rejected:
  the law is eight offsets and three loops; a trait lattice is
  convenience layering over nothing (ADR-0022's instinct).
- **Accept the `step` monolith as harmless** — rejected: the audit
  exists to name monsters, and a 9.3% item that hides ten stages makes
  every future size regression anonymous.
