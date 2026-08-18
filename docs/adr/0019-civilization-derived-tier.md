# ADR-0019: Civilizations as a derived tier over peoples and realms

- **Status:** Accepted
- **Date:** 2026-08-17
- **Touches:** `game/rust/src/civ.rs` (new), `state.rs` (`Peoples.civs`),
  `systems.rs` (`CivPulse`), `society.rs` (`boon`), `event.rs` (`Era`),
  `entity.rs` (`Civilization`), `telling.rs` (arc pattern),
  `snapshot.rs` (civs wire lane), `bin/diagnose.rs` (M13 gates)

## Context

M12 left the engine with two axes (ADR-0018): peoples on the
generational clock, realms on the political clock, and a kinship metric
binding them. Empires already *happen* — unions, vassal webs, tribute —
but nothing names the emergent tier, so the telling narrates a flicker
of secessions where a chronicler would write "the fall of an empire."
M13 asks for the arc: golden ages, overstretch, collapse into successor
realms, told whole.

## Decision

A **civilization is derived state, never authoritative.** It is the
kinship-closure of living peoples (edges at `kinship ≥ 0.45`, members
retained to 0.35 — hysteresis so borders don't flap), recomputed yearly
by a `CivPulse` system and *matched* to the existing roster by people
overlap. No settlement, realm or people stores a civ id; deleting
`Peoples.civs` loses names and stage, never simulation truth.

The arc is a four-stage machine driven only by quantities that already
exist: legitimacy, asabiyyah, treasury (golden gate, sustained N
years), and an overstretch index `Σ pop^0.85 / capacity` (Bettencourt
reuse from the influence maps). Waning surfaces the Khaldun decay as
court-rot events; collapse opens an interregnum that **reuses the M11
ladder** — the civ pass raises unrest and guts asabiyyah on member
realms and lets the existing secession/coup rungs mint the successor
realms. Collapse therefore mints realms, never peoples, by
construction (ADR-0018 invariant), and closes the civ's registry entry
instead of deleting rows.

Golden ages act through one hook: `Society.boon`, a research-pace
multiplier the civ pass owns (reset to 1.0 every pass, raised for
members of golden civs). Monuments ride the existing artifact system
(`kind: "monument"`); hegemony (M13.5) is read off the vassal/tribute
edges already in `Politics` and stored as `paramount` on the civ row.

Wire: one `civs` block, whole-form hash-gated like `cultures` (E4.3);
civs are few and move on decade clocks. New vocabulary appended last —
`EventKind::Era`, `EntityKind::Civilization` — so every existing wire
discriminant is unchanged.

## Consequences

- The sifter tells rise-and-fall whole: civ stage transitions are Era
  events tagged with the civ's entity id, so the arc pattern is the
  settlement rise-fall pattern at a century scale — no text anchors.
- Polity count oscillates: interregnums fragment, survivors
  re-qualify, the cycle turns (the M13 gate measures this).
- Costs: one more yearly pass (≤ ~20 peoples, trivial); membership is
  recomputed, so a civ's composition can drift a people at a time —
  accession events keep the record honest.

## Alternatives considered

- **Civ as authoritative entity** (settlements/realms store civ ids) —
  a third axis to keep consistent through conquest, secession, fusion;
  the derived form cannot desync because it is recomputed from truth.
- **Fragmentation as bespoke collapse code** — would duplicate the M11
  secession machinery and could mint peoples by accident; driving the
  existing ladder keeps one code path for realm-minting.
- **Text-anchored sifter detection** (as the patina diagnose counters
  do) — fragile inside the engine; entity-id beats are structural.
