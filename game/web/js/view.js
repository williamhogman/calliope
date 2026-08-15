// Pan/zoom controller (CSS-pixel space).

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

  fit(worldSize) {
    this.worldSize = worldSize;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    const pad = 40;
    this.scale = Math.min((w - pad * 2) / worldSize, (h - pad * 2) / worldSize);
    this.minScale = this.scale * 0.5;
    this.tx = (w - worldSize * this.scale) / 2;
    this.ty = (h - worldSize * this.scale) / 2;
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
    let dragging = false;
    let lastX = 0, lastY = 0, moved = 0;

    c.addEventListener("pointerdown", (e) => {
      dragging = true;
      moved = 0;
      lastX = e.clientX;
      lastY = e.clientY;
      c.setPointerCapture(e.pointerId);
      c.classList.add("dragging");
    });
    c.addEventListener("pointermove", (e) => {
      if (!dragging) return;
      const dx = e.clientX - lastX;
      const dy = e.clientY - lastY;
      moved += Math.abs(dx) + Math.abs(dy);
      lastX = e.clientX;
      lastY = e.clientY;
      this.tx += dx;
      this.ty += dy;
      this.onChange?.();
    });
    const up = (e) => {
      dragging = false;
      c.classList.remove("dragging");
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
