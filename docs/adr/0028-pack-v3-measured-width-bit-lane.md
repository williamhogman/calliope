# ADR-0028: Pack v3 — the measured-width bit lane for categorical grids

- **Status:** Accepted
- **Date:** 2026-08-22
- **Touches:** `game/rust/src/pack.rs`, `game/web/js/net.js`,
  `game/rust/src/bin/diagnose.rs`, `game/rust/src/systems.rs`
- **Extends:** [ADR-0016](0016-pack-v2-quantized-crc-payload.md)

## Context

Pack v2 spent a full byte per cell on every categorical grid. By M69 there
were eight of them (`biomes`, `crops`, `strahler`, `flags`, `rock`, `soil`,
`landform`, `coastform`), and the payload sat at exactly 26.0 B/cell — the
hard edge of the `pack bytes per cell` band, with no headroom left for the
next vocabulary. Their live value ranges do not remotely fill a byte: a
biome id reaches 11, a soil order 10, a rock province 3, a coast form 3.

The temptation was to hard-code each vocabulary's width from its enum. That
would be a second copy of every vocabulary size, drifting the day someone
adds a landform.

## Decision

We add a fourth wire lane, `wire bits`, and bump the protocol to
`PACK_VERSION = 3`. The lane is *lossless*: the packer measures the grid's
own live maximum, spends `ceil(log2(max+1))` bits per cell (1..=8), writes
them LSB-first, and records the width as `bits` on the array entry. Nothing
about a vocabulary's size is assumed anywhere — the width is measured, the
way quantization already measures `scale` and `offset`.

The client expands a bit section back to a full `Uint8Array` at the unpack
edge, so everything downstream of `unpack()` sees exactly the arrays it
always did. `validate_pack` sizes a bit section as the ceiling of its bit
run and rejects any width outside 1..=8; `diagnose properties` decodes the
lane the same way the client does and demands bit-equality, not tolerance.

Storage, the GPU upload path and the determinism hash are untouched: this
is strictly a wire concern (ADR-0016).

## Consequences

- Measured at 512 across seeds 12345 / 777 / 90210: **26.0 → 21.90 B/cell**,
  back inside the sweet band with real headroom. Blob 8.36 MB → 7.17 MB.
- A new categorical grid costs the bits it earns, not a byte, and a
  vocabulary that grows past a power of two widens itself.
- Costs: the wire is now a bit-addressed format for those sections, so a
  reader must go through the documented expander; a version bump was owed
  and taken (`pack: 3`, refused by any other client).

## Alternatives considered

- **Enum-derived widths** — a second copy of every vocabulary size, and a
  silent truncation the day one grows. Rejected.
- **Two categorical ids per byte (fixed nibbles)** — only helps grids under
  16 values, misses `landform` (26), and hard-codes a pairing order.
- **Compress the blob** — brotli already rides transport (E3.10); it does
  not reduce the in-memory payload the band measures.
