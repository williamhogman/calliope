// World engine bridge: the simulation is Rust compiled to WASM, running in
// a worker. The pack v2 payload is [u32 header_len][header json][blob]:
// the header is stamped `pack: 2`, carries a CRC-32 of the blob (E3.6),
// the territory grid as RLE (E3.5), and per-array quantization descriptors
// (`q`, E3.4) that this edge dequantizes back to float32 — everything
// downstream of unpack() sees exactly the arrays it always did.

import { EVENT_KINDS } from "./gen/constants.js";

// E1.12 — event kinds ride the wire as small ints; give them their names
// back once, at this edge, so everything downstream keys by name.
const EV_NAME = EVENT_KINDS.map((k) => k.name);
function decodeEvents(list) {
  for (const e of list || []) {
    if (typeof e.k === "number") e.k = EV_NAME[e.k] ?? String(e.k);
  }
  return list || [];
}

const DTYPES = {
  float32: Float32Array,
  float64: Float64Array,
  uint8: Uint8Array,
  int8: Int8Array,
  uint16: Uint16Array,
  int16: Int16Array,
  int32: Int32Array,
  uint32: Uint32Array,
};

// CRC-32 (IEEE, reflected) — mirrors util::crc32 in the engine (E3.6).
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[i] = c >>> 0;
  }
  return t;
})();

function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

// Engine RLE ([run, value, …] row-major, −1 = wild) → owner grid (E3.5).
function decodeTerritory(rle, cells) {
  const owner = new Int16Array(cells).fill(-1);
  let i = 0;
  for (let k = 0; k + 1 < rle.length; k += 2) {
    const run = rle[k], v = rle[k + 1];
    if (v >= 0) owner.fill(v, i, i + run);
    i += run;
  }
  return owner;
}

export function unpack(buf) {
  const dv = new DataView(buf);
  const hlen = dv.getUint32(0, true);
  const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 4, hlen)));
  if (header.pack !== 2) {
    throw new Error(`world payload is pack v${header.pack ?? 1}; this client speaks v2`);
  }
  const base = 4 + hlen;
  const got = crc32(new Uint8Array(buf, base));
  if (got !== header.crc32) {
    throw new Error(
      `world payload corrupt: crc ${got.toString(16)} ≠ ${header.crc32.toString(16)}`,
    );
  }
  const arrays = {};
  for (const a of header.arrays) {
    if (a.q) {
      // quantized u16 wire (E3.4) — dequantize to the field's true float32
      const q = new Uint16Array(buf, base + a.offset, a.nbytes / 2);
      const out = new Float32Array(q.length);
      const { scale, offset, xform } = a.q;
      if (xform === "sqrt") {
        for (let i = 0; i < q.length; i++) {
          const t = offset + q[i] * scale;
          out[i] = t * t;
        }
      } else {
        for (let i = 0; i < q.length; i++) out[i] = offset + q[i] * scale;
      }
      arrays[a.name] = out;
    } else {
      const Ctor = DTYPES[a.dtype];
      if (!Ctor) throw new Error(`unknown dtype ${a.dtype}`);
      arrays[a.name] = new Ctor(buf, base + a.offset, a.nbytes / Ctor.BYTES_PER_ELEMENT);
    }
  }
  // territory rides the header as RLE (E3.5); expand to the grid the
  // renderer and picker expect.
  if (header.territory) {
    arrays.territory = decodeTerritory(header.territory, header.size * header.width);
  }
  return { header, arrays };
}

const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
let seq = 0;
const pending = new Map();

worker.onmessage = (e) => {
  const { id, ok, buf, json, events, error } = e.data;
  const p = pending.get(id);
  if (!p) return;
  pending.delete(id);
  if (ok) p.resolve({ buf, json, events });
  else p.reject(new Error(error));
};
worker.onerror = (e) => {
  const err = new Error(e.message || "simulation worker failed");
  for (const [, p] of pending) p.reject(err);
  pending.clear();
};

function call(msg, transfer = []) {
  return new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, ...msg }, transfer);
  });
}

export async function generateWorld(seed, size) {
  const { buf } = await call({ op: "generate", seed, size });
  const world = unpack(buf);
  // E3.1 — the pack header is lean; vocabulary tables and entity state
  // ride a second, small bootstrap call, merged here so everything
  // downstream sees the header it always did.
  const { json } = await call({ op: "bootstrap" });
  Object.assign(world.header, JSON.parse(json));
  decodeEvents(world.header.events);
  return world;
}

// Tick v2 (E4): the payload carries only what changed — absent key means
// "you already hold the truth". Events ride a separate cursor pull done by
// the worker (E4.4); headlines arrive as indices into that fresh slice
// (E4.8), resolved to event objects here so the UI never sees the trick.
export async function tickWorld(_id, months) {
  const { json, events } = await call({ op: "tick", months });
  const res = JSON.parse(json);
  res.events = decodeEvents(JSON.parse(events || "[]"));
  res.headlines = (res.headlines || []).map((i) => res.events[i]).filter(Boolean);
  return res;
}

// Term ledger for a derived quantity ("why is this so?"); null when the
// engine has nothing to say about that entity.
export async function explainWorld(kind, key) {
  const { json } = await call({ op: "explain", kind, key });
  const parsed = JSON.parse(json);
  return parsed && parsed.terms ? parsed : null;
}

// Generation stage timings in seconds — the debug side channel (E3.9);
// wall-clock no longer rides the pack header.
export async function timingsWorld() {
  const { json } = await call({ op: "timings" });
  return Object.fromEntries(JSON.parse(json));
}

// ---------- the telling (M6) ----------

// Ranked microstories the sifter lifted from the chronicle (M6.5/M6.7).
export async function storiesWorld() {
  const { json } = await call({ op: "stories" });
  return JSON.parse(json);
}

// The chronicle's cast — every named entity, alive and dead (M6.1).
export async function entitiesWorld() {
  const { json } = await call({ op: "entities" });
  return JSON.parse(json);
}

// Every chronicle entry that speaks of one entity, oldest first (M6.6).
export async function entityLogWorld(entId) {
  const { json } = await call({ op: "entityLog", ent: String(entId) });
  return decodeEvents(JSON.parse(json));
}

// The relics and their provenance (M6.3).
export async function artifactsWorld() {
  const { json } = await call({ op: "artifacts" });
  return JSON.parse(json);
}
