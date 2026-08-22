// The coast law's third executor, held to a gate (M67 follow-on, ADR-0027).
//
// The WGSL kernel and the Rust CPU twin prove byte-parity on a real
// device every suite run. The JS port in game/web/js/render/compositor.js
// is the executor no device holds — the wasm-less browser path, a
// hand-mirrored copy, and exactly the silent-fork shape ADR-0026/0027
// exist to kill. This lane replays a golden seed field exported from the
// Rust twin (`diagnose compute --golden <file>`) through the JS port and
// demands byte-equality. The seed field, not the distance field, IS the
// law: integers end to end, so parity is exact or it is a fork.
//
// Usage: bun scripts/coast-js-parity.mjs <golden-file>
// Emits a diagnostic report block in the harness's own format.

import { readFileSync } from "node:fs";
import { coastSeedField } from "../game/web/js/render/compositor.js";

const path = process.argv[2];
const line = (s = "") => process.stdout.write(s + "\n");

line("========================================================================");
line(" CALLIOPE DIAGNOSTIC · COAST-JS                     M67 third executor");
line("========================================================================");

const rows = [];
const check = (name, ok, value, target) => {
  rows.push({ name, ok, value, target });
};

function readCases(buf) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  if (String.fromCharCode(...buf.subarray(0, 4)) !== "CJFA") {
    throw new Error("not a golden file (bad magic)");
  }
  let o = 4;
  const n = dv.getUint32(o, true); o += 4;
  const cases = [];
  for (let k = 0; k < n; k++) {
    const nameLen = dv.getUint32(o, true); o += 4;
    const name = new TextDecoder().decode(buf.subarray(o, o + nameLen)); o += nameLen;
    const w = dv.getUint32(o, true); o += 4;
    const h = dv.getUint32(o, true); o += 4;
    const cells = w * h;
    // copies, because the file's byte offset is not 4-byte aligned
    const hgt = new Float32Array(buf.buffer.slice(buf.byteOffset + o, buf.byteOffset + o + cells * 4));
    o += cells * 4;
    const seeds = new Uint32Array(buf.buffer.slice(buf.byteOffset + o, buf.byteOffset + o + cells * 4));
    o += cells * 4;
    cases.push({ name, w, h, hgt, seeds });
  }
  return cases;
}

try {
  if (!path) throw new Error("usage: bun scripts/coast-js-parity.mjs <golden-file>");
  const buf = readFileSync(path);
  const cases = readCases(buf);
  if (cases.length === 0) throw new Error("golden file carries no cases");
  line(` golden: ${path} · ${cases.length} case(s) · ${buf.length} B`);
  for (const c of cases) {
    const t0 = performance.now();
    const mine = coastSeedField(c.hgt, c.w, c.h);
    const ms = performance.now() - t0;
    let diverge = 0;
    for (let i = 0; i < mine.length; i++) if (mine[i] !== c.seeds[i]) diverge++;
    line(` ${c.name}: ${c.w}×${c.h} · js ${ms.toFixed(0)} ms · ${diverge} of ${mine.length} cells diverge`);
    check(
      `${c.name} js/rust seed parity`,
      diverge === 0,
      diverge === 0 ? "byte-parity" : `${diverge} diverge`,
      "M67 gate: the JS port of the coast law walks the same seed field as the Rust twin — one law, three executors",
    );
  }
} catch (e) {
  check("coast-js lane runs", false, "error", `M67 gate: the JS twin must be executable — ${e.message}`);
}

line();
line("---- checks ----------------------------------------------------------");
let pass = 0, fail = 0;
for (const r of rows) {
  if (r.ok) pass++; else fail++;
  line(`[${r.ok ? "PASS" : "FAIL"}] ${r.name.padEnd(38)} ${String(r.value).padStart(14)}   (${r.target})`);
}
line(`CHECKS: ${pass} pass · 0 warn · ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
