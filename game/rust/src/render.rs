//! Orbital — the GPU imagery engine, in Rust on wgpu.
//!
//! One fullscreen pass renders every raster layer of the map: the satellite
//! "true colour" composite, the political tint, and the analytic layers.
//! World fields live in float textures and are bilinearly resampled in the
//! shader, so relief, coasts and climate stay smooth at any zoom; water
//! moves; seasons glide. Runs on WebGPU where the browser has it and falls
//! back to WebGL2 everywhere else — same WGSL either way.
//!
//! The 2D canvas above this draws only informational annotation (routes,
//! labels, settlements); the JS side falls back to its CPU compositor if no
//! adapter exists at all.

use wasm_bindgen::prelude::*;

const SHADER: &str = r#"
struct Uni {
  cam:  vec4<f32>,  // world-cell bounds of the viewport: left, right, top, bottom
  geo:  vec4<f32>,  // W, H, layer, month
  anim: vec4<f32>,  // time, rivers, snow, shade
  opts: vec4<f32>,  // hasTint, shadeK, srgb, unused
};

@group(0) @binding(0) var<uniform> U: Uni;
@group(0) @binding(1) var tHeight: texture_2d<f32>; // R32F height
@group(0) @binding(2) var tClim:   texture_2d<f32>; // tmean, tamp, precip, fertility
@group(0) @binding(3) var tMisc:   texture_2d<f32>; // discharge01, river, lake, coast/32
@group(0) @binding(4) var tTint:   texture_2d<f32>; // political tint rgba8

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
  var out: VOut;
  let x = f32(i32(vi) % 2) * 4.0 - 1.0;
  let y = f32(i32(vi) / 2) * 4.0 - 1.0;
  out.pos = vec4<f32>(x, y, 0.0, 1.0);
  out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
  return out;
}

// ---- sampling ---------------------------------------------------------------

fn bil_coords(p: vec2<f32>) -> vec4<f32> {
  // returns (i0.x, i0.y, f.x, f.y) for a clamped bilinear fetch
  let size = U.geo.xy;
  let q = clamp(p - vec2<f32>(0.5), vec2<f32>(0.0), size - vec2<f32>(1.0));
  let fl = floor(q);
  return vec4<f32>(fl, q - fl);
}

fn sample_h(p: vec2<f32>) -> f32 {
  let bc = bil_coords(p);
  let i0 = vec2<i32>(bc.xy);
  let mx = vec2<i32>(U.geo.xy) - vec2<i32>(1);
  let i1 = min(i0 + vec2<i32>(1), mx);
  let a = textureLoad(tHeight, i0, 0).r;
  let b = textureLoad(tHeight, vec2<i32>(i1.x, i0.y), 0).r;
  let c = textureLoad(tHeight, vec2<i32>(i0.x, i1.y), 0).r;
  let d = textureLoad(tHeight, i1, 0).r;
  return mix(mix(a, b, bc.z), mix(c, d, bc.z), bc.w);
}

fn sample_clim(p: vec2<f32>) -> vec4<f32> {
  let bc = bil_coords(p);
  let i0 = vec2<i32>(bc.xy);
  let mx = vec2<i32>(U.geo.xy) - vec2<i32>(1);
  let i1 = min(i0 + vec2<i32>(1), mx);
  let a = textureLoad(tClim, i0, 0);
  let b = textureLoad(tClim, vec2<i32>(i1.x, i0.y), 0);
  let c = textureLoad(tClim, vec2<i32>(i0.x, i1.y), 0);
  let d = textureLoad(tClim, i1, 0);
  return mix(mix(a, b, bc.z), mix(c, d, bc.z), bc.w);
}

fn sample_misc(p: vec2<f32>) -> vec4<f32> {
  let bc = bil_coords(p);
  let i0 = vec2<i32>(bc.xy);
  let mx = vec2<i32>(U.geo.xy) - vec2<i32>(1);
  let i1 = min(i0 + vec2<i32>(1), mx);
  let a = textureLoad(tMisc, i0, 0);
  let b = textureLoad(tMisc, vec2<i32>(i1.x, i0.y), 0);
  let c = textureLoad(tMisc, vec2<i32>(i0.x, i1.y), 0);
  let d = textureLoad(tMisc, i1, 0);
  return mix(mix(a, b, bc.z), mix(c, d, bc.z), bc.w);
}

// ---- noise ------------------------------------------------------------------

fn hash12(p: vec2<f32>) -> f32 {
  return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn vnoise(p: vec2<f32>) -> f32 {
  let i = floor(p);
  var f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  let a = hash12(i);
  let b = hash12(i + vec2<f32>(1.0, 0.0));
  let c = hash12(i + vec2<f32>(0.0, 1.0));
  let d = hash12(i + vec2<f32>(1.0, 1.0));
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn fbm(p: vec2<f32>) -> f32 {
  return vnoise(p) * 0.55 + vnoise(p * 2.7) * 0.3 + vnoise(p * 6.1) * 0.15;
}

// ---- ramps (ported from palette.js) ------------------------------------------

fn seg(v: f32, a: f32, b: f32, ca: vec3<f32>, cb: vec3<f32>) -> vec3<f32> {
  return mix(ca, cb, clamp((v - a) / (b - a), 0.0, 1.0));
}

fn veg_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 0.14, vec3<f32>(191.0, 168.0, 128.0), vec3<f32>(167.0, 148.0, 103.0));
  if (v > 0.14) { c = seg(v, 0.14, 0.32, vec3<f32>(167.0, 148.0, 103.0), vec3<f32>(128.0, 128.0, 74.0)); }
  if (v > 0.32) { c = seg(v, 0.32, 0.50, vec3<f32>(128.0, 128.0, 74.0), vec3<f32>(92.0, 106.0, 58.0)); }
  if (v > 0.50) { c = seg(v, 0.50, 0.72, vec3<f32>(92.0, 106.0, 58.0), vec3<f32>(55.0, 78.0, 44.0)); }
  if (v > 0.72) { c = seg(v, 0.72, 1.00, vec3<f32>(55.0, 78.0, 44.0), vec3<f32>(30.0, 54.0, 32.0)); }
  return c / 255.0;
}

fn elev_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 0.15, vec3<f32>(77.0, 124.0, 68.0), vec3<f32>(125.0, 154.0, 82.0));
  if (v > 0.15) { c = seg(v, 0.15, 0.30, vec3<f32>(125.0, 154.0, 82.0), vec3<f32>(176.0, 168.0, 107.0)); }
  if (v > 0.30) { c = seg(v, 0.30, 0.45, vec3<f32>(176.0, 168.0, 107.0), vec3<f32>(140.0, 122.0, 91.0)); }
  if (v > 0.45) { c = seg(v, 0.45, 0.62, vec3<f32>(140.0, 122.0, 91.0), vec3<f32>(152.0, 145.0, 138.0)); }
  if (v > 0.62) { c = seg(v, 0.62, 0.80, vec3<f32>(152.0, 145.0, 138.0), vec3<f32>(201.0, 201.0, 201.0)); }
  if (v > 0.80) { c = seg(v, 0.80, 1.00, vec3<f32>(201.0, 201.0, 201.0), vec3<f32>(244.0, 244.0, 244.0)); }
  return c / 255.0;
}

fn sea_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 0.25, vec3<f32>(58.0, 119.0, 184.0), vec3<f32>(43.0, 92.0, 150.0));
  if (v > 0.25) { c = seg(v, 0.25, 0.60, vec3<f32>(43.0, 92.0, 150.0), vec3<f32>(28.0, 63.0, 110.0)); }
  if (v > 0.60) { c = seg(v, 0.60, 1.00, vec3<f32>(28.0, 63.0, 110.0), vec3<f32>(18.0, 42.0, 77.0)); }
  return c / 255.0;
}

fn temp_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, -35.0, -20.0, vec3<f32>(35.0, 48.0, 110.0), vec3<f32>(59.0, 92.0, 196.0));
  if (v > -20.0) { c = seg(v, -20.0, -8.0, vec3<f32>(59.0, 92.0, 196.0), vec3<f32>(111.0, 163.0, 224.0)); }
  if (v > -8.0) { c = seg(v, -8.0, 0.0, vec3<f32>(111.0, 163.0, 224.0), vec3<f32>(221.0, 233.0, 238.0)); }
  if (v > 0.0) { c = seg(v, 0.0, 8.0, vec3<f32>(221.0, 233.0, 238.0), vec3<f32>(242.0, 213.0, 128.0)); }
  if (v > 8.0) { c = seg(v, 8.0, 18.0, vec3<f32>(242.0, 213.0, 128.0), vec3<f32>(238.0, 154.0, 60.0)); }
  if (v > 18.0) { c = seg(v, 18.0, 28.0, vec3<f32>(238.0, 154.0, 60.0), vec3<f32>(216.0, 79.0, 42.0)); }
  if (v > 28.0) { c = seg(v, 28.0, 35.0, vec3<f32>(216.0, 79.0, 42.0), vec3<f32>(163.0, 29.0, 29.0)); }
  return c / 255.0;
}

fn precip_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 200.0, vec3<f32>(217.0, 197.0, 142.0), vec3<f32>(207.0, 208.0, 138.0));
  if (v > 200.0) { c = seg(v, 200.0, 500.0, vec3<f32>(207.0, 208.0, 138.0), vec3<f32>(158.0, 201.0, 127.0)); }
  if (v > 500.0) { c = seg(v, 500.0, 900.0, vec3<f32>(158.0, 201.0, 127.0), vec3<f32>(95.0, 174.0, 120.0)); }
  if (v > 900.0) { c = seg(v, 900.0, 1500.0, vec3<f32>(95.0, 174.0, 120.0), vec3<f32>(58.0, 143.0, 143.0)); }
  if (v > 1500.0) { c = seg(v, 1500.0, 2200.0, vec3<f32>(58.0, 143.0, 143.0), vec3<f32>(47.0, 107.0, 176.0)); }
  if (v > 2200.0) { c = seg(v, 2200.0, 3000.0, vec3<f32>(47.0, 107.0, 176.0), vec3<f32>(39.0, 77.0, 176.0)); }
  return c / 255.0;
}

fn hydro_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 0.35, vec3<f32>(22.0, 40.0, 62.0), vec3<f32>(43.0, 108.0, 176.0));
  if (v > 0.35) { c = seg(v, 0.35, 0.75, vec3<f32>(43.0, 108.0, 176.0), vec3<f32>(87.0, 168.0, 224.0)); }
  if (v > 0.75) { c = seg(v, 0.75, 1.00, vec3<f32>(87.0, 168.0, 224.0), vec3<f32>(165.0, 221.0, 255.0)); }
  return c / 255.0;
}

fn fert_ramp(v: f32) -> vec3<f32> {
  var c = seg(v, 0.0, 0.15, vec3<f32>(58.0, 54.0, 48.0), vec3<f32>(107.0, 95.0, 66.0));
  if (v > 0.15) { c = seg(v, 0.15, 0.35, vec3<f32>(107.0, 95.0, 66.0), vec3<f32>(138.0, 124.0, 62.0)); }
  if (v > 0.35) { c = seg(v, 0.35, 0.55, vec3<f32>(138.0, 124.0, 62.0), vec3<f32>(125.0, 154.0, 60.0)); }
  if (v > 0.55) { c = seg(v, 0.55, 0.75, vec3<f32>(125.0, 154.0, 60.0), vec3<f32>(77.0, 156.0, 63.0)); }
  if (v > 0.75) { c = seg(v, 0.75, 1.00, vec3<f32>(77.0, 156.0, 63.0), vec3<f32>(31.0, 143.0, 77.0)); }
  return c / 255.0;
}

// ---- satellite composite ------------------------------------------------------

fn sea_surface(p: vec2<f32>, h: f32, tmean: f32, coast: f32, t: f32, cpp: f32) -> vec3<f32> {
  let depth_raw = clamp(-h / 0.85, 0.0, 1.0);
  let warm = clamp((tmean + 2.0) / 24.0, 0.0, 1.0);
  let turq = vec3<f32>(26.0 + warm * 30.0, 102.0 + warm * 36.0, 116.0 + warm * 32.0) / 255.0;
  let navy = vec3<f32>(6.0, 16.0, 40.0) / 255.0;
  // depth carries the bathymetry; far from any coast the water floors toward
  // the abyss so border shallows never glow, but the shelf stays turquoise
  let depth_vis = pow(depth_raw, 0.48);
  let open = smoothstep(3.0, 16.0, coast) * 0.85;
  let vis = max(depth_vis, open);
  var col = mix(turq, navy, vis);

  // slow swell — two broad drifting noise fields breathe over the surface
  let n1 = vnoise(p * 0.09 + t * vec2<f32>(0.05, 0.033));
  let n2 = vnoise(p * 0.18 - t * vec2<f32>(0.041, 0.027));
  col += vec3<f32>(((n1 - 0.5) * 0.05 + (n2 - 0.5) * 0.03) * (1.0 - vis * 0.6));
  // sparse sun sparkle riding the swell, brightest over the shelf
  let sp = vnoise(p * 2.4 + vec2<f32>(t * 0.6, -t * 0.45));
  col += vec3<f32>(smoothstep(0.86, 0.99, sp * (0.55 + 0.45 * n1)) * 0.08 * (1.0 - vis * 0.5));
  // leaned in, a fine moving ripple keeps the water alive
  if (cpp < 0.12) {
    let rip = vnoise(p * 7.0 + vec2<f32>(t * 0.5, t * 0.31)) - 0.5;
    col += vec3<f32>(rip * 0.035 * (1.0 - cpp / 0.12) * (1.0 - vis * 0.5));
  }

  // breakers whiten the last cells before the strand
  let foam = (1.0 - smoothstep(0.15, 1.6, coast))
           * smoothstep(0.45, 0.85, vnoise(p * 2.6 + t * vec2<f32>(0.22, 0.16)));
  col = mix(col, vec3<f32>(0.82, 0.90, 0.95), foam * 0.30);
  return col;
}

fn land_surface(p: vec2<f32>, h: f32, clim: vec4<f32>, cpp: f32) -> vec3<f32> {
  let moist = clamp((clim.z - 130.0) / 1050.0, 0.0, 1.0);
  let warm = clamp((clim.x + 9.0) / 27.0, 0.0, 1.0);
  let veg = clamp(clamp(moist * (0.3 + 0.7 * warm), 0.0, 1.0) * 0.8 + clim.w * 0.3, 0.0, 1.0);
  var col = veg_ramp(veg);

  // cold lands grey toward tundra, then pale into the ice sheet
  let chill = clamp((4.0 - clim.x) / 16.0, 0.0, 1.0) * 0.65;
  col = mix(col, vec3<f32>(127.0, 117.0, 99.0) / 255.0, chill);
  let frozen = clamp((-9.0 - clim.x) / 9.0, 0.0, 1.0);
  col = mix(col, vec3<f32>(213.0, 218.0, 224.0) / 255.0, frozen);

  // altitude: bare rock above the treeline, firn on the peaks
  let rock = clamp((h - 0.5) / 0.32, 0.0, 1.0) * 0.85;
  col = mix(col, vec3<f32>(118.0, 108.0, 98.0) / 255.0, rock);
  let firn = clamp((h - 0.7) / 0.22, 0.0, 1.0) * clamp((8.0 - clim.x) / 18.0 + 0.2, 0.0, 1.0);
  col = mix(col, vec3<f32>(237.0, 240.0, 245.0) / 255.0, firn);

  // mottled canopy and field texture at four world scales, plus zoom-gated
  // fine octaves that fade in as pixels shrink — detail never runs out
  var m = (fbm(p * 0.15) - 0.5) * 0.5
        + (fbm(p * 0.5) - 0.5) * 0.32
        + (fbm(p * 2.3) - 0.5) * 0.22
        + (fbm(p * 9.1) - 0.5) * 0.10;
  if (cpp < 0.12) { m += (vnoise(p * 28.0) - 0.5) * 0.30 * (1.0 - cpp / 0.12); }
  if (cpp < 0.035) { m += (vnoise(p * 80.0) - 0.5) * 0.22 * (1.0 - cpp / 0.035); }
  col += m * (5.0 + veg * 13.0) / 255.0 * vec3<f32>(1.0, 1.1, 0.8) * 2.2;
  return col;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  let size = U.geo.xy;
  let p = vec2<f32>(mix(U.cam.x, U.cam.y, in.uv.x), mix(U.cam.z, U.cam.w, in.uv.y));
  // cells per screen pixel — gates the zoom-dependent detail octaves
  // (derivative taken before any return so it stays well-defined)
  let cpp = abs(dpdx(p.x));

  // the void beyond the map: deep space with a soft atmospheric halo
  if (p.x < 0.0 || p.y < 0.0 || p.x > size.x || p.y > size.y) {
    let q = clamp(p, vec2<f32>(0.0), size);
    let d = length(p - q);
    var space = vec3<f32>(5.0, 8.0, 15.0) / 255.0;
    space += vec3<f32>(0.10, 0.16, 0.26) * exp(-d / 3.5);
    if (U.opts.z > 0.5) { space = pow(space, vec3<f32>(2.2)); }
    return vec4<f32>(space, 1.0);
  }

  let layer = i32(U.geo.z);
  let month = U.geo.w;
  let t = U.anim.x;

  let h = sample_h(p);
  let clim = sample_clim(p);
  let misc = sample_misc(p);
  let tnow = clim.x + clim.y * cos(6.2831853 * month / 12.0);
  let coast = misc.w * 32.0;
  let land_m = smoothstep(-0.006, 0.006, h);
  // misc.z encodes standing water: 1 fresh lake, 0.55 salt flat, -1 wadi bed
  let lake_m = smoothstep(0.72, 0.95, misc.z);
  let salt_m = smoothstep(0.30, 0.50, misc.z) * (1.0 - smoothstep(0.62, 0.80, misc.z));
  let wadi_m = clamp(-misc.z, 0.0, 1.0);
  let water_m = max(1.0 - land_m, max(lake_m, salt_m));

  // hillshade from the smooth height field (light from the NW) — forward
  // differences reuse the centre sample, two taps instead of four
  let hx = sample_h(p + vec2<f32>(1.0, 0.0));
  let hy = sample_h(p + vec2<f32>(0.0, 1.0));
  let shade = clamp(1.0 + U.opts.y * ((h - hx) + (h - hy)) * 0.9, 0.6, 1.32);

  var col = vec3<f32>(0.0);

  if (layer == 0 || layer == 1) {
    let sea = sea_surface(p, h, clim.x, coast, t, cpp);
    let land = land_surface(p, h, clim, cpp);
    col = mix(sea, land, land_m);
    // lakes ride on top of land
    let ln = vnoise(p * 1.3) - 0.5;
    col = mix(col, vec3<f32>(25.0 + ln * 7.0, 57.0 + ln * 7.0, 69.0 + ln * 7.0) / 255.0, lake_m);
    // dead seas: blinding mineral crusts with a faint aqua bloom at the rim
    let sc = vec3<f32>(216.0 + ln * 10.0, 222.0 + ln * 8.0, 214.0 + ln * 6.0) / 255.0;
    col = mix(col, mix(sc, vec3<f32>(158.0, 199.0, 196.0) / 255.0, 0.25 + 0.3 * ln), salt_m);
    if (layer == 1) {
      // mute the imagery so informational tints read like annotation
      let lum = dot(col, vec3<f32>(0.3, 0.59, 0.11));
      col = (col * 0.52 + vec3<f32>(lum) * 0.48) * 0.84;
      if (U.opts.x > 0.5) {
        let ti = clamp(vec2<i32>(floor(p)), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
        let tint = textureLoad(tTint, ti, 0);
        col = mix(col, tint.rgb, tint.a);
      }
    }
  } else if (layer == 2) {
    if (h < 0.0) {
      col = sea_ramp(min(1.0, -h / 0.75)) * vec3<f32>(0.9, 0.95, 1.0);
    } else {
      col = mix(elev_ramp(h), vec3<f32>(74.0, 128.0, 168.0) / 255.0, lake_m);
      col = mix(col, vec3<f32>(198.0, 202.0, 196.0) / 255.0, salt_m);
    }
  } else if (layer == 3) {
    col = temp_ramp(tnow);
    if (h < 0.0) { col *= vec3<f32>(0.82, 0.85, 0.9); }
  } else if (layer == 4) {
    if (h < 0.0) { col = vec3<f32>(22.0, 39.0, 63.0) / 255.0; }
    else { col = precip_ramp(clim.z); }
  } else if (layer == 5) {
    if (h < 0.0) { col = vec3<f32>(14.0, 28.0, 48.0) / 255.0; }
    else {
      let flow = misc.x;
      if (flow > 0.42) { col = hydro_ramp((flow - 0.42) / 0.58); }
      else { col = vec3<f32>(19.0, 26.0, 36.0) / 255.0 * shade; }
      col = mix(col, vec3<f32>(46.0, 95.0, 143.0) / 255.0, lake_m);
      col = mix(col, vec3<f32>(176.0, 182.0, 178.0) / 255.0, salt_m);
    }
  } else {
    if (h < 0.0) { col = vec3<f32>(20.0, 33.0, 52.0) / 255.0; }
    else { col = mix(fert_ramp(clim.w), vec3<f32>(46.0, 95.0, 143.0) / 255.0, lake_m); }
    col = mix(col, vec3<f32>(176.0, 182.0, 178.0) / 255.0, salt_m * land_m);
  }

  // hillshade on land (soft for the analytic climate layers, none for hydro)
  if (U.anim.w > 0.5 && layer != 5) {
    var s = shade;
    if (layer >= 3) { s = 1.0 + (shade - 1.0) * 0.45; }
    col *= mix(s, 1.0, water_m);
  }

  // rivers overlay — width and weight follow Strahler order (misc.y holds
  // channel strength), and wadis breathe with the rains: brimming in the
  // wet season, a pale ribbon of cracked silt in the dry.
  if (U.anim.y > 0.5 && layer != 5) {
    let str = misc.y;
    let riv = smoothstep(0.14, 0.42, str);
    let north = step(p.y, size.y * 0.5);
    let wet = 0.5 - 0.5 * cos(6.2831853 * (month - 5.5 - 6.0 * (1.0 - north)) / 12.0);
    let presence = mix(1.0, 0.10 + 0.90 * wet, wadi_m);
    let a = min(0.85, 0.30 + misc.x * 0.40 + 0.25 * str) * riv * land_m * (1.0 - lake_m) * presence;
    col = mix(col, vec3<f32>(62.0, 124.0, 186.0) / 255.0, a);
    let dry = riv * wadi_m * (1.0 - presence) * land_m * 0.4;
    col = mix(col, vec3<f32>(203.0, 192.0, 168.0) / 255.0, dry);
  }

  // seasonal snow and sea ice, breaking up along a noisy snowline of floes
  if (U.anim.z > 0.5 && layer != 3) {
    let breakup = clamp(0.8 + 0.4 * vnoise(p * 0.33), 0.0, 1.0);
    let a_land = clamp((-1.0 - tnow) / 6.0, 0.0, 1.0) * 0.85 * breakup;
    let a_sea = clamp((-2.0 - tnow) / 8.0, 0.0, 1.0) * 0.9 * breakup;
    col = mix(col, vec3<f32>(240.0, 245.0, 250.0) / 255.0, a_land * (1.0 - water_m));
    col = mix(col, vec3<f32>(216.0, 229.0, 240.0) / 255.0, a_sea * water_m);
  }

  // atmospheric limb — a faint scatter where the world meets the void
  let e = min(p, size - p);
  let edge = min(e.x, e.y);
  col += vec3<f32>(0.03, 0.055, 0.10) * (1.0 - smoothstep(0.0, 9.0, edge));
  col = mix(col, vec3<f32>(2.0, 6.0, 13.0) / 255.0, (1.0 - smoothstep(0.0, 1.2, edge)) * 0.55);

  if (U.opts.z > 0.5) { col = pow(max(col, vec3<f32>(0.0)), vec3<f32>(2.2)); }
  return vec4<f32>(col, 1.0);
}
"#;

/// Chamfer distance (in cells) from every sea cell to the nearest land.
fn coast_distance(height: &[f32], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e9;
    let mut d = vec![0.0f32; w * h];
    for i in 0..w * h {
        d[i] = if height[i] >= 0.0 { 0.0 } else { INF };
    }
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut best = d[i];
            if x > 0 {
                best = best.min(d[i - 1] + 1.0);
            }
            if y > 0 {
                best = best.min(d[i - w] + 1.0);
                if x > 0 {
                    best = best.min(d[i - w - 1] + 1.4);
                }
                if x < w - 1 {
                    best = best.min(d[i - w + 1] + 1.4);
                }
            }
            d[i] = best;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut best = d[i];
            if x < w - 1 {
                best = best.min(d[i + 1] + 1.0);
            }
            if y < h - 1 {
                best = best.min(d[i + w] + 1.0);
                if x < w - 1 {
                    best = best.min(d[i + w + 1] + 1.4);
                }
                if x > 0 {
                    best = best.min(d[i + w - 1] + 1.4);
                }
            }
            d[i] = best;
        }
    }
    d
}

#[wasm_bindgen]
pub struct Orbital {
    /// Keeps the backend instance alive for the lifetime of the surface.
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    tint_tex: Option<wgpu::Texture>,
    world_w: u32,
    world_h: u32,
    cfg_w: u32,
    cfg_h: u32,
    srgb: f32,
    backend: &'static str,
}

/// Device descriptor with every requested limit clamped to what the adapter
/// actually offers — a GL adapter capped at 6 color attachments must not be
/// asked for the 8 in the downlevel defaults.
fn device_desc(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let al = adapter.limits();
    let mut limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(al.clone());
    limits.max_color_attachments = limits.max_color_attachments.min(al.max_color_attachments);
    limits.max_texture_dimension_1d = limits.max_texture_dimension_1d.min(al.max_texture_dimension_1d);
    limits.max_texture_dimension_2d = limits.max_texture_dimension_2d.min(al.max_texture_dimension_2d);
    wgpu::DeviceDescriptor {
        label: Some("calliope-orbital"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
    }
}

fn tex_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[wasm_bindgen]
impl Orbital {
    /// Bring the engine up on a canvas: WebGPU first, WebGL2 as the fallback.
    ///
    /// A canvas is claimed forever by its first `getContext` call — a webgpu
    /// context on a browser with no WebGPU adapter would lock webgl2 out. So
    /// WebGPU is probed adapter-first (no canvas involved), and the canvas is
    /// only handed to the backend that proved it has hardware behind it.
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<Orbital, JsValue> {
        // WebGPU is probed adapter-and-device first, with no canvas involved:
        // only a backend that fully brought a device up gets to claim the
        // canvas, so a browser whose WebGPU rejects our device request still
        // falls through cleanly to WebGL2.
        let probe = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        if let Some(adapter) = probe
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
        {
            if let Ok((device, queue)) = adapter.request_device(&device_desc(&adapter), None).await
            {
                let surface = probe
                    .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                    .map_err(|e| JsValue::from_str(&format!("webgpu surface: {e}")))?;
                return Self::finish(probe, surface, adapter, device, queue, "webgpu");
            }
        }
        Self::create_gl(canvas).await
    }

    /// Bring the engine up on WebGL2 directly, skipping the WebGPU probe.
    ///
    /// Exposed on its own so the client can retry on a fresh canvas when a
    /// browser's WebGPU hands out a device but never presents a frame — the
    /// original canvas is claimed by its webgpu context forever, so the retry
    /// must arrive with a new one.
    pub async fn create_gl(canvas: web_sys::HtmlCanvasElement) -> Result<Orbital, JsValue> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..Default::default()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&format!("webgl surface: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| JsValue::from_str("no graphics adapter"))?;
        let (device, queue) = adapter
            .request_device(&device_desc(&adapter), None)
            .await
            .map_err(|e| JsValue::from_str(&format!("device request failed: {e}")))?;
        Self::finish(instance, surface, adapter, device, queue, "webgl2")
    }

    /// Which backend the engine came up on: "webgpu" or "webgl2".
    pub fn backend(&self) -> String {
        self.backend.to_string()
    }

    fn finish(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: &'static str,
    ) -> Result<Orbital, JsValue> {
        // surface any late GPU validation error in the console instead of
        // swallowing it into a silent white canvas
        device.on_uncaptured_error(Box::new(|e| {
            web_sys::console::error_1(&format!("wgpu uncaptured: {e}").into());
        }));

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];
        let srgb = if format.is_srgb() { 1.0 } else { 0.0 };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("orbital"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("orbital-bind"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_layout_entry(1),
                tex_layout_entry(2),
                tex_layout_entry(3),
                tex_layout_entry(4),
            ],
        });

        let pipe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("orbital-pipe"),
            layout: Some(&pipe_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("orbital-uni"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Orbital {
            _instance: instance,
            device,
            queue,
            surface,
            format,
            pipeline,
            bind_layout,
            uniforms,
            bind: None,
            tint_tex: None,
            world_w: 0,
            world_h: 0,
            cfg_w: 0,
            cfg_h: 0,
            srgb,
            backend,
        })
    }

    fn make_tex(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn upload(&self, tex: &wgpu::Texture, data: &[u8], w: u32, h: u32, bpp: u32) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * bpp),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Upload the world fields as textures. Coast distance is computed here.
    #[allow(clippy::too_many_arguments)]
    pub fn set_world(
        &mut self,
        w: u32,
        h: u32,
        height: &[f32],
        tmean: &[f32],
        tamp: &[f32],
        precip: &[f32],
        fertility: &[f32],
        discharge: &[f32],
        flags: &[u8],
        strahler: &[u8],
    ) {
        let n = (w * h) as usize;
        self.world_w = w;
        self.world_h = h;

        let mut clim = vec![0.0f32; n * 4];
        for i in 0..n {
            clim[i * 4] = tmean[i];
            clim[i * 4 + 1] = tamp[i];
            clim[i * 4 + 2] = precip[i];
            clim[i * 4 + 3] = if fertility.is_empty() { 0.4 } else { fertility[i] };
        }
        let mut dmax = 0.0f32;
        for &v in discharge {
            if v > dmax {
                dmax = v;
            }
        }
        let dlog = (1.0 + dmax).ln().max(1e-6);
        let coast = coast_distance(height, w as usize, h as usize);
        let mut misc = vec![0.0f32; n * 4];
        // river strength: Strahler order sets channel weight — creeks stay
        // threads, 7th-order mainstems read as broad valley rivers
        let strength = |i: usize| -> f32 {
            if flags[i] & 1 == 0 {
                return 0.0;
            }
            let o = if i < strahler.len() { strahler[i] as f32 } else { 1.0 };
            0.35 + 0.65 * ((o - 1.0) / 6.0).clamp(0.0, 1.0)
        };
        for i in 0..n {
            misc[i * 4] = (1.0 + discharge[i]).ln() / dlog;
            misc[i * 4 + 1] = strength(i);
            // z encodes standing water: 1 fresh lake, 0.55 salt flat, -1 wadi
            misc[i * 4 + 2] = if flags[i] & 4 != 0 {
                0.55
            } else if flags[i] & 2 != 0 {
                1.0
            } else if flags[i] & 8 != 0 {
                -1.0
            } else {
                0.0
            };
            misc[i * 4 + 3] = coast[i].min(32.0) / 32.0;
        }
        // bridge diagonal river steps so the shader's bilinear mask reads as
        // a continuous channel instead of a string of beads
        let (wu, _hu) = (w as usize, h as usize);
        let riv = |i: usize, f: &[u8]| f[i] & 1 != 0;
        for y in 0..(h as usize - 1) {
            for x in 0..(wu - 1) {
                let i = y * wu + x;
                let (a, b) = (riv(i, flags), riv(i + 1, flags));
                let (c, d) = (riv(i + wu, flags), riv(i + wu + 1, flags));
                if a && d && !b && !c {
                    let v = strength(i).min(strength(i + wu + 1)) * 0.6;
                    misc[(i + 1) * 4 + 1] = misc[(i + 1) * 4 + 1].max(v);
                    misc[(i + wu) * 4 + 1] = misc[(i + wu) * 4 + 1].max(v);
                } else if b && c && !a && !d {
                    let v = strength(i + 1).min(strength(i + wu)) * 0.6;
                    misc[i * 4 + 1] = misc[i * 4 + 1].max(v);
                    misc[(i + wu + 1) * 4 + 1] = misc[(i + wu + 1) * 4 + 1].max(v);
                }
            }
        }


        let t_height = self.make_tex(w, h, wgpu::TextureFormat::R32Float);
        let t_clim = self.make_tex(w, h, wgpu::TextureFormat::Rgba32Float);
        let t_misc = self.make_tex(w, h, wgpu::TextureFormat::Rgba32Float);
        let t_tint = self.make_tex(w, h, wgpu::TextureFormat::Rgba8Unorm);
        self.upload(&t_height, bytemuck::cast_slice(height), w, h, 4);
        self.upload(&t_clim, bytemuck::cast_slice(&clim), w, h, 16);
        self.upload(&t_misc, bytemuck::cast_slice(&misc), w, h, 16);
        self.upload(&t_tint, &vec![0u8; n * 4], w, h, 4);

        let views: Vec<wgpu::TextureView> = [&t_height, &t_clim, &t_misc, &t_tint]
            .iter()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
            .collect();
        self.bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("orbital-bind"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&views[3]) },
            ],
        }));
        self.tint_tex = Some(t_tint);
    }

    /// Update the political tint texture (RGBA8, one texel per cell).
    pub fn set_tint(&mut self, rgba: &[u8]) {
        if let Some(t) = &self.tint_tex {
            self.upload(t, rgba, self.world_w, self.world_h, 4);
        }
    }

    /// Render one frame. View maps screen px to world cells: world = (px - t) / scale.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        px_w: u32,
        px_h: u32,
        css_w: f32,
        css_h: f32,
        tx: f32,
        ty: f32,
        scale: f32,
        layer: u32,
        month: f32,
        time: f32,
        rivers: u32,
        snow: u32,
        shade: u32,
        has_tint: u32,
    ) {
        if self.bind.is_none() || px_w == 0 || px_h == 0 {
            return;
        }
        if (px_w, px_h) != (self.cfg_w, self.cfg_h) {
            self.surface.configure(
                &self.device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: self.format,
                    width: px_w,
                    height: px_h,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode: wgpu::CompositeAlphaMode::Auto,
                    view_formats: vec![],
                },
            );
            self.cfg_w = px_w;
            self.cfg_h = px_h;
        }

        let shade_k = self.world_h as f32 / 16.0;
        let uni: [f32; 16] = [
            -tx / scale,
            (css_w - tx) / scale,
            -ty / scale,
            (css_h - ty) / scale,
            self.world_w as f32,
            self.world_h as f32,
            layer as f32,
            month,
            time,
            rivers as f32,
            snow as f32,
            shade as f32,
            has_tint as f32,
            shade_k,
            self.srgb,
            0.0,
        ];
        self.queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uni));

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                self.cfg_w = 0; // force a reconfigure next frame
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.03,
                            b: 0.06,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.bind.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
        frame.present();
    }
}
