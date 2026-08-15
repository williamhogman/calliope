// Renderer: composites the world into an offscreen canvas, draws to screen.

import {
  TEMP_GRAD, PRECIP_GRAD, ELEV_LAND_GRAD, SEA_GRAD, HYDRO_GRAD,
  hash2, hexRgb, settlementColor,
} from "./palette.js";

const TIER_RADIUS = { Camp: 3, Village: 4.5, Town: 6, City: 8 };

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.off = document.createElement("canvas");
    this.octx = this.off.getContext("2d");
    this.cacheKey = null;
    this.tmonthCache = new Map();
    this.territoryCache = { version: -1, owner: null };
  }

  setWorld(world) {
    this.world = world;
    const size = world.header.size;
    this.size = size;
    this.off.width = size;
    this.off.height = size;
    this.cacheKey = null;
    this.tmonthCache.clear();
    this.territoryCache = { version: -1, owner: null };

    // biome palette by id
    this.biomePal = [];
    for (const b of world.header.biomes) this.biomePal[b.id] = b.color;

    // hillshade (light from NW)
    const h = world.arrays.height;
    const sh = new Float32Array(size * size);
    const k = size / 16;
    for (let y = 0; y < size; y++) {
      const y0 = Math.max(0, y - 1) * size;
      const y1 = Math.min(size - 1, y + 1) * size;
      const yr = y * size;
      for (let x = 0; x < size; x++) {
        const x0 = Math.max(0, x - 1);
        const x1 = Math.min(size - 1, x + 1);
        const dzdx = (h[yr + x1] - h[yr + x0]) * 0.5;
        const dzdy = (h[y1 + x] - h[y0 + x]) * 0.5;
        let s = 1 + k * (-dzdx - dzdy) * 0.9;
        sh[yr + x] = s < 0.6 ? 0.6 : s > 1.32 ? 1.32 : s;
      }
    }
    this.shade = sh;

    // discharge normaliser
    const d = world.arrays.discharge;
    let max = 0;
    for (let i = 0; i < d.length; i++) if (d[i] > max) max = d[i];
    this.dischargeLogMax = Math.log1p(max);

    // trade route geometry (cumulative lengths for caravan animation)
    this.routes = (world.header.routes || []).map((r, idx) => {
      const pts = r.path;
      const cum = [0];
      for (let i = 1; i < pts.length; i++) {
        const dx = pts[i][0] - pts[i - 1][0];
        const dy = pts[i][1] - pts[i - 1][1];
        cum.push(cum[i - 1] + Math.hypot(dx, dy));
      }
      return { ...r, pts, cum, total: cum[cum.length - 1] || 1, phase: (idx * 0.37) % 1 };
    });
  }

  routePoint(route, t) {
    const target = t * route.total;
    const { pts, cum } = route;
    let lo = 0, hi = cum.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (cum[mid] < target) lo = mid + 1; else hi = mid;
    }
    const i = Math.max(1, lo);
    const seg = cum[i] - cum[i - 1] || 1;
    const f = (target - cum[i - 1]) / seg;
    return [
      pts[i - 1][0] + (pts[i][0] - pts[i - 1][0]) * f,
      pts[i - 1][1] + (pts[i][1] - pts[i - 1][1]) * f,
    ];
  }

  monthTemp(month) {
    const m = ((month % 12) + 12) % 12;
    if (this.tmonthCache.has(m)) return this.tmonthCache.get(m);
    const { tmean, tamp } = this.world.arrays;
    const c = Math.cos((2 * Math.PI * m) / 12);
    const out = new Float32Array(tmean.length);
    for (let i = 0; i < tmean.length; i++) out[i] = tmean[i] + tamp[i] * c;
    if (this.tmonthCache.size > 12) this.tmonthCache.clear();
    this.tmonthCache.set(m, out);
    return out;
  }

  territory(version) {
    if (this.territoryCache.version === version && this.territoryCache.owner) {
      return this.territoryCache.owner;
    }
    const size = this.size;
    const { biomes } = this.world.arrays;
    const owner = new Int16Array(size * size).fill(-1);
    const dist = new Float32Array(size * size).fill(Infinity);
    for (const s of this.world.header.settlements) {
      const r = (2 + 2.4 * Math.log10(Math.max(s.pop, 10))) * (size / 512) * 2.2;
      const r2 = r * r;
      const x0 = Math.max(0, Math.floor(s.x - r));
      const x1 = Math.min(size - 1, Math.ceil(s.x + r));
      const y0 = Math.max(0, Math.floor(s.y - r));
      const y1 = Math.min(size - 1, Math.ceil(s.y + r));
      for (let y = y0; y <= y1; y++) {
        for (let x = x0; x <= x1; x++) {
          const i = y * size + x;
          if (biomes[i] === 0) continue;
          const d2 = (x - s.x) ** 2 + (y - s.y) ** 2;
          if (d2 <= r2 && d2 < dist[i]) {
            dist[i] = d2;
            owner[i] = s.id;
          }
        }
      }
    }
    this.territoryCache = { version, owner };
    return owner;
  }

  composite(state) {
    const { layer, overlays, month, version } = state;
    const monthDependent = layer === "temperature" || overlays.snow;
    const key = [
      layer, overlays.rivers, overlays.snow, overlays.hillshade,
      monthDependent ? ((month % 12) + 12) % 12 : "-",
      layer === "political" ? version : "-",
    ].join("|");
    if (key === this.cacheKey) return;
    this.cacheKey = key;

    const size = this.size;
    const { height, tmean, precip, discharge, biomes, flags } = this.world.arrays;
    const img = this.octx.createImageData(size, size);
    const px = img.data;
    const shade = this.shade;
    const useShade = overlays.hillshade;
    const tnow = monthDependent ? this.monthTemp(month) : null;
    const owner = layer === "political" ? this.territory(version) : null;
    const dLogMax = this.dischargeLogMax || 1;

    const sColors = {};
    if (owner) {
      for (const s of this.world.header.settlements) sColors[s.id] = settlementColor(s.id);
    }

    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const i = y * size + x;
        const o = i * 4;
        const h = height[i];
        const sea = h < 0;
        const lake = (flags[i] & 2) !== 0;
        const isWater = sea || lake;
        let r, g, b;

        if (layer === "biomes" || layer === "political") {
          if (sea) {
            const t = Math.min(1, -h / 0.75);
            [r, g, b] = SEA_GRAD(t);
          } else if (lake) {
            r = 64; g = 118; b = 158;
          } else {
            const c = this.biomePal[biomes[i]] || [255, 0, 255];
            const dth = (hash2(x, y) - 0.5) * 9;
            r = c[0] + dth; g = c[1] + dth; b = c[2] + dth;
          }
          if (layer === "political") {
            // desaturate base
            const lum = 0.3 * r + 0.59 * g + 0.11 * b;
            r = r * 0.35 + lum * 0.65;
            g = g * 0.35 + lum * 0.65;
            b = b * 0.35 + lum * 0.65;
            const ow = owner[i];
            if (ow >= 0) {
              const c = sColors[ow];
              // border detection (4-neighbourhood)
              const left = x > 0 ? owner[i - 1] : ow;
              const up = y > 0 ? owner[i - size] : ow;
              const right = x < size - 1 ? owner[i + 1] : ow;
              const down = y < size - 1 ? owner[i + size] : ow;
              const isBorder = left !== ow || up !== ow || right !== ow || down !== ow;
              const a = isBorder ? 0.85 : 0.42;
              r = r * (1 - a) + c[0] * a;
              g = g * (1 - a) + c[1] * a;
              b = b * (1 - a) + c[2] * a;
            }
          }
        } else if (layer === "elevation") {
          if (sea) {
            const t = Math.min(1, -h / 0.75);
            const c = SEA_GRAD(t);
            r = c[0] * 0.9; g = c[1] * 0.95; b = c[2];
          } else if (lake) {
            r = 74; g = 128; b = 168;
          } else {
            [r, g, b] = ELEV_LAND_GRAD(h);
          }
        } else if (layer === "temperature") {
          [r, g, b] = TEMP_GRAD(tnow[i]);
          if (sea) { r *= 0.82; g *= 0.85; b *= 0.9; }
        } else if (layer === "precip") {
          if (sea) { r = 22; g = 39; b = 63; }
          else [r, g, b] = PRECIP_GRAD(precip[i]);
        } else { // hydro
          if (sea) { r = 14; g = 28; b = 48; }
          else if (lake) { r = 46; g = 95; b = 143; }
          else {
            const t = Math.log1p(discharge[i]) / dLogMax;
            if (t > 0.42) [r, g, b] = HYDRO_GRAD((t - 0.42) / 0.58);
            else {
              const s = this.shade[i];
              r = 19 * s; g = 26 * s; b = 36 * s;
            }
          }
        }

        // hillshade on land
        if (useShade && !isWater && layer !== "hydro") {
          const s = layer === "temperature" || layer === "precip"
            ? 1 + (shade[i] - 1) * 0.45
            : shade[i];
          r *= s; g *= s; b *= s;
        }

        // rivers overlay
        if (overlays.rivers && (flags[i] & 1) && layer !== "hydro") {
          const t = Math.log1p(discharge[i]) / dLogMax;
          const a = Math.min(0.85, 0.45 + t * 0.55);
          r = r * (1 - a) + 92 * a;
          g = g * (1 - a) + 158 * a;
          b = b * (1 - a) + 216 * a;
        }

        // snow & sea ice overlay
        if (overlays.snow && layer !== "temperature") {
          const t = (tnow || this.monthTemp(month))[i];
          if (!isWater && t < -1) {
            const a = Math.min(1, (-1 - t) / 6) * 0.85;
            r = r * (1 - a) + 240 * a;
            g = g * (1 - a) + 245 * a;
            b = b * (1 - a) + 250 * a;
          } else if (isWater && t < -2) {
            const a = Math.min(1, (-2 - t) / 8) * 0.9;
            r = r * (1 - a) + 216 * a;
            g = g * (1 - a) + 229 * a;
            b = b * (1 - a) + 240 * a;
          }
        }

        px[o] = r; px[o + 1] = g; px[o + 2] = b; px[o + 3] = 255;
      }
    }
    this.octx.putImageData(img, 0, 0);
  }

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
    ctx.fillStyle = "#0a0f18";
    ctx.fillRect(0, 0, w, hgt);
    if (!this.world) return;

    this.composite(state);

    ctx.save();
    ctx.imageSmoothingEnabled = false;
    ctx.translate(view.tx, view.ty);
    ctx.scale(view.scale, view.scale);
    ctx.drawImage(this.off, 0, 0);
    ctx.restore();

    // vector overlays in screen space
    if (state.overlays.routes) this._drawRoutes(ctx, view, state);
    if (state.overlays.resources) this._drawDeposits(ctx, view);
    if (state.overlays.settlements) this._drawSettlements(ctx, view, state);
    if (hover && view.scale > 4) this._drawHover(ctx, view, hover);
  }

  _drawRoutes(ctx, view, state) {
    const s = view.scale;
    ctx.save();
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.setLineDash([6, 5]);
    ctx.strokeStyle = "rgba(224, 196, 140, 0.55)";
    ctx.lineWidth = Math.min(2.2, Math.max(1.1, s * 0.16));
    for (const r of this.routes) {
      ctx.beginPath();
      for (let i = 0; i < r.pts.length; i++) {
        const px = view.tx + (r.pts[i][0] + 0.5) * s;
        const py = view.ty + (r.pts[i][1] + 0.5) * s;
        if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);
    // caravans while time flows
    if (state.playing) {
      const now = performance.now() / 1000;
      for (const r of this.routes) {
        const t = ((now / 14 + r.phase) % 1);
        const tt = t < 0.5 ? t * 2 : (1 - t) * 2; // there and back again
        const [wx, wy] = this.routePoint(r, tt);
        const px = view.tx + (wx + 0.5) * s;
        const py = view.ty + (wy + 0.5) * s;
        ctx.beginPath();
        ctx.arc(px, py, 2.6, 0, Math.PI * 2);
        ctx.fillStyle = "#f2d9a0";
        ctx.fill();
        ctx.lineWidth = 1;
        ctx.strokeStyle = "rgba(0,0,0,0.65)";
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  _drawDeposits(ctx, view) {
    const meta = this.world.header.resources;
    const s = view.scale;
    const rad = Math.max(2.2, Math.min(6, s * 0.45));
    for (const d of this.world.header.deposits) {
      const sx = view.tx + (d.x + 0.5) * s;
      const sy = view.ty + (d.y + 0.5) * s;
      if (sx < -10 || sy < -10 || sx > this.canvas.clientWidth + 10 || sy > this.canvas.clientHeight + 10) continue;
      ctx.beginPath();
      ctx.moveTo(sx, sy - rad);
      ctx.lineTo(sx + rad, sy);
      ctx.lineTo(sx, sy + rad);
      ctx.lineTo(sx - rad, sy);
      ctx.closePath();
      ctx.fillStyle = meta[d.r]?.color || "#ccc";
      ctx.globalAlpha = 0.95;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.lineWidth = 1;
      ctx.strokeStyle = "rgba(0,0,0,0.55)";
      ctx.stroke();
    }
  }

  _drawSettlements(ctx, view, state) {
    const s = view.scale;
    const showAllLabels = s > 1.6;
    ctx.textAlign = "center";
    for (const st of this.world.header.settlements) {
      const sx = view.tx + (st.x + 0.5) * s;
      const sy = view.ty + (st.y + 0.5) * s;
      if (sx < -60 || sy < -30 || sx > this.canvas.clientWidth + 60 || sy > this.canvas.clientHeight + 30) continue;
      const r = TIER_RADIUS[st.tier] || 3;
      const [cr, cg, cb] = settlementColor(st.id);
      ctx.beginPath();
      ctx.arc(sx, sy, r, 0, Math.PI * 2);
      ctx.fillStyle = "#f4ecd7";
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = `rgb(${cr | 0},${cg | 0},${cb | 0})`;
      ctx.stroke();
      ctx.lineWidth = 0.75;
      ctx.strokeStyle = "rgba(0,0,0,0.6)";
      ctx.stroke();

      const isBig = st.tier === "Town" || st.tier === "City";
      if (showAllLabels || isBig) {
        ctx.font = `600 ${isBig ? 12 : 11}px Inter, sans-serif`;
        ctx.lineWidth = 3;
        ctx.strokeStyle = "rgba(8,12,20,0.85)";
        ctx.lineJoin = "round";
        ctx.strokeText(st.name, sx, sy - r - 5);
        ctx.fillStyle = "#f0ead8";
        ctx.fillText(st.name, sx, sy - r - 5);
        if (s > 3) {
          const pop = st.pop.toLocaleString("en-US");
          ctx.font = "500 9.5px Inter, sans-serif";
          ctx.strokeText(pop, sx, sy + r + 11);
          ctx.fillStyle = "#b9c0cf";
          ctx.fillText(pop, sx, sy + r + 11);
        }
      }
    }
  }

  _drawHover(ctx, view, hover) {
    const s = view.scale;
    ctx.strokeStyle = "rgba(255,255,255,0.7)";
    ctx.lineWidth = 1.2;
    ctx.strokeRect(view.tx + hover.x * s, view.ty + hover.y * s, s, s);
  }
}
