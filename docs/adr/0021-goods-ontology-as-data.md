# ADR-0021: The goods ontology as one declarative table

- **Status:** Accepted
- **Date:** 2026-08-17
- **Touches:** `game/rust/src/resources.rs` (`GOODS`, `GoodSpec`, `Place`,
  `Transport`, `ontology_lint`), `game/rust/src/bin/diagnose.rs`
  (resources lint row)

## Context

Good-shape knowledge lived in nine hand-kept match arms spread across
`resources.rs`: `abundance()`, `requires()`, `isa()`, `category()`,
`color()`, `foundin()`, `initial_known_p()`, `reserve_months()` and the
nineteen-arm `suitability()`. Adding one good (M14 adds a dozen) meant
editing all nine and hoping none was missed — the exact drift class
ADR-0015 killed for fields, events and bands. The client meta had
already drifted once: `resource_meta()` hand-added a grain row whose
`isa`/`category` disagreed with the engine's own accessors, and the
craft goods had no meta rows at all.

## Decision

One `pub const GOODS: [GoodSpec; Good::COUNT]` table, in variant
(= alphabetical) order, is the single declaration point. Each row
carries: ISA chain, shelf category, tech REQUIRES, abundance, FOUNDIN,
color, transport class (`Bulk`/`Ordinary`/`Precious`), perishability,
placement rule, dawn-known probability and reserve parameters.
Everything derives:

- **Accessors** — `abundance()`, `requires()`, `isa()`, `category()`,
  `color()`, `foundin()`, plus new `transport()`/`perishable()` — read
  `Good::spec()`, a direct index into the table.
- **`suitability()`** — one interpreter over the `Place` column (seven
  rule shapes replace nineteen arms). Thresholds stay `f32` because the
  legacy arms compared against `f32` literals; every mask is
  byte-identical (verified: `diagnose resources` reports for seeds
  12345/777/90210 diff clean against pre-refactor baselines, and the
  determinism triple passes).
- **`resource_meta()`** — emits every good row-by-row from the table;
  goods the map never places carry `"virtual": true`. Craft goods gain
  real meta rows (and colors) for the first time.
- **Deposit machinery** — `initial_known_p` and `reserve_months` read
  the `known_p` and `reserve` columns; the reserve formula stays code.

Two guards, per the ADR-0015 doctrine of checked-not-trusted:

- A `const` block asserts `GOODS[i].good as usize == i` — a misordered
  row is a compile error, not a wrong world.
- `ontology_lint()` cross-checks the hot-path closure flags
  (`is_food` … `is_fuel`, `foundin`↔`is_metal`) against the ISA column;
  `diagnose resources` prints violations as a `[FAIL]`. Grain is the
  one documented exemption: the client shelves it under food, but the
  legacy price math excluded it and the M8 hash gate forbids changing
  that.

`ALL_PLACEABLE` stays as a second, order-bearing list: placement order
indexes noise planes and rng streams (legacy order, load-bearing), and
folding it into the alphabetical table would re-roll every world.

## Consequences

- M14's dozen new goods are each one table row plus an
  `ALL_PLACEABLE` append — no new match arms anywhere.
- `transport` and `perishable` are declared vocabulary now; M14.7
  prices them into route viability without touching the table shape.
- The is_* flags remain hand-written `matches!` for const-fn hot paths;
  the lint is the seam that keeps them honest.
- Costs: the table row is wide (twelve columns), and `Place` can only
  express rule shapes the interpreter knows — a genuinely novel
  placement (M14.2 salt pans) adds one enum variant plus one
  interpreter arm, still a single site.

## Alternatives considered

- **Deriving is_* from ISA at runtime** — str-compare in price loops;
  the flags are hot and const. Lint instead.
- **Folding ALL_PLACEABLE into the table** — an `order` column or
  alphabetical placement would either keep two lists anyway or re-roll
  every world's deposits. The legacy order is data, kept as data.
- **Hand discipline** — nine match arms per good is how the grain meta
  drift happened; the failure mode this ADR exists to kill.
