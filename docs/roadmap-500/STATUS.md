# The Five Hundred — Status Ledger

Landed phases of `../ROADMAP-500.md`, one row per phase, appended when
the phase's gate runs green in the diagnostics harness. The specs in
the era files are binding; this ledger only records completion.

| Phase | Title | Landed | Gate evidence |
|---|---|---|---|
| M16 | Plates Remembered | 2026-08-18 | terrain lane: plate count/mean drift-age banded, sketch+heightmap regen byte-identical ×3 seeds; full suite 381 pass · 14 warn · 0 fail (new warns = host ~1.8× slower across untouched stages — perf.txt vs c4c51c0; assay law fixed: phantom baseline on unlisted goods) |

## Ready queue

- M17 — Orogeny Ages (in progress)

## Notes

- ADR numbering: the era specs were drafted before ADR-0018–0023
  landed; where a spec names a `new:` ADR number already taken, the
  next free number is used (M16's sketch ADR is **0024**, not 0018).
  The spec content is binding; the number is not.
