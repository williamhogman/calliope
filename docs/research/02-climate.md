# 02 — Climate & Weather Simulation

## Sources

1. **An Apple Pie from Scratch: Climate** — Alex, Worldbuilding Pasta (2020) — https://worldbuildingpasta.blogspot.com/2020/03/an-apple-pie-from-scratch-part-via.html — READ. Global forcing, circulation, precipitation; heuristics ground-truthed against ExoPlaSim GCM runs.
2. **WorldEngine biome/climate docs** — https://worldengine.readthedocs.io/en/latest/biomes.html — READ. Wind-passed moisture with rain-shadow coefficients.
3. **Undiscovered Worlds: Climates** — https://undiscoveredworlds.blogspot.com/2021/01/climates-again.html — READ. Escaping "stringy" line-following rain via iterative wind fields.
4. **Azgaar: Biomes** — https://azgaar.wordpress.com/2017/06/30/biomes-generation-and-rendering/ — READ. Whittaker grid, 2D wind map.
5. **Holdridge Life Zones** — https://en.wikipedia.org/wiki/Holdridge_Life_Zones — SKIM. Biotemperature + precipitation + PET; robust for exotic worlds.
6. **HDA: Wind Model** — https://heredragonsabound.blogspot.com/2018/11/continent-maps-part-4-wind-model.html — READ. Pressure-zone-driven wind grid beats fixed diagonal wind.
7. **ExoPlaSim** — https://github.com/alphaparrot/ExoPlaSim — SKIM. Full GCM as lookup-table oracle.
8. **Geoff's Climate Cookbook** — Geoff Eddy — READ. The manual-worldbuilding bible: pressure belts at 0°/30°/60°, hand-drawn winds and shadows.
9. **PerfectWorld3.lua** (Civ) — Marinaccio — https://github.com/ianbjorndilling/civ6-perfect-world — SKIM. Geostrophic wind + monsoon dynamics in a shipped-game script.
10. **Linear Theory of Orographic Precipitation** — Smith & Barstad (2004) — https://journals.ametsoc.org/view/journals/atsc/61/12/1520-0469_2004_061_1377_altoop_2.0.co_2.pdf — ABSTRACT. Rain ≈ uplift rate × moisture capacity; uplift = terrain gradient · wind vector.
11. **Budyko-Sellers EBMs** — https://mason.gmu.edu/~lhinnov/paleoguide/tutorial2.html — SKIM. 1D latitude energy balance.
12. **Whittaker diagram** — READ. T×P biome lookup — Calliope's current classifier.
13. **Köppen classification** — SKIM. Rule-heavy; harder procedurally than Holdridge.
14. **Frostpunk snow** — SKIM. Micro-scale only.
15. **Hadley cell primers** — NOAA — SKIM. Cell widths from rotation/radius.
16. **Artifexian climate series** — SKIM. ITCZ migration visualized.
17. **Munk gyres** — SKIM. Wind-driven ocean current loops shaped by coastlines + Coriolis.
18. **Distance-transform continentality** — READ. C = d/(d+k); scales seasonal amplitude.
19. **Thermal inertia** — Cowan et al. (2012) — SKIM.
20. **Monsoon procedural model** — ProcGenesis — SKIM. Monsoon = seasonal ITCZ migration crossing large landmasses, reversing winds.

## Synthesis

Cheap believable climate = static field generator, not GCM:

- **Temperature:** T_sea = T_eq − (T_eq−T_pole)·sin^1.5..2(lat) (≈28 °C to −25 °C), lapse −6.5 °C/km, seasonal amplitude scaled by distance-to-coast continentality, optional current bias.
- **Wind:** fixed belts — trades (0-30°, E→W), westerlies (30-60°, W→E), polar easterlies — with the whole system migrating 5-10° toward the summer hemisphere (ITCZ shift). That one seasonal shift is what produces monsoons and seasonal deserts.
- **Precipitation:** moisture advected in the wind; gain over water, lose over land; orographic extraction ∝ max(0, ∇H·V̂); rain shadow from depletion + descent; subsidence drying near 30°.
- **Biomes:** Whittaker (T,P) lookup; Holdridge (PET-based) distinguishes savanna vs scrub better and generalizes to exotic suns.

## Calliope

`climate.rs` already has: lat^1.7 bands, −6.5 °C/km lapse, EDT continentality, multi-pass advection with trade/westerly bands, gaussian ITCZ boost, evapotranspiration recycling, subsidence penalty (tuned to ~23 % desert).

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | **Dynamic ITCZ shift** — move the ITCZ gaussian with the season → monsoons, seasonal deserts, wet/dry seasons | S | High — ~5 lines, whole new climate storytelling |
| 2 | Orographic dot-product (wind vector · height gradient instead of upwind Δh) | M | Better rain on diagonal ranges |
| 3 | Ocean gyre heuristic (precomputed flow field biasing coastal temperature) | M | Breaks pure-latitude coastal monotony ("Gulf Stream" coasts) |
| 4 | Continentality asymmetry (upwind vs downwind coast weighting) | S | East/west coast contrast |
| 5 | Forest evapotranspiration feedback (already partial) | M | Deep-interior moisture |
| 6 | Holdridge life zones as alternative classifier | S | Marginal for Earth-like defaults |
