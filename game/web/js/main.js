// Calliope client — the composition root (E8.7). The work lives in four
// focused modules; this file only builds the shared context, wires them
// together, mounts the Solid UI, and boots the first world.
//
//   sim.js        the simulation driver: generation, months, histories
//   input.js      selection, pointer picking, hover, keyboard
//   gpu-audit.js  GPU bring-up, present audit, the render loop
//   inspect.js    cell inspection and map-side lookups

import { Renderer } from "./render.js";
import { View } from "./view.js";
import { mountUI } from "./ui/app.js";
import { crashWorkerForDebug } from "./net.js";
import {
  world, month, setWorldSize, setLayer, overlays, setOverlays, setSpeed,
} from "./ui/state.js";
import { initInspect, inspectCell, locateEvent } from "./inspect.js";
import {
  initSim, generate, advance, playPause, step, fitView,
  explain, entityLog, refreshLegends, popHistoryOf, priceHistoryOf,
} from "./sim.js";
import { initInput, select } from "./input.js";
import { initGpu } from "./gpu-audit.js";

// ---------- shared context ----------

const canvas = document.getElementById("map");
const renderer = new Renderer(canvas);

const ctx = {
  canvas,
  renderer,
  view: null,
  dirty: { v: true },        // render loop repaint flag
  version: { n: 0 },         // bumps when world content changes (tints, picking)
  hover: { cell: null },     // hovered cell for the annotation layer
  markDirty: () => { ctx.dirty.v = true; },
};
ctx.view = new View(canvas, ctx.markDirty);

// ---------- debug / gate hooks ----------

window.__calliope = {
  view: ctx.view, renderer, world, month,
  advance: (m) => advance(m),
  // M7 gate evidence: label placement stats from the last drawn frame
  labelStats: () => renderer.labelStats(),
  // E7.10 chaos hook: kill the sim worker, watch recovery replay the world
  crashWorker: () => crashWorkerForDebug(),
  gpuMode: () => "starting",
  gpuForceLive: () => {},
};

// ---------- modules ----------

initInspect(ctx);
initSim(ctx);
initInput(ctx);
const gpu = initGpu(ctx);
window.__calliope.gpuMode = gpu.gpuMode;
window.__calliope.gpuForceLive = gpu.gpuForceLive;

// ---------- actions (the UI's only door into the engine side) ----------

mountUI({
  generate,
  setLayer: (id) => { setLayer(id); ctx.markDirty(); },
  toggleOverlay: (id) => { setOverlays(id, !overlays[id]); ctx.markDirty(); },
  playPause,
  step,
  setSpeed,
  select,
  flyTo: (x, y, scale) => ctx.view.flyTo(x, y, Math.max(ctx.view.scale, scale || 6)),
  fitView,
  explain,
  inspectCell,
  locateEvent,
  popHistoryOf,
  priceHistoryOf,
  refreshLegends,
  entityLog,
});

// ---------- boot ----------

const params = new URLSearchParams(location.search);
const bootSeed = params.get("seed")
  ? Number(params.get("seed"))
  : Math.floor(Math.random() * 2147483646) + 1;
const bootSize = Number(params.get("size"));
if ([384, 512, 640, 768].includes(bootSize)) setWorldSize(bootSize);
generate(String(bootSeed));
