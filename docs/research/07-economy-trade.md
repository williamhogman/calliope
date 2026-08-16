# 07 — Economy, Trade & Production

## Sources

1. **Emergent Economies for Role Playing Games** — Doran & Parberry (2010/12) — https://ianparberry.com/pubs/econ.pdf — READ. The BazaarBot paper: agents with private price beliefs, double-auction clearinghouse, belief nudges on fill/fail, role-switching toward profitable trades; rejects RL as impractical.
2-3. **bazaarBot repo; Doucet explainer** — https://github.com/larsiusprime/bazaarBot — SKIM/READ. Executable pseudocode; documented failure modes (goods with no supplier spike forever).
4. **Moneta** — SKIM. Second implementation + earlier tech report.
5-7. **Victoria 3: economy deep dive (Game Developer 2023); goods DD; production methods DD** — READ/SKIM. One price per connected **market area**; buildings run swappable production methods (recipes); prices move on buy/sell order ratios; convoys bound divergence between areas.
8-9. **EVE QENs (CCP economist); economist hire release** — SKIM. Real price indices (MPI/PPI/CPI) over regional order books; faucet/sink monitoring as inflation control; persistent regional price gaps = the arbitrage loop.
10. **In-game Economics of Ultima Online** — Zachary Booth Simpson (1999) — https://simonsarris.com/IngameEconomicsofUltimaOnline.pdf — READ. Failure-mode catalog: vendor-spread arbitrage, infinite respawning resources destroying scarcity, faucet/sink imbalance.
11-13. **Inflation prevention threads; NetEase GDC 2020; Albion GDC** — SKIM/ABSTRACT. Sinks checklist; live observability as design requirement.
14-15. **Tesfatsion: Agent-based Computational Economics** — READ/SKIM. "Culture-dish" economics: no equilibrium assumption, dynamics purely from interaction rules — the formal justification for simulator economies.
16. **Gravity model of trade** — Anderson/USITC — SKIM. flow(a,b) ∝ mass_a·mass_b / resistance — fast validation model.
17. **Ricardian comparative advantage classroom experiment** — Wesleyan — SKIM. Why specialization + trade beats autarky.
18-19. **Medieval price lists** — Hodges (UC Davis); Goucher C14 — READ/SKIM. Real L/s/d ratios for grain, tools, livestock, armor, wages — calibration targets for `base_value`.
20. **Port Royale 4 economy guide** — SKIM. Town-level price arbitrage loop.
21-22. **Simutrans industry chains; OpenTTD IndustrySpec** — SKIM. Logistics-gated production; OpenTTD's no-price contrast case.
23-24. **X4 Foundations station economy** — SKIM. Price = f(local storage fill) between floor/ceiling — the simplest local-market rule that works.
25. **Banished: crop rotation cut** — Hodorowicz — READ. Fidelity that adds bookkeeping without legible agency is a net loss.
26. **Banished ecology analysis** — Play the Past — SKIM. Production-without-market already yields boom/bust.
27. **Anno Union: designing an Anno game** — SKIM. Tiered chains as the central loop.

## Synthesis

Three market architectures: **global pooled** (cheap, no regional stories — Calliope today), **local/regional with limited interconnection** (Victoria 3 market areas; X4 stock-based station prices; EVE empirically) — the architecture that makes trade routes *mean* something, and **agent bazaars** (BazaarBot) where price is an emergent statistic of matched trades. Stable price rules share: damping (Calliope's 75/25 smoothing ✓), bounds (✓), shock-vs-drift separation (✓ via `shock()`), and the named failure modes (pinning, infinite supply, faucet/sink imbalance) — Calliope's relative-scarcity renormalization is structurally immune to currency inflation. Production chains are recipes; trade flow is either explicit-path (Calliope ✓) or gravity-model; medieval price lists give real calibration ratios.

## Calliope

`economy.rs`/`trade.rs`: one world market; goods are terminal (no ore→metal→tools); trade income is geography-only (no arbitrage); no merchants; food demand never couples to actual local starvation.

| # | Technique | Cost | Value |
|---|---|---|---|
| 1 | Keep the damped/clamped/shock-split price backbone | — | Non-negotiable stability foundation |
| 2 | Famine coupling: per-settlement subsistence check → local demand spike, pop loss, migration events | S-M | High drama, small code |
| 3 | Production recipes (ore→metal→goods) gated on workforce + inputs | M | Price cascades; processing towns vs extraction camps |
| 4 | Market areas from route connectivity (union-find), per-area prices, arbitrage-driven trade income | L | The single highest-value economic change |
| 5 | Gravity-model flow as harness validation | S | QA tool |
| 6 | Merchant agents with belief updating | L | Follow-on to #4, not before |

Calibration: check grain:iron:gold ratios and settlement income vs upkeep against the Hodges/Goucher lists in the diagnostics harness.
