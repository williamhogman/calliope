# Era IX — The Sealed Instrument (M456–M515)

Full four-field specs for Era IX of `../ROADMAP-500.md`, the final
sixty phases of the Five Hundred: the causal graph and the why query,
counterfactual branching, a query language over history, explanation
coverage to one hundred percent, the archival world format and its
exports, the cross-run portfolio and observatory, the final
determinism audits, the closing sweep, the long soak, and the seal.
Era IX has no forge; it ends at the seal. The one-liners in the parent
file are binding; these specs expand them.

### M456 — The Causal Graph
- **Intent:** History stops being a flat log and becomes a structure a player, or the harness, can walk backward through.
- **Build:** Add a `CausalEdge { effect: EventId, cause: EventId, kind: CauseKind }` record emitted alongside every `Event` push in `chronicle.rs`, populated by each system that already knows its trigger (famine from drought, migration from famine, succession from death); store edges in a `CausalGraph` keyed by effect for O(1) backward walks and by cause for forward fan-out.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/event.rs, game/rust/src/famine.rs, game/rust/src/politics.rs, new: game/rust/src/causality.rs
- **Gate:** `diagnose causality` reports every non-genesis event has at least one recorded cause edge and zero dangling `EventId` references, on all seeds in the sweep.

### M457 — Cause Propagation
- **Intent:** The graph gains texture: system-level couplings (drought → famine → rising) are named, not just implied by adjacency in time.
- **Build:** Extend `CauseKind` with named coupling variants per cross-system pathway already computed in `famine.rs`, `climate.rs`, and `politics.rs` (drought-to-famine, famine-to-migration, famine-to-unrest, unrest-to-rising), and have each producing system tag its edge with the coupling it fired through rather than a generic label.
- **Touches:** game/rust/src/causality.rs, game/rust/src/famine.rs, game/rust/src/climate.rs, game/rust/src/politics.rs, game/rust/src/society.rs
- **Gate:** `diagnose causality --couplings` confirms every drought event with a downstream famine within the coupling's documented lag window carries a matching typed edge, across the seed sweep.

### M458 — Causal Determinism
- **Intent:** The why-chain must be as reproducible as the world it explains, or its answers are noise dressed as history.
- **Build:** Fold the causal graph's edge list (sorted by effect id then cause id) into `hash_state` alongside existing fields and entities, and verify edge insertion order never depends on iteration over unordered collections.
- **Touches:** game/rust/src/causality.rs, game/rust/src/bin/diagnose.rs, game/rust/src/snapshot.rs
- **Gate:** `diagnose determinism` shows the state hash, now including causal edges, bit-identical across native and WASM and across repeated runs of the same seed.

### M459 — The Why Query
- **Intent:** A player can finally ask "why did this happen" and receive the chain back to first causes instead of a shrug.
- **Build:** Implement `causality::why(graph, event_id) -> Vec<EventId>` walking cause edges backward until a root (no incoming edge) is reached, cycle-guarded by a visited set, exposed through a new `World::why` WASM binding returning JSON in causal order, with a matching `why` subcommand in diagnose.rs.
- **Touches:** game/rust/src/causality.rs, game/rust/src/lib.rs, game/web/js/ui/inspector.js, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose causality --why <event>` terminates within the graph's depth bound and returns a strictly time-nonincreasing chain ending at a zero-indegree root, on every sweep seed.

### M460 — Counterfactuals Against the Archive
- **Intent:** History earns weight when you can ask what would have happened if one cause had not fired.
- **Build:** Add a branch-at-cause facility that clones world state at the tick preceding a chosen cause event, suppresses that cause's effect, re-runs simulation to the original archive's end tick using the same derived RNG streams, and diffs entity and field state between branches.
- **Touches:** game/rust/src/world.rs, game/rust/src/causality.rs, game/rust/src/snapshot.rs, new: game/rust/src/counterfactual.rs
- **Gate:** `diagnose counterfactual` shows the branched run's untouched subsystems hash-identical to baseline and the suppressed-cause subtree diverging, on all sweep seeds.

### M461 — Causal-Graph Properties
- **Intent:** A causal graph that can loop through time or explode in fan-in is a graph nobody can trust to answer "why."
- **Build:** Add structural checks over the graph: acyclicity with respect to event timestamps (no cause may postdate its effect), full connectivity from every non-genesis event to some root, and a bounded per-effect fan-in ceiling derived from the coupling table in M457.
- **Touches:** game/rust/src/causality.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose causality --properties` reports zero time-inverted edges, zero unreachable-to-root events, and max fan-in under the documented ceiling, across the seed sweep.

### M462 — A Query Language Over History
- **Intent:** The chronicle and entity registry deserve one language to filter, join, and aggregate instead of ad-hoc Rust loops per question.
- **Build:** Define a small query grammar (predicate filters on event kind/time/entity, joins across events and the entity registry, aggregate reducers count/sum/percentile) compiled to a closure pipeline over `chronicle.events` and `chronicle.registry`, with a parser and an AST evaluated read-only against `World`.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/lib.rs, new: game/rust/src/query.rs
- **Gate:** `diagnose query` runs a fixed corpus of representative queries against the sweep seeds and asserts each returns byte-identical results run to run.

### M463 — Query Surfaces
- **Intent:** One query engine should answer identically whether asked from the inspector, the map browser, or a terminal.
- **Build:** Expose `query::run` through a new `World::query` WASM binding consumed by a query bar in `inspector.js` and a new browser panel, and add a `diagnose query --exec <string>` command-line entry point sharing the same parser.
- **Touches:** game/rust/src/query.rs, game/rust/src/lib.rs, game/web/js/ui/inspector.js, new: game/web/js/ui/querybar.js
- **Gate:** the same query string issued through inspector, browser panel, and CLI on a fixed seed returns identical JSON, checked by `diagnose query --cross-surface`.

### M464 — Query Performance
- **Intent:** A query over a thousand years of history must feel instant, not batch.
- **Build:** Add indices over `chronicle.events` (by kind, by entity, by time-bucket, BTreeMap-backed for deterministic iteration) built once per snapshot load and consulted by the query planner in `query.rs` before falling back to full scan.
- **Touches:** game/rust/src/query.rs, game/rust/src/chronicle.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose query --bench` holds full-history query latency under 100 ms at the p95 band on a thousand-year archive across sweep seeds.

### M465 — Explain-Everything
- **Intent:** Every number a player sees should be able to justify itself the way settlement growth and good prices already do.
- **Build:** Extend `explain.rs`'s additive-ledger pattern to every remaining surfaced derived quantity — climate indices, resource yields, culture and faith metrics — each new `explain_*` function mirroring its live computation term for term with cross-reference comments at both sites.
- **Touches:** game/rust/src/explain.rs, game/rust/src/climate.rs, game/rust/src/resources.rs, game/rust/src/culture.rs, game/rust/src/society.rs
- **Gate:** `diagnose explain --coverage` finds zero surfaced numeric fields without a registered `explain_*` ledger, across the full field inventory.

### M466 — Provenance Cards
- **Intent:** Any value on screen should offer its derivation on demand, not just in a diagnostics log.
- **Build:** Add a `ProvenanceCard { title, terms: Vec<(label, value, source_kind)>, total }` rendering layer atop `explain::explain`, and wire a click-to-explain affordance in the inspector that renders the card from the same JSON the harness audits.
- **Touches:** game/rust/src/explain.rs, game/web/js/ui/inspector.js, new: game/web/js/ui/provenance.js
- **Gate:** `diagnose explain --render` confirms every provenance card's listed terms sum exactly to its displayed total, for every explainable kind on all sweep seeds.

### M467 — Explanation Coverage
- **Intent:** "Explain everything" is a claim that must be measured, not assumed true after M465's sweep.
- **Build:** Add a coverage audit that statically enumerates every value written to the wire payload (`pack.rs`) or UI state (`game/web/js/ui/state.js`) and cross-checks each against the `explain_*` registry, flagging any surfaced-but-unexplained field.
- **Touches:** game/rust/src/explain.rs, game/rust/src/pack.rs, game/rust/src/bin/diagnose.rs, game/web/js/ui/state.js
- **Gate:** `diagnose explain --audit` reports one hundred percent of payload and UI-state fields matched to a registered explanation, zero exceptions.

### M468 — The Archival World Format
- **Intent:** A world that outlives the session it was generated in needs a format that documents itself, not tribal knowledge of `pack.rs`.
- **Build:** Define an archival container format wrapping the existing pack payload with a versioned header (format version, generation parameters, schema hash), specified to the rigor of ADR-0007's binary protocol, extending the `archive::write`/`archive::read` pair from M391 to honor it.
- **Touches:** game/rust/src/pack.rs, game/rust/src/archive.rs, docs/adr/0007-binary-pack-protocol.md, new: docs/adr (archival-world-format ADR, numbered at land time)
- **Gate:** `diagnose archive --roundtrip` shows write-then-read producing a state hash identical to the source world, for every sweep seed and format version in the compatibility table.

### M469 — The Run Archive
- **Intent:** Complete histories, not just terrain snapshots, must be storable compact and queryable without replaying the run.
- **Build:** Extend `archive.rs` to serialize the full chronicle event stream and causal graph alongside terrain and entity state, compressed and indexed by the same event indices from M464 so a cold archive answers queries without simulation.
- **Touches:** game/rust/src/archive.rs, game/rust/src/chronicle.rs, game/rust/src/causality.rs, game/rust/src/query.rs
- **Gate:** `diagnose archive --query-cold` runs the M462 query corpus against a serialized-then-reloaded archive and matches live-run results exactly, across sweep seeds.

### M470 — Archive Integrity
- **Intent:** An archive claiming decade-stable readability must prove it survives corruption, versions, and time.
- **Build:** Add CRC32 checksums per archive section (mirroring `pack.rs`'s existing checksum discipline from ADR-0016), a migration-lane dispatcher that upgrades older format versions to current, and a fixture corpus of archives at each historical format version checked in for regression.
- **Touches:** game/rust/src/archive.rs, game/rust/src/pack.rs, docs/adr/0016-pack-v2-quantized-crc-payload.md, new: game/reports/archive-fixtures
- **Gate:** `diagnose archive --integrity` verifies every fixture archive's checksum, successfully migrates every non-current version, and rejects deliberately corrupted fixtures with a checksum failure.

### M471 — Export Surfaces
- **Intent:** The world's shape should leave the simulator as documents a historian would recognize — atlases, chronicles, gazetteers, genealogies.
- **Build:** Add exporters that read from the archive format (not live world state) to produce atlas plates, chronicle text, a gazetteer of named places, and genealogy trees, each an archival document type with its own schema version.
- **Touches:** game/rust/src/archive.rs, game/rust/src/telling.rs, game/rust/src/naming.rs, new: game/rust/src/export.rs
- **Gate:** `diagnose export --all` produces all four document types from a fixture archive with zero missing referenced entities and schema-valid output, across sweep seeds.

### M472 — The Printed World
- **Intent:** The atlas should be able to leave the screen entirely and stand as a printable artifact.
- **Build:** Add a publication-grade plate renderer building on the existing wgpu fullscreen-shader pipeline, rasterizing to a fixed-DPI vector-friendly output (SVG or high-resolution PNG plates) with legend, scale bar, and title block composited deterministically from archive data.
- **Touches:** game/rust/src/render.rs, game/rust/src/export.rs, new: game/rust/src/plate.rs
- **Gate:** `diagnose export --plate` renders the same archive twice and produces byte-identical plate files, verified by checksum in the sweep.

### M473 — Export Fidelity
- **Intent:** A document regenerated from the same archive and version must be the same document, or the archive was never really the source of truth.
- **Build:** Add a fidelity check comparing freshly regenerated exports (atlas, chronicle, gazetteer, genealogy, plate) against a checked-in golden set keyed by archive hash and export schema version, failing on any byte divergence not explained by a version bump.
- **Touches:** game/rust/src/export.rs, game/rust/src/plate.rs, game/rust/src/bin/diagnose.rs, new: game/reports/export-golden
- **Gate:** `diagnose export --fidelity` shows byte-identical regeneration against golden fixtures for every export type and every fixture archive.

### M474 — Cross-Run Statistics
- **Intent:** A single world is an anecdote; the portfolio of worlds the sweep already generates is a dataset worth analyzing as one.
- **Build:** Add a statistics module computing distributions (mean, percentile bands, histograms) over history-level metrics across the seed sweep — event-kind frequencies, dynasty lifespans, famine incidence, reversal counts from `telling.rs` — persisted alongside `game/reports/SUMMARY.txt`.
- **Touches:** game/rust/src/telling.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, new: game/rust/src/portfolio.rs
- **Gate:** `diagnose portfolio --stats` emits stable percentile bands for every tracked cross-run metric, reproducible run to run for a fixed seed set.

### M475 — The Comparative Atlas
- **Intent:** Worlds should be placeable side by side, structurally, not just eyeballed on separate screens.
- **Build:** Add a comparative rendering mode built on `plate.rs` that overlays or tiles corresponding plates from multiple archives with shared legend and normalized scale, plus a structural-distance metric (biome composition, settlement count, dynasty count) driving the comparison layout.
- **Touches:** game/rust/src/plate.rs, game/rust/src/portfolio.rs, new: game/rust/src/compare.rs
- **Gate:** `diagnose portfolio --compare` produces a deterministic comparative plate for a fixed archive pair and reports the structural-distance metric within its expected band.

### M476 — Portfolio Distinctiveness
- **Intent:** The oatmeal problem's final answer: prove the portfolio's worlds are perceptually distinct, not just numerically varied.
- **Build:** Implement the oatmeal-detector metrics from research digest 11 — biome patch fragmentation, settlement-spacing entropy, feature-type Jaccard distance between seed pairs — computed across the full sweep and banded as a diagnostics check.
- **Touches:** game/rust/src/portfolio.rs, game/rust/src/compare.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose portfolio --distinctiveness` shows median pairwise Jaccard distance and fragmentation entropy both above their documented floor across the full sweep.

### M477 — The Observatory UI
- **Intent:** Checks, budgets, and portfolio statistics have lived in scattered reports; they belong on one screen a person can actually watch.
- **Build:** Add an observatory panel in the web UI that ingests `game/reports/SUMMARY.txt` and the portfolio statistics JSON, rendering checks by band color, budget trend sparklines, and portfolio distinctiveness scores in one layout.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/portfolio.rs, game/web/js/ui/app.js, new: game/web/js/ui/observatory.js
- **Gate:** the observatory panel renders all current SUMMARY.txt checks and portfolio metrics with zero parse failures against the live report format.

### M478 — The Run Notebook
- **Intent:** Watching a world unfold deserves a place to mark what mattered, not just a static report screen.
- **Build:** Add a notebook feature storing tick-stamped annotations against a loaded archive or live run, bookmarkable to a specific tick and entity, persisted as a small JSON sidecar keyed by archive hash.
- **Touches:** game/web/js/ui/observatory.js, game/web/js/ui/state.js, new: game/web/js/ui/notebook.js
- **Gate:** bookmarks saved in a notebook session survive a reload of the same archive and resolve to the same tick and entity, verified by a scripted UI check.

### M479 — Observatory Performance Held in Bands
- **Intent:** The observatory must stay responsive even as it aggregates the whole portfolio, or nobody will use it while tuning.
- **Build:** Add render-time and data-load budgets for the observatory panel to the existing budget-tracking discipline, profiling panel refresh against growing report and portfolio sizes.
- **Touches:** game/web/js/ui/observatory.js, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose bench --observatory` holds panel refresh time under its documented millisecond band for the full-size sweep report.

### M480 — Every Layer Explains
- **Intent:** The explain and causality machinery must span the whole stack — geology through faith — as one fabric, not isolated demos.
- **Build:** Close remaining gaps by wiring `causality.rs` edges and `explain.rs` ledgers through the layers not yet covered (erosion, prospecting, patina, artifact provenance), so a single causal query can cross from a geological event to a settlement's founding to a dynasty's rise.
- **Touches:** game/rust/src/erosion.rs, game/rust/src/prospecting.rs, game/rust/src/patina.rs, game/rust/src/artifact.rs, game/rust/src/causality.rs, game/rust/src/explain.rs
- **Gate:** `diagnose causality --fabric` walks a sample chain from a geological cause to a faith-layer effect with no missing edge, across sweep seeds.

### M481 — The Grand Inspector
- **Intent:** A player's curiosity about one entity should be answerable on a single card, not a scavenger hunt across panels.
- **Build:** Compose a unified inspector card assembling causes and effects (via `query.rs`/`causality.rs`), kin (via `culture.rs`/genealogy from `export.rs`), and works (artifacts, founded settlements) for any selected entity, sourced entirely from existing query and explain machinery.
- **Touches:** game/rust/src/query.rs, game/rust/src/causality.rs, game/rust/src/export.rs, game/web/js/ui/inspector.js
- **Gate:** the grand inspector card for a fixed entity on a fixed seed matches a golden JSON fixture across all four sections, checked by `diagnose inspector --golden`.

### M482 — Fabric Integrity
- **Intent:** A cross-layer fabric only holds if every reference across layers actually resolves; dangling references are the fabric's tears.
- **Build:** Add a full cross-layer reference audit walking every entity, event, and causal edge for references into other layers (kin ids, settlement ids, artifact ids, causal edge endpoints) and flagging any that resolve to nothing.
- **Touches:** game/rust/src/causality.rs, game/rust/src/query.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose fabric --audit` reports one hundred percent of cross-layer references resolved, zero dangling ids, across the full sweep.

### M483 — The ADR-0003 Program's Final Audit
- **Intent:** Determinism has been law since ADR-0003; the era's close demands proof it holds everywhere it claims to, not just where it's been tested.
- **Build:** Enumerate every generation and tick module against ADR-0003's derived-stream discipline, verify each subsystem's RNG offset is unique and documented, and audit every ordered-iteration requirement (BTreeMap over HashMap) across the codebase named in the ADR.
- **Touches:** docs/adr/0003-single-seed-determinism.md, game/rust/src/util.rs, game/rust/src/bin/diagnose.rs
- **Gate:** `diagnose determinism --audit` finds zero unordered-iteration violations and zero colliding RNG stream offsets across every module in the ADR's touch list.

### M484 — The Float Question Closed
- **Intent:** Cross-platform float divergence is the one determinism risk ADR-0003 flagged but never fully resolved; the era closes it by decision, not hope.
- **Build:** Survey every floating-point operation reachable from the state hash for platform-divergent behavior (transcendental functions, fused-multiply-add, summation order), decide and document the bit-identity scope in a new ADR building on Era VIII's float-determinism-policy work, and enforce it with a lint or explicit wrapper functions where the ADR mandates fixed-order or fixed-point substitutes.
- **Touches:** game/rust/src/noisegen.rs, game/rust/src/climate.rs, game/rust/src/hydrology.rs, docs/adr/0003-single-seed-determinism.md, new: docs/adr (float-determinism-scope ADR, numbered at land time)
- **Gate:** `diagnose determinism --cross-platform` shows the state hash bit-identical between native and WASM builds for every seed in the sweep, with the ADR's scope covering every flagged operation.

### M485 — Determinism Regression Lanes
- **Intent:** Determinism must stop being a manual check someone remembers to run and become a lane nobody can skip.
- **Build:** Wire the determinism, cross-platform, and causal-hash checks from M458, M483, and M484 into a permanent fast lane in `report.sh` that runs on every change, distinct from the full multi-seed sweep, sized to finish in seconds.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs
- **Gate:** the determinism regression lane completes in under ten seconds and is invoked unconditionally by `report.sh`, with no flag to bypass it.

### M486 — The Budget Ledger Sealed
- **Intent:** The era's performance promises stop being aspirations and become a permanent, dated record the harness cannot silently drift from.
- **Build:** Every existing diagnose.rs budget check (generation time, tick time, memory watermark, payload size) gains a final target band plus a persisted history table written to game/reports/ on each report.sh run, so bands are compared against both the current threshold and the last N recorded runs for drift, not just pass/fail.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, new: game/reports/budget-history.jsonl, docs/ROADMAP-500.md
- **Gate:** report.sh full prints all budget checks PASS against final bands and appends a history row; a synthetic 10% regression injected in a test run is flagged WARN by drift comparison.

### M487 — The Thousand-Year Watermark
- **Intent:** Memory must prove it is bounded, not merely observed to be small on the runs anyone has actually tried.
- **Build:** Extend diagnose.rs with a dedicated long-run allocation probe that ticks a world one thousand simulated years under the alloc-count feature, sampling resident allocation totals at fixed intervals and fitting a linear trend; the check fails if slope exceeds a near-zero epsilon per century.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/state.rs, game/rust/src/world.rs, game/rust/scripts/report.sh, new: game/reports/memory-1000y.txt
- **Gate:** thousand-year alloc-count probe reports allocation-growth slope below the fitted epsilon band across three seeds, WARN otherwise, with the raw samples archived for audit.

### M488 — The Payload at Rest
- **Intent:** The boundary formats that cross the WASM/JS and archive edges stop changing shape once this phase closes, so nothing downstream can rot.
- **Build:** Freeze the pack v2 payload (ADR-0016) and snapshot.rs's serialization surface at documented final field layouts, add a byte-size budget per world size class, and write the specification document enumerating every field, its type, and its version tag.
- **Touches:** game/rust/src/pack.rs, game/rust/src/snapshot.rs, docs/adr/0016-pack-v2-quantized-crc-payload.md, new: docs/PACK-SPEC.md
- **Gate:** diagnose.rs's pack round-trip check confirms byte-identical unpack-then-pack for all sweep seeds and payload sizes stay within the documented budget per size class.

### M489 — The Module Map
- **Intent:** Anyone opening the repo cold should find every module's purpose and its governing ADR without asking a person.
- **Build:** Produce a module map cross-referencing every file in game/rust/src/ to the ADRs and research digests that justify its design, generated by a script that scans module doc-comments and ADR links so drift is caught automatically rather than trusted to memory.
- **Touches:** game/rust/src/lib.rs, docs/adr/README.md, new: docs/MODULE-MAP.md, new: game/rust/scripts/check-module-map.sh
- **Gate:** check-module-map.sh exits zero only when every .rs file under game/rust/src/ has a corresponding MODULE-MAP.md entry and cited ADR, run as part of report.sh.

### M490 — Debt Zero
- **Intent:** The codebase closes its own books — nothing left half-finished, nothing referenced that no longer exists.
- **Build:** Sweep the repo for TODO/FIXME markers, orphaned functions unreferenced outside tests, and dead registry entries flagged by the codegen system (ADR-0015), resolving or formally deferring each one with a linked ADR or roadmap phase.
- **Touches:** game/rust/src, docs/GAP-ANALYSIS.md, docs/ROADMAP-500.md, new: game/rust/scripts/check-debt.sh
- **Gate:** check-debt.sh finds zero TODO/FIXME markers and zero dead-code warnings under cargo build with warnings-as-errors, wired into report.sh full.

### M491 — The Build Sealed
- **Intent:** A stranger with nothing but the repo and a shell should reach a fully green suite in one command, and know how long to wait.
- **Build:** Write a single top-level script that performs a clean checkout build (cargo build release, wasm target, web asset build) through report.sh full end to end, timing each stage, with a documented wall-clock band for the whole sequence on reference hardware.
- **Touches:** game/rust/scripts/report.sh, docs/README.md, new: scripts/clean-build-verify.sh
- **Gate:** clean-build-verify.sh run from a fresh git clone exits zero with total wall-clock inside the documented band, printed at the end.

### M492 — Systems, to the Source
- **Intent:** Every simulated behavior in the world should trace back to the literature that justified it, so future tuning has ground to stand on.
- **Build:** Rewrite docs/SYSTEMS.md so each system section cites its governing research digest under docs/research/ by number, states the scaling law or model in use, and links the diagnose.rs check that proves it holds.
- **Touches:** docs/SYSTEMS.md, docs/research, game/rust/src/bin/diagnose.rs
- **Gate:** a doc-truth script confirms every SYSTEMS.md section contains at least one research citation and one check-id reference, zero sections exempted.

### M493 — The Corpus Closed
- **Intent:** Two hundred eighty sources of research either landed in the world or were consciously turned away, and that ledger closes here.
- **Build:** Walk every docket in docs/research/ and every row of the SYNTHESIS.md gap list, marking each implication landed (with phase number), rejected (with reason), or deferred (with ADR), consolidating the result into a single closing table.
- **Touches:** docs/research/SYNTHESIS.md, docs/research, new: docs/research/CLOSEOUT.md
- **Gate:** every row in the SYNTHESIS.md gap list and every digest's Calliope table has a landed/rejected/deferred marker in CLOSEOUT.md, verified by a line-count cross-check script.

### M494 — Zero Structural Gaps
- **Intent:** The gap analysis that has tracked missing capability since early in the project reaches its terminal, honest state.
- **Build:** Revise docs/GAP-ANALYSIS.md to its final form, closing every entry against a landed phase or an ADR recording deliberate rejection, leaving no entry in an undecided state.
- **Touches:** docs/GAP-ANALYSIS.md, docs/adr
- **Gate:** a script parses GAP-ANALYSIS.md and confirms every row resolves to either a phase number in 1–515 or an ADR filename, zero rows unresolved.

### M495 — The Operator's Manual
- **Intent:** Running, sweeping, and querying the instrument should never again require reading source to figure out.
- **Build:** Write the operator's manual covering report.sh invocation modes, the query language from M462, provenance-card lookups from M466, and extension points for new systems, each section demonstrated with a runnable command.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, new: docs/OPERATORS-MANUAL.md
- **Gate:** every command block in OPERATORS-MANUAL.md executes successfully against the current build, checked by a doc-example runner script.

### M496 — The Theory of the Instrument
- **Intent:** The reasons behind Calliope's shape — deterministic core, layered generation, harness-as-gate — deserve one document a newcomer can read start to end.
- **Build:** Write a synthesis document explaining the instrument's governing choices by walking ADR-0002, ADR-0003, ADR-0005, ADR-0009, and ADR-0015 in narrative order, connecting each to the research principles in SYNTHESIS.md that motivated it.
- **Touches:** docs/adr/0002-rust-core-compiled-to-wasm.md, docs/adr/0003-single-seed-determinism.md, docs/adr/0005-layered-generation-then-tick.md, docs/adr/0009-diagnostics-harness-as-gate.md, docs/research/SYNTHESIS.md, new: docs/THEORY-OF-THE-INSTRUMENT.md
- **Gate:** doc-truth audit confirms every claim in THEORY-OF-THE-INSTRUMENT.md cites either an ADR number or a research digest number, zero uncited claims.

### M497 — Doc-Truth Audited
- **Intent:** Documentation that claims a property without a check behind it is fiction wearing a lab coat, and this phase ends that.
- **Build:** Build a doc-truth auditor that scans every markdown file under docs/ for factual claims about the system (numbers, invariants, bands) and cross-references them against diagnose.rs check identifiers, flagging any claim lacking a matching proving check.
- **Touches:** docs, game/rust/src/bin/diagnose.rs, new: game/rust/scripts/doc-truth.sh
- **Gate:** doc-truth.sh exits zero with every quantitative claim across docs/ matched to a check id, run as a standing report.sh full stage.

### M498 — The Five Hundred's Own History
- **Intent:** The roadmap that built this world deserves the same historical treatment the world itself receives — causes, reversals, and consequences.
- **Build:** Write a chronicle of the project's own eras: the decisions that shaped it, the ADRs superseded, and the phases that reversed earlier choices (e.g. ADR-0007 superseded by ADR-0016), narrated with the same selection-memory-consequence discipline from docs/research/15-tellability-chronicle-prose.md.
- **Touches:** docs/adr/README.md, docs/ROADMAP-500.md, new: docs/THE-FIVE-HUNDRED.md
- **Gate:** THE-FIVE-HUNDRED.md names every superseded ADR and every era's forge phase, cross-checked by a script against docs/adr/ status fields and roadmap headings.

### M499 — The ADR Corpus Complete
- **Intent:** Every architecture question this project ever asked itself now has its answer indexed in one place, for good.
- **Build:** Audit docs/adr/ against the full module map and roadmap for any architecturally significant decision made in code without a corresponding ADR, backfilling records for the ones found, and finalize docs/adr/README.md's index and cross-reference table.
- **Touches:** docs/adr/README.md, docs/adr, docs/MODULE-MAP.md
- **Gate:** check-module-map.sh reports zero modules citing an undocumented architectural decision, and docs/adr/README.md's index count matches the file count under docs/adr/.

### M500 — The Five-Hundredth Number
- **Intent:** The roadmap's marker phase demands its own reckoning — proof that everything built through here still stands.
- **Build:** Run report.sh full twice in independent processes against the same seed set, diff every determinism hash, budget band, and check result between runs, and publish the combined result as the M500 audit record.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, new: game/reports/M500-audit.txt
- **Gate:** both independent report.sh full runs produce byte-identical determinism hashes and identical PASS/WARN/FAIL tables, zero discrepancies between the two.

### M501 — The Stranger's Eye
- **Intent:** The instrument has never been examined by someone who didn't build it, and that blind spot closes now.
- **Build:** Conduct a structured external-grade review walking the operator's manual, theory document, and a fresh clone build as an outside engineer would, logging every point of confusion, undocumented assumption, or surprising behavior encountered.
- **Touches:** docs/OPERATORS-MANUAL.md, docs/THEORY-OF-THE-INSTRUMENT.md, new: docs/REVIEW-FINDINGS.md
- **Gate:** REVIEW-FINDINGS.md contains a dated, itemized findings list with severity tags, each item assigned a tracking phase or ADR reference before this phase closes.

### M502 — Findings Landed
- **Intent:** A review that changes nothing was theater; this phase makes the stranger's findings either fixed or formally refused.
- **Build:** Work through every item in docs/REVIEW-FINDINGS.md, landing code or documentation fixes for accepted findings and writing a one-paragraph rejection rationale for declined ones, updating the module map and doc-truth audit where fixes touch documented claims.
- **Touches:** docs/REVIEW-FINDINGS.md, docs/MODULE-MAP.md, game/rust/src, docs
- **Gate:** every REVIEW-FINDINGS.md item carries a closed status of fixed-with-commit-reference or rejected-with-rationale, verified by a status-completeness script, zero items open.

### M503 — Strangeness, Standing
- **Intent:** The outsider's review should not be a one-time event but a repeatable discipline the instrument submits to on schedule.
- **Build:** Turn the M501 review procedure into a standing checklist script that re-runs the clean-clone build, manual walkthrough steps, and doc-truth audit on demand, storing dated results for comparison across future audits.
- **Touches:** docs/REVIEW-FINDINGS.md, scripts/clean-build-verify.sh, new: scripts/strangeness-audit.sh, new: docs/STRANGENESS-LOG.md
- **Gate:** strangeness-audit.sh runs unattended to completion and appends a dated, structured entry to STRANGENESS-LOG.md, verified present after execution.

### M504 — The Final Sweep Design
- **Intent:** The closing proof of the whole instrument needs a deliberately chosen set of seeds, sizes, and lengths before it can be run.
- **Build:** Design the closing sweep specification — the seed list, world size classes, and simulated-year lengths chosen to cover the expressive range established by the ERA histograms from M474, documented with the rationale for each choice.
- **Touches:** game/rust/src/bin/diagnose.rs, docs/research/11-pcg-theory.md, new: docs/CLOSING-SWEEP-SPEC.md
- **Gate:** CLOSING-SWEEP-SPEC.md enumerates a fixed seed/size/year matrix of at least twenty runs and diagnose.rs accepts it as a named sweep profile without error.

### M505 — The Closing Portfolio
- **Intent:** The definitive set of worlds this instrument will be remembered by gets generated once, archived, and made visible.
- **Build:** Generate every run in the CLOSING-SWEEP-SPEC.md matrix, archive each to the M468/M469 archival format, and publish the full set to the observatory's portfolio view built in M474–M479.
- **Touches:** game/rust/src/snapshot.rs, game/rust/src/bin/diagnose.rs, new: game/reports/closing-portfolio/
- **Gate:** every run in the closing-sweep matrix archives successfully with valid checksums and appears in the observatory portfolio listing, count matching the spec exactly.

### M506 — Portfolio Verified
- **Intent:** The closing portfolio must prove it survives every check the instrument has ever accumulated, not just the ones convenient to run.
- **Build:** Run the complete diagnose.rs check suite — determinism, budgets, seam invariants, ERA histograms, causal-graph properties, explanation coverage — against every archived world in the closing portfolio, collating results into one verification table.
- **Touches:** game/rust/src/bin/diagnose.rs, game/reports/closing-portfolio/, new: game/reports/portfolio-verification.txt
- **Gate:** portfolio-verification.txt shows every closing-portfolio world at all-PASS across every check category, zero WARN or FAIL entries.

### M507 — The Long Soak
- **Intent:** Stability claims made over months of tuning need to survive an uninterrupted run longer than any single sitting.
- **Build:** Run a continuous multi-day soak of the reference world configuration at final specification, sampling memory, tick timing, and determinism checkpoints at fixed intervals without process restart, logging every sample to a soak report.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/world.rs, new: game/reports/soak-log.jsonl
- **Gate:** the soak run completes its full scheduled duration without crash, with memory slope and tick-time variance both inside the M486/M487 sealed bands throughout.

### M508 — The Cold Start
- **Intent:** An archive that only reads back correctly on the machine that wrote it isn't archival at all.
- **Build:** Re-verify every archived world from the closing portfolio by unpacking and replaying it inside a freshly provisioned, dependency-minimal environment distinct from the one that generated it, confirming the archival format's self-description holds.
- **Touches:** game/rust/src/pack.rs, game/rust/src/snapshot.rs, game/reports/closing-portfolio/, new: game/reports/cold-start-verify.txt
- **Gate:** every closing-portfolio archive unpacks and replays byte-identical to its original determinism hash from a clean environment with zero dependency on generation-time state.

### M509 — Soak Findings Closed
- **Intent:** Anomalies noticed during the long soak cannot be waved off as noise; each one gets a name and a cause.
- **Build:** Triage every anomaly flagged in soak-log.jsonl and cold-start-verify.txt, root-causing each to a specific mechanism, landing a fix or a documented band widening, and re-running the affected check to confirm closure.
- **Touches:** game/reports/soak-log.jsonl, game/reports/cold-start-verify.txt, game/rust/src/bin/diagnose.rs, new: docs/SOAK-FINDINGS.md
- **Gate:** SOAK-FINDINGS.md lists every anomaly with a root cause and closing check reference, and a re-run of the soak and cold-start checks shows zero open anomalies.

### M510 — The Whole-Suite Gate
- **Intent:** Every check this project has ever written, from Era I through Era IX, must be provable green in a single sitting.
- **Build:** Assemble a whole-suite runner that invokes report.sh full plus every era-specific standing check (determinism regression lanes, doc-truth audit, module-map check, portfolio verification, soak replay) in one sequenced pass, timing the total.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, new: scripts/whole-suite-gate.sh
- **Gate:** whole-suite-gate.sh exits zero with every constituent check PASS and total wall-clock inside a documented band, run against the closing portfolio.

### M511 — The Meta-Gate
- **Intent:** The gate itself needs a gate — proof that the roadmap, the proof ledger, and the documentation agree with each other and with the code.
- **Build:** Build a meta-gate script that runs a roadmap-completeness check (every phase M1–M515 has a corresponding spec and landed or forge status), the doc-truth audit from M497, and a proof-ledger consistency check that every diagnose.rs check id referenced in documentation actually exists in the binary.
- **Touches:** docs/ROADMAP-500.md, game/rust/scripts/doc-truth.sh, game/rust/src/bin/diagnose.rs, new: scripts/meta-gate.sh
- **Gate:** meta-gate.sh exits zero with roadmap completeness at 515/515, doc-truth audit clean, and zero dangling check-id references, all three run together.

### M512 — The Instrument's Statement
- **Intent:** The machine that has been tuned for five hundred phases should be able to declare, in its own output, that it is finished.
- **Build:** Compose a completion-evidence printer that runs whole-suite-gate.sh and meta-gate.sh, then emits a single signed statement document listing every check that ran, its result, and the determinism hashes of the closing portfolio as cryptographic evidence of the claim.
- **Touches:** scripts/whole-suite-gate.sh, scripts/meta-gate.sh, new: scripts/print-completion-evidence.sh, new: game/reports/COMPLETION-EVIDENCE.txt
- **Gate:** print-completion-evidence.sh produces COMPLETION-EVIDENCE.txt containing every check id, its PASS result, and a checksum of the closing-portfolio hashes, regenerable byte-identical on a second run.

### M513 — The Last Ledger
- **Intent:** Nothing may cross the seal half-done; every open item in the project's tracking must resolve to closed or explicitly, permanently deferred.
- **Build:** Sweep docs/GAP-ANALYSIS.md, docs/research/CLOSEOUT.md, docs/REVIEW-FINDINGS.md, docs/SOAK-FINDINGS.md, and the roadmap itself for any remaining open, queued, or pending marker, closing or formally deferring each with an ADR reference.
- **Touches:** docs/GAP-ANALYSIS.md, docs/research/CLOSEOUT.md, docs/REVIEW-FINDINGS.md, docs/SOAK-FINDINGS.md, docs/ROADMAP-500.md, new: docs/LAST-LEDGER.md
- **Gate:** LAST-LEDGER.md aggregates zero open items across all five source documents, verified by a script that greps each for pending/open/queued markers and finds none.

### M514 — The Seal ADR
- **Intent:** The closing of the Five Hundred is itself an architectural decision and deserves the record every other decision here received.
- **Build:** Write the seal ADR declaring the Five Hundred complete, summarizing the instrument's final state, citing docs/LAST-LEDGER.md and game/reports/COMPLETION-EVIDENCE.txt as its evidence, and formally closing the roadmap to further phase additions.
- **Touches:** docs/adr/README.md, docs/LAST-LEDGER.md, game/reports/COMPLETION-EVIDENCE.txt, new: docs/adr (the-seal ADR, numbered at land time)
- **Gate:** the seal ADR is Accepted, listed in docs/adr/README.md's index, and cites both LAST-LEDGER.md and COMPLETION-EVIDENCE.txt by path.

### M515 — The Sealed Instrument
- **Intent:** The arc that began with the first diagnostics report closes with a single script whose exit code is the final word on the Five Hundred.
- **Build:** Compose the terminal gate script that runs meta-gate.sh, whole-suite-gate.sh, print-completion-evidence.sh, and a check that the seal ADR is Accepted, in that order, refusing to exit zero unless every prior phase's standing checks are green.
- **Touches:** scripts/meta-gate.sh, scripts/whole-suite-gate.sh, scripts/print-completion-evidence.sh, docs/adr, new: scripts/the-sealed-instrument.sh
- **Gate:** scripts/the-sealed-instrument.sh exits zero exactly once all constituent gates and the seal ADR are green, and its exit code is the harness's final, unskippable verdict on the Five Hundred.
