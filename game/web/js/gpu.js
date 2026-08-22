// GPU imagery engine — a thin bridge to the Rust wgpu renderer ("Orbital").
//
// The whole raster stack (terrain "satellite" composite, political tint, and
// every analytic layer) lives in one WGSL fragment shader inside the WASM
// crate. It runs on WebGPU where the browser has it and falls back to WebGL2
// everywhere else. World fields upload once as float textures; the shader
// resamples them bilinearly so relief, coasts and climate stay smooth at any
// zoom; water is animated; seasons glide.
//
// If no adapter exists at all the caller keeps the CPU compositor.

import { loadEngine } from "./wasm-load.js";
// Engine vocabulary generated from the Rust tables (E2.4) — layer ids as
// the shader branches on them, and the field registry in upload order.
import { LAYER_ID, FIELDS } from "./gen/constants.js";

const EMPTY = {
  float32: new Float32Array(0),
  uint8: new Uint8Array(0),
  int16: new Int16Array(0),
};

export async function createGpu(canvas, { forceGl = false } = {}) {
  const { Orbital } = await loadEngine();
  const orbital = forceGl ? await Orbital.create_gl(canvas) : await Orbital.create(canvas);
  // M67 — record the compute lane's verdict: the CPU twin is the law on
  // every browser path (the shipped wasm carries no device executor; the
  // kernel is proven natively on lavapipe each suite run — ADR-0027). The
  // verdict lands in compute_status() for the HUD and the browser probe.
  // Guarded so a stale wasm build (predating the lane) still boots;
  // Promise.resolve absorbs both the sync form and older async builds.
  if (typeof orbital.compute_bringup === "function") {
    Promise.resolve(orbital.compute_bringup()).then(
      (s) => console.log("[calliope] compute lane: " + s),
      (e) => console.warn("[calliope] compute bring-up failed:", e),
    );
  }
  return new GpuEngine(canvas, orbital);
}

// Some browsers hand out a WebGPU device that never presents a frame (broken
// drivers, software rasterisers). The canvas is claimed by its webgpu context
// forever, so recovery means a fresh canvas brought up straight on WebGL2.
export async function recreateGpuOnGl(oldCanvas) {
  const fresh = oldCanvas.cloneNode(false);
  oldCanvas.replaceWith(fresh);
  const gpu = await createGpu(fresh, { forceGl: true });
  return { gpu, canvas: fresh };
}

class GpuEngine {
  constructor(canvas, orbital) {
    this.canvas = canvas;
    this.orbital = orbital;
    this.hasWorld = false;
    this.hasTint = false;
    this.tintVersion = -1;
    this._month = 0;
    this._t0 = performance.now();
    this._lastT = this._t0;
  }

  backend() {
    return this.orbital.backend();
  }

  // M67 — the compute lane's verdict string, for the HUD and the probe.
  computeStatus() {
    return typeof this.orbital.compute_status === "function"
      ? this.orbital.compute_status()
      : "not probed (stale engine build)";
  }

  setWorld(world) {
    const W = world.header.width || world.header.size;
    const H = world.header.size;
    // E2.2 — the upload argument list derives from the generated field
    // registry, so it cannot drift from Orbital's set_world signature.
    const args = FIELDS.filter((f) => f.gpu).map(
      (f) => world.arrays[f.name] ?? EMPTY[f.dtype],
    );
    this.orbital.set_world(W, H, ...args);
    this.hasWorld = true;
    this.hasTint = false;
    this.tintVersion = -1;
    this._tw = W;
    this._month = world.header.month || 0;
  }

  // Political tint upload. When only a row band changed (E9.2) and the
  // engine supports it, upload just those texture rows; a fresh engine or
  // a fresh world always takes the full texture first.
  setTint(rgba, version, rows = null) {
    if (version === this.tintVersion) return;
    this.tintVersion = version;
    if (rows && this.hasTint && typeof this.orbital.set_tint_rows === "function") {
      this.orbital.set_tint_rows(
        rows.y0,
        rgba.subarray(rows.y0 * this._tw * 4, rows.y1 * this._tw * 4),
      );
    } else {
      this.orbital.set_tint(rgba);
    }
    this.hasTint = true;
  }

  render(state, view, cssW, cssH) {
    if (!this.hasWorld || cssW <= 0 || cssH <= 0) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const pw = Math.max(1, Math.round(cssW * dpr));
    const ph = Math.max(1, Math.round(cssH * dpr));
    if (this.canvas.width !== pw || this.canvas.height !== ph) {
      this.canvas.width = pw;
      this.canvas.height = ph;
    }

    const now = performance.now();
    const dt = Math.min((now - this._lastT) / 1000, 0.1);
    this._lastT = now;
    // seasons glide instead of stepping (snap on big jumps, e.g. regeneration)
    const diff = state.month - this._month;
    this._month += Math.abs(diff) > 24 ? diff : diff * Math.min(1, dt * 3.2);

    // M10.6 — the culture lens rides the political shader path: same muted
    // satellite + tint texture, but the texture holds the people-axis tint.
    const tinted = state.layer === "political" || state.layer === "culture";
    this.orbital.render(
      pw, ph, cssW, cssH,
      view.tx, view.ty, view.scale,
      LAYER_ID[state.layer === "culture" ? "political" : state.layer] ?? 0,
      this._month,
      (now - this._t0) / 1000,
      state.overlays.rivers ? 1 : 0,
      state.overlays.snow ? 1 : 0,
      state.overlays.hillshade ? 1 : 0,
      this.hasTint && tinted ? 1 : 0,
    );
  }
}
