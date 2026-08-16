# ADR-0015: Registries and codegen as the single declaration point

- **Status:** Accepted
- **Date:** 2026-08-16
- **Touches:** `game/rust/src/world.rs` (`field_registry!`, `event_table!`),
  `game/rust/src/util.rs` (`Band`), per-system `BANDS` tables,
  `game/rust/src/bin/genjs.rs`, `game/web/js/gen/`, `scripts/build.sh`

## Context

World-shape knowledge used to live in several hand-kept copies: `pack()`
listed the field arrays, `hash_state` listed them again, Orbital's
`set_world` and `gpu.js` repeated the order a third and fourth time;
`LAYER_ID` was hand-copied into JS; event-kind metadata was spread across
`telling.rs` and UI filter lists (which had silently drifted — three kinds
missing); and `diagnose.rs` duplicated every tuning band inline, twice for
the sweep means. Each copy was a place for drift to hide.

## Decision

Every piece of engine vocabulary is declared exactly once, in a table
beside the system it belongs to, and everything else derives from it:

- **`field_registry!`** (`world.rs`) — every per-cell grid with name,
  dtype, units, pack/hash inclusion, GPU upload flag. `pack()`,
  `hash_state`, and the `set_world` upload order all iterate it.
- **`event_table!`** (`world.rs`) — kind, family, sifter weight, fortune
  lean in one block. Chronicle prose stays at emission sites: each line is
  composed from live context no template column could carry.
- **`BANDS`** (per system module, `util::Band`) — diagnostics tuning
  ranges declared beside geo, climate, biomes, agriculture, hydrology,
  resources, chronicle, economy, settlements, world; `diagnose` consumes
  them by name and panics on unknown names.
- **`genjs`** (build step) — emits `game/web/js/gen/constants.js`
  (vocabulary) and `gen/types.js` (wire typedefs introspected from live
  payloads serialized by the engine's own `Serialize` impls).

Forbidden to hand-write from here on: engine vocabulary in JS (layer ids,
field lists, kind tables), any second copy of pack/hash/upload order, and
band numbers inside the harness.

## Consequences

- The field-order, kind-table, and band-number drift classes die
  structurally; a missing band is a panic, not a silent stale check.
- Generated files are committed artifacts (like `wasm/version.js`) so the
  no-build dev preview keeps working; `scripts/build.sh` regenerates them
  with every engine rebuild.
- Costs: the build runs one extra native binary; `types.js` is structural
  typing — an optional field only surfaces once a sample payload
  exercises it, so the two tick samples must stay representative.

## Alternatives considered

- **`ts-rs` / `schemars`** — heavier dependencies, a TS toolchain or a
  schema→JSDoc emitter we would still have to write; introspection of the
  real payloads is smaller and cannot disagree with serde attributes.
- **Hand discipline** — the exact failure mode this ADR kills; it had
  already produced the `config.js` kind drift.
- **Runtime vocabulary exports from WASM** — pays at every page load for
  what is static at build time, and still needs hand-kept lists.
