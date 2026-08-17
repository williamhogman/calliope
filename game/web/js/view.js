// Pan/zoom controller (CSS-pixel space). One pointer pans, two pinch-zoom,
// wheel zooms around the cursor. Any user input cancels a camera flight
// immediately (E9.7) — the map always answers the hand on it.

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
    this.flying = false; // true while a flyTo animation is in charge
    this._bind();
  }

  fit(worldW, worldH = worldW) {
    this.cancelFlight();
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
    this.cancelFlight();
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
    this.flying = true;
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
      else this.flying = false;
    };
    this._flight = requestAnimationFrame(tick);
  }

  cancelFlight() {
    cancelAnimationFrame(this._flight);
    this.flying = false;
  }

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
    // Two-slot pointer tracking (E9.8): the pan/pinch math only ever cares
    // about the first two pointers, so they live in plain fields — no Map
    // spread, no per-move allocation.
    let id1 = -1, x1 = 0, y1 = 0;
    let id2 = -1, x2 = 0, y2 = 0;
    let lastX = 0, lastY = 0;      // single-pointer pan anchor
    let pmx = 0, pmy = 0, pdist = 0; // previous two-finger frame
    let pinching = false;

    c.addEventListener("pointerdown", (e) => {
      this.cancelFlight(); // the hand on the map outranks the camera (E9.7)
      if (id1 < 0) {
        id1 = e.pointerId; x1 = e.clientX; y1 = e.clientY;
        lastX = e.clientX; lastY = e.clientY;
        c.classList.add("dragging");
      } else if (id2 < 0 && e.pointerId !== id1) {
        id2 = e.pointerId; x2 = e.clientX; y2 = e.clientY;
        pmx = (x1 + x2) / 2; pmy = (y1 + y2) / 2;
        pdist = Math.hypot(x1 - x2, y1 - y2);
        pinching = true;
      } // third and later pointers are ignored, as before
      try { c.setPointerCapture(e.pointerId); } catch { /* synthetic pointer */ }
    });

    c.addEventListener("pointermove", (e) => {
      if (e.pointerId === id1) { x1 = e.clientX; y1 = e.clientY; }
      else if (e.pointerId === id2) { x2 = e.clientX; y2 = e.clientY; }
      else return;
      if (pinching) {
        const mx = (x1 + x2) / 2, my = (y1 + y2) / 2;
        const dist = Math.hypot(x1 - x2, y1 - y2);
        if (pdist > 0 && dist > 0) this._zoomAt(pmx, pmy, dist / pdist);
        this.tx += mx - pmx;
        this.ty += my - pmy;
        pmx = mx; pmy = my; pdist = dist;
        this.onChange?.();
      } else if (e.pointerId === id1) {
        this.tx += e.clientX - lastX;
        this.ty += e.clientY - lastY;
        lastX = e.clientX;
        lastY = e.clientY;
        this.onChange?.();
      }
    });

    const up = (e) => {
      if (e.pointerId === id1) {
        // promote the second finger to a smooth single-finger pan
        id1 = id2; x1 = x2; y1 = y2;
        id2 = -1;
      } else if (e.pointerId === id2) {
        id2 = -1;
      } else return;
      pinching = false;
      if (id1 >= 0) { lastX = x1; lastY = y1; }
      else c.classList.remove("dragging");
    };
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);

    c.addEventListener("wheel", (e) => {
      e.preventDefault();
      this.cancelFlight();
      const factor = Math.exp(-e.deltaY * 0.0016);
      this._zoomAt(e.clientX, e.clientY, factor);
    }, { passive: false });

    c.addEventListener("dblclick", (e) => {
      this.cancelFlight();
      this._zoomAt(e.clientX, e.clientY, 1.8);
    });
  }
}
