# ADR-0002: Rust core compiled to WASM

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/`, `game/web/js/worker.js`, `game/web/js/wasm-load.js`

## Context

The simulator began as a Python 3.13 port of the original Hy/cocos2d game,
served world payloads over HTTP from uvicorn. Generation of a 512² world took
~4.9 s vectorized NumPy; every simulation tick round-tripped the network; the
dev server was a stateful process that could hold stale code (an orphaned
worker once served a `KeyError` for hours). The target experience — instant
regeneration, month-scrubbing, offline-capable preview — wanted the whole
simulation in the client.

## Decision

We port the entire simulation core to Rust (`game/rust/`), compiled to WASM
via wasm-bindgen, running in a Web Worker. The browser owns the world; there
is no simulation server. Numerics use `ndarray`, RNG uses `rand_pcg`,
payloads cross the JS boundary through a binary pack (ADR-0007). The same
crate builds natively for harnesses and diagnostics (ADR-0009).

## Consequences

- 512² generation: ~4.9 s Python → ~0.88 s native / ~1.5 s WASM.
- One codebase, two targets: native binaries give fast headless testing;
  the WASM build is the product.
- No server-side state to go stale; the Python tree (`game/game/`) is now
  reference-only.
- Costs: WASM/JS glue versioning is a real failure surface (see ADR-0007),
  and all dependencies must be wasm-clean (no threads, no filesystem).

## Alternatives considered

- **Stay in Python, optimize** — NumPy was already near its ceiling;
  interactivity still gated on HTTP round-trips.
- **TypeScript port** — no ndarray-grade numerics, ~5-10× slower inner
  loops, and no shared native harness for CI-style diagnostics.
- **Server-side Rust, thin client** — keeps the network in the loop and a
  process to babysit; loses offline preview.
