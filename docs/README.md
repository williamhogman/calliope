# Calliope — Documentation

Calliope is a deterministic fantasy-world simulator: a Rust core (compiled to
WASM for the browser) that generates terrain, climate, hydrology, biomes,
resources, settlements, cultures, trade, an economy, societies and a running
chronicle from a single seed, rendered by a wgpu shader engine with a
Solid.js UI.

## Map of this directory

| Document | What it is |
|---|---|
| [`ROADMAP.md`](ROADMAP.md) | The forward plan: milestones, ready queue, acceptance gates |
| [`ROADMAP-ENGINE.md`](ROADMAP-ENGINE.md) | The platform track: engine optimization, data formats, macro/codegen discipline, UI/render surface polish |
| [`GAP-ANALYSIS.md`](GAP-ANALYSIS.md) | Every Calliope system measured against the literature |
| [`adr/`](adr/) | Architecture Decision Records — why the system is shaped the way it is |
| [`research/`](research/) | The research corpus: per-domain digests of ~250 primary sources, plus a cross-cutting synthesis |

## Reading order

1. `research/SYNTHESIS.md` — what the field knows.
2. `GAP-ANALYSIS.md` — where Calliope stands against it.
3. `ROADMAP.md` — what we do about it, in what order.
4. `adr/` — the standing decisions any change must respect (or supersede
   explicitly with a new ADR).

## Ground rules

- **Determinism is law.** Every system is a pure function of the seed
  (ADR-0003). Any proposal that breaks bit-reproducibility is rejected or
  redesigned.
- **The harness is the gate.** No balance or systems change lands without a
  clean `game/rust/scripts/report.sh` run (ADR-0009). New systems ship with
  new checks.
- **Decisions get written down.** Anything that shapes architecture, a
  trade-off held against re-litigation, or a tuning philosophy goes in an
  ADR at the time it is decided.
