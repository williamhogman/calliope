# Architecture Decision Records

This directory holds every architecturally significant decision made on
Calliope, one file per decision, numbered and immutable once accepted.

## Why

The simulator is layered deep — terrain feeds climate feeds hydrology feeds
biomes feeds everything human — and most decisions here trade one kind of
believability against another. Six months from now the question is never
"what does the code do" (the code says), it's "why is it built this way and
what did we reject". ADRs answer that at the moment the answer is cheap.

## Format

MADR-lite. See [`template.md`](template.md). Sections:

- **Status** — `Proposed` | `Accepted` | `Superseded by ADR-NNNN`.
  Records 0002–0014 are marked `Accepted (backfilled)`: the decision was
  made and shipped before the ADR system existed; the record reconstructs
  the context honestly rather than pretending contemporaneity.
- **Context** — the forces at play, what was true when the decision was made.
- **Decision** — one paragraph, active voice: "We do X."
- **Consequences** — what got easier, what got harder, what we now must do.
- **Alternatives** — what was considered and why it lost. This section is
  the one that prevents re-litigation; write it even when it feels obvious.

## Rules

1. Number sequentially, four digits: `NNNN-short-slug.md`.
2. An accepted ADR is never edited beyond typo fixes and status changes.
   Changing course means a **new** ADR that supersedes the old one, with the
   old one's status updated to point at it.
3. Write the ADR **in the same change** that implements the decision —
   an ADR PR-ed after the fact drifts.
4. Small enough to read in two minutes. Link to code and reports rather
   than duplicating them.
5. Tuning philosophy counts as architecture here (e.g. pricing
   normalization, growth pacing). If a constant embodies a stance, the
   stance gets an ADR; the constant's value lives in code and the
   diagnostics bands.

## Index

| # | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-rust-core-compiled-to-wasm.md) | Rust core compiled to WASM | Accepted (backfilled) |
| [0003](0003-single-seed-determinism.md) | Single-seed determinism with derived RNG streams | Accepted (backfilled) |
| [0004](0004-square-grid-4km-cells.md) | Square grid at 4 km/cell, not a graph world | Accepted (backfilled) |
| [0005](0005-layered-generation-then-tick.md) | Layered one-shot generation, then monthly ticks | Accepted (backfilled) |
| [0006](0006-wgpu-fullscreen-shader-renderer.md) | wgpu fullscreen-shader renderer ("Orbital") | Accepted (backfilled) |
| [0007](0007-binary-pack-protocol.md) | Binary pack protocol with version-locked loader | Superseded by 0016 |
| [0008](0008-vendored-solidjs-no-build.md) | Vendored Solid.js UI without a build step | Accepted (backfilled) |
| [0009](0009-diagnostics-harness-as-gate.md) | Native diagnostics harness as the tuning gate | Accepted (backfilled) |
| [0010](0010-terrain-priced-trade.md) | Terrain-priced trade with sea/land asymmetry | Accepted (backfilled) |
| [0011](0011-discovery-depletion-economy.md) | Hidden-deposit discovery/depletion economy | Accepted (backfilled) |
| [0012](0012-relative-scarcity-pricing.md) | Relative-scarcity market pricing | Accepted (backfilled) |
| [0013](0013-resource-floor-guarantees.md) | Resource floor guarantees | Accepted (backfilled) |
| [0014](0014-ocean-frame-falloff.md) | Ocean-frame falloff: no clipped landmasses | Accepted (backfilled) |
| [0015](0015-registry-codegen-architecture.md) | Registries and codegen as the single declaration point | Accepted |
| [0016](0016-pack-v2-quantized-crc-payload.md) | Pack v2: quantized, checksummed, header/meta split | Accepted |
| [0017](0017-json-tick-lane-retained.md) | JSON tick lane retained; no binary tick payload | Accepted |
| [0018](0018-people-realm-axis-split.md) | People and Realm are separate axes | Accepted |
| [0019](0019-civilization-derived-tier.md) | Civilizations as a derived tier over peoples and realms | Amended by 0020 |
| [0020](0020-overstretch-as-span-of-control.md) | Overstretch as span of control, not population mass | Accepted |
| [0021](0021-goods-ontology-as-data.md) | The goods ontology as one declarative table | Accepted |
| [0022](0022-hand-rolled-lattice-over-bevy-ecs.md) | Hand-rolled system lattice, not bevy_ecs | Accepted (renumbered from 0015) |
| [0023](0023-no-sab-field-mirror.md) | No shared-memory field mirror; transferables stay the lane | Accepted (renumbered from 0015) |
| [0024](0024-plate-history-sketch.md) | The plate-history sketch: deep past as input, never simulation | Accepted |
| [0025](0025-cross-runtime-replay-identity.md) | Cross-runtime replay identity for gated ledgers | Accepted |

## Era gates

When an era of `../ROADMAP-500.md` reaches its gate phase, the decisions
it opened are recorded closed here — so the index says not just what was
decided but which chapter each decision belongs to.

**Era I — The Deep Earth (M16–M65, gate: `diagnose gate`).** The era's
two reopened questions closed as follows. The plate-history question —
simulate the deep past or sketch it — closed as ADR-0024 (sketch as
input, never simulation) and held through fifty phases without a
superseding record; the calibration against Earth's own numbers (M64)
and the era gate ran on worlds built from the sketch. The GPU-erosion
reopening (displaced from the original M57 slot by operator re-scoping)
is **not** silently dropped: its spec is preserved in
`../roadmap-500/STATUS.md`'s Ready queue and scheduled as M67's first
client — a deferral on the record, not a decision. ADR-0003 was
reaffirmed at era scale by the gate's structural leg (300 years under
two tick chunkings, one state) and ADR-0025's native↔wasm ledger
identity. The gate's verdict is live, not ceremonial: it composes every
suite lane and holds the era open on any standing [FAIL].
