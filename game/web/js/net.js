// World engine bridge: the simulation is Rust compiled to WASM, running in
// a worker. The pack v2 payload is [u32 header_len][header json][blob]:
// the header is stamped `pack: 2`, carries a CRC-32 of the blob (E3.6),
// the territory grid as RLE (E3.5), and per-array quantization descriptors
// (`q`, E3.4) that this edge dequantizes back to float32 — everything
// downstream of unpack() sees exactly the arrays it always did.
//
// E7 — this edge also runs the worker line discipline: request stamps
// (E7.1), per-op reply deadlines (E7.2), tick coalescing (E7.3), abortable
// staged generation with progress (E7.4/E7.5), and crash recovery that
// respawns the worker, regenerates the seed and fast-forwards to the month
// the sky fell on (E7.10) — determinism makes the replay exact.

import { EVENT_KINDS } from "./gen/constants.js";
import { OP, DEADLINE } from "./proto.js";
import { loadModule } from "./wasm-load.js";

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
    if (a.bits !== undefined && a.bits < 8) {
      // M70 bit lane — a categorical byte grid packed LSB-first at the
      // sub-byte width its live maximum earns. Expand it back to the full
      // Uint8Array the rest of the client has always seen; the values are
      // exact, this lane loses nothing.
      const src = new Uint8Array(buf, base + a.offset, a.nbytes);
      const cells = a.shape[0] * a.shape[1];
      const bits = a.bits;
      const mask = (1 << bits) - 1;
      const out = new Uint8Array(cells);
      let acc = 0, have = 0, p = 0;
      for (let i = 0; i < cells; i++) {
        while (have < bits) {
          acc |= (src[p++] || 0) << have;
          have += 8;
        }
        out[i] = acc & mask;
        acc >>>= bits;
        have -= bits;
      }
      arrays[a.name] = out;
    } else if (a.q) {
      // quantized wire (E3.4) — u16 or u8 depending on the field's lane;
      // dequantize to the field's true float32. Read through a byte view so
      // a u8 lane can leave the following u16 section at an odd offset.
      const bytes = new Uint8Array(buf, base + a.offset, a.nbytes);
      const q =
        a.dtype === "uint8"
          ? bytes
          : new Uint16Array(bytes.slice().buffer, 0, a.nbytes / 2);
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
      const need = base + a.offset;
      arrays[a.name] =
        need % Ctor.BYTES_PER_ELEMENT === 0
          ? new Ctor(buf, need, a.nbytes / Ctor.BYTES_PER_ELEMENT)
          : new Ctor(
              buf.slice(need, need + a.nbytes),
              0,
              a.nbytes / Ctor.BYTES_PER_ELEMENT,
            );
    }
  }

  // territory rides the header as RLE (E3.5); expand to the grid the
  // renderer and picker expect.
  if (header.territory) {
    arrays.territory = decodeTerritory(header.territory, header.size * header.width);
  }
  return { header, arrays };
}

// ---------- the worker line (E6.7 · E7) ----------

let worker = null;
let seq = 0;
let pending = new Map();

// E7.1 — the stamp of the world the UI believes in; every world-reading
// request carries it, and the worker refuses if a regenerate got there first.
let currentStamp = null;

// E7.10 — enough truth to rebuild everything from scratch: the seed pair
// plus how far time has run. Determinism does the rest.
let lastGen = null; // { seed, size }
let monthsRun = 0;
let genId = null; // in-flight generate request id — the abort target (E7.4)
let restoring = false;

const lostHandlers = [];
const restoredHandlers = [];
export function onWorkerLost(cb) {
  lostHandlers.push(cb);
}
export function onWorkerRestored(cb) {
  restoredHandlers.push(cb);
}

function call(msg, { transfer = [], onProgress, deadline } = {}) {
  return new Promise((resolve, reject) => {
    const id = ++seq;
    if (msg.op === OP.GENERATE) genId = id;
    // E7.2 — a tripwire for a hung worker, not a latency budget.
    const ms = deadline ?? DEADLINE[msg.op] ?? DEADLINE.default;
    const timer = setTimeout(() => {
      if (!pending.has(id)) return;
      pending.delete(id);
      reject(new Error(`[${msg.op}] no reply after ${Math.round(ms / 1000)}s — the worker looks hung`));
    }, ms);
    pending.set(id, { op: msg.op, resolve, reject, onProgress, timer });
    worker.postMessage({ id, ...msg }, transfer);
  });
}

function spawnWorker() {
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (e) => {
    const { id, ok, buf, json, events, stamp, error, progress } = e.data;
    const p = pending.get(id);
    if (!p) return;
    if (progress) {
      p.onProgress?.(progress); // not terminal — the request lives on
      return;
    }
    pending.delete(id);
    clearTimeout(p.timer);
    if (ok) p.resolve({ buf, json, events, stamp });
    else p.reject(new Error(p.op ? `[${p.op}] ${error}` : error));
  };
  worker.onerror = (e) => workerDown(new Error(e.message || "simulation worker crashed"));
  worker.onmessageerror = () => workerDown(new Error("simulation worker message failed"));
  // E6.7 — compile the binary once on this thread, hand the Module to the
  // worker as its first message. Every later op queues behind this send.
  (async () => {
    let module = null;
    try {
      module = await loadModule();
    } catch {
      /* worker falls back to its own compile */
    }
    try {
      await call(module ? { op: OP.INIT, module } : { op: OP.INIT });
    } catch {
      /* init failure surfaces on the first real op */
    }
  })();
}
spawnWorker();

// E7.10 — the muse's understudy: on a worker crash, reject what was in
// flight, spawn a fresh worker, regenerate the same seed and fast-forward
// to the month the sky fell on.
function workerDown(err) {
  for (const [, p] of pending) {
    clearTimeout(p.timer);
    p.reject(err);
  }
  pending.clear();
  try {
    worker.terminate();
  } catch {
    /* already gone */
  }
  spawnWorker();
  for (const cb of lostHandlers) {
    try {
      cb(err);
    } catch {
      /* the UI's problem */
    }
  }
  if (lastGen && !restoring) restore();
}

async function restore() {
  restoring = true;
  try {
    const target = monthsRun;
    const { seed, size } = lastGen;
    await rawGenerate(seed, size);
    for (let left = target; left > 0; ) {
      const step = Math.min(240, left); // the engine's per-call ceiling
      await call({ op: OP.TICK, months: step, expect: currentStamp });
      left -= step;
    }
    monthsRun = target;
    const w = await packWorld();
    for (const cb of restoredHandlers) {
      try {
        cb(w);
      } catch {
        /* the UI's problem */
      }
    }
  } catch (err) {
    console.error("world restore failed:", err);
  } finally {
    restoring = false;
  }
}

// Merge the small bootstrap call (E3.1 — vocabulary tables, entity state)
// into a freshly unpacked pack payload: the full header the UI expects.
async function mergeBootstrap(world) {
  const { json } = await call({ op: OP.BOOTSTRAP, expect: currentStamp });
  Object.assign(world.header, JSON.parse(json));
  decodeEvents(world.header.events);
  return world;
}

async function rawGenerate(seed, size, onProgress) {
  const res = await call({ op: OP.GENERATE, seed, size }, { onProgress });
  genId = null;
  currentStamp = res.stamp ?? null;
  monthsRun = 0;
  return mergeBootstrap(unpack(res.buf));
}

// Repack the live world at whatever month it has reached (E7.10).
async function packWorld() {
  const res = await call({ op: OP.PACK, expect: currentStamp });
  return mergeBootstrap(unpack(res.buf));
}

export async function generateWorld(seed, size, onProgress) {
  lastGen = { seed, size };
  try {
    return await rawGenerate(seed, size, onProgress);
  } finally {
    genId = null;
  }
}

// E7.4 — the user changes their mind: condemn the in-flight generation.
// The worker's previous world survives untouched; the generate call
// rejects with "generation abandoned".
export function abortGenerate() {
  if (genId == null) return false;
  call({ op: OP.ABORT, target: genId }).catch(() => {});
  return true;
}

// E7.10 — chaos hook: kill the worker on purpose and watch the understudy
// take the stage. Wired to window.__calliope for console use; harmless to
// ship, priceless to verify.
export function crashWorkerForDebug() {
  workerDown(new Error("debug crash"));
}

// Tick v2 (E4): the payload carries only what changed — absent key means
// "you already hold the truth". Events ride a separate cursor pull done by
// the worker (E4.4); headlines arrive as indices into that fresh slice
// (E4.8), resolved to event objects here so the UI never sees the trick.
//
// E7.3 — overlapping calls coalesce: k callers waiting become one engine
// call for the summed months, and every waiter receives the same merged
// delta (deltas are cumulative "since last shipped", so the merge is exact).
let tickBusy = false;
let queuedMonths = 0;
let queuedWaiters = [];

async function rawTick(months) {
  const { json, events } = await call({ op: OP.TICK, months, expect: currentStamp });
  const res = JSON.parse(json);
  if (typeof res.month === "number") monthsRun = res.month;
  res.events = decodeEvents(JSON.parse(events || "[]"));
  res.headlines = (res.headlines || []).map((i) => res.events[i]).filter(Boolean);
  return res;
}

function drainTicks() {
  if (!queuedWaiters.length) return;
  const months = queuedMonths;
  const waiters = queuedWaiters;
  queuedMonths = 0;
  queuedWaiters = [];
  tickBusy = true;
  rawTick(months)
    .then(
      (r) => waiters.forEach((w) => w.res(r)),
      (e) => waiters.forEach((w) => w.rej(e)),
    )
    .finally(() => {
      tickBusy = false;
      drainTicks();
    });
}

export function tickWorld(_id, months) {
  if (tickBusy) {
    queuedMonths += months;
    return new Promise((res, rej) => queuedWaiters.push({ res, rej }));
  }
  tickBusy = true;
  const p = rawTick(months);
  p.catch(() => {}).finally(() => {
    tickBusy = false;
    drainTicks();
  });
  return p;
}

// Term ledger for a derived quantity ("why is this so?"); null when the
// engine has nothing to say about that entity. Ledgers carry `terms`;
// the M61 cell-provenance answer carries `chain` instead.
export async function explainWorld(kind, key) {
  const { json } = await call({ op: OP.EXPLAIN, kind, key, expect: currentStamp });
  const parsed = JSON.parse(json);
  return parsed && (parsed.terms || parsed.chain) ? parsed : null;
}

// Generation stage timings in seconds — the debug side channel (E3.9);
// wall-clock no longer rides the pack header.
export async function timingsWorld() {
  const { json } = await call({ op: OP.TIMINGS, expect: currentStamp });
  return Object.fromEntries(JSON.parse(json));
}

// ---------- the telling (M6) ----------

// Ranked microstories the sifter lifted from the chronicle (M6.5/M6.7).
export async function storiesWorld() {
  const { json } = await call({ op: OP.STORIES, expect: currentStamp });
  return JSON.parse(json);
}

// The chronicle's cast — every named entity, alive and dead (M6.1).
export async function entitiesWorld() {
  const { json } = await call({ op: OP.ENTITIES, expect: currentStamp });
  return JSON.parse(json);
}

// Every chronicle entry that speaks of one entity, oldest first (M6.6).
export async function entityLogWorld(entId) {
  const { json } = await call({ op: OP.ENTITY_LOG, ent: String(entId), expect: currentStamp });
  return decodeEvents(JSON.parse(json));
}

// The relics and their provenance (M6.3).
export async function artifactsWorld() {
  const { json } = await call({ op: OP.ARTIFACTS, expect: currentStamp });
  return JSON.parse(json);
}
