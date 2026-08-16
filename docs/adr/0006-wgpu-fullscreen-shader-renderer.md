# ADR-0006: wgpu fullscreen-shader renderer ("Orbital")

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/render.rs`, WGSL shaders, `game/web/js/gpu.js`

## Context

The map view began as a 2D canvas renderer with CPU-computed hillshading and
per-frame pixel pushes. It capped out visually (no animated water, no
subpixel relief, no atmospheric depth) and computationally (full-map redraws
on pan/zoom). The bar was "satellite imagery of a real place", which needs
per-pixel lighting, specular water, seasonal snowlines — shader work.

## Decision

We render the world with wgpu (WebGL backend in browsers) as fullscreen-quad
WGSL passes over field textures: analytic hillshading from the height
gradient, true-color terrain from moisture/temperature/fertility, animated
noise-driven water with specular glints and foam, seasonal snow/ice lines,
zoom-adaptive detail octaves, and limb/atmosphere framing. A JS frame
governor drops to on-demand rendering when the frame budget is missed.
Entity/vector overlays (labels, routes, borders, markers) stay in canvas 2D
above the GPU layer.

## Consequences

- Pan/zoom and water animation are GPU-costed, not proportional to map size.
- Visual features (snowline advance, glints) are uniforms, not repaints.
- Hybrid stack: GPU base + canvas overlay keeps text crisp and label logic
  simple.
- Costs: WebGL context loss handling, shader debugging in the browser, and
  a compatibility fallback path must stay alive.

## Alternatives considered

- **Keep canvas 2D and optimize** — CPU ceilings on exactly the effects the
  visual bar demanded.
- **Three.js / external engine** — a scene graph buys nothing for one
  fullscreen quad; adds a dependency layer over what wgpu already gives the
  Rust side.
- **Render everything (labels too) on GPU** — text quality and collision
  culling are far easier in canvas; no benefit at our label counts.
