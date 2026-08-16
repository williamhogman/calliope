# 08 — Ecology, Wildlife & Agriculture

## Sources

1. **Stability switching in discrete Lotka-Volterra/Ricker systems** — Puma & Mager (2023) — https://www.mdpi.com/2075-1680/12/4/390 — READ. Euler-stepped LV explodes; Ricker form N_{t+1}=N_t·e^{r(1−N/K)} stays positive and stable.
2. **Rain World AI postmortem** — Jakobsson (2017) — READ. Offscreen ecology as abstract state machines — persistence without cost.
3. **S.T.A.L.K.E.R. A-Life interview** — Champandard (2008) — READ. Offline agents on coarse graphs with outcome tables.
4. **DF biome/fauna wiki** — SKIM. Per-region wildlife pools, depletion + migration refill.
5. **FireBGCv2** — Keane et al. (2011) — ABSTRACT. Gap-model succession Grass→Shrub→Sapling→Forest with fire.
6. **Equilibrium dynamics of pre-industrial populations** — Fanta et al. (2018) — READ. Malthusian hovering at climate-set K; logistic recovery after shocks.
7. **GAEZ v4 model docs** — FAO — SKIM. Crop suitability = product of 0-1 reduction factors (temp, moisture, soil).
8. **USDA crop climatic profiles** — SKIM. Crop packages: wheat 15-25 °C temperate, rice 25-35 °C monsoon, maize warm-subtropical.
9. **Pastoralism boundary** — Blyakharchuk (2014) — READ. Below ~300 mm precipitation, farming fails without irrigation → pastoral nomadism.
10. **River development & fisheries** — Nature 2023 — ABSTRACT. Flood pulses drive delta productivity.
11. **Megafauna overkill modeling** — Kope (2024) — READ; **Alroy (2001)** — ABSTRACT. 2 %/yr hunting can extinguish slow breeders over 1000 y.
12. **Eco design pillars** — Strange Loop (2019) — READ. Full food web; over-harvest → permanent collapse.
13. **Plague transmission on trade routes** — Yue (2017) — READ. Black Death 1.5-5 km/day overland; ports as superspreaders jumping by sea.
14. **Neimark-Sacker bifurcation** — Khan (2020) — READ. Discrete eco-models need small steps or stabilized forms.
15. **Tarn Adams on trees** — SKIM. Trees as slow creatures; over-harvest hits biome.
16. **LANDIS succession** — He (2005) — READ. Grid state-transition with age; disturbance resets.
17. **Epidemic trade** — Boerner & Severgnini (2014) — READ. Betweenness centrality predicts infection order.
18. **Hunter-gatherer density** — Tallavaara (2017) — READ. 0.01-0.1 persons/km², NPP-capped.
19. **Markov chains for succession** — Diener — SKIM.
20. **Equilinox Q&A** — SKIM.
21. **England 1209-1869 agriculture** — Clark (2007) — READ. Grain 60-100 p/km²; sheep-corn 20-40.
22. **Newman: epidemics on networks** — ABSTRACT. Degree distribution sets epidemic threshold.
23. **Land per capita vs technology** — Kaplan (2017) — READ. Land = Land₀·T^−0.5.
24. **GAEZ v3: Length of Growing Period** — READ. LGP = days with P > 0.5·PET predicts the crop package.
25. **State-and-transition hysteresis** — Brischke (2018) — READ. Overgrazed grassland → shrub does not revert.

## Synthesis

Cheap believable ecology = carrying capacity by **crop package** (hunter-gatherer 0.05 / pastoralist 2 / wheat 30 / rice 120 p/km²) with logistic growth ΔP = rP(1−P/K); crop suitability as multiplicative reduction factors; wildlife as seasonal Ricker updates with hunting-pressure extirpation; vegetation as Markov state transitions with fire/harvest disturbance and hysteresis; epidemics as SIR over the trade graph with slow spatial diffusion + fast port jumps (R₀ 2-3, γ≈0.1).

## Calliope

`agriculture.rs` has one fertility scalar; growth is logistic-ish against 900·food. No crops, wildlife, succession, or disease.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Crop packages (wheat/rice/maize/pastoral masks from T, P, LGP) → K per package; wheat belts and rice deltas emerge | S | High — distinct agrarian regions + trade patterns |
| 2 | K from package + tech (Kaplan T^−0.5), keep Ricker/logistic growth | S | Grounded population ceilings |
| 3 | Pastoral aridity boundary (<300 mm → pastoralist K unless irrigated floodplain) | S | Steppe cultures make sense |
| 4 | Trade-network SIR plagues (ports as superspreaders; chronicle beats; population shocks) | L | Highest drama; touches pop, trade, chronicle |
| 5 | Markov vegetation succession + wood-cutting/fire | M | Living map; deforestation history |
| 6 | Wildlife layer (Ricker deer/wolves, hunting pressure, extirpation events) | M | Hunting economy + chronicle color |
