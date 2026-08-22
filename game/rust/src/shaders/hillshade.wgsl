// M7.5 / M63 — multi-directional oblique-weighted hillshade with a
// curvature accent (research/10 #18): four low suns (NW leading, N, W,
// SW filling) instead of one, so ridges running every direction carve;
// a Laplacian term etches ridgelines bright and valley floors dark
// (texture shading), all from the same four height taps the caller
// already holds. `k` is the world-scaled shade strength (U.opts.y).
fn mdow_shade(he: f32, hs: f32, hw: f32, hn: f32, h: f32, k: f32) -> f32 {
  let gx = (he - hw) * 0.5;
  let gy = (hs - hn) * 0.5;
  let mdow = (-gx - gy) * 0.62 + (-gy) * 0.24 + (-gx) * 0.24 + (gx - gy) * 0.08;
  let curv = clamp((he + hs + hw + hn - 4.0 * h) * k * 0.55, -0.10, 0.10);
  return clamp(1.0 + k * mdow * 1.05 - curv, 0.58, 1.34);
}
