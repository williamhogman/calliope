# The Five Hundred — Full Specs

The binding four-field specification for every phase of
`../ROADMAP-500.md`. The parent file carries the one-line sketch and
the quarterly rollup; each era file here expands its phases into the
form a phase is actually opened with.

## The four fields

Every phase is one block:

```
### M<n> — Title
- **Intent:** why the phase exists and what the world gains.
- **Build:** the concrete deliverables — algorithms, structures, laws.
- **Touches:** real repo paths, plus `new:` paths the phase creates.
- **Gate:** the measurable pass condition the harness will check.
```

Rules of the corpus: the one-liner in `../ROADMAP-500.md` is binding —
a spec expands it, never contradicts it. Every Gate is checkable by a
script or the diagnostics harness (ADR-0009). Any phase adding state
folds it into the determinism hash (ADR-0003). Forge phases never add
world behavior; their gates are hash-unchanged, budgets green, suite
green.

## Era files

| File | Era | Phases |
|---|---|---|
| `01-era-i-the-deep-earth.md` | I — The Deep Earth (+ Forge I) | M16–M70 |
| `02-era-ii-the-long-sky.md` | II — The Long Sky (+ Forge II) | M71–M125 |
| `03-era-iii-the-named-lives.md` | III — The Named Lives (+ Forge III) | M126–M180 |
| `04-era-iv-the-living-land.md` | IV — The Living Land (+ Forge IV) | M181–M235 |
| `05-era-v-the-tongues.md` | V — The Tongues (+ Forge V) | M236–M290 |
| `06-era-vi-the-unseen-order.md` | VI — The Unseen Order (+ Forge VI) | M291–M345 |
| `07-era-vii-the-wide-world.md` | VII — The Wide World (+ Forge VII) | M346–M400 |
| `08-era-viii-the-proof.md` | VIII — The Proof (+ Forge VIII) | M401–M455 |
| `09-era-ix-the-sealed-instrument.md` | IX — The Sealed Instrument | M456–M515 |

## The gate

`../../scripts/roadmap-500-spec-check.sh` exits non-zero unless all
500 blocks M16–M515 are present exactly once, in ascending order
across the era files, each carrying all four fields at substantive
length, with no drafting stubs anywhere. It is the companion to
`../../scripts/roadmap-500-check.sh`, which holds the one-line layer.
