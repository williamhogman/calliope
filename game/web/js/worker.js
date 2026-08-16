// Simulation worker: hosts the WASM world off the main thread so the map
// stays responsive while a world generates or years tick by.

import { loadEngine } from "./wasm-load.js";

const ready = loadEngine();
let world = null;
// E4.4 — chronicle cursor: ticks carry [from, to) instead of event arrays;
// the worker pulls the new slice through events_range and ships it beside
// the payload, so the engine never serializes its events twice.
let evCursor = 0;

self.onmessage = async (e) => {
  const { id, op, seed, size, months, kind, key, ent } = e.data;
  try {
    const { WasmWorld } = await ready;
    if (op === "generate") {
      if (world) {
        world.free();
        world = null;
      }
      world = new WasmWorld(seed >>> 0, size);
      evCursor = world.events_len();
      const bytes = world.pack();
      self.postMessage({ id, ok: true, buf: bytes.buffer }, [bytes.buffer]);
    } else if (op === "tick") {
      if (!world) throw new Error("no world — generate one first");
      const json = world.tick(months);
      const n = world.events_len();
      const events = evCursor < n ? world.events_range(evCursor, n) : "[]";
      evCursor = n;
      self.postMessage({ id, ok: true, json, events });
    } else if (op === "explain") {
      if (!world) throw new Error("no world — generate one first");
      if (typeof world.explain !== "function") throw new Error("engine has no explain");
      self.postMessage({ id, ok: true, json: world.explain(kind, key) });
    } else if (op === "stories") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.stories() });
    } else if (op === "entities") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.entities() });
    } else if (op === "entityLog") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.entity_log(BigInt(ent)) });
    } else if (op === "artifacts") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.artifacts() });
    } else if (op === "timings") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.timings() });
    } else if (op === "bootstrap") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.bootstrap() });
    } else {
      throw new Error(`unknown op: ${op}`);
    }
  } catch (err) {
    self.postMessage({ id, ok: false, error: String((err && err.message) || err) });
  }
};
