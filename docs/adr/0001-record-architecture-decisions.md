# ADR-0001: Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-08-16
- **Touches:** `docs/adr/`

## Context

Calliope has accumulated a deep stack of interlocking decisions — Rust/WASM
port, determinism rules, grid representation, pricing philosophy, tuning
bands — that live only in commit history and chat logs. Each new system
negotiates against all previous ones; without written records the same
trade-offs get re-argued and constants get "fixed" without knowing what they
protect.

## Decision

We record architecturally significant decisions as numbered, immutable ADRs
in `docs/adr/`, using the MADR-lite template. Significant means: it shapes
more than one module, embodies a trade-off we intend to hold, or sets a
tuning philosophy the diagnostics bands enforce. Existing foundational
decisions are backfilled honestly (marked as such) rather than left implicit.

## Consequences

- New systems land together with their ADR; reviewers can check changes
  against standing decisions by number.
- Reversing course costs a superseding ADR — deliberate friction.
- The backfill (0002–0014) freezes today's understanding of why the system
  is shaped as it is.

## Alternatives considered

- **Keep decisions in commit messages** — unsearchable as a set, no
  alternatives section, no supersession chain.
- **A single DESIGN.md** — rots; one growing file invites editing history
  instead of appending to it.
