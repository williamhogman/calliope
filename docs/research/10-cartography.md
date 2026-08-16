# 10 — Cartography & Map Rendering

## Sources

1. **Cartographic Relief Presentation** — Eduard Imhof (1982/2007) — https://archive.org/details/cartographicreli0000imho — READ. The Swiss school: NW light, aerial perspective (contrast and saturation rise with elevation), relief as "plastic" form.
2. **Positioning Names on Maps** — Imhof (1975) — READ. The label grammar: no overlaps; point labels prefer top-right; area names letter-spaced across their extent; italics for water; caps hierarchy for political/physical rank.
3. **Texture Shading** — Leland Brown (2010) — https://mountaincartography.icaci.org/activities/workshops/banff_canada/papers/brown.pdf — READ. Fractional-Laplacian relief enhancement — ridge/canyon contrast with no light-direction bias; blend with hillshade.
4. **Cross-blended Hypsometric Tints** — Patterson & Jenny (2011) — https://www.shadedrelief.com/hypso/hypso.html — READ. Elevation ramps blended by climate — kills the "Green Sahara" artifact.
5. **Terrain Texture Shader** — Patterson (2010) — READ. Practical texture+hillshade blending recipes.
6. **Tanaka illuminated contours** — Morita (2001) — SKIM. Line-only 3D effect.
7. **Point-feature label placement study** — Christensen, Marks, Shieber (1995) — https://www.merl.com/publications/docs/TR96-04.pdf — READ. Simulated annealing best quality; greedy-with-priorities "good enough" real-time.
8. **Töpfer's Radical Law** — READ. n_c = n_s·√(M_s/M_c): feature count under zoom-out follows a square-root law — the principled culling governor.
9. **Stamen Watercolor process** — Watson (2012) — READ. Noise-perturbed edges, blurs, texture masks as a layered pipeline.
10. **Azgaar: Styling the Map** — READ. Label halos; coastal vignettes (concentric coast-parallel lines) as the strongest single "map feel" device.
11. **watabou: improved map labels** — READ. Readability-at-a-glance beats optimal placement.
12. **Curved label placement** — Haunert et al. (2014) — SKIM. Spline labels with curvature limits.
13. **Hand-drawn coastal effects** — John Nelson (2017) — READ. Stippling density functions, vignette depth.
14. **Swiss-style methodology** — Räber et al. (2009) — READ. Valley desaturation, peak vibrancy ramps.
15. **MapLibre style expressions** — READ. Zoom-interpolated size/visibility as the data-driven standard.
16. **Why early cartography fails fantasy maps** — K.M. Alexander (2020) — READ. Hybrid aesthetic: modern precision, engraved embellishment.
17. **Portolan charts** — Campbell (2021) — SKIM. Rhumb networks, coast-heavy labeling — grounding for sea-lane styling.
18. **Multi-directional hillshading** — USGS — READ. 4-direction light kills single-source blind spots.

## Synthesis

Authoritative map feel = precision passed through cartographic filters: **relief** (texture shading + multi-directional light + aerial perspective), **color** (climate-blended hypsometric ramps), **labels** (Imhof's grammar: hierarchy by case/spacing, association by curve-following, conflict resolution by annealing or good greedy), **generalization** (Töpfer's law: density governed by the square root of scale change — cull, don't shrink). Ocean treatment (vignettes, stippling) is what separates "imagery" from "a map".

## Calliope

The wgpu satellite pass is strong; the *atlas* layer is where the gaps are.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Coastal vignettes from existing coast-distance field (sin(dist·f) term in shader) | S | Instant "engraved atlas" feel |
| 2 | Töpfer-law settlement/label culling from `view.scale` | M | Even density at every zoom; world feels bigger |
| 3 | Letter-spaced area labels (regions, oceans span their extent) | S | Imhof hierarchy for near-free |
| 4 | Cross-blended hypsometric option for the elevation layer | M | Fixes green-desert reading in informational modes |
| 5 | Multi-directional hillshade (4-tap WGSL) | M | No more invisible south slopes |
| 6 | Texture-shading pass (precomputed Laplacian texture at gen time) | M | Tactile flatland detail |
| 7 | Curved river labels along the run | L | The holy grail; do last |
