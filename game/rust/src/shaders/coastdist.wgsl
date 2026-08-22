// coastdist.wgsl — the compute lane's first client (M67, ADR-0027).
//
// One jump-flood pass over the seed grid: every cell looks at nine
// candidates a stride away and keeps the nearest land seed. The law is
// integer end to end — seeds are u32 cell indices, distances are exact
// u32 squares, ties break toward the smaller seed index — so the CPU
// twin in compute.rs (`jfa_pass_cpu`) replays this kernel bit for bit
// on any conformant device. Byte-parity between the two IS the lane's
// contract; a device that fails it is degraded to the twin.
//
// The schedule (strides next_pow2(max(w,h))/2 … 1) lives in one place,
// `compute::jfa_strides`, and both executors walk it.

struct Params {
  w: u32,
  h: u32,
  stride: u32,
  _pad: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;

const NONE: u32 = 0xffffffffu;

fn d2_to(seed: u32, x: i32, y: i32) -> u32 {
  let sx = i32(seed % P.w);
  let sy = i32(seed / P.w);
  let dx = sx - x;
  let dy = sy - y;
  return u32(dx * dx + dy * dy);
}

@compute @workgroup_size(8, 8)
fn jfa(@builtin(global_invocation_id) gid: vec3<u32>) {
  if (gid.x >= P.w || gid.y >= P.h) {
    return;
  }
  let x = i32(gid.x);
  let y = i32(gid.y);
  let i = gid.y * P.w + gid.x;
  var best = src[i];
  var bd = 0u;
  if (best != NONE) {
    bd = d2_to(best, x, y);
  }
  let s = i32(P.stride);
  for (var dy = -1; dy <= 1; dy = dy + 1) {
    for (var dx = -1; dx <= 1; dx = dx + 1) {
      if (dx == 0 && dy == 0) {
        continue;
      }
      let nx = x + dx * s;
      let ny = y + dy * s;
      if (nx < 0 || ny < 0 || nx >= i32(P.w) || ny >= i32(P.h)) {
        continue;
      }
      let cand = src[u32(ny) * P.w + u32(nx)];
      if (cand == NONE) {
        continue;
      }
      let d = d2_to(cand, x, y);
      if (best == NONE || d < bd || (d == bd && cand < best)) {
        best = cand;
        bd = d;
      }
    }
  }
  dst[i] = best;
}
