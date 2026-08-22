// Input: selection, pointer picking, the hover tooltip, and keyboard
// shortcuts. Split from main.js (E8.7) — everything here answers "what
// did the player just do?".

import { pick } from "./picking.js";
import { inspectCell } from "./inspect.js";
import { advance, playPause, fitView } from "./sim.js";
import {
  world, entities, settlementsById, cultures, realms,
  selection, setSelection, setHoverTip,
  overlays, setLayer, searchOpen, setSearchOpen, closePopovers,
  overlaysOpen, setOverlaysOpen, legendOpen, setLegendOpen,
  worldMenuOpen, notifOpen, playing, isMobile, sheet, setSheet,
} from "./ui/state.js";
import { LAYERS } from "./ui/config.js";

let ctx = null;

// ---------- selection ----------

export function select(sel) {
  closePopovers();
  const { view } = ctx;
  if (!sel) {
    setSelection(null);
    ctx.markDirty();
    return;
  }
  if (sel.kind === "settlement") {
    const s = settlementsById().get(sel.id);
    if (s && sel.fly) view.flyTo(s.x + 0.5, s.y + 0.5, Math.max(view.scale, 6));
  } else if (sel.kind === "feature" && sel.fly) {
    const f = (world()?.header.features || [])[sel.id];
    if (f) {
      const big = f.t === "ocean" || f.t === "continent" || f.t === "sea";
      view.flyTo(f.x, f.y, big ? Math.max(view.scale, 1.6) : Math.max(view.scale, 5));
    }
  } else if (sel.kind === "deposit" && sel.fly) {
    view.flyTo(sel.x + 0.5, sel.y + 0.5, Math.max(view.scale, 9));
  } else if (sel.kind === "ruin" && sel.fly) {
    const r = (world()?.header.ruins || []).find((x) => x.eid === sel.id);
    if (r) view.flyTo(r.x + 0.5, r.y + 0.5, Math.max(view.scale, 6));
  } else if (sel.kind === "entity" && sel.fly) {
    const e = entities().find((x) => x.id === sel.id);
    if (e && e.x >= 0) view.flyTo(e.x + 0.5, e.y + 0.5, Math.max(view.scale, 6));
  }
  setSelection(sel);
  ctx.markDirty();
}

// ---------- pointer ----------

function wirePointer() {
  const { canvas, view, renderer } = ctx;

  // tap/click (not drag, not pinch) picks the most specific thing under the
  // cursor. On touch there is no hover, so a tap on ground inspects the cell.
  let downAt = null, pointersDown = 0, multiTouch = false;
  canvas.addEventListener("pointerdown", (e) => {
    pointersDown++;
    if (pointersDown > 1) multiTouch = true;
    downAt = [e.clientX, e.clientY];
    view.cancelFlight();
    closePopovers();
  });
  canvas.addEventListener("pointercancel", () => {
    pointersDown = Math.max(0, pointersDown - 1);
    if (pointersDown === 0) { multiTouch = false; downAt = null; }
  });
  canvas.addEventListener("pointerup", (e) => {
    pointersDown = Math.max(0, pointersDown - 1);
    if (pointersDown > 0) return; // other fingers still down
    const wasPinch = multiTouch;
    multiTouch = false;
    const w = world();
    if (!downAt || !w || wasPinch) { downAt = null; return; }
    const moved = Math.hypot(e.clientX - downAt[0], e.clientY - downAt[1]);
    downAt = null;
    if (moved > 5) return;
    const touch = e.pointerType === "touch";
    const hit = pick(w, view, renderer, e.clientX, e.clientY, {
      touch,
      resourcesOn: overlays.resources,
      labelsOn: overlays.labels,
    });
    if (touch) { ctx.hover.cell = null; setHoverTip(null); }
    if (!hit) { select(null); return; }
    if (hit.kind === "cell") {
      // clicking open ground: deselect if something was selected, else inspect
      const cur = selection();
      if (cur && cur.kind !== "cell") { select(null); return; }
      if (cur && cur.kind === "cell" && cur.x === hit.x && cur.y === hit.y) { select(null); return; }
    }
    select(hit);
  });

  // ---------- hover tooltip ----------

  let tipTimer = 0;
  canvas.addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch") return; // touch pans; taps inspect
    const w = world();
    if (!w) return;
    if (e.buttons) { ctx.hover.cell = null; setHoverTip(null); return; } // dragging
    const [wx, wy] = view.screenToWorld(e.clientX, e.clientY);
    const cx = Math.floor(wx), cy = Math.floor(wy);
    const W = w.header.width || w.header.size;
    const H = w.header.size;
    const inWorld = cx >= 0 && cy >= 0 && cx < W && cy < H;
    ctx.hover.cell = inWorld ? { x: cx, y: cy } : null;
    ctx.markDirty();
    clearTimeout(tipTimer);
    if (!inWorld) { setHoverTip(null); return; }
    // a light, throttled tooltip — the full story arrives on click
    tipTimer = setTimeout(() => {
      // settlement under cursor? tease its name instead of the ground
      const hit = pick(w, view, renderer, e.clientX, e.clientY, {
        resourcesOn: overlays.resources, labelsOn: false,
      });
      if (hit?.kind === "settlement") {
        const s = settlementsById().get(hit.id);
        const c = (cultures() || [])[s?.people];
        const r = (realms() || [])[s?.realm];
        if (s) {
          setHoverTip({
            px: e.clientX, py: e.clientY,
            title: s.name,
            sub: `${s.tier}${c ? ` of the ${c.people}` : ""}${r ? ` \u00b7 ${r.name}` : ""} \u00b7 ${s.pop.toLocaleString("en-US")} souls`,
            line: "click to inspect",
          });
          return;
        }
      }
      if (hit?.kind === "ruin") {
        const r = (w.header.ruins || []).find((x) => x.eid === hit.id);
        if (r) {
          setHoverTip({
            px: e.clientX, py: e.clientY,
            title: r.name,
            sub: `abandoned Y${Math.floor(r.since / 12) + 1}${r.people ? ` \u00b7 once of the ${r.people}` : ""}`,
            line: "click to inspect",
          });
          return;
        }
      }
      if (hit?.kind === "deposit") {
        const meta = w.header.resources[hit.id] || {};
        setHoverTip({
          px: e.clientX, py: e.clientY,
          title: hit.id,
          sub: `${meta.category || "resource"} \u00b7 ${meta.abundance || ""}`,
          line: "click to inspect",
        });
        return;
      }
      const info = inspectCell(cx, cy);
      if (!info) { setHoverTip(null); return; }
      setHoverTip({
        px: e.clientX, py: e.clientY,
        title: info.place || info.biome,
        sub: info.place
          ? `${info.biome} \u00b7 ${info.elevation} m \u00b7 ${info.tempNow}\u00b0C`
          : `${info.elevation} m \u00b7 ${info.tempNow}\u00b0C${info.isWater ? "" : ` \u00b7 ${info.precip} mm`}`,
        line: info.notes?.[0] || (info.territory || ""),
      });
    }, 90);
  });
  canvas.addEventListener("pointerleave", (e) => {
    if (e.pointerType === "touch") return;
    ctx.hover.cell = null;
    clearTimeout(tipTimer);
    setHoverTip(null);
    ctx.markDirty();
  });
}

// ---------- keyboard ----------

function wireKeyboard() {
  const { canvas, view } = ctx;
  window.addEventListener("keydown", (e) => {
    if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
    if (searchOpen()) return; // the omnibox owns the keys while open
    const k = e.key;
    if (k >= "0" && k <= "9") {
      // 1..9 pick the first nine lenses, 0 the tenth (M63 grew the strip)
      const lens = LAYERS[k === "0" ? 9 : Number(k) - 1];
      if (lens) { setLayer(lens[0]); ctx.markDirty(); }
    } else if (e.code === "Space") {
      e.preventDefault();
      playPause();
    } else if (k === "n") {
      advance(1);
    } else if (k === "o") {
      const v = !overlaysOpen(); closePopovers(); setOverlaysOpen(v);
    } else if (k === "l") {
      const v = !legendOpen(); closePopovers(); setLegendOpen(v);
    } else if (k === "/") {
      e.preventDefault();
      setSearchOpen(true);
    } else if (k === "f") {
      fitView();
    } else if (k === "+" || k === "=") {
      view.flyTo(...view.screenToWorld(canvas.clientWidth / 2, canvas.clientHeight / 2), view.scale * 1.6, 240);
    } else if (k === "-") {
      view.flyTo(...view.screenToWorld(canvas.clientWidth / 2, canvas.clientHeight / 2), view.scale / 1.6, 240);
    } else if (k === "Escape") {
      if (worldMenuOpen() || overlaysOpen() || legendOpen() || notifOpen()) closePopovers();
      else if (isMobile() && sheet()) setSheet(null);
      else if (selection()) select(null);
    }
  });
}

export function initInput(c) {
  ctx = c;
  wirePointer();
  wireKeyboard();
}
