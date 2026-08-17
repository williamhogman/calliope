// Renderer — the coordinator (E9.4). The work lives in three focused
// modules under render/; this class holds the shared state (world, fields,
// caches) and draws the static annotation canvas on damage frames plus the
// animated canvas on its own clock.
//
//   render/compositor.js   fields → pixels; territory & political tint
//   render/labels.js       the label engine: cached layout, draw, picking
//   render/overlays.js     routes, markers, scale bar; caravans & winds

import { hexRgb, settlementColor } from "./palette.js";
import {
  buildShade, buildSatellite, monthTemp, composite,
  decodeTerritory, territoryGrid, cultureOf, tintRgba,
} from "./render/compositor.js";
import { drawLabels, labelBoxesAt } from "./render/labels.js";
import {
  buildDrawPaths, routePoint,
  drawRouteNetwork, drawDeposits, drawSettlements,
  drawHover, drawSelectedCell, drawScaleBar,
  drawWinds, drawCaravans,
} from "./render/overlays.js";

export class Renderer {
  constructor(canvas, animCanvas = null) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.anim = animCanvas;
    this.actx = animCanvas ? animCanvas.getContext("2d") : null;
    this.off = document.createElement("canvas");
    this.octx = this.off.getContext("2d");
    this.cacheKeyBase = null;
    this.cacheVersion = null;
    this.tmonthCache = new Map();
    this.territoryCache = { version: -1, owner: null };
    this.tintEpoch = 0;      // advances only when tint content changes (E9.2)
    this.lastTintRows = null;
  }

  setWorld(world) {
    this.world = world;
    const w = world.header.width || world.header.size;
    const hh = world.header.size;
    this.w = w;
    this.h = hh;
    this.size = hh; // rows — used by scale-relative heuristics
    this.off.width = w;
    this.off.height = hh;
    this.cacheKeyBase = null;
    this.cacheVersion = null;
    this._img = null;
    this.tmonthCache.clear();
    this.territoryCache = { version: -1, owner: null };
    this._tintCache = null;
    this._tintRows = undefined;
    this._compRows = undefined;
    this._polDirty = true;
    this.tintEpoch++;
    this.lastTintRows = null;
    this._labelLayout = null;
    this._riverPaths = new Map(); // curved-label courses, rebuilt per world

    // Engine-authoritative political map (M4.1): owner culture per cell,
    // −1 wilderness. Ships in the pack, updates arrive as RLE patches.
    this.territoryOwner = world.arrays.territory || null;
    this.ownerIsCulture = !!this.territoryOwner;

    // biome palette by id
    this.biomePal = [];
    for (const b of world.header.biomes) this.biomePal[b.id] = b.color;

    // culture colors
    this.cultureRgb = (world.header.cultures || []).map((c) => hexRgb(c.color));

    buildShade(this);

    // discharge normaliser
    const d = world.arrays.discharge;
    let max = 0;
    for (let i = 0; i < d.length; i++) if (d[i] > max) max = d[i];
    this.dischargeLogMax = Math.log1p(max);

    buildSatellite(this);
    this.setRoutes(world.header.routes || []);
  }

  setRoutes(routes) {
    if (this.world) this.world.header.routes = routes;
    // trade route geometry (cumulative lengths for caravan animation)
    this.routes = (routes || []).map((r, idx) => {
      const pts = r.path;
      const cum = [0];
      for (let i = 1; i < pts.length; i++) {
        const dx = pts[i][0] - pts[i - 1][0];
        const dy = pts[i][1] - pts[i - 1][1];
        cum.push(cum[i - 1] + Math.hypot(dx, dy));
      }
      return { ...r, pts, cum, total: cum[cum.length - 1] || 1, phase: (idx * 0.37) % 1 };
    });
    buildDrawPaths(this);
  }

  setTerritory(rle) { decodeTerritory(this, rle); }
  territory(version) { return territoryGrid(this, version); }
  tintRgba(version) { return tintRgba(this, version); }
  monthTemp(month) { return monthTemp(this, month); }
  routePoint(route, t) { return routePoint(route, t); }
  composite(state) { composite(this, state); }

  /// Culture that holds cell i, or −1 for wilderness — semantics-safe
  /// across both the engine grid (culture ids) and the fallback (settlement ids).
  ownerCultureAt(i) {
    if (this.territoryOwner) return this.territoryOwner[i];
    const owner = this.territoryCache.owner;
    if (!owner || owner[i] < 0) return -1;
    return cultureOf(this)[owner[i]] ?? -1;
  }

  cultureColor(s) {
    return this.cultureRgb[s.culture ?? 0] || settlementColor(s.id);
  }

  // ---------- static annotation (damage frames only) ----------

  draw(state, view, hover) {
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth;
    const hgt = this.canvas.clientHeight;
    if (this.canvas.width !== (w * dpr) | 0 || this.canvas.height !== (hgt * dpr) | 0) {
      this.canvas.width = (w * dpr) | 0;
      this.canvas.height = (hgt * dpr) | 0;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const gpuOn = !!(this.gpu && this.gpu.hasWorld);
    if (gpuOn) {
      // the wgpu canvas beneath carries the imagery — keep this layer clear
      ctx.clearRect(0, 0, w, hgt);
    } else {
      ctx.fillStyle = "#05080f";
      ctx.fillRect(0, 0, w, hgt);
    }
    if (!this.world) return;

    if (!gpuOn) {
      this.composite(state);
      ctx.save();
      ctx.imageSmoothingEnabled = false;
      ctx.translate(view.tx, view.ty);
      ctx.scale(view.scale, view.scale);
      ctx.drawImage(this.off, 0, 0);
      ctx.restore();
    }

    // a breath of atmosphere: a faint rim on the world, vignette in the void
    ctx.save();
    ctx.shadowColor = "rgba(96, 150, 212, 0.22)";
    ctx.shadowBlur = 12;
    ctx.strokeStyle = "rgba(128, 172, 222, 0.22)";
    ctx.lineWidth = 1;
    ctx.strokeRect(view.tx - 0.5, view.ty - 0.5, this.w * view.scale + 1, this.h * view.scale + 1);
    ctx.restore();
    const vg = ctx.createRadialGradient(
      w / 2, hgt / 2, Math.min(w, hgt) * 0.42,
      w / 2, hgt / 2, Math.hypot(w, hgt) * 0.62);
    vg.addColorStop(0, "rgba(0,0,0,0)");
    vg.addColorStop(1, "rgba(2, 6, 13, 0.5)");
    ctx.fillStyle = vg;
    ctx.fillRect(0, 0, w, hgt);

    // vector overlays in screen space — marks first, then the label pass
    // (cached layout, M7.2/E9.5) so every name stays collision-free
    if (state.overlays.routes) drawRouteNetwork(this, ctx, view);
    if (state.overlays.resources) drawDeposits(this, ctx, view);
    if (state.overlays.settlements) drawSettlements(this, ctx, view, state);
    drawLabels(this, ctx, view, state);
    drawScaleBar(this, ctx, view);
    if (state.selectedCell) drawSelectedCell(ctx, view, state.selectedCell);
    if (hover && view.scale > 4) drawHover(ctx, view, hover);
  }

  // ---------- animated layer (its own canvas and clock — E9.3) ----------

  drawAnim(state, view) {
    const c = this.anim;
    if (!c) return;
    const ctx = this.actx;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth, hgt = c.clientHeight;
    if (c.width !== (w * dpr) | 0 || c.height !== (hgt * dpr) | 0) {
      c.width = (w * dpr) | 0;
      c.height = (hgt * dpr) | 0;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, hgt);
    if (!this.world) return;
    if (state.overlays.winds) drawWinds(this, ctx, view, state.playing);
    if (state.overlays.routes && state.playing) drawCaravans(this, ctx, view);
  }

  // ---------- picking & gate evidence ----------

  labelBoxesAt(view) { return labelBoxesAt(this, view); }

  labelStats() {
    return this.labelStatsData || null;
  }
}
