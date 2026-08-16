# ADR-0017: JSON tick lane retained; no binary tick payload

- **Status:** Accepted
- **Date:** 2026-08
- **Touches:** `game/rust/src/world.rs::tick_json`, `game/web/js/worker.js`, `game/web/js/net.js`

## Context

The engine roadmap carried E4.6/E7.6: replace the tick's JSON string (built
in Rust, structured-cloned through `postMessage`, parsed on the main thread)
with one transferable binary buffer — columnar deltas plus a string table.
The delta work that preceded it (E4.1–E4.5, E4.8) already cut the payload to
a median 3,976 B at year 100 (`diagnose bench`), and the pack path (ADR-0007,
pack v2) proved the binary machinery works when it pays.

Before designing the format we measured what it would recover. Instrumented
`JSON.parse` on the main thread in Chromium, 120 single-month ticks against
a year-100 512² world (seed 777):

- parse per tick: median **0.080 ms**, p90 0.145 ms, max 0.720 ms (65 KB payload)
- total parse across the run: 16.6 ms of an 11,437 ms wall — **0.15 %**
- tick wall time is dominated by the canvas draw path (E9's ground), not
  serialization on either side of the boundary

## Decision

The tick lane stays JSON. E4.6 and E7.6 are rejected on measurement.

A columnar delta codec with string-table maintenance would be a second,
hand-maintained encoding of every tick section — seventeen heterogeneous,
mostly string-bearing shapes — to reclaim sub-millisecond parse time per
tick that arrives at most once per playback second. The dual-declaration
cost is the same one that killed E3.7/E3.8; the win here is smaller still.

## Consequences

- The tick wire contract stays human-readable — replay/recovery (E7.10) and
  the properties suite (P4 byte-truth replay) keep diffing plain JSON.
- The optimization budget moves where the measurement points: the render
  path (E9.5 label layout cache, E9.9 dirty-region compositor).
- If tick cadence ever becomes per-frame (streamed playback), re-measure;
  a superseding ADR is the only way this reopens.

## Alternatives considered

- **Columnar deltas + string table (the E4.6 design)** — saves ≤ 0.7 ms in
  the worst observed tick; costs a second schema for every payload section.
- **Binary lane for numeric sections only (`s_hot`, `m_hot`, territory)** —
  the numeric sections are the cheap ones to parse; the string-heavy
  sections are exactly the ones a partial lane would leave in JSON.
- **MessagePack/CBOR off-the-shelf** — still decode-then-build-objects on
  the JS side; V8's `JSON.parse` is faster than JS-level decoders at these
  sizes.
