// M22 — the wasm leg of the cross-runtime replay gate.
//
// Loads the shipped wasm-bindgen module (web target) under bun, generates
// the world for a fixed seed, ticks a fixed number of months, and prints
// the seismic ledger hash — nothing else — so report.sh can compare it
// byte-for-byte against `diagnose seismic-hash` for the same arguments.
//
//   bun scripts/wasm-replay.mjs <seed> <size> <months>
//
// Exit codes: 0 hash printed · 3 stale wasm (no seismic_hash export) ·
// 1 anything else. A stale binary is "skipped, not failed" upstream.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const glue = join(here, "..", "game", "web", "js", "wasm", "calliope.js");
const wasm = join(here, "..", "game", "web", "js", "wasm", "calliope_bg.wasm");

const seed = Number(process.argv[2] ?? 777);
const size = Number(process.argv[3] ?? 512);
const months = Number(process.argv[4] ?? 240);

const mod = await import(glue);
await mod.default(readFileSync(wasm));

const world = new mod.WasmWorld(seed, size);
if (typeof world.seismic_hash !== "function") {
  console.error("stale wasm: no seismic_hash export — rebuild with scripts/build.sh");
  process.exit(3);
}
let left = months;
while (left > 0) {
  const step = Math.min(left, 240);
  world.tick(step);
  left -= step;
}
if (process.argv[5] === "debug" && typeof world.seismic_debug === "function") {
  console.log(world.seismic_debug());
} else {
  console.log(world.seismic_hash());
}
