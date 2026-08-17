# Era VIII — The Proof (M401–M455)

Full four-field specs for Era VIII of `../ROADMAP-500.md`: the
calibration corpus and its evidence standards, historical envelopes
for demography, cities, prices, wars, and polities, property lanes and
the invariant census, the metamorphic lattice, fuzz and numeric
honesty, statistically honest bands, the oatmeal detector at full
strength, ablations that prove no system is dead, and the proof ledger
that indexes it all — closed by Forge VIII (M451–M455), which recasts
the harness itself. The one-liners in the parent file are binding;
these specs expand them.

### M401 — The Calibration Corpus
- **Intent:** Grounds every future proof in curated fact rather than vibes, giving Calliope a fixed body of historical numbers to check itself against.
- **Build:** Add a versioned `game/rust/data/calibration/` fixture set (life tables, urbanization series, price lists, war-duration tables) each stored as frozen TSV/JSON with a manifest recording source, edition, and retrieval date; write the evidence-standards ADR establishing what qualifies as a citable envelope and how fixtures are updated.
- **Touches:** new: game/rust/data/calibration/manifest.toml, new: docs/adr (evidence-standards ADR, numbered at land time), game/rust/src/lib.rs, docs/adr/README.md
- **Gate:** `diagnose calibration --list` enumerates every fixture with source, license, and checksum, and the harness fails if any fixture is missing its manifest entry or checksum mismatches.

### M402 — Envelope Framework
- **Intent:** Turns "does this match history" into a repeatable machine question by giving every calibration a named, executable envelope check.
- **Build:** Introduce an `Envelope` type in diagnose.rs wrapping a metric name, a fixture reference, and a tolerance band derived from the fixture's own reported variance, distinct from the tuning `Checks::band` used for design intent; wire a `diagnose envelope <name>` subcommand that loads a fixture, computes the simulated metric, and reports PASS/WARN/FAIL against the sourced envelope.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/util.rs, new: game/rust/src/bin/envelope.rs, game/rust/data/calibration/manifest.toml
- **Gate:** at least three envelopes (one demographic, one economic, one political) run end-to-end and report a banded verdict distinguishable in output from ordinary tuning bands.

### M403 — Provenance Discipline
- **Intent:** Protects the proof from rot by making every fixture's citation, license, and freeze date a checked property of the corpus, not a comment.
- **Build:** Extend the manifest schema with required `citation`, `license`, `frozen_at`, and `derivation_notes` fields; add a corpus-linter subcommand that fails on any fixture missing required metadata or whose checksum drifts from the frozen copy, and document the update procedure (new source supersedes, never edits in place) in the evidence-standards ADR.
- **Touches:** game/rust/data/calibration/manifest.toml, game/rust/src/bin/diagnose.rs, docs/adr (evidence-standards ADR from M401)
- **Gate:** `diagnose calibration --lint` exits nonzero on any fixture lacking citation/license/freeze metadata or on checksum drift, and passes clean on the current corpus.

### M404 — Demographic Proof
- **Intent:** Proves the population engine's age structure and growth behave like real pre-modern demography, not an arbitrary curve.
- **Build:** Compute simulated life-table quantities (life expectancy at birth, age-specific mortality, dependency ratio) from `society.rs` cohort state and check them against the medieval/early-modern life-table fixtures via the M402 envelope framework; add a `demography` diagnose subcommand reporting growth-rate envelopes across a full run.
- **Touches:** game/rust/src/society.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml, game/rust/src/systems.rs
- **Gate:** simulated life expectancy and crude growth rate fall inside their sourced envelopes across all three sweep seeds, reported PASS via `diagnose demography`.

### M405 — Urban Proof
- **Intent:** Confirms Calliope's cities grow and rank the way Bairoch-class historical urban series say cities must.
- **Build:** Extract simulated rank-size slope, urbanization share of total population, and city growth-rate distribution from `settlements.rs`, and check each against the corresponding calibration envelope; extend the existing Zipf check (diagnose.rs rank-size slope) into an envelope rather than a tuning band.
- **Touches:** game/rust/src/settlements.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** rank-size slope, urbanization share, and city growth rate each land inside their historical envelope across the multi-seed sweep, with no seed FAILing more than one metric.

### M406 — Famine and Plague Return-Times
- **Intent:** Tests whether Calliope's crisis cadence matches the historical rhythm of subsistence failure and epidemic recurrence, not just its magnitude.
- **Build:** Compute inter-event return-time distributions for famine and plague from the chronicle event log and compare their mean and coefficient of variation against historical return-time fixtures using the envelope framework; add return-time tracking to `famine.rs` and the plague path in `society.rs`.
- **Touches:** game/rust/src/famine.rs, game/rust/src/society.rs, game/rust/src/chronicle.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** famine and plague return-time means and CVs fall inside their sourced envelopes over a 150-year run across all sweep seeds.

### M407 — Price Ratios, Wages, and Trade Gradients
- **Intent:** Checks that Calliope's market prices reproduce the relative scarcity and wage ratios recorded in the Hodges/Goucher-class historical lists.
- **Build:** Derive simulated price ratios (grain:tool, wage:grain) and cross-settlement trade-gradient decay from `economy.rs` and `trade.rs`, and compare each against the corresponding fixture-derived envelope; extend the manifest with the wage/price fixture set.
- **Touches:** game/rust/src/economy.rs, game/rust/src/trade.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** simulated price ratios and trade-gradient decay constant fall inside sourced envelopes across the sweep, with envelope FAIL blocking `report.sh` full mode.

### M408 — Zipf, Gibrat, and Bettencourt at Full Depth
- **Intent:** Pushes the scaling-law proofs from spot checks to full-depth coverage across every seed and every era of the run.
- **Build:** Compute Zipf rank-size slope, Gibrat's-law variance-independent-of-size test, and Bettencourt infrastructure/output exponents (pop^0.85 / pop^1.15) continuously across all eras rather than at a single snapshot, storing per-era time series; add regression checks that the exponents hold steady rather than drifting with world age.
- **Touches:** game/rust/src/settlements.rs, game/rust/src/economy.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** Zipf slope, Gibrat variance-independence, and both Bettencourt exponents stay inside band at every checkpoint era across all sweep seeds, not just at final tick.

### M409 — Market Integration
- **Intent:** Verifies the literature's claim that better-connected settlements converge in price faster, turning a qualitative expectation into a measured slope.
- **Build:** Compute pairwise price convergence rate as a function of trade-graph distance/connectivity from `trade.rs` route data, fit the convergence-vs-connectivity relationship, and check its sign and magnitude against the literature-derived envelope; expose per-pair convergence half-life in a new report table.
- **Touches:** game/rust/src/trade.rs, game/rust/src/economy.rs, game/rust/src/bin/diagnose.rs
- **Gate:** convergence rate rises monotonically with connectivity across the sweep and the fitted slope sits inside its sourced envelope, checked as a metamorphic property not just a snapshot.

### M410 — War Frequency, Duration, and Casualty Distributions
- **Intent:** Grounds Calliope's wars in the empirical shape of real conflict, not an arbitrary Poisson process.
- **Build:** Extract simulated war inter-arrival times, duration distribution, and casualty-per-war distribution from `politics.rs`/chronicle war events, and check their shape (mean, tail heaviness) against Correlates-of-War-class envelopes; add a `diagnose wars` subcommand.
- **Touches:** game/rust/src/politics.rs, game/rust/src/chronicle.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** war frequency, duration, and casualty distribution moments fall inside sourced envelopes across all sweep seeds and a 150-year window.

### M411 — Polity Survival Curves and Imperial-Cycle Periods
- **Intent:** Tests whether Calliope's polities rise and fall on Turchin-scale cliodynamic cycles rather than an ahistorical decay curve.
- **Build:** Compute polity survival (Kaplan-Meier-style lifetime curve) and dominant imperial-cycle period via spectral analysis of territory-share time series from `politics.rs`, and check both against the Turchin-derived envelope; store cycle-period estimates per seed for regression tracking.
- **Touches:** game/rust/src/politics.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** median polity lifetime and dominant cycle period fall inside sourced envelopes across the sweep, with the spectral peak reproducible from stored seed data.

### M412 — Succession, Coup, and Secession Shares
- **Intent:** Confirms the mix of ways polities actually end in Calliope matches the historical ledger's balance of causes.
- **Build:** Classify every polity-ending event by cause (succession crisis, coup, secession, conquest) in `politics.rs`, tabulate their relative shares, and check the share vector against a historical-ledger envelope using a categorical distance metric.
- **Touches:** game/rust/src/politics.rs, game/rust/src/chronicle.rs, game/rust/src/bin/diagnose.rs, game/rust/data/calibration/manifest.toml
- **Gate:** categorical divergence between simulated and historical cause-shares stays under its sourced threshold across all sweep seeds.

### M413 — Property Lanes Over Every Subsystem
- **Intent:** Extends the M15 proptest program from its original scope to every era and subsystem so invariants hold under generated inputs everywhere, not just where first written.
- **Build:** Audit existing proptest coverage, then add property lanes for every subsystem lacking one (culture, artifact, prospecting, patina, telling) using `proptest` strategies seeded from the world RNG discipline, each asserting a structural invariant (e.g. artifact provenance chains acyclic, prospecting yields never negative).
- **Touches:** game/rust/src/culture.rs, game/rust/src/artifact.rs, game/rust/src/prospecting.rs, game/rust/src/patina.rs, game/rust/src/telling.rs, new: game/rust/tests/proptest_lanes.rs
- **Gate:** `cargo test --release proptest_lanes` passes 256+ generated cases per subsystem with zero shrinkable failures, and every subsystem file has at least one lane.

### M414 — Invariant Census
- **Intent:** Ends the practice of documented-but-unenforced guarantees by turning every written invariant into a machine check.
- **Build:** Grep every "must", "always", "never" guarantee documented across ADRs and module doc-comments into a tracked invariant registry, cross-reference each against an existing property/envelope/band check, and write new proptest or diagnose checks for any orphaned invariant found.
- **Touches:** new: game/rust/data/invariants.toml, game/rust/src/bin/diagnose.rs, docs/adr, game/rust/tests/proptest_lanes.rs
- **Gate:** `diagnose invariants --census` reports zero orphaned invariants, i.e. every registry entry links to a passing check.

### M415 — Coverage Metric
- **Intent:** Makes "how proven is the world" a single trending number instead of a felt impression.
- **Build:** Define system-under-proof coverage as the fraction of registered subsystems (per `invariants.toml` and the module list) with at least one envelope, one property lane, and one metamorphic check; compute and report it as a percentage in `diagnose coverage`, tracked release over release.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/data/invariants.toml, game/rust/scripts/report.sh
- **Gate:** `diagnose coverage` reports a numeric percentage and the harness fails the phase gate below 60% with an explicit target trajectory toward 100% recorded in the report.

### M416 — Metamorphic Lattice
- **Intent:** Catches regressions in the direction of a system's effect, the class of bug numeric bands alone cannot see.
- **Build:** Build a metamorphic-test table keyed by the causal-coupling pairs already documented in the research digests (rainfall↑ ⇒ river cells not↓, connectivity↑ ⇒ price convergence not slower), each entry perturbing one input and asserting the sign of the output delta across paired seeds.
- **Touches:** new: game/rust/src/bin/metamorphic.rs, game/rust/data/invariants.toml, game/rust/src/climate.rs, game/rust/src/hydrology.rs, game/rust/src/trade.rs
- **Gate:** every entry in the metamorphic table runs across the full seed sweep and reports the correct sign of effect with zero direction failures.

### M417 — Counterfactual Harness
- **Intent:** Bounds how much a single perturbed input is allowed to ripple, catching chaotic over-sensitivity before players see it.
- **Build:** Add a counterfactual runner that perturbs one input parameter (e.g. one seed's rainfall multiplier) while holding all derived RNG streams fixed, then measures divergence of downstream state hash and key metrics against an expected-divergence band from ADR-0003's stream-isolation guarantee.
- **Touches:** game/rust/src/bin/metamorphic.rs, game/rust/src/util.rs, game/rust/src/bin/diagnose.rs
- **Gate:** single-input perturbations produce downstream divergence within the declared expected-divergence band for every tested parameter, across all sweep seeds.

### M418 — Metamorphic Coverage Held in Bands
- **Intent:** Prevents the metamorphic lattice itself from silently shrinking as code changes, holding its coverage inside a maintained band.
- **Build:** Track the count and category breakdown of active metamorphic checks over time, comparing against a target-count band recorded in `report.sh`'s summary; flag any drop below the floor as a harness regression requiring explicit justification.
- **Touches:** game/rust/src/bin/metamorphic.rs, game/rust/scripts/report.sh, game/rust/data/invariants.toml
- **Gate:** metamorphic check count sits inside its declared band on every `report.sh` run, with any drop below the floor causing a FAIL in SUMMARY.txt.

### M419 — Fuzz Lanes
- **Intent:** Hardens the pack format and config surface against malformed or hostile input so corrupted data degrades gracefully, never crashes.
- **Build:** Add `cargo-fuzz` (or equivalent libFuzzer) targets for pack unpacking, config parsing, and truncated-archive recovery, exercising `pack.rs` decode paths against random and mutated byte streams; ensure every fuzz target returns a typed error rather than panicking on malformed input.
- **Touches:** new: game/rust/fuzz/fuzz_targets/unpack.rs, new: game/rust/fuzz/fuzz_targets/config.rs, game/rust/src/pack.rs, game/rust/src/state.rs
- **Gate:** each fuzz target runs 5-minute CI sessions with zero panics, zero unhandled aborts, and only typed decode errors on malformed input.

### M420 — Numeric Honesty
- **Intent:** Settles, by ADR, whether float determinism holds under NaN/inf/overflow stress, closing an open question the harness has quietly relied on.
- **Build:** Write the float-determinism-policy ADR (deny NaN propagation into state, saturate rather than overflow, document platform float-op restrictions from ADR-0003), then add sweep checks in diagnose.rs that inject extreme parameter values and assert no NaN/inf reaches the state hash.
- **Touches:** new: docs/adr (float-determinism-policy ADR, numbered at land time), game/rust/src/util.rs, game/rust/src/bin/diagnose.rs, docs/adr/README.md
- **Gate:** `diagnose numeric-sweep` runs boundary and overflow-inducing inputs across all subsystems and finds zero NaN/inf values reaching `hash_state`.

### M421 — Findings Ledger
- **Intent:** Ensures every bug the fuzz and numeric lanes find becomes a permanent tripwire instead of a one-off fix.
- **Build:** Add a findings ledger recording each fuzz/numeric crash's minimized input, root cause, and fix commit, and convert every entry into a permanent regression test replayed on every `report.sh` run.
- **Touches:** new: game/rust/data/findings-ledger.toml, game/rust/fuzz/fuzz_targets/unpack.rs, game/rust/tests/proptest_lanes.rs, game/rust/scripts/report.sh
- **Gate:** every ledger entry has a corresponding regression test that fails without its fix and passes with it, verified in CI on every run.

### M422 — Bands Earn Confidence Intervals
- **Intent:** Replaces point-estimate banding with statistically honest interval reporting, so a PASS means something under sampling noise.
- **Build:** Extend `Checks::band` to compute a bootstrap or normal-approximation confidence interval from the multi-seed sweep sample, report interval width alongside PASS/WARN/FAIL, and add multiple-comparison correction (Bonferroni or Benjamini-Hochberg) across the full check suite to control the family-wise false-positive rate.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/util.rs, game/rust/scripts/report.sh
- **Gate:** every reported band includes a confidence interval and the corrected significance threshold, and simulated false-positive rate under a null-perturbation run stays under 5% across 20 repeated sweeps.

### M423 — Sweep Design
- **Intent:** Sizes the seed count and run length so every statistical claim the harness makes has adequate power, not just adequate speed.
- **Build:** Run a power analysis for each headline envelope metric to determine minimum seed count and run length for a target effect size and power (0.8), and update `report.sh`'s full-mode sweep parameters accordingly, documenting the derivation in a new report section.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, new: docs/research/power-analysis-notes.md
- **Gate:** documented power analysis shows every headline envelope achieves at least 0.8 power at the chosen seed count, and `report.sh full` uses the derived seed count.

### M424 — The Honest Report
- **Intent:** Formalizes PASS/WARN/FAIL as a disciplined vocabulary and drives flakiness to zero so a report is trustworthy on first read.
- **Build:** Codify the PASS/WARN/FAIL semantics precisely (PASS = inside sweet band with CI, WARN = inside hard band or CI straddling sweet boundary, FAIL = outside hard band) in diagnose.rs, then run the full suite repeatedly to identify and eliminate any seed-order or timing-dependent flake sources.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, docs/adr/0009-diagnostics-harness-as-gate.md
- **Gate:** ten consecutive full-suite runs on identical seeds produce byte-identical SUMMARY.txt verdicts, i.e. flake rate zero.

### M425 — Oatmeal Detector at Full Strength
- **Intent:** Answers Compton's oatmeal problem head-on: do seeds feel different, structurally, at every layer of the generated world.
- **Build:** Extend the existing oatmeal-detector groundwork into a full structural-distinctiveness suite covering biome patch fragmentation, settlement-spacing entropy, route-topology Jaccard, and culture/naming divergence between seed pairs, aggregated into a single distinctiveness score per layer.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/biomes.rs, game/rust/src/settlements.rs, game/rust/src/culture.rs, new: game/rust/src/bin/oatmeal.rs
- **Gate:** `diagnose oatmeal --full` reports a per-layer distinctiveness score for every layer and every seed pair in the sweep, with none silently omitted.

### M426 — Typicality and Novelty as Axes
- **Intent:** Stops treating unusual worlds as bugs by giving the sweep a portfolio view where typical and novel worlds both have a legitimate place.
- **Build:** Plot each swept world on two axes — typicality (distance to sweep centroid across headline metrics) and novelty (distinctiveness score from M425) — and classify worlds into quadrants, flagging extremes for human review rather than failing them outright.
- **Touches:** game/rust/src/bin/oatmeal.rs, game/rust/src/bin/diagnose.rs, new: game/reports/portfolio.txt
- **Gate:** the sweep report plots every seed's typicality/novelty coordinates and extremes (beyond 2 sigma on either axis) are flagged, never causing a FAIL by themselves.

### M427 — Distinctiveness Floors
- **Intent:** Guarantees the sweep never collapses toward sameness by enforcing hard minimum and mean divergence requirements.
- **Build:** Set minimum-pairwise and mean distinctiveness floors per layer from M425's scores, calibrated against the current sweep's observed distribution, and wire them into diagnose.rs as hard-gate checks distinct from the portfolio's soft flagging.
- **Touches:** game/rust/src/bin/oatmeal.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** minimum pairwise distinctiveness and mean distinctiveness for every layer stay above their floors across the full sweep, FAILing the suite otherwise.

### M428 — The ERA Program Complete
- **Intent:** Completes expressive-range analysis for every era's systems so coverage gaps and bias clusters are visible, not assumed absent.
- **Build:** Generate 2D metric-pair histograms (land % × biome entropy, settlement count × route length, price dispersion × connectivity, and equivalents for every later-era system) exported per sweep as plate images or data tables, following Smith & Whitehead's expressive-range method; ensure every system introduced through Era VII has at least one ERA plate.
- **Touches:** game/rust/src/bin/diagnose.rs, new: game/rust/src/bin/era_plates.rs, game/rust/scripts/report.sh, new: game/reports/era-plates/
- **Gate:** `diagnose era-plates --all` emits a plate for every registered system with nonzero cell counts across the full metric-pair space and no system missing a plate.

### M429 — Cross-Era ERA Plates
- **Intent:** The era-by-era proofs so far test systems in isolation, but history's real signature is coupling, so this phase measures it directly.
- **Build:** Extend `cmd_era` and `era_metrics` into a cross-era mode that emits 2D metric-pair histograms spanning subsystem boundaries — climate variability index against GDP-per-capita proxy, faith adherence share against war frequency, drought years against famine incidence — reusing the Smith & Whitehead expressive-range method already wired for single-era plates and writing plate files per pair under the report tree.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/climate.rs, game/rust/src/economy.rs, game/rust/src/politics.rs, new: game/reports/era-cross
- **Gate:** `diagnose era-cross` emits at least six cross-era plates per sweep with non-degenerate bins (no plate collapses to a single occupied cell) and the plate set is stable byte-for-byte across repeat runs at fixed seeds.

### M430 — ERA Regression Diffing
- **Intent:** Expressive-range plates are only useful as proof if silent drift between engine versions gets caught, not just admired.
- **Build:** Add a plate-diff mode that loads two ERA plate snapshots (current and a stored baseline), computes per-bin occupancy delta and a distributional distance (earth-mover or chi-square over the histogram), and flags any pair whose divergence exceeds a tuned threshold as drift requiring a written justification.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, new: game/reports/era-baseline, new: docs/research/era-baselines.md
- **Gate:** `diagnose era-diff` against the stored baseline returns zero flagged plates on an unmodified build and flags exactly the plates touched by an injected synthetic regression in a harness self-test.

### M431 — Ablation Harness
- **Intent:** The instrument must prove every system it carries actually does something to the world, not merely occupies code.
- **Build:** Add per-system toggle flags threaded through `systems.rs` generation and tick dispatch (erosion, seasonal ITCZ shift, famine coupling, succession, market arbitrage, and so on), each toggle deterministically substituting a no-op or frozen-default in place of the real computation, and a `diagnose ablate` command that runs the standard sweep once per toggle and records the resulting metric fingerprint.
- **Touches:** game/rust/src/systems.rs, game/rust/src/bin/diagnose.rs, game/rust/src/constants.rs, new: game/reports/ablation
- **Gate:** every registered system toggle completes a full sweep without panic and produces a fingerprint recorded to `game/reports/ablation/<system>.txt`, with toggle wiring covered by a unit test asserting the no-op path is reachable.

### M432 — No Dead Systems
- **Intent:** An ablation harness is only proof once its results are read: nothing in the world may be inert scenery.
- **Build:** Compare each system's ablated fingerprint against the baseline fingerprint using the same metric set as the calibration envelopes (M401–M412), require a minimum measured effect size per system, and fail the sweep naming any system whose removal moves no tracked metric beyond noise floor.
- **Touches:** game/rust/src/bin/diagnose.rs, game/reports/ablation, docs/research/SYNTHESIS.md, new: docs/adr (system-effect-floors ADR, numbered at land time)
- **Gate:** `diagnose ablate --check` exits non-zero if any system's effect size falls below its declared floor across all seeds in the sweep, and the current mainline tree passes with zero dead systems.

### M433 — The Interaction Map
- **Intent:** The causal table in the docs has always been assertion; this phase turns which systems actually couple into a measurement.
- **Build:** Run pairwise ablations (both systems off, each alone, both on) across the standard seed sweep, compute interaction terms from the four-cell fingerprint deltas per tracked metric, and emit a system-by-system interaction matrix distinguishing additive, synergistic, and antagonistic couplings.
- **Touches:** game/rust/src/bin/diagnose.rs, game/reports/ablation, new: game/reports/interaction-map.txt, new: docs/research/interaction-map.md
- **Gate:** `diagnose interact` produces a complete N×N interaction matrix with no missing cells and every off-diagonal entry the documentation claims as coupled shows a nonzero measured interaction term.

### M434 — Noise-World Baselines
- **Intent:** Every claim of structure needs a null hypothesis it beats, or it is not a claim at all.
- **Build:** Generate matched noise worlds that preserve marginal statistics (elevation histogram, biome area shares, settlement counts) but scramble spatial and causal structure via shuffled placement and permuted event ordering, then run the full calibration and oatmeal metric suite against both real and noise worlds per seed.
- **Touches:** game/rust/src/world.rs, game/rust/src/bin/diagnose.rs, game/rust/src/bin/worldgen.rs, new: game/rust/src/nullworld.rs
- **Gate:** `diagnose null-baseline` shows Calliope worlds beating matched-noise worlds on every tracked structural metric by at least the declared minimum effect size, across the full seed sweep with zero exceptions.

### M435 — Anti-Oatmeal at World Scale
- **Intent:** Compton's oatmeal problem applies at every scale, and distinctiveness must be proven from hemisphere down to realm, not only between whole seeds.
- **Build:** Extend the existing seed-pair distinctiveness metrics (biome patch fragmentation, settlement-spacing entropy, feature-type Jaccard) to operate on hemisphere, continent, and realm-scale sub-regions extracted per seed, and set minimum-divergence floors at each scale calibrated from the seed-level oatmeal detector's existing bands.
- **Touches:** game/rust/src/geo.rs, game/rust/src/bin/diagnose.rs, game/rust/src/biomes.rs, new: game/reports/oatmeal-scales
- **Gate:** every hemisphere, continent, and realm pair in the sweep clears its scale-specific distinctiveness floor, with the harness reporting exact divergence scores per region pair.

### M436 — Novelty Cadence
- **Intent:** The marvelous — a truly singular settlement, war, or omen — must stay rare enough to mean something and common enough to be found.
- **Build:** Define a novelty score per chronicle-worthy entity or event using the existing typicality/novelty axes from M426, then track novelty incidence rate across the sweep and band it against a target cadence (roughly one standout per era per seed) derived from historical rarity of comparably distinctive real-world events.
- **Touches:** game/rust/src/chronicle.rs, game/rust/src/telling.rs, game/rust/src/bin/diagnose.rs, new: game/reports/novelty-cadence.txt
- **Gate:** novelty incidence across the full seed sweep falls within the declared per-era band on both ends, with zero seeds producing either zero novel entities or a novelty-saturated chronicle.

### M437 — The Proof Ledger
- **Intent:** Three years of scattered checks need one authoritative index or nobody, including the harness, can say what is actually proven.
- **Build:** Build a check registry data structure enumerating every calibration envelope, property, metamorphic relation, ablation floor, and ERA plate by stable numeric id, source citation, and current status, generated from the existing check call sites via a registration macro rather than hand-maintained.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/constants.rs, new: game/rust/src/checkreg.rs, new: docs/research/proof-ledger.md
- **Gate:** `diagnose ledger` enumerates every check currently executed by `report.sh` with a unique id and citation, and a harness self-test fails if any check call site lacks a registry entry.

### M438 — Coverage Gates
- **Intent:** The ledger is only a guardrail once it can block a landing that skipped its homework.
- **Build:** Define a coverage requirement per system (an envelope check, a property lane, a metamorphic relation, and an ERA plate entry) and add a `diagnose ledger --coverage` mode that cross-references the system list in `systems.rs` against the ledger, refusing to report full coverage until all four categories are present per system.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, game/rust/src/systems.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose ledger --coverage` reports one hundred percent four-category coverage across all systems registered in `systems.rs`, and the check is wired into `report.sh`'s exit status.

### M439 — The Ledger Browsable
- **Intent:** A proof nobody can browse is a proof nobody trusts; the ledger belongs in the UI beside the world it certifies.
- **Build:** Serialize the check registry to a JSON pack surface consumed by a new Solid.js panel listing each check with its citation, current PASS/WARN/FAIL status from the latest sweep, and a drill-down into the raw report text, following the existing vendored-no-build UI pattern.
- **Touches:** game/rust/src/pack.rs, game/rust/src/checkreg.rs, new: game/web/js/ui/ledger-panel.js
- **Gate:** the ledger panel renders every registry entry with correct live status against a fresh `report.sh` run and the panel's data round-trips through pack/unpack byte-identical.

### M440 — Replication Kit
- **Intent:** A claimed number is only proof if a stranger can reproduce it from nothing but a seed and a version tag.
- **Build:** Add a `diagnose replicate <check-id> <seed> <version>` command that regenerates exactly the world state and metric computation a ledger entry depends on, printing the reported number alongside the recomputed one, packaged with a version-pinned build script so the whole kit runs from a clean checkout.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, game/rust/scripts/report.sh, new: game/rust/scripts/replicate.sh
- **Gate:** `replicate.sh` reproduces the exact reported figure (bit-identical for hashes, within stated tolerance for floats) for every ledger entry, run from a fresh clone against a pinned commit.

### M441 — Format Stability
- **Intent:** A proof that only holds for the newest archive format is a proof with an expiration date the sim shouldn't have.
- **Build:** Extend the version-locked pack loader with a compatibility matrix asserting every archive produced since pack v2 (ADR-0016) still verifies and unpacks under the current engine, adding fixture archives per historical version and a loader shim path for superseded field layouts.
- **Touches:** game/rust/src/pack.rs, docs/adr/0016-pack-v2-quantized-crc-payload.md, new: game/rust/tests/fixtures/pack-v2, new: game/rust/tests/format_stability.rs
- **Gate:** `cargo test format_stability` verifies every stored historical-version fixture archive unpacks without error and reproduces its recorded state hash under the current engine.

### M442 — Standing Calibration Lanes
- **Intent:** A calibration checked once and forgotten is a calibration that silently rots as the engine changes underneath it.
- **Build:** Promote every M401–M412 calibration envelope from a manually invoked check into a standing lane executed on every `report.sh full` run, with results appended to the historical trend store rather than only the current report.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, new: game/reports/trend-history
- **Gate:** every calibration envelope from M401–M412 executes on each `report.sh full` invocation and appends a timestamped row to the trend store with zero manual invocation required.

### M443 — Performance Proof
- **Intent:** Speed claims deserve the same rigor as demographic claims: a budget without a band is an anecdote.
- **Build:** Convert the existing generation/tick/memory/payload budgets into banded checks with recorded history, tracking wall-clock and allocation counts (via the `alloc-count` feature) per phase across engine versions and flagging regressions beyond a tolerance band rather than a single hard ceiling.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/reports/bench-history.txt, new: docs/research/performance-bands.md
- **Gate:** `diagnose bench --history` bands current wall-clock and allocation figures against the stored trend and fails only when a metric exits its tolerance band, verified stable across three consecutive clean runs.

### M444 — Hunt the Unfalsifiable
- **Intent:** A check that can never fail is not a proof, it is decoration wearing a proof's clothes.
- **Build:** Audit the full check registry for tautological or unreachable failure conditions (bands wide enough to admit anything, assertions on constants, dead code paths), strengthen each with a tighter band or reachable counterexample, or remove and record the removal with reasoning.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, new: docs/research/unfalsifiable-audit.md
- **Gate:** the audit document lists every check inspected with a disposition (strengthened, removed, retained-with-justification), and a harness self-test mutation-injects a failure into each retained check to confirm it can actually fail.

### M445 — Red-Team Sweeps
- **Intent:** The proof lattice must survive someone actively trying to break it, not just the seeds it was tuned against.
- **Build:** Generate an adversarial seed and config corpus targeting known-fragile boundaries (extreme aridity, maximal war density, degenerate small worlds, boundary-value config overrides) and run the full check suite against it, treating any silent pass-through of a broken invariant as a harness bug rather than a world quirk.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/world.rs, new: game/rust/src/redteam.rs, new: game/reports/redteam
- **Gate:** `diagnose redteam` runs the full check suite against at least twenty adversarial configurations with zero silent invariant violations and every genuine failure landing a findings-ledger entry per M421's precedent.

### M446 — Documented Proof Methods
- **Intent:** A check nobody can read the method of is a check nobody outside the codebase can trust.
- **Build:** Write replication-grade method documentation for every ledger entry — data source, statistical test, sample size, tolerance derivation — cross-referenced from the check registry so `diagnose ledger` can print a method summary alongside status.
- **Touches:** game/rust/src/checkreg.rs, docs/research, new: docs/research/proof-methods.md
- **Gate:** `diagnose ledger --methods` prints a non-empty, citation-bearing method description for every registry entry, and a lint checks no entry is left with an empty method field.

### M447 — The Proof Synthesis
- **Intent:** The scattered ledger, envelopes, and bands need one document stating plainly what is proven, at what strength, and on what evidence.
- **Build:** Compile a synthesis document walking every proof category (demographic, economic, political, property, metamorphic, ablation, ERA, oatmeal) with its strongest and weakest supporting evidence, generated in part from the check registry's citation and status fields to keep it honest as the ledger changes.
- **Touches:** game/rust/src/checkreg.rs, docs/research/SYNTHESIS.md, new: docs/research/PROOF-SYNTHESIS.md
- **Gate:** `PROOF-SYNTHESIS.md` names every ledger category with at least one cited check per category, and a script diffing it against the live registry reports zero uncited categories.

### M448 — Suite Wall-Clock in Band
- **Intent:** A proof lattice too slow to run stops being run, and an unrun proof is not a proof.
- **Build:** Profile the full `report.sh full` run end to end, identify and parallelize or cache the slowest lanes (sweep generation, ERA plates, ablation matrix), and set a banded wall-clock budget for the whole suite consistent with the forge charter's "growing suite stays fast" mandate.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, game/reports/bench-history.txt
- **Gate:** `report.sh full` completes within its declared wall-clock band on the reference machine across three consecutive runs, with per-lane timing recorded in the bench history.

### M449 — `diagnose proof`
- **Intent:** The check of checks: one command that answers whether the instrument, as a whole, still proves what it claims.
- **Build:** Add a `diagnose proof` command aggregating ledger coverage, calibration envelope status, property and metamorphic lane results, ablation floors, ERA regression, and performance bands into a single PASS/WARN/FAIL verdict, wired as the final stage of `report.sh`.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/checkreg.rs
- **Gate:** `report.sh full` ends with a single `diagnose proof` verdict line, PASS only when every constituent category is green, and the exit code propagates to CI.

### M450 — Era VIII Gate: The Proof Lattice
- **Intent:** The era closes when the instrument can prove, end to end, that it is the definitive simulator it claims to be.
- **Build:** Run the complete proof lattice — every calibration envelope, property lane, metamorphic relation, ablation floor, ERA plate, oatmeal floor, and performance band — across the full seed sweep as the era's closing verification, with no manual overrides permitted in the gate path.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/checkreg.rs, game/reports
- **Gate:** `diagnose proof` returns PASS with full lattice coverage and every envelope green across all seeds in the standard sweep, recorded as the Era VIII closing report.

### M451 — Harness Architecture Re-Cut
- **Intent:** Three years of accretion left the harness as hand-written call sites; the forge recasts it as generated code with the registry as source of truth.
- **Build:** Rework `diagnose.rs`'s check dispatch so runners are generated from the check registry's declarations rather than hand-invoked per subcommand, following the E-track registry-codegen discipline (ADR-0015), with hindsight ADRs recorded for any check-shape changes this forces.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, docs/adr/0015-registry-codegen-architecture.md, new: docs/adr (harness-codegen-recast ADR, numbered at land time)
- **Gate:** determinism hash is unchanged across the recast, `report.sh full` reproduces identical PASS/WARN/FAIL verdicts pre- and post-refactor, and full suite is green.

### M452 — Report Formats and Trend Store
- **Intent:** Text reports have served the harness well but a machine-readable format is what lets the ledger, UI, and CI actually consume results.
- **Build:** Emit a structured (JSON or equivalent typed binary) report format alongside the existing text reports, backed by a historical trend store that appends every run's per-check results keyed by commit and timestamp, feeding both the ledger UI panel and the performance-band history.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, new: game/rust/src/reportfmt.rs, game/reports/trend-history
- **Gate:** every `report.sh` run emits a structured report validating against a fixed schema, and the trend store round-trips a full run's results without data loss, verified by a read-back test.

### M453 — Incremental Checking
- **Intent:** The full suite is slow because it always re-runs everything; the forge teaches it which lanes a change actually touches.
- **Build:** Map each check registry entry to the source paths and systems it exercises, then add a `report.sh --affected <diff>` mode that runs only the lanes whose dependency set overlaps a given changeset, falling back to the full suite when the mapping is ambiguous.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/checkreg.rs, game/rust/src/bin/diagnose.rs, new: game/rust/scripts/affected-lanes.sh
- **Gate:** `report.sh --affected` run against a synthetic single-module diff executes strictly the mapped subset of lanes and its verdict matches the full-suite verdict for that changeset in a harness self-test.

### M454 — CI-Grade Gating
- **Intent:** The forge closes the loop: every landing must be checked by its affected lanes before it can be called landed, not merely reported on afterward.
- **Build:** Wire the affected-lanes runner into a landing-gate script enforcing budget bands and check verdicts as hard pass/fail conditions, with a documented escape hatch requiring explicit written justification for any override.
- **Touches:** game/rust/scripts/report.sh, game/rust/scripts/affected-lanes.sh, new: game/rust/scripts/ci-gate.sh, new: docs/adr (ci-grade-gating ADR, numbered at land time)
- **Gate:** `ci-gate.sh` blocks a synthetic landing with an injected budget or verdict regression and passes a clean synthetic landing, with zero unlogged overrides possible.

### M455 — The Ledger Cleared
- **Intent:** The forge charter's fifth question closes the era: every debt the harness accrued gets landed now or rejected on the record.
- **Build:** Enumerate every refactor and cleanup deferred during Era VIII's proof work (queued in code comments, ADR follow-ups, and the unfalsifiable-audit's retained items), land the ones worth landing, and write explicit rejection rationale for the rest in a closing ledger document.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/checkreg.rs, docs/research/unfalsifiable-audit.md, new: docs/research/forge-viii-ledger.md
- **Gate:** determinism hash is unchanged through the forge's refactors, budgets green, full suite green, and `forge-viii-ledger.md` accounts for every deferred item with a landed-or-rejected disposition.
