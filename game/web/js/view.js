// Pan/zoom controller (CSS-pixel space). One pointer pans, two pinch-zoom,
// wheel zooms around the cursor.

export class View {
  constructor(canvas, onChange) {
    this.canvas = canvas;
    this.onChange = onChange;
    this.scale = 1;
    this.tx = 0;
    this.ty = 0;
    this.minScale = 0.5;
    this.maxScale = 48;
    this.worldSize = 512;
    this._bind();
  }

  fit(worldW, worldH = worldW) {
    this.worldSize = Math.max(worldW, worldH);
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    const pad = Math.min(40, w * 0.04);
    const contain = Math.min((w - pad * 2) / worldW, (h - pad * 2) / worldH);
    // Contain on screens that roughly match the world's shape; when it would
    // leave the map a thin band (portrait phones), start at cover instead —
    // the map owns the screen, and pinching out still reaches the whole world.
    const fill = Math.min((worldW * contain) / w, (worldH * contain) / h);
    this.scale = fill < 0.6 ? Math.max(w / worldW, h / worldH) : contain;
    this.minScale = contain * 0.5;
    this.tx = (w - worldW * this.scale) / 2;
    this.ty = (h - worldH * this.scale) / 2;
    this.onChange?.();
  }

  screenToWorld(px, py) {
    return [(px - this.tx) / this.scale, (py - this.ty) / this.scale];
  }

  centerOn(wx, wy, scale) {
    if (scale) this.scale = Math.max(this.minScale, Math.min(this.maxScale, scale));
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    this.tx = w / 2 - wx * this.scale;
    this.ty = h / 2 - wy * this.scale;
    this.onChange?.();
  }

  // Smooth camera flight to a world point; any user input cancels it.
  flyTo(wx, wy, scale, ms = 550) {
    cancelAnimationFrame(this._flight);
    const s1 = Math.max(this.minScale, Math.min(this.maxScale, scale || this.scale));
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    const from = { tx: this.tx, ty: this.ty, s: this.scale };
    const to = { tx: w / 2 - wx * s1, ty: h / 2 - wy * s1, s: s1 };
    const t0 = performance.now();
    const ease = (t) => 1 - Math.pow(1 - t, 3);
    const tick = (now) => {
      const t = Math.min(1, (now - t0) / ms);
      const k = ease(t);
      // zoom interpolates in log space so the flight feels even
      this.scale = from.s * Math.pow(to.s / from.s, k);
      const ks = (this.scale - from.s) / (to.s - from.s || 1e-9);
      const kk = to.s === from.s ? k : Math.max(0, Math.min(1, ks));
      this.tx = from.tx + (to.tx - from.tx) * kk;
      this.ty = from.ty + (to.ty - from.ty) * kk;
      this.onChange?.();
      if (t < 1) this._flight = requestAnimationFrame(tick);
    };
    this._flight = requestAnimationFrame(tick);
  }

  cancelFlight() { cancelAnimationFrame(this._flight); }

  _zoomAt(px, py, factor) {
    const ns = Math.max(this.minScale, Math.min(this.maxScale, this.scale * factor));
    if (ns === this.scale) return;
    const [wx, wy] = this.screenToWorld(px, py);
    this.scale = ns;
    this.tx = px - wx * ns;
    this.ty = py - wy * ns;
    this.onChange?.();
  }

  _bind() {
    const c = this.canvas;
    const pts = new Map(); // pointerId -> {x, y}
    let lastX = 0, lastY = 0; // single-pointer pan anchor
    let pinch = null; // {mx, my, dist} of the previous two-finger frame

    const midOf = () => {
      const [a, b] = [...pts.values()];
      return {
        mx: (a.x + b.x) / 2,
        my: (a.y + b.y) / 2,
        dist: Math.hypot(a.x - b.x, a.y - b.y),
      };
    };

    c.addEventListener("pointerdown", (e) => {
      pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pts.size === 1) {
        lastX = e.clientX;
        lastY = e.clientY;
        c.classList.add("dragging");
      } else if (pts.size === 2) {
        pinch = midOf();
      }
      try { c.setPointerCapture(e.pointerId); } catch { /* synthetic pointer */ }
    });

    c.addEventListener("pointermove", (e) => {
      if (!pts.has(e.pointerId)) return;
      pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pts.size >= 2 && pinch) {
        const m = midOf();
        if (pinch.dist > 0 && m.dist > 0) this._zoomAt(pinch.mx, pinch.my, m.dist / pinch.dist);
        this.tx += m.mx - pinch.mx;
        this.ty += m.my - pinch.my;
        pinch = m;
        this.onChange?.();
      } else if (pts.size === 1) {
        this.tx += e.clientX - lastX;
        this.ty += e.clientY - lastY;
        lastX = e.clientX;
        lastY = e.clientY;
        this.onChange?.();
      }
    });

    const up = (e) => {
      pts.delete(e.pointerId);
      if (pts.size === 1) {
        // one finger lifted mid-pinch: hand off to a smooth single-finger pan
        const p = [...pts.values()][0];
        lastX = p.x;
        lastY = p.y;
        pinch = null;
      } else if (pts.size === 0) {
        pinch = null;
        c.classList.remove("dragging");
      }
    };
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);

    c.addEventListener("wheel", (e) => {
      e.preventDefault();
      const factor = Math.exp(-e.deltaY * 0.0016);
      this._zoomAt(e.clientX, e.clientY, factor);
    }, { passive: false });

    c.addEventListener("dblclick", (e) => {
      this._zoomAt(e.clientX, e.clientY, 1.8);
    });
  }
}
