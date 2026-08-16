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

import init, { Orbital } from "./wasm/calliope.js";

const LAYER_ID = {
  biomes: 0, political: 1, elevation: 2, temperature: 3,
  precip: 4, hydro: 5, fertility: 6,
};

export async function createGpu(canvas) {
  await init();
  const orbital = await Orbital.create(canvas);
  return new GpuEngine(canvas, orbital);
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

  setWorld(world) {
    const W = world.header.width || world.header.size;
    const H = world.header.size;
    const { height, tmean, tamp, precip, fertility, discharge, flags } = world.arrays;
    this.orbital.set_world(
      W, H, height, tmean, tamp, precip,
      fertility || new Float32Array(0), discharge, flags,
    );
    this.hasWorld = true;
    this.hasTint = false;
    this.tintVersion = -1;
    this._month = world.header.month || 0;
  }

  setTint(rgba, version) {
    if (version === this.tintVersion) return;
    this.tintVersion = version;
    this.orbital.set_tint(rgba);
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

    this.orbital.render(
      pw, ph, cssW, cssH,
      view.tx, view.ty, view.scale,
      LAYER_ID[state.layer] ?? 0,
      this._month,
      (now - this._t0) / 1000,
      state.overlays.rivers ? 1 : 0,
      state.overlays.snow ? 1 : 0,
      state.overlays.hillshade ? 1 : 0,
      this.hasTint && state.layer === "political" ? 1 : 0,
    );
  }
}
