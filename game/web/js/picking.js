// Unified hit-testing: one click resolves to the most specific entity
// under the cursor — settlement, deposit, feature label, else the cell.

export function pick(world, view, renderer, px, py, opts = {}) {
  if (!world) return null;
  const touch = !!opts.touch;
  const header = world.header;
  const W = header.width || header.size;
  const H = header.size;

  // 1. settlements — small targets, biggest priority
  let best = null, bestD = Infinity;
  for (const s of header.settlements) {
    const sx = view.tx + (s.x + 0.5) * view.scale;
    const sy = view.ty + (s.y + 0.5) * view.scale;
    const d = Math.hypot(px - sx, py - sy);
    if (d < bestD) { bestD = d; best = s; }
  }
  if (best && bestD <= (touch ? 24 : 14)) {
    return { kind: "settlement", id: best.id };
  }

  // 1b. ruins (M9.1) — quiet marks on the ground, a touch harder to hit
  let bru = null, bruD = Infinity;
  for (const r of header.ruins || []) {
    const sx = view.tx + (r.x + 0.5) * view.scale;
    const sy = view.ty + (r.y + 0.5) * view.scale;
    const d = Math.hypot(px - sx, py - sy);
    if (d < bruD) { bruD = d; bru = r; }
  }
  if (bru && bruD <= (touch ? 20 : 11)) {
    return { kind: "ruin", id: bru.eid };
  }

  // 2. mineral deposits, when the resources overlay is lit
  if (opts.resourcesOn) {
    let bd = Infinity, bdep = null;
    for (const d of header.deposits || []) {
      const sx = view.tx + (d.x + 0.5) * view.scale;
      const sy = view.ty + (d.y + 0.5) * view.scale;
      const dist = Math.hypot(px - sx, py - sy);
      if (dist < bd) { bd = dist; bdep = d; }
    }
    if (bdep && bd <= (touch ? 20 : 11)) {
      return { kind: "deposit", id: bdep.r, x: bdep.x, y: bdep.y };
    }
  }

  // 3. feature labels — the renderer remembers where names were drawn
  if (opts.labelsOn) {
    for (const lb of renderer.labelBoxes || []) {
      if (px >= lb.x0 && px <= lb.x1 && py >= lb.y0 && py <= lb.y1) {
        return { kind: "feature", id: lb.index };
      }
    }
  }

  // 4. the ground itself
  const [wx, wy] = view.screenToWorld(px, py);
  const cx = Math.floor(wx), cy = Math.floor(wy);
  if (cx >= 0 && cy >= 0 && cx < W && cy < H) {
    return { kind: "cell", x: cx, y: cy };
  }
  return null;
}
