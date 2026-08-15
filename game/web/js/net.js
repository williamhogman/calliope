// Network layer: binary world unpacking + JSON tick.

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

export async function generateWorld(seed, size) {
  const res = await fetch("/api/world", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ seed, size }),
  });
  if (!res.ok) throw new Error(`world generation failed (${res.status})`);
  return unpack(await res.arrayBuffer());
}

export async function tickWorld(id, months) {
  const res = await fetch(`/api/world/${encodeURIComponent(id)}/tick`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ months }),
  });
  if (!res.ok) throw new Error(`tick failed (${res.status})`);
  return res.json();
}
