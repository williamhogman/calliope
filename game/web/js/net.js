// World engine bridge: the simulation is Rust compiled to WASM, running in
// a worker. Same binary payload format as before, so unpack() is unchanged.

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

export function unpack(buf) {
  const dv = new DataView(buf);
  const hlen = dv.getUint32(0, true);
  const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 4, hlen)));
  const base = 4 + hlen;
  const arrays = {};
  for (const a of header.arrays) {
    const Ctor = DTYPES[a.dtype];
    if (!Ctor) throw new Error(`unknown dtype ${a.dtype}`);
    arrays[a.name] = new Ctor(buf, base + a.offset, a.nbytes / Ctor.BYTES_PER_ELEMENT);
  }
  return { header, arrays };
}

const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
let seq = 0;
const pending = new Map();

worker.onmessage = (e) => {
  const { id, ok, buf, json, error } = e.data;
  const p = pending.get(id);
  if (!p) return;
  pending.delete(id);
  if (ok) p.resolve({ buf, json });
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
  return unpack(buf);
}

export async function tickWorld(_id, months) {
  const { json } = await call({ op: "tick", months });
  return JSON.parse(json);
}
