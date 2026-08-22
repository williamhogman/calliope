# ADR-0027: The compute lane and the one coast law

- **Status:** Accepted
- **Date:** 2026-08-22
- **Touches:** `game/rust/src/compute.rs` (new),
  `game/rust/src/shaders/coastdist.wgsl` (new), `game/rust/src/render.rs`
  (coast distance moved out; bring-up contract), `game/web/js/render/compositor.js`
  (JS twin), `game/web/js/gpu.js`, `game/rust/src/bin/diagnose.rs`
  (`diagnose compute`), `game/rust/scripts/report.sh` (gpu-flavor lane),
  `scripts/browser-probe.py`.

## Context

M67's spec asked for two things at once: generalize GPU compute into a
reusable engine facility, and migrate the displaced GPU-erosion pass as
its first client. Building it surfaced a conflict the spec could not
have seen from where it was written:

- **Erosion cannot move to a device without forking the world.** The
  fluvial ledger walks the drainage tree donor-before-receiver in f64,
  and `hash_state` remembers every bit (ADR-0003). WGSL has no f64, and
  the walk is sequential by construction — a parallel relaxation is a
  *different algorithm*, not the same one faster. A GPU erosion pass
  would either fork determinism across devices or force a fixed-point
  recast of the era's most numerically sensitive stage for zero measured
  need (erosion is ~290 ms of a ~3 s generation).
- **A "CPU-fallback contract" that never executes is a claim.** The
  sandbox's headless Chromium exposes no `navigator.gpu`; a lane whose
  GPU leg only runs on developer machines is gated by hope. Measured
  during bring-up: native wgpu *does* find mesa's software Vulkan
  adapter (lavapipe) when pointed at `lvp_icd.x86_64.json` — the kernel
  can execute and be compared in CI, headless, every suite run.
- Meanwhile the crate carried **two copies of the coast-distance law**
  (a 1/1.4 chamfer in render.rs and compositor.js — the same silent-fork
  shape ADR-0026 just closed for the flow law), and the chamfer's ~8%
  radial error is why close-in shore rings read slightly square.

## Decision

- **Simulation truth never moves to a device.** The lane is built for
  display-side derived state — fields that are recomputed from packed
  truth and never enter `hash_state`. The GPU-erosion Ready item closes
  as this decision, not as code: the lane exists, erosion is not its
  client, and any future device-side *simulation* requires a superseding
  record here first.
- **`compute.rs` is the facility M67 asked for**: buffer staging,
  dispatch sizing, readback synchronization and the fallback contract as
  shared code (`ComputeLane`), so the next pass registers a kernel and a
  CPU twin instead of duplicating wgpu boilerplate.
- **The first client is the coast law, and the law is one object**: an
  integer jump-flood (JFA) — seeds are u32 cell indices, distances exact
  u32 squares, ties break toward the smaller index. Integer end to end
  means the WGSL kernel and the CPU twin produce **byte-identical seed
  fields** on any conformant device; that byte-parity *is* the contract,
  not an epsilon. Three executors, one law: `coastdist.wgsl`, the Rust
  twin in `compute.rs`, and the JS port in compositor.js for the
  wasm-less path. The chamfer copies are deleted.
- **Execution, not claim.** `diagnose compute` holds the JFA against an
  exact Euclidean distance transform (Felzenszwalb–Huttenlocher, the
  harness-only referee) and runs the GPU leg on lavapipe in the suite
  (`report.sh` builds a gpu-flavored diagnose in its own target dir —
  the assay-profile doctrine — and sources the adapter via nix). Referee
  comparisons are exact integers on both sides — comparing rounded f32
  distances counted 1–2% phantom misses with zero real error.
- **The shipped wasm carries no device executor.** Linking the browser
  bring-up executor measured +108 KB of wgpu/naga compute paths
  (3.35 → 3.45 MiB, ~214 new linked items), breaching the E6.4 hard
  band (≤3.4 MiB) for a leg whose output no pixel consumes — the
  uploaded texture is the twin's on every path. Browser bring-up
  therefore records capability and the twin-is-law verdict only; the
  lane's device side compiles under the native `gpu` feature alone. The
  first per-frame device client (Era II's sky is the expected customer)
  ships the executor and pays its weight inside that phase's budget.
- **Production truth stays on the CPU twin.** The texture render.rs
  uploads is always the twin's output — equal to the GPU's wherever the
  contract holds, present even where no adapter exists. A per-frame
  client (Era II's sky is the expected customer) is what flips
  production dispatch to the device, riding this same contract.

## Consequences

- Measured envelope at 512² across the suite seeds: worst
  |jfa−exact| 0.764 cells, 1 miss in ~485k sea cells, CPU twin 53–55 ms
  once per world upload. Bands derive from the display consumer's
  physics — sub-cell error is invisible under bilinear resampling — so
  sweet is ≤1 cell, not the measured best dressed up as a law.
- Shore rings are now truly circular where the chamfer read ~8% square;
  all three paths ring the same shore.
- The lane's next client pays for a kernel and a twin, nothing else;
  the fixture handshake and the lavapipe leg come free.
- The determinism perimeter (ADR-0003/0025) gains an explicit wall:
  the lane refuses f64 clients by construction, because WGSL has none.

## Alternatives considered

- **GPU erosion as specced** — rejected: forks determinism or forces a
  fixed-point recast of the fluvial ledger; no measured need.
- **Browser-only bring-up as the lane's proof** — rejected: headless CI
  has no WebGPU; a contract that executes only on developer machines is
  a claim. The native lavapipe leg makes the kernel run every suite.
- **Ship the browser executor now** — rejected by measurement: +108 KB
  of shipped weight (past the E6.4 hard band) to defend a dispatch path
  that production never takes today. Defense-in-depth on real adapters
  becomes load-bearing exactly when a client consumes device output —
  land the weight with that client, not before.
- **Keep the chamfer as the law, JFA on GPU only** — rejected: two laws
  again, the exact fork ADR-0026 closed; and the chamfer's radial error
  is the visible artifact the JFA removes.
