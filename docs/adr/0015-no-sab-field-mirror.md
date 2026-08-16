# ADR-0015: No shared-memory field mirror; transferables stay the lane

- **Status:** Accepted
- **Date:** 2026-08
- **Touches:** `game/web/js/net.js`, `game/web/js/worker.js`, `scripts/serve.py`

## Context

E7.9 asked whether the render thread should read grid state through a
`SharedArrayBuffer` mirror instead of receiving copies. With COOP/COEP now
served in dev (E7.8), `crossOriginIsolated` is true and SAB is available
there, so the question could be answered with measurements rather than
speculation.

## Decision

Rejected. The field grids cross the boundary exactly once per world, as a
single transferable pack buffer (ADR-0007), and a transfer is a pointer
move — the payload is never copied through `postMessage`. Measured on the
live app: a 512×640 world's ~12.5 MB pack lands and parses in milliseconds,
and the typed-array views feed GPU upload directly. After generation the
grids are immutable — ticks mutate settlements, markets, and the chronicle,
none of which live in the pack's field sections — so a live mirror would
share memory that never changes.

The deployment cost is real, though: the production host's headers are not
ours to set, so an SAB-dependent architecture would fork into a fast dev
path and a broken (or silently different) production path. COOP/COEP stays
on in `scripts/serve.py` for the precise timers, not for SAB.

## Consequences

- One wire discipline everywhere: transferable buffers for bulk, JSON for
  low-frequency ops. Dev and production behave identically.
- No `crossOriginIsolated` conditionals in application code.
- Revisit trigger: if mid-run grid mutation becomes real (E4.7 dirty tiles,
  live erosion), the calculus changes — that future ADR should measure tile
  patches over transferables first, which the current protocol already
  supports.

## Alternatives considered

- **SAB mirror of all field grids** — shares immutable data; saves nothing
  measurable; breaks on hosts without COEP.
- **SAB ring for tick deltas** — tick payloads are ~4 KB quantized deltas
  (E4); `postMessage` of a transferable at that size is far below frame
  budget. Complexity without a measured win.
