// GPU bring-up, the present audit, and the render loop with its frame
// governor. Split from main.js (E8.7) — the whole per-frame path lives in
// this one auditable module.

import { createGpu, recreateGpuOnGl } from "./gpu.js";
import {
  world, layer, overlays, month, playing, selection,
} from "./ui/state.js";

export function initGpu(ctx) {
  const { canvas, renderer } = ctx;
  let glCanvas = document.getElementById("gl");

  // GPU imagery: bring up the Rust wgpu engine (WebGPU, else WebGL2).
  // If no adapter exists the CPU compositor stays in charge.
  createGpu(glCanvas)
    .then((gpu) => {
      renderer.gpu = gpu;
      const w = world();
      if (w) gpu.setWorld(w);
      ctx.markDirty();
    })
    .catch((err) => {
      console.warn("GPU engine unavailable; CPU compositor in charge:", err);
      glCanvas.remove();
    });

  // ---------- GPU present audit ----------
  //
  // Some browsers hand out a GPU device that never puts a pixel on screen
  // (broken WebGPU drivers, headless software rasterisers). After the engine
  // has had a fair number of frames with a world, read the canvas back once:
  // if it is still fully transparent, the imagery never arrived — retry on
  // WebGL2 with a fresh canvas, and past that let the CPU compositor carry.
  const gpuAudit = { engine: null, frames: 0 };

  function gpuCanvasHasPixels() {
    const t = document.createElement("canvas");
    t.width = 16; t.height = 16;
    const c = t.getContext("2d", { willReadFrequently: true });
    try { c.drawImage(glCanvas, 0, 0, 16, 16); } catch { return true; }
    const d = c.getImageData(0, 0, 16, 16).data;
    for (let i = 3; i < d.length; i += 4) if (d[i] !== 0) return true;
    return false;
  }

  function handleBlankGpu() {
    const gpu = renderer.gpu;
    if (!gpu) return;
    if (gpu.backend() === "webgpu") {
      console.warn("calliope: WebGPU presents nothing — retrying on WebGL2");
      renderer.gpu = null;
      ctx.markDirty();
      recreateGpuOnGl(glCanvas)
        .then(({ gpu: g, canvas: fresh }) => {
          glCanvas = fresh;
          renderer.gpu = g;
          const w = world();
          if (w) g.setWorld(w);
          ctx.markDirty();
        })
        .catch((err) => {
          console.warn("calliope: WebGL2 retry failed — CPU compositor in charge:", err);
          glCanvas.remove();
          ctx.markDirty();
        });
    } else {
      console.warn("calliope: GL engine presents nothing — CPU compositor in charge");
      renderer.gpu = null;
      glCanvas.remove();
      ctx.markDirty();
    }
  }

  // ---------- render loop ----------

  // Frame governor: on hardware GL the fullscreen pass is ~free, so it runs
  // every frame (living water, gliding seasons). Software rasterisers can't
  // afford that — when the frame time stays heavy, fall back to on-demand
  // GL rendering: everything still works, the water just holds still.
  let lastTs = 0;
  let frameEma = 16;
  let gpuLive = true;
  let governorOn = true;

  function frame(ts) {
    window.__calliope.frames = (window.__calliope.frames || 0) + 1;
    if (lastTs) {
      frameEma += (Math.min(ts - lastTs, 250) - frameEma) * 0.05;
      if (governorOn && gpuLive && ts > 6000 && frameEma > 70) {
        gpuLive = false;
        ctx.dirty.v = true;
        console.info("calliope: slow rasteriser detected — GL renders on demand");
      }
    }
    lastTs = ts;
    const gpu = renderer.gpu;
    if (gpu && gpu.hasWorld && renderer.world && (gpuLive || ctx.dirty.v)) {
      if (layer() === "political") gpu.setTint(renderer.tintRgba(ctx.version.n), ctx.version.n);
      try {
        gpu.render(
          { layer: layer(), overlays, month: month() },
          ctx.view, canvas.clientWidth, canvas.clientHeight,
        );
        // Present audit once the engine has had 40 world frames. WebGL2 reads
        // back in the same task (without preserveDrawingBuffer the drawing
        // buffer is only valid here). WebGPU reads back a macrotask later:
        // drawImage then snapshots the *presented* frame — exactly the thing
        // a broken driver never delivers, while a same-task read would see
        // the submitted texture and miss the failure.
        if (gpuAudit.engine !== gpu) { gpuAudit.engine = gpu; gpuAudit.frames = 0; }
        if (++gpuAudit.frames === 40) {
          if (gpu.backend() === "webgpu") {
            setTimeout(() => {
              if (renderer.gpu === gpu && !gpuCanvasHasPixels()) handleBlankGpu();
            }, 0);
          } else if (!gpuCanvasHasPixels()) {
            handleBlankGpu();
          }
        }
      } catch (err) {
        // one bad GPU frame must not kill the annotation layer or the loop
        console.error("calliope: GPU frame failed, CPU compositor takes over:", err);
        renderer.gpu = null;
        glCanvas.remove();
        ctx.dirty.v = true;
      }
    }
    // caravans and winds animate continuously while time flows
    if (playing() && (overlays.routes || overlays.winds)) ctx.dirty.v = true;
    if (ctx.dirty.v) {
      ctx.dirty.v = false;
      const sel = selection();
      renderer.draw({
        layer: layer(),
        overlays,
        month: month(),
        version: ctx.version.n,
        playing: playing(),
        selectedId: sel?.kind === "settlement" ? sel.id : null,
        selectedRuin: sel?.kind === "ruin" ? sel.id : null,
        selectedCell: sel?.kind === "cell" ? sel : null,
      }, ctx.view, ctx.hover.cell);
      window.__calliope.draws = (window.__calliope.draws || 0) + 1;
    }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
  window.addEventListener("resize", ctx.markDirty);

  return {
    gpuMode: () => (gpuLive ? "live" : "on-demand"),
    gpuForceLive: () => { governorOn = false; gpuLive = true; },
  };
}
