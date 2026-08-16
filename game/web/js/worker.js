// Simulation worker: hosts the WASM world off the main thread so the map
// stays responsive while a world generates or years tick by.
//
// E6.7 — the first message is always `init`, carrying the compiled
// `WebAssembly.Module` from the main thread (or nothing, if that compile
// failed — then this worker compiles for itself).
//
// E7 — the protocol grew up:
//   · ops run strictly in arrival order through a promise chain, except
//     `abort`, which cuts the line on purpose (E7.4);
//   · `generate` climbs the staged builder ladder, posting {id, progress}
//     between stages and yielding to the queue so an abort can land (E7.5);
//   · the world in memory carries a stamp, and requests carrying `expect`
//     are refused when a regenerate got there first — a late tick can
//     never bite the wrong world (E7.1).

import { loadEngine } from "./wasm-load.js";
import { OP } from "./proto.js";

let resolveEngine;
const engineReady = new Promise((r) => (resolveEngine = r));
let world = null;
let stamp = null; // E7.1 — identity of the world in memory
let genCount = 0;
// E4.4 — chronicle cursor: ticks carry [from, to) instead of event arrays;
// the worker pulls the new slice through events_range and ships it beside
// the payload, so the engine never serializes its events twice.
let evCursor = 0;
// E7.4 — generate request ids condemned while their ladder still climbs.
const aborted = new Set();

// A macrotask hop: queued messages (notably abort) get delivered.
const breathe = () => new Promise((r) => setTimeout(r, 0));

function need(expect) {
  if (!world) throw new Error("no world — generate one first");
  if (expect && expect !== stamp) {
    throw new Error(`stale request: the world is now ${stamp}, caller expected ${expect}`);
  }
}

async function handle(data) {
  const { id, op, seed, size, months, kind, key, ent, expect } = data;
  try {
    const { WasmWorldBuilder } = await (await engineReady);
    if (op === OP.GENERATE) {
      // The old world stays alive until the new one is real: an abandoned
      // or failed generation leaves the map exactly as it was.
      const builder = new WasmWorldBuilder(seed >>> 0, size);
      try {
        let info;
        do {
          info = JSON.parse(builder.step());
          self.postMessage({ id, progress: info });
          await breathe(); // let an abort land between stages
          if (aborted.has(id)) throw new Error("generation abandoned");
        } while (!info.done);
        const next = builder.finish();
        if (world) world.free();
        world = next;
        genCount += 1;
        stamp = `${seed >>> 0}-${size}-g${genCount}`;
        evCursor = world.events_len();
        const bytes = world.pack();
        self.postMessage({ id, ok: true, stamp, buf: bytes.buffer }, [bytes.buffer]);
      } finally {
        aborted.delete(id);
        builder.free(); // an abandoned ladder frees every intermediate
      }
    } else if (op === OP.TICK) {
      need(expect);
      const json = world.tick(months);
      const n = world.events_len();
      const events = evCursor < n ? world.events_range(evCursor, n) : "[]";
      evCursor = n;
      self.postMessage({ id, ok: true, json, events });
    } else if (op === OP.PACK) {
      // E7.10 — repack the live world at its current month: the crash-
      // recovery path rebuilds the UI's whole truth from this one payload.
      need(expect);
      const bytes = world.pack();
      self.postMessage({ id, ok: true, stamp, buf: bytes.buffer }, [bytes.buffer]);
    } else if (op === OP.EXPLAIN) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.explain(kind, key) });
    } else if (op === OP.STORIES) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.stories() });
    } else if (op === OP.ENTITIES) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.entities() });
    } else if (op === OP.ENTITY_LOG) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.entity_log(BigInt(ent)) });
    } else if (op === OP.ARTIFACTS) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.artifacts() });
    } else if (op === OP.TIMINGS) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.timings() });
    } else if (op === OP.BOOTSTRAP) {
      need(expect);
      self.postMessage({ id, ok: true, json: world.bootstrap() });
    } else {
      throw new Error(`unknown op: ${op}`);
    }
  } catch (err) {
    self.postMessage({ id, ok: false, op, error: String((err && err.message) || err) });
  }
}

// Strict arrival-order execution for world ops — the chain preserves the
// ordering the old blocking dispatch gave for free, now that generation
// yields between stages. Two ops bypass the line on purpose: `abort`,
// whose whole job is to cut it, and `init`, which must resolve the engine
// even when a generate posted earlier is already parked in the chain
// awaiting it (the main thread compiles the module before sending init,
// so init can genuinely arrive second).
let chain = Promise.resolve();
self.onmessage = (e) => {
  const { id, op } = e.data;
  if (op === OP.ABORT) {
    aborted.add(e.data.target);
    self.postMessage({ id, ok: true, json: "true" });
    return;
  }
  if (op === OP.INIT) {
    resolveEngine(loadEngine(e.data.module ?? undefined));
    (async () => {
      try {
        await (await engineReady);
        self.postMessage({ id, ok: true, json: "true" });
      } catch (err) {
        self.postMessage({ id, ok: false, op, error: String((err && err.message) || err) });
      }
    })();
    return;
  }
  chain = chain.then(() => handle(e.data));
};
