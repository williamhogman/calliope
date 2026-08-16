# ADR-0016: Pack v2 — quantized, checksummed, header/meta split

- **Status:** Accepted
- **Date:** 2026-08
- **Supersedes:** [ADR-0007](0007-binary-pack-protocol.md)
- **Touches:** `game/rust/src/world.rs::pack`, `game/rust/src/util.rs::crc32`,
  `game/web/js/net.js`, `game/web/js/worker.js`, `scripts/build.sh`,
  `scripts/serve.py`

## Context

Pack v1 (ADR-0007) shipped 13 raw arrays at 38 B/cell — ~26 MB at 768×896 —
plus a JSON header that duplicated nearly the whole tick payload
(settlements, cultures, events, routes). Three structural problems: the
float grids carried f32 precision the client never uses at full width; the
territory grid shipped raw i16 although ticks already send it as RLE; and
the header's wall-clock timings were the one nondeterministic region of the
payload. There was also no integrity check — a truncated fetch rendered
garbage instead of failing.

## Decision

Pack v2 keeps the outer frame — `[u32 header_len][header json (padded to
4)][blob]` — and changes what rides inside:

- **Version + CRC (E3.6).** The header carries `pack: 2` and `crc32` (IEEE
  802.3, mirrored bit-for-bit in `net.js`). The client refuses any other
  version and fails loud on checksum mismatch.
- **Wire quantization (E3.4).** The field registry gained a `wire` column:
  `raw`, `u16` (linear over the field's live range, `scale`/`offset` in the
  header), or `u16sqrt` (linear in sqrt-space, for wide-dynamic-range
  fields — discharge spans ~6 decades and keeps relative precision at the
  low end). The client dequantizes to float32 at the unpack edge; nothing
  downstream changes. Quantization is wire-only: storage and the
  determinism hash always see full f32.
- **Territory as RLE (E3.5).** Contiguous realms compress ~1000×, and the
  client already decodes this encoding for tick patches.
- **Single-pass blob (E3.3).** Sections are written straight from grid
  storage in registry order — no per-field temporary buffers.
- **Header/meta split (E3.1).** The pack header carries identity,
  dimensions and the array table only. Vocabulary tables and entity state
  ride a separate `bootstrap()` JSON call, merged client-side in
  `generateWorld()`.
- **Timings off the wire (E3.9).** Generation timings moved to a
  `timings()` debug call; the payload is now 100 % deterministic.
- **Brotli precompression (E3.10).** `scripts/build.sh` writes `.br`
  siblings into `dist/`; `scripts/serve.py` negotiates `Content-Encoding`.

Result: 20.1 B/cell (12.5 MB → 6.6 MB at 512), verified by `diagnose
properties` (crc, quantization ≤ half a step, RLE exactness) and a
`pack bytes per cell` band in `diagnose bench`.

## Consequences

- Payload roughly halves before compression; quantized u16 sections also
  compress better than f32 noise.
- Corruption and version skew fail loudly at the unpack edge instead of
  rendering garbage.
- Costs: the `q` descriptor is part of the versioned contract — changing a
  field's wire mode or the encode-space transform is a `PACK_VERSION` bump;
  clients hold dequantized float32 copies (transfer shrinks, client memory
  does not).
- The versioning rule stands: field order, wire modes, and header keys are
  one contract; any change bumps `PACK_VERSION`, and the client refuses
  mismatches rather than guessing.

## Alternatives considered

- **Keep raw f32 + rely on brotli alone** — compression does not recover
  the 2× from quantization and decompression costs main-thread time.
- **Quantize in the shader (upload u16 textures)** — saves client RAM but
  spreads the dequant contract across WGSL and JS; rejected for contract
  sprawl.
- **Per-field fixed ranges instead of live min/max** — simpler header but
  wastes precision on quiet worlds and clips loud ones.
- **CRC over header + blob** — the header would then need its own frame;
  the blob is where silent corruption bites (typed arrays), so it alone is
  stamped.
