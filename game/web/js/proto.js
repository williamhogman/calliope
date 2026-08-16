// E7.7 — the worker protocol's single vocabulary. net.js speaks it and
// worker.js dispatches on it, so a typo'd op is an import-time reference
// error instead of a silent "unknown op" at runtime. The ops are a JS↔JS
// contract — the Rust engine never sees these strings — which is why this
// module lives here rather than in gen/ with the engine-derived constants.
export const OP = Object.freeze({
  INIT: "init", //          E6.7 — precompiled WebAssembly.Module handoff
  GENERATE: "generate", //  E7.5 — staged; posts {id, progress} between stages
  ABORT: "abort", //        E7.4 — condemn an in-flight generate by id
  TICK: "tick",
  PACK: "pack", //          E7.10 — repack the live world at its current month
  BOOTSTRAP: "bootstrap",
  EXPLAIN: "explain",
  TIMINGS: "timings",
  STORIES: "stories",
  ENTITIES: "entities",
  ENTITY_LOG: "entityLog",
  ARTIFACTS: "artifacts",
});

// E7.2 — per-op reply deadlines (ms). The engine is local wasm, so these
// are tripwires for a hung or dead worker, not latency budgets; generation
// and compilation get minutes on purpose.
export const DEADLINE = Object.freeze({
  [OP.INIT]: 120000,
  [OP.GENERATE]: 300000,
  [OP.TICK]: 120000,
  default: 30000,
});
