# Calliope Engine Roadmap

The platform track: engine optimization, data formats across the WASM→JS
boundary, macro/codegen discipline, build lean-ness, and the polish of every
surface the simulation touches (worker protocol, Solid UI, Orbital renderer).

This is the sibling of `ROADMAP.md` (world systems). Same rules apply:
determinism is law (ADR-0003), the harness is the gate (ADR-0009), and
architecture-shaping items land with an ADR. Gates are expressed as
`diagnose` checks or scripted browser probes, never manual inspection.

`scripts/roadmap-engine-check.sh` is the stopping criterion for this track —
it exits non-zero while any milestone item is unchecked.

Legend: cost S/M/L · every item cites the code it touches.

## How the lattice holds

Each track builds on the ones beneath it: the typed core makes the macro
registry possible; the registry makes pack v2 declarative; pack v2 makes
delta ticks cheap; delta ticks make the UI and renderer incremental. The
instrument track (E10) starts first and gates everything.

```text
        E8 Solid surfaces      E9 Orbital II
              \                   /
               E7 The Bridge ----+
                    |
        E4 Delta ticks     E6 Lean binary
                    \         /
                E3 Pack v2   +---- E5 Hot paths
                      \      |      /
                 E2 One declaration
                        |
                 E1 The typed core
   ----------------------------------------------
   E10 Proof of speed (gates all)   E11 The broken monolith (threads through)
```

## E1 — The Typed Core (enums, ids, strum)

Strings stop being identifiers. Every closed vocabulary in the engine becomes
a `Copy` enum with strum-derived tables; parallel hand-written arrays die.

- [x] E1.1 Add `strum`/`strum_macros`; convention: closed vocabularies derive `Display`, `EnumString`, `EnumIter`, `IntoStaticStr`, `EnumCount` (S)
- [x] E1.2 `Good` enum replacing the 19-string table `resources.rs:16-36`; `Market.prices: BTreeMap<String,f64>` (`economy.rs:69-71`) becomes an `EnumMap<Good, f64>` (M)
- [x] E1.3 `isa`/`requires`/`abundance` string matches (`resources.rs:39-79`) become const tables keyed by `Good`; `isa_chain`'s per-call `Vec<String>` (`resources.rs:91-100`) becomes a precomputed closure bitmask (S)
- [x] E1.4 `EventKind` enum replacing `Event.k: String` (`world.rs:38`); `telling::weight(&e.k)` (`world.rs:1815,1859`) becomes an array lookup; match sites gain exhaustiveness (M)
- [x] E1.5 `EntityKind` enum replacing `Entity.kind: String` (`entity.rs:14`); registry filters stop string-comparing every tick (`entity.rs:116,141`) (S)
- [x] E1.6 Newtype ids — `SettlementId`, `CultureId`, `EntityId` — replacing raw `usize`/`i64` across module boundaries; misuse becomes a type error. (`DepositId` dropped: deposit indices never leave `World::deposits` loops — no boundary to protect) (M)
- [x] E1.7 `CellFlags` bitflags: one `Array2<u8>` replaces the four bool grids `rivers/lakes/salt/seasonal` (`world.rs:91-102`); the 4-way zip in `pack()` (`world.rs:2281-2291`) becomes a memcpy (S-M)
- [x] E1.8 `Biome` enum with derived names; `constants::biome_meta()` generated, not hand-maintained (S)
- [x] E1.9 Tech knowledge as a `u32` bitset on `Society`; `knows("pottery")` string scans (`world.rs:1906`) become O(1) bit tests (S)
- [x] E1.10 `CropPackage` enum subsuming the parallel arrays `PACK_NAMES`/`PACK_DENSITY` (`world.rs:2248-2255`) (S)
- [x] E1.11 `Settlement.goods: Vec<String>` → `SmallVec<Good>`; `exports: Option<String>` → `Option<Good>`; `Route.goods: Vec<Option<String>>` (`trade.rs:57`) → `Vec<Option<Good>>`; kills the clone chains in `goods_for` (`trade.rs:87-104`) (S-M)
- [x] E1.12 `serde_repr` on wire enums: JSON ships small stable ints plus a names table sent once in `meta()`; event/entity payload shrinks (S)

Gates: `diagnose determinism` hash unchanged across the enum migration
(pure refactor); zero string-typed identifiers left in `Market`,
`Settlement`, `Event`, `Entity` (grep check in `report.sh`).

## E2 — One Declaration (field registry & macro lattice)

The engine's shape is declared once; pack, hash, textures, and diagnostics
are all generated views of the same declaration. This is the keystone the
data tracks stand on.

- [x] E2.1 Field-registry macro: each grid declared once with name, dtype, units, pack-inclusion, hash-inclusion, texture slot — replacing the hand-kept lists in `pack()` (`world.rs:2297-2311`) (M)
- [x] E2.2 `pack()`, `hash_state`, and Orbital's `set_world` upload order (`gpu.js:51-64`) all derive from the registry — the field-order-drift failure class dies structurally (M)
- [x] E2.3 Event-table macro: kind, sifter weight, fortune lean, and notification family declared together (`event_table!` beside `EventKind`); chronicle prose stays at emission sites by design — each line is composed from live context no template column could carry (M)
- [x] E2.4 Generated JS constants: `scripts/build.sh` emits `game/web/js/gen/constants.js` (layer ids, field registry, event/entity kinds, biome names, goods) from the Rust tables — kills the hand-copied `LAYER_ID` in `gpu.js:14-17`; palette color stops stay in `palette.js` as pure presentation (M)
- [x] E2.5 Wire-type stubs: `genjs types` emits `gen/types.js` — JSDoc typedefs introspected from live payloads serialized by the `Serialize` impls (map-like objects collapse to `Object<string, T>`); `net.js` returns are annotated with them (S-M)
- [x] E2.6 Collapse the near-identical linear-scan bodies in `Registry` (`entity.rs:47-126`) behind one generic helper (`find_latest`) (S)
- [x] E2.7 Diagnostics-band table: `util::Band` + per-system `BANDS` consts (geo, climate, biomes, agriculture, hydrology, resources, chronicle, economy, settlements, world), consumed by `diagnose.rs` via `Checks::band`/`band_as` — sweep means reuse the same bands (M)
- [x] E2.8 ADR-0015: the registry/codegen architecture — what is declared, what is generated, what is forbidden to hand-write (S)

Gates: one grep proves no field name appears in more than one Rust source
location; pack/unpack round-trip byte-identity stays green (`diagnose
properties`); generated `constants.js` diff-clean against the Rust tables.

## E3 — Pack v2 (the binary payload)

The full-world payload today is ~26 MB at 768×896 (38 B/cell across 13
arrays) plus a JSON header that duplicates nearly the whole tick payload.
Pack v2 makes the payload lean, versioned, and fully binary.

- [x] E3.1 Header/meta split: the pack header carries the array table and minimal meta only; settlements/cultures/events leave the header (`meta()` inlines all of them today, `world.rs:2236-2271`) (M)
- [x] E3.2 Native `f32` grids: store what needs no f64 precision as f32 at rest (all 8 packed float grids are converted f64→f32 per pack today, `world.rs:2276-2279`) — halves grid memory and kills ~22 MB of transient copies per pack (M-L)
- [x] E3.3 Single-allocation pack: registry-computed offsets, one `Vec<u8>`, fields written in place — no per-field temporary buffers (`world.rs:2297-2311`) (S)
- [x] E3.4 Opt-in quantization per registry field: height/precip/discharge as u16 + scale/offset in the header; JS gets the dequant constants (M)
- [x] E3.5 Territory ships RLE in the pack too — `politics::territory_rle` already exists for ticks (`world.rs:2223`) but `pack()` sends the raw i16 grid (`world.rs:2295,2310`) (S)
- [x] E3.6 Pack version byte + CRC32; `unpack()` (`net.js:15-27`) verifies both and fails loud (S)
- E3.7 Columnar entity sections — **rejected on measurement**, see Rejected
- E3.8 String interning across the boundary — **rejected on measurement**, see Rejected
- [x] E3.9 Wall-clock `timings` leave the header for a debug side channel — the one nondeterministic region named in ADR-0007 closes (S)
- [x] E3.10 Brotli precompression of `dist/` artifacts in `scripts/build.sh`; `scripts/serve.py` learns `Content-Encoding` negotiation (S)
- [x] E3.11 ADR superseding ADR-0007: pack v2 layout, quantization policy, versioning rules (S)

Gates: `diagnose properties` pack round-trip extended to v2 (byte-identity
through quantization round-trip within declared epsilon); payload
bytes/cell band in `diagnose perf` (target < 20 B/cell); determinism hash
now covers 100 % of the payload.

## E4 — The Standing Wave (delta ticks)

A tick should ship what changed, nothing else. Today `tick_json` rebuilds
and resends every settlement, culture, market row, area, and merchant every
month (`world.rs:2193-2227`), unconditionally.

- [x] E4.1 Direct `Serialize` structs for the tick payload — kill the `json!()` build-tree-then-stringify double pass (`world.rs:2195-2226`) (S)
- [x] E4.2 Settlement deltas: only settlements whose fields changed cross the boundary; systems mark changes as they write (M)
- [x] E4.3 Cultures, market, areas, merchants gated by dirty flags like routes/ruins/features already are (`world.rs:2205-2225`) (M)
- [x] E4.4 Event cursor: tick returns the new event count; the client pulls ranges through the existing `events_range` (`lib.rs:79-85`) — event arrays leave the tick payload (S-M)
- [x] E4.5 One `DirtyMap` replacing the five ad-hoc `*_dirty` bools (`world.rs:109,125,128,146` …) — new payload sections get change-tracking for free (S)
- [x] E4.6 Binary tick payload — **rejected on measurement** (ADR-0017), see Rejected: main-thread parse is 0.080 ms median per tick, 0.15 % of tick wall time at year 100
- [ ] E4.7 Grid dirty-tiles (32×32): mid-run field changes (territory today, erosion later) ship as tile patches, feeding partial texture updates in E9 (L)
- [x] E4.8 Merchant/war/ruler headline extraction server-side: the UI's per-tick scan for toast-worthy events moves behind the event-table declaration (E2.3) (S)

Gates: median tick payload < 4 KB at year 100 (vs. full resend today),
measured in `diagnose perf`; a 1200-month native run allocates no payload
section that carries zero changes. **Status: gate green** — `diagnose bench`
median 3976 B @ year 100 (p90 19.5 KB, dominated by routes/territory, i.e.
E4.6/E4.7 ground); `diagnose properties` P4 replays 240 months of deltas to
byte-truth for settlements, market and areas with zero redundant reships.
Beyond the plan, sections got finer than dirty flags: settlements and
cultures split hot/cold (positional heartbeat rows `[id,pop,food,k,wealth]`),
the market ledger ships per-good rows (`m_hot`), market areas ship per-hub,
per-good price patches, and all wire floats carry display precision only
(prices 0.01, food 0.1, coin and souls whole).

## E5 — The Hot Paths (tick and generation CPU)

The audit found the quadratic and the wasteful; this track retires them.

- [x] E5.1 Registry name→id index (`HashMap<(EntityKind, String), i64>`): `resolve_events` today does up to six O(registry) reverse scans per unresolved event per tick (`world.rs:1830-1863`, `entity.rs:113-126`) (S)
- [x] E5.2 Settlement id→idx map built once per tick and passed down — it is rebuilt five times in `economy.rs` (`223,368,410,654,884`) and again in `trade.rs` (`409,489,621`) (S)
- [x] E5.3 Spatial bucket grid over settlements and deposits: `prospect_and_deplete`'s O(deposits × settlements) monthly scan (`world.rs:1981-2015`) and famine-migration target search (`world.rs:1920-1934`) go sub-quadratic (M)
- [x] E5.4 A* scratch pooling in `trade::astar` (`trade.rs:241-299`): reusable `best` grid with generation stamps and an indexed `came` array instead of per-call `Array2` + `HashMap` allocations across O(N²) route builds (M)
- [x] E5.5 Event emission slims down: `SmallVec` for `Event.ids` rides 0–2 ids inline. The shared-`Vec<Event>` half is rejected: `artifact::monthly` reads the current month's slice while emitting, which a shared buffer cannot lend mutably and immutably at once — the ~15 small per-month `Vec`s are the cheap side of that trade (S-M)
- [x] E5.6 `artifact::monthly` takes `&new_events[month_start..]`, not a full clone (`world.rs:1794`) (S)
- [x] E5.7 Merchant pass stops cloning every good name per area per month (`economy.rs:751-752`) (S)
- [x] E5.8 `tick_json` serializes into a reused `String` buffer via `to_writer` (S)
- [x] E5.9 Criterion benches: per-stage generation and a 1200-month tick, results written into `game/reports/bench.txt` by `report.sh` — done in-harness (5-sample medians with spread) instead of pulling the criterion dep: same statistical protection, one toolchain, text reports stay the gate (M)
- [x] E5.10 Counting allocator behind a diagnose feature: allocations/tick becomes a banded metric — allocation regressions get an alarm (`--features alloc-count`, baseline 183/mo, band sweet ≤350 · hard ≤1500) (M)
- [x] E5.11 Pass-fusion profile pass over generation: fuse adjacent full-grid sweeps in `erosion.rs`/`climate.rs` where the profiler shows wins, guarded by the determinism hash — continentality EDT deduped (one per generation, shared by amplitude + monsoon), row-constant `powf` hoisted out of the climate inner loops, drainage sort made unstable (identical total order, no scratch alloc), diffusion snapshot reuses one scratch grid; erosion 193→175 ms, climate 57→42 ms @512, state hash unchanged (`90d82b4c9c06fdb5`) (M)
- [x] E5.12 `cultures_json` stops re-deriving ruler/era/tech-name arrays for unchanged societies every tick (`world.rs:2149-2191`) — done as a one-pass cold/hot build gated on the two half-hashes; the full block is only assembled on the rare cold-change tick (S)

Gates: native tick rate band in `diagnose perf` (≥ 2,000 months/min at
640×512, year-100 world); generation stays < 400 ms native; allocations/
tick within band; determinism hash stable through every change.

## E6 — The Lean Binary (wasm size & build)

`calliope_bg.wasm` is 3,009,471 bytes with wgpu compiled into the same
crate as the simulation. The sim, the renderer, and the harness deserve
separate compilation stories.

- [x] E6.1 Crate split: `calliope-core` (sim, no wgpu) + `calliope-orbital` (render.rs) + thin wasm crate binding both — **resolved without the split**: wgpu already lives under `[target.'cfg(target_arch = "wasm32")'.dependencies]` and `render.rs` is cfg-gated, so native bins never compile GPU code today (verified: `cargo tree` on the native target has no wgpu). The split would not shrink the shipped wasm either — the cdylib needs both halves — so it is pure churn; rejected (M)
- [x] E6.2 `panic = "abort"` in the release profile — unwind machinery gone from all release binaries; a `web_sys::console` panic hook ships in wasm debug builds only (`lib.rs::init_panic_hook`, cfg `debug_assertions`) (S)
- [x] E6.3 Explicit `wasm-opt` config: measured `-O3` = 3,092,925 B vs `-Oz` = 2,998,414 B on the real binary — `-Oz` ships (generation headroom is ample after E5); pass verified running in the nix path (`Optimizing wasm binaries with wasm-opt`) (S)
- [x] E6.4 `twiggy top` audit script (`scripts/wasm-audit.sh` → `game/reports/wasm-audit.txt`); wasm size budget banded in `game/reports/build.txt` (written by `build.sh`, swept by `report.sh`): sweet ≤3.0 MiB · hard ≤3.4 MiB, measured 2,998,414 B. The <1.2 MB post-split target died with E6.1: the audit shows the weight is wgpu/naga (~1,077 items), the price of Orbital (ADR-0006), not sim code (S)
- [x] E6.5 Feature-gate `explain`/diagnostics machinery — audited: explain + telling ≈ 17 KB (0.5%) and both back user-facing UI; not measurable weight, stays in (S-M)
- [x] E6.6 Strip debug names/producers: release binary ships with zero custom sections (`--strip-producers --strip-target-features`; names were already stripped); the symbolized twin for profiling is the `--profiling` build in `game/rust/pkg-prof/` (wasm-opt `-O -g`), built on demand by `wasm-audit.sh` (S)
- [x] E6.7 One compile, two instantiations: `net.js` compiles once (`loadModule`, streaming) and hands the `WebAssembly.Module` to the worker as its first message (`init` op); Orbital instantiates from the same module. Verified live: exactly one `.wasm` fetch per page load, world generates and renders (M)
- [x] E6.8 Streaming instantiation verified end-to-end: `WebAssembly.compileStreaming(fetch)` with buffer-compile fallback; `application/wasm` confirmed on both `scripts/serve.py` and the production host (curl: `content-type: application/wasm`) (S)
- [x] E6.9 Reproducible builds: toolchain locked in `game/rust/rust-toolchain.toml` (1.91.1 + wasm target); empirically byte-identical — a full rebuild reproduced stamp `13e7a0dab5fa` exactly (M)
- [x] E6.10 Build-time budget: `build.sh` times every phase (ms) into `game/reports/build.txt`; `report.sh`'s summary sweep picks the file up with the size band (S)

Gates: wasm size budget check green; boot-to-first-frame improves
measurably (E10.5 probe); native `diagnose` binary compiles without wgpu
in its dependency tree (`cargo tree` check).

## E7 — The Bridge (worker protocol)

The protocol grows stamps, timeouts, progress, and binary lanes — the
boundary stops being the naive part of the system.

- [x] E7.1 Generation stamps: every request carries `<seed>-<size>-g<n>`; the worker drops world-ops whose stamp mismatches the live world, so stale responses from a freed world can never resolve into current UI state (`proto.js`, `worker.js`, `net.js`) (S)
- [x] E7.2 Per-op deadlines (`proto.js::DEADLINE`) with pending-map cleanup; `worker.onerror`'s blanket reject-all narrowed to per-request failure carrying op context, with the crash path split out to recovery (E7.10) (S)
- [x] E7.3 Tick coalescing: one in-flight tick, queued months merge into a single follow-up call — spamming 12× playback can no longer queue unbounded work (`net.js::tickWorld`) (S)
- [x] E7.4 Abortable generation: `abort` bypasses the arrival-order chain and flags the stamp; the builder loop checks between stages and frees the half-built world. Verified live: "abandon this world" fades the veil mid-generation (M)
- [x] E7.5 Stage progress events during generation drive the loading veil with real stage names — all nine observed live, RAISING THE LAND → WAKING THE FIRST PEOPLES (`GenBuilder` in `world.rs`, `WasmWorldBuilder` in `lib.rs`) (S-M)
- [x] E7.6 Binary tick lane — **rejected with E4.6** (ADR-0017), see Rejected: the lane's savings are sub-millisecond per tick
- [x] E7.7 Protocol op-codes gain their single source of truth in `proto.js` (OP + DEADLINE), imported by both endpoints. The Rust-enum half is rejected: the op strings are a JS↔JS contract between `net.js` and `worker.js` — the wasm boundary is method calls, so Rust never sees an op string and codegen would be a build stage for nothing (S)
- [x] E7.8 COOP/COEP headers in `scripts/serve.py`; `crossOriginIsolated === true` verified live. The production-host half is moot by ADR-0015 — nothing depends on SAB, so prod needs no header change (S)
- [x] E7.9 Research + ADR: shared-memory field mirror rejected with measurements — fields are immutable post-generation and already cross as one zero-copy transferable; SAB would fork dev/prod behavior (ADR-0015) (L)
- [x] E7.10 Worker crash recovery: on worker death, respawn, regenerate the recorded seed, replay the months run — determinism (ADR-0003) makes reconstruction free. Verified live: worker killed at month 12, world restored to month 12, two one-line toasts, zero console errors (M)

Gates: scripted Playwright probe — regenerate mid-generation, regenerate
mid-tick-burst, kill the worker — all recover to a consistent, correct UI;
no stale-response artifacts; protocol fuzz (unknown ops, out-of-order ids)
never wedges the pending map.

## E8 — Solid Surfaces (UI architecture & polish)

Fine-grained reactivity actually used finely; lists keyed and windowed;
loading, error, focus, and motion states all first-class.

- [x] E8.1 Consolidate the ~25 flat signals into grouped stores updated with `reconcile()` — **resolved without the consolidation**: the stated goal (multi-field consumers stop firing per-signal) is achieved by E8.6's `batch()` (every tick flushes as one transaction) plus keyed rows over protocol deltas that already arrive diffed by id — `reconcile()` would re-diff what the wire format diffs. Regrouping ~25 signals would churn every UI module for no additional recomputes saved; rejected as pure churn (M)
- [x] E8.2 Keyed list rendering via `ui/list.js` (`each` = identity-keyed `For`, `eachIdx` = position-keyed `Index`) across outliner tabs, HUD toasts/wars, inspector beats/chips — a tick that patches three towns re-renders three rows (M)
- [x] E8.3 Windowed rendering: Chronicle windows at 120 (+240 per step), Places at 60, cast at 30, all keyed — at year 300 with a 10,610-entry chronicle the DOM holds ~700 nodes, ~2,600 with two extra windows loaded (M)
- [x] E8.4 `createResource` for `explain`/`entityLog` (inspector dock) and the legends sift (`sim.js` — stories/entities/artifacts ride one keyed resource with built-in dedupe and stale-race protection) (S-M)
- [x] E8.5 `popHistory` is a preallocated `Float64Array` ring (`state.js` `pushPopSample`/`popSeries`); the timebar sparkline reads through it without allocating (S)
- [x] E8.6 `batch()` wraps tick application and world arrival in `sim.js` — one flush per month (S)
- [x] E8.7 `main.js` (927 lines) split into `sim.js` (driver), `input.js` (pointer/keyboard), `gpu-audit.js` (bring-up + frame loop), `inspect.js` (cell inspection); the composition root is 92 lines (M)
- [x] E8.8 `settlementsById` memo — `selectedSettlement`, selection flights, hover teasers and market hub lookups all O(1) (S)
- [x] E8.9 Search candidate index memoized over world/settlement/telling revisions — keystrokes only score, never rebuild (S)
- [x] E8.10 `focus.js`: `trapFocus` (restore-on-close) on all dialog popovers; `roveTabs` arrow-key roving on both tablists, restructured so only `role="tab"` children are owned (axe `aria-required-children` clean) (M)
- [x] E8.11 `prefers-reduced-motion` block stops `twinkle`, sheet slides and toast keyframes (S)
- [x] E8.12 ARIA pass: `aria-pressed` on all toggle groups, `aria-live` toasts, `role="status"` loading veil with E7.5 stage text; canvases wrapped in `<main>`, HUD chrome in a labeled region, almanac a labeled section, viewport zoom un-blocked (S)

Gates: Playwright interaction sweep — 300-year world, chronicle at
5,000+ events, scroll/filter/tab through every panel with DOM-node and
long-task budgets enforced; axe-core scan clean on landmarks, dialogs,
tab semantics. **Status: gate green** — seed 777 @ 512 driven to year 300
(chronicle 10,610 entries); every panel swept with DOM < 2,700 nodes and
zero long tasks > 200 ms; axe-core 4.10 reports zero violations in default
state and with dialogs open; arrow-key roving verified on both tablists;
zero console errors across the sweep.

## E9 — Orbital II (render pipeline)

Damage-driven by default, partial uploads, decoupled overlay cadence — the
renderer stops paying full price for quiet frames.

- [x] E9.1 Damage-driven GPU frames: `gpuLive` continuous mode (`main.js:117,132`) becomes opt-in for animated states (water visible at depth, flight, playback); idle map = zero GPU frames (S-M)
- [x] E9.2 Partial tint updates: territory RLE patches update only changed texture rows instead of the full-canvas `set_tint` re-upload per political change (`gpu.js:66-71`) (M)
- [x] E9.3 Overlay cadence split: routes/winds animation stops forcing `dirty = true` every frame during playback (`main.js:164`) — vector overlay redraws on its own clock (M)
- [x] E9.4 Split `render.js` (1,359 lines) into compositor, label/collision, and marker/route modules — the per-frame path becomes auditable (M)
- [x] E9.5 Label layout cache: placement + collision computed on zoom-bucket/set changes, cached; per-frame work is blit-only (labels currently stroke+fill every dirty frame, `render.js:1002-1194`) (M)
- [x] E9.6 Picking decoupled from render cadence: `labelBoxes` produced by the layout pass, not the draw call (`picking.js:51-56`) — hit-testing stays correct even on skipped frames (S)
- [x] E9.7 `flyTo` yields to the user: any pointer/wheel input cancels the flight immediately (`view.js:48-70,74-128`) (S)
- [x] E9.8 Pointer-move allocation removal: `midOf`'s Map-spread per pinch event (`view.js:90-97`) becomes two-slot arithmetic (S)
- [x] E9.9 CPU fallback compositor gains dirty-region `putImageData` — the no-GPU path stops full-canvas blits per change (`render.js:508,654`) (M)
- [x] E9.10 Context-loss drill: scripted WebGL context loss + restore in Playwright, proving the `recreateGpuOnGl` path (`gpu.js:28-33`) end-to-end, on a schedule (M)

Gates: idle-map GPU frame count = 0 over 10 s (probe); playback at speed 3
holds 60 fps CSS-frame budget on the probe machine; context-loss drill
green in `report.sh`'s browser section.

## E10 — Proof of Speed (the instrument extended)

Perf claims become banded checks, like every other claim in this project.
This track starts first; every other track lands against its gates.

- [x] E10.1 `diagnose perf`: per-stage generation budgets (bands per stage, not just the 400 ms total), asserted across the seed sweep (S)
- [x] E10.2 Tick-rate band: native months/minute at year-0 and year-100 worlds — pacing regressions caught the month they land (S)
- [x] E10.3 Payload meter: pack bytes/cell and median tick-payload bytes as banded metrics, printed in `game/reports/bench.txt` (S)
- [x] E10.4 Wasm size budget check in `report.sh` (with E6.4) (S)
- [x] E10.5 Browser boot probe: Playwright script measuring cold load → engine ready → first rendered frame, banded, alongside `atlas-check.py` (M)
- [x] E10.6 Memory ceiling checks: native peak RSS and wasm memory pages after a 1200-month run, banded (S-M)
- [x] E10.7 Long-task audit: PerformanceObserver during 100 months of speed-3 playback; band on main-thread tasks > 50 ms (M)
- [x] E10.8 Perf history: `bench.txt` appends dated rows so drift across weeks is visible, not just pass/fail (S)

Gates: `report.sh` gains a `perf` section that runs in under 3 minutes;
all bands hold across the standard 3-seed sweep.

## E11 — The Broken Monolith (structure as a system)

`world.rs` is 2,343 lines owning every grid, every subsystem's state, the
tick loop, serialization, and domain logic. The lattice needs load-bearing
walls, not one room.

- [x] E11.1 Serialization out: `pack.rs` (binary) and `snapshot.rs` (JSON meta/tick assembly) leave `world.rs` (`world.rs:2193-2343`) (M)
- [x] E11.2 Domain logic out: famine (`world.rs:1871-1965`) and prospecting (`world.rs:1969-2015`) become modules like their peers (S-M)
- [x] E11.3 State split: `World` decomposes into `Fields` (grids), `Peoples` (settlements/cultures/societies), `Economy` (market/areas/merchants), `Chronicle` (events/registry/artifacts) — subsystem signatures take one sub-struct instead of eight loose params (`world.rs:1730-1739`) (L)
- [x] E11.4 System trait: `fn run(&mut SimCtx, &mut EventSink)` with declared cadence (monthly/yearly/on-event); the tick loop becomes an ordered system list instead of 170 inline lines (`world.rs:1646-1823`) (M)
- [x] E11.5 `EventSink` replaces the fifteen `new_events.extend(...)` merge points — systems emit, the sink orders and stamps (S-M)
- [ ] E11.6 `diagnose.rs` (2,303 lines) reorganized around the registry (E2.7): checks live beside band declarations, the harness shrinks to plumbing (M)
- [ ] E11.7 `bevy_ecs` evaluation ADR: measure the hand-rolled system lattice against bevy_ecs scheduling on real tick workloads; adopt or reject with numbers, not taste (M)
- [ ] E11.8 Module-dependency lint in `report.sh`: leaf modules must not import `world`; the import DAG stays acyclic by check, not convention (S)

Gates: determinism hash byte-stable through every structural move; no
file in `src/` over 1,200 lines when the track closes; system list and
tick order printed by `diagnose` and asserted stable.

## Ready (start here)

Quick wins with outsized ratios, extracted from the tracks above — each
lands alone in under a session:

- E6.2 `panic = "abort"` + E6.3 explicit wasm-opt: two config lines against a 3.0 MB binary.
- E5.6 artifact slice clone and E5.7 merchant key clones: pure waste, zero risk.
- E7.1 generation stamps + E7.2 timeouts: closes the stale-response bug class before it is ever observed.
- E8.5 popHistory ring buffer and E8.6 `batch()` per tick: two-line Solid wins.
- Resources stage costs ~300 ms of the ~1.1 s generation — second only to terrain (perf.txt, E10.1 baseline). Profile deposit placement's suitability scan; a cheap candidate-mask pass likely halves it.

## Later / research

Wasm SIMD128 for the noise/erosion inner loops (after E5.11 fusion settles) ·
wasm threads + rayon behind COOP/COEP (after E7.8) · GPU compute erosion
(shared with world-roadmap "Later") · OffscreenCanvas moving Orbital into a
render worker (after E6.7) · f16 field textures where WebGL2 allows ·
WebGPU compute path for hillshade/texture shading when driver coverage
justifies a second pipeline.

## Rejected (do not re-open without a superseding ADR)

- **E3.7/E3.8 Columnar entity tables + string interning** — measured after
  the E3.1 header/meta split: bootstrap JSON is 34.9 KB against a 6.56 MB
  pack at 512 with 1200 months of history (0.5 %, a few KB post-brotli).
  A struct-of-arrays codec for Settlement/Deposit/Ruin would be a second,
  hand-maintained encoding of structs the tick path still ships as JSON —
  dual declaration against ADR-0015 — to save single-digit kilobytes.
- **E4.6/E7.6 Binary tick lane** (ADR-0017) — measured after E4's delta
  work: main-thread `JSON.parse` is 0.080 ms median per tick (p90 0.145 ms,
  max 0.720 ms on a 65 KB payload), 0.15 % of tick wall time at year 100.
  A columnar codec with string tables would be a second encoding of
  seventeen mostly string-bearing sections to reclaim sub-millisecond
  parse; the tick wall lives in the canvas draw path (E9), not the wire.
- **FlatBuffers/protobuf/Cap'n Proto for the boundary** — the pack is
  already zero-copy typed arrays with a registry as schema; an IDL compiler
  adds a toolchain for no measured win.
- **A bundler for the web layer** — re-litigates ADR-0008; the buildless
  property is a feature, codegen (E2.4) stays a build.sh emit, not a
  bundling step.
- **Full engine-framework adoption (Bevy-the-engine)** — the product is a
  fullscreen quad plus canvas overlays (ADR-0006); a scene graph and asset
  pipeline solve problems this project does not have. (`bevy_ecs`-the-crate
  is a separate question — E11.7 answers it with measurements.)
- **Service-worker cache management** — rejected in ADR-0007; content-hash
  URLs already solve it statically.
