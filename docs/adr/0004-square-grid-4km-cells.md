# ADR-0004: Square grid at 4 km/cell, not a graph world

- **Status:** Accepted (backfilled)
- **Date:** 2026-08 (decision predates ADR system)
- **Touches:** `game/rust/src/geo.rs`, `constants.rs` (`KM_PER_CELL`), renderer

## Context

Fantasy map generators split into two families: raster grids (Dwarf
Fortress, WorldEngine) and irregular graphs/Voronoi meshes (mewo2, Azgaar,
Red Blob's polygonal maps). Graphs give organic coastlines cheaply and make
"region" a first-class object; grids give O(1) neighborhoods, trivially
vectorized passes, straightforward WASM-side packing, and pixel-aligned GPU
rendering. Calliope's identity is layered field computation — climate,
moisture advection, flow accumulation, fertility — which is naturally raster.

## Decision

We represent the world as a square grid (default 640×512, up to 768) with a
declared scale of 4 km per cell, and derive all physical constants and UI
affordances (scale bar, distances, route costs) from `KM_PER_CELL`.
Organic-looking coastlines come from domain-warped noise rather than mesh
irregularity.

## Consequences

- Every field pass is a tight array loop; native 512² generation ~150 ms.
- The renderer consumes fields directly as textures; no triangulation.
- A declared scale anchors believability budgets: one cell ≈ half a day's
  walk; towns 5-8 cells apart are market-town spacing.
- Costs: grid-axis artifacts must be actively suppressed (warp, jitter);
  region adjacency (cultures, territories) is computed, not structural.

## Alternatives considered

- **Voronoi/Delaunay world (mewo2/Azgaar style)** — beautiful coasts, but
  every field algorithm (advection, flow routing, GPU rendering) gets harder;
  packing irregular meshes to WASM/JS is heavier than flat arrays.
- **Hex grid** — nicer isotropy, but no native texture alignment and all
  tooling (ndimage-style ops, D8 hydrology) is square-grid literature.
