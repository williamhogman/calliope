// GPU bring-up, the present audit, and the render loop with its frame
// governor. Split from main.js (E8.7) — the whole per-frame path lives in
// this one auditable module.
//
// Damage-driven frames (E9.1): the GPU pass runs continuously only while
// something on it visibly moves — playback, a camera flight, the season
// glide after a step, or animated water up close. An idle map costs zero
// GPU frames; any damage still renders exactly one. The animated vector
// layer (caravans, winds) draws on its own canvas and clock (E9.3), so
// motion never forces the label/marker canvas to repaint.

import { createGpu, recreateGpuOnGl } from "./gpu.js";
import {
  world, layer, overlays, month, playing, selection,
} from "./ui/state.js";

export function initGpu(ctx) {
  const { canvas, renderer } = ctx;
  let glCanvas = document.getElementById("gl");

  // Live-window state: fresh engines and fresh worlds render continuously
  // for a few seconds so the present audit gets its 40 frames even on an
  // otherwise idle map.
  let warmupUntil = 0;
  const armWarmup = () => { warmupUntil = performance.now() + 9000; };

  // Context loss is silent (E9.10 drill finding): a lost WebGL context
  // throws nothing — GL calls become no-ops and the imagery just freezes.
  // The event is the only tell, so every canvas the engine adopts gets a
  // listener that routes straight into the recovery path.
  function armLossWatch(c) {
    c.addEventListener("webglcontextlost", (e) => {
      e.preventDefault(); // we recover on a fresh canvas, not via restore
      if (renderer.gpu) handleGpuFrameError(new Error("WebGL context lost"));
    }, { once: true });
  }

  // GPU imagery: bring up the Rust wgpu engine (WebGPU, else WebGL2).
  // If no adapter exists the CPU compositor stays in charge.
  createGpu(glCanvas)
    .then((gpu) => {
      renderer.gpu = gpu;
      if (gpu.backend() !== "webgpu") armLossWatch(glCanvas);
      const w = world();
      if (w) gpu.setWorld(w);
      armWarmup();
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

  function adoptFreshGl({ gpu: g, canvas: fresh }) {
    glCanvas = fresh;
    renderer.gpu = g;
    const w = world();
    if (w) g.setWorld(w);
    armWarmup();
    ctx.markDirty();
  }

  function handleBlankGpu() {
    const gpu = renderer.gpu;
    if (!gpu) return;
    if (gpu.backend() === "webgpu") {
      console.warn("calliope: WebGPU presents nothing — retrying on WebGL2");
      renderer.gpu = null;
      ctx.markDirty();
      recreateGpuOnGl(glCanvas)
        .then(adoptFreshGl)
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

  // ---------- context-loss recovery (E9.10) ----------
  //
  // A thrown GPU frame usually means the context died (driver reset, tab
  // eviction of GPU memory). One recovery attempt brings a fresh canvas up
  // on WebGL2 and re-uploads the world; a second failure hands the map to
  // the CPU compositor for good.
  let glRecoveryTried = false;

  function handleGpuFrameError(err) {
    console.error("calliope: GPU frame failed:", err);
    renderer.gpu = null;
    ctx.dirty.v = true;
    if (!glRecoveryTried) {
      glRecoveryTried = true;
      console.warn("calliope: attempting GPU recovery on a fresh WebGL2 canvas");
      recreateGpuOnGl(glCanvas)
        .then((res) => {
          gpuAudit.engine = null; // audit the recovered engine from scratch
          adoptFreshGl(res);
        })
        .catch((e) => {
          console.warn("calliope: recovery failed — CPU compositor in charge:", e);
          glCanvas.remove();
          ctx.markDirty();
        });
    } else {
      console.warn("calliope: second GPU failure — CPU compositor in charge");
      glCanvas.remove();
    }
  }

  // ---------- render loop ----------

  // Frame governor: on hardware GL the fullscreen pass is ~free. Software
  // rasterisers can't afford continuous frames — when the frame time stays
  // heavy, live mode switches off: everything still works on damage frames,
  // the water just holds still.
  let lastTs = 0;
  let frameEma = 16;
  let gpuLiveAllowed = true;
  let governorOn = true;
  let lastWorld = null;
  let lastMonth = null;
  let glideUntil = 0;
  let waterSig = "";
  let waterSeen = false;

  // Is animated water actually in view, close enough to read? A coarse
  // 10×8 sample of the viewport, cached until the camera moves.
  function waterInView() {
    const w = renderer.world;
    if (!w) return false;
    const v = ctx.view;
    const sig = ((v.tx / 24) | 0) + "," + ((v.ty / 24) | 0) + "," + ((v.scale * 32) | 0);
    if (sig === waterSig) return waterSeen;
    waterSig = sig;
    const { height, flags } = w.arrays;
    const W = renderer.w, H = renderer.h;
    const cw = canvas.clientWidth, ch = canvas.clientHeight;
    waterSeen = false;
    out:
    for (let sy = 0; sy < 8; sy++) {
      for (let sx = 0; sx < 10; sx++) {
        const wx = Math.floor((cw * (sx + 0.5) / 10 - v.tx) / v.scale);
        const wy = Math.floor((ch * (sy + 0.5) / 8 - v.ty) / v.scale);
        if (wx < 0 || wy < 0 || wx >= W || wy >= H) continue;
        const i = wy * W + wx;
        if (height[i] < 0 || (flags[i] & 2)) { waterSeen = true; break out; }
      }
    }
    return waterSeen;
  }

  function frame(ts) {
    window.__calliope.frames = (window.__calliope.frames || 0) + 1;

    const w = world();
    if (w !== lastWorld) { lastWorld = w; waterSig = ""; armWarmup(); }
    const m = month();
    if (m !== lastMonth) {
      // seasons glide for a moment after a step — keep frames flowing
      if (lastMonth !== null) glideUntil = ts + 700;
      lastMonth = m;
    }

    const isPlaying = playing();
    const wantsLive = isPlaying || ctx.view.flying || ts < glideUntil ||
      ts < warmupUntil || (ctx.view.scale >= 3 && waterInView());

    if (lastTs) {
      frameEma += (Math.min(ts - lastTs, 250) - frameEma) * 0.05;
      if (governorOn && gpuLiveAllowed && wantsLive && ts > 6000 && frameEma > 70) {
        gpuLiveAllowed = false;
        ctx.dirty.v = true;
        console.info("calliope: slow rasteriser detected — GL renders on demand");
      }
    }
    lastTs = ts;

    const live = gpuLiveAllowed && wantsLive;
    const gpu = renderer.gpu;
    if (gpu && gpu.hasWorld && renderer.world && (live || ctx.dirty.v)) {
      if (layer() === "political") {
        // tintEpoch only advances when borders actually moved (E9.2), so
        // an unchanged month costs neither a rebuild nor an upload
        const tint = renderer.tintRgba(ctx.version.n);
        gpu.setTint(tint, renderer.tintEpoch, renderer.lastTintRows);
      }
      try {
        gpu.render(
          { layer: layer(), overlays, month: month() },
          ctx.view, canvas.clientWidth, canvas.clientHeight,
        );
        window.__calliope.gpuFrames = (window.__calliope.gpuFrames || 0) + 1;
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
        handleGpuFrameError(err);
      }
    }

    // the animated vector layer: continuous while time flows, one clean
    // repaint on damage frames otherwise (E9.3)
    const animate = isPlaying && (overlays.routes || overlays.winds);
    if (animate || ctx.dirty.v) {
      renderer.drawAnim({ overlays, playing: isPlaying }, ctx.view);
    }

    if (ctx.dirty.v) {
      ctx.dirty.v = false;
      const sel = selection();
      renderer.draw({
        layer: layer(),
        overlays,
        month: month(),
        version: ctx.version.n,
        playing: isPlaying,
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
    gpuMode: () => (gpuLiveAllowed ? "live" : "on-demand"),
    gpuForceLive: () => { governorOn = false; gpuLiveAllowed = true; armWarmup(); },
  };
}
