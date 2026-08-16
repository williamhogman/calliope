# ADR-0007: Binary pack protocol with version-locked loader

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/world.rs::pack`, `game/web/js/wasm-load.js`, `scripts/serve.py`

## Context

The world crossing WASM→JS is several megabytes of fields plus entity lists.
JSON serialization of arrays was both slow and bloated. Separately, a stale
browser cache once paired an old `calliope_bg.js` glue file with a new
`.wasm` binary, producing `function import requires a callable` crashes that
looked like corruption — glue and binary must be treated as one artifact.

## Decision

We pack the world as a single binary buffer: a small JSON header (meta,
entities, timings) followed by typed-array field sections (`bytemuck`-cast),
parsed zero-copy on the JS side. The loader (`wasm-load.js`) version-locks
glue and binary by appending their content hashes to both URLs, and the dev
server (`scripts/serve.py`) serves `Cache-Control: no-store` with correct
MIME types, so a mismatched pair is impossible to load.

## Consequences

- Payload transfer and parse are milliseconds; fields land as typed arrays
  ready for GPU upload.
- The stale-glue class of bug is structurally dead.
- Costs: the pack layout is a versioned contract — field order changes need
  a header version bump; the header's wall-clock timings are the one
  nondeterministic region and are excluded from state hashing (ADR-0003).

## Alternatives considered

- **JSON everything** — measured too slow and too large at 512²+.
- **Multiple fetches per field** — multiplies cache-coherency failure modes,
  the exact class of bug this ADR kills.
- **Service-worker cache management** — heavier machinery to solve what
  content-hash URLs solve statically.
