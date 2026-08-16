// Simulation worker: hosts the WASM world off the main thread so the map
// stays responsive while a world generates or years tick by.

import { loadEngine } from "./wasm-load.js";

const ready = loadEngine();
let world = null;

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
      const bytes = world.pack();
      self.postMessage({ id, ok: true, buf: bytes.buffer }, [bytes.buffer]);
    } else if (op === "tick") {
      if (!world) throw new Error("no world — generate one first");
      self.postMessage({ id, ok: true, json: world.tick(months) });
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
    } else {
      throw new Error(`unknown op: ${op}`);
    }
  } catch (err) {
    self.postMessage({ id, ok: false, error: String((err && err.message) || err) });
  }
};
