# Era II — The Long Sky (M71–M125)

Full four-field specs for Era II of `../ROADMAP-500.md`: interannual
variability, oscillation modes and teleconnections, storm tracks and
tropical cyclones, drought and flood as dated events, century-scale
drift and the named ages, the weather of history coupled into famine,
migration, and war — closed by Forge II (M121–M125). The one-liners in
the parent file are binding; these specs expand them.

### M71 — The Year Stops Repeating
- **Intent:** Give every simulated year its own weather instead of the climate mean, so no two harvests taste the same.
- **Build:** Add a per-year, per-cell anomaly field over temperature and precipitation, drawn from a seeded fbm keyed on `(x, y, year)` in the spirit of `famine.rs`'s existing drought noise, with amplitude scaled by `lat_deg` — polar cells swing wider than tropical ones, matching the digest's latitude-shaped variance; anomalies are additive on `tmean`/`precip` before any downstream read.
- **Touches:** game/rust/src/climate.rs, game/rust/src/world.rs, game/rust/src/state.rs, game/rust/src/constants.rs
- **Gate:** `diagnose climate` reports annual anomaly standard deviation rising monotonically with `|lat|` across three latitude bands, and repeated runs at one seed reproduce identical per-year anomaly grids bit-for-bit.

### M72 — The Year That Was
- **Intent:** Let harvests, river discharge, and pasture react to the actual weather a given year delivered, not the long-run average.
- **Build:** Thread the M71 anomaly field through `famine_pass`, `hydrology::hydrology`'s discharge computation, and the pastoral fertility term in `agriculture.rs`, replacing mean-climate lookups with mean-plus-anomaly lookups at the settlement's year and cell; discharge gets a monthly anomaly-driven multiplier bounded to keep the DAG's flow ordering unchanged.
- **Touches:** game/rust/src/famine.rs, game/rust/src/hydrology.rs, game/rust/src/agriculture.rs, game/rust/src/world.rs
- **Gate:** `diagnose civ` shows harvest and discharge variance tracking the anomaly field's variance within 15%, with river ordering and endorheic classification unchanged from Era I baselines.

### M73 — Variability Held in Bands
- **Intent:** Pin the interannual noise to believable, checkable magnitudes so weather feels lively but never becomes chaos.
- **Build:** Add a `climate-variance` diagnostic pass computing per-latitude-band anomaly variance and cross-seed spread, checked against sweet/hard bands derived from the Budyko-Sellers-consistent scaling in the climate digest; fold the anomaly field's seed-derived values into `hash_state`.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/state.rs, game/rust/scripts/report.sh
- **Gate:** `diagnose climate-variance` passes on all sweep seeds with tropical anomaly stdev under 1.5°C and polar under 4°C, and `diagnose determinism` remains hash-stable with the new field folded in.

### M74 — The Seesaw Seas
- **Intent:** Introduce a slow ocean-atmosphere oscillation whose swings, not chance, drive multi-year climate fortune.
- **Build:** New oscillation module implements a single sinusoidal-plus-noise mode (ENSO-class) with period drawn once per seed from a 2–7 year band and amplitude from a bounded distribution, producing a scalar phase series indexed by month; the phase feeds an index field analogous to the Southern Oscillation Index for later teleconnection use.
- **Touches:** new: game/rust/src/oscillation.rs, game/rust/src/world.rs, game/rust/src/state.rs
- **Gate:** `diagnose oscillation` (new) confirms period lands in the 24–84 month band and amplitude stays within its configured envelope across all sweep seeds, hash-stable at fixed seed.

### M75 — The Tilted Belts
- **Intent:** Make the oscillation's phase reach across the world, tilting rain belts on the far side the way real teleconnections do.
- **Build:** Couple the oscillation phase into `climate.rs`'s precipitation pass as a hemisphere-asymmetric bias term added after the ITCZ gaussian, strengthening or weakening trade-wind moisture delivery on a lag proportional to the mode's current phase sign, consistent with the wind-belt model already in `climate.rs`.
- **Touches:** game/rust/src/climate.rs, game/rust/src/oscillation.rs
- **Gate:** `diagnose climate` shows rainfall correlation between opposite hemispheres' trade belts exceeding 0.3 in magnitude and flipping sign with oscillation phase across a full period.

### M76 — Reading the Seesaw
- **Intent:** Prove the oscillation behaves like a real quasi-periodic mode rather than decorative sine noise.
- **Build:** Add spectral analysis (discrete Fourier peak detection) and phase-lock statistics to a new diagnostics pass, checking the dominant period against the configured band and confirming teleconnection lag statistics are stable across seeds.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/oscillation.rs
- **Gate:** `diagnose oscillation` finds a single dominant spectral peak inside the 24–84 month band with power at least 3x the noise floor, on every sweep seed.

### M77 — The Storm Corridors
- **Intent:** Give the westerlies their storms, so mid-latitude coasts live under recurring, mappable cyclone tracks.
- **Build:** New storms module generates deterministic cyclone genesis points along the mid-latitude baroclinic zone and advects them downwind through the existing westerly wind field from `climate.rs`, producing dated track polylines with intensity decaying over land, seeded per year per hemisphere.
- **Touches:** new: game/rust/src/storms.rs, game/rust/src/climate.rs, game/rust/src/world.rs
- **Gate:** `diagnose climate` reports storm genesis density peaking within the 30–60° latitude band and track counts per century matching the region's coastline length within a factor of 2, hash-stable at fixed seed.

### M78 — Warm-Sea Fury
- **Intent:** Let tropical cyclones spin up over warm seas and curve ashore as dated, named disasters.
- **Build:** Extend `storms.rs` with a genesis rule gated on sea-surface temperature above a threshold (reading `tmean` over water), a curving track model bending poleward with distance from the equator, and a landfall event emitted through `event.rs`'s `Disaster` kind with dated month, path, and peak intensity.
- **Touches:** game/rust/src/storms.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose climate` shows tropical genesis confined to sea cells with `tmean` ≥ 26°C and zero genesis poleward of 30°, with landfall events appearing in the chronicle at a rate of 0.5–3 per century per coastal region.

### M79 — Coasts That Remember
- **Intent:** Make storms cost something the world carries forward — scattered fleets, wrecked harbors, a chronicle that does not forget.
- **Build:** Wire storm landfall events into `trade.rs`'s route viability (temporary sea-lane closure and cargo loss) and `settlements.rs`'s harbor state (damage flag reducing trade capacity for a recovery window), with matching chronicle entries and a ruin marker when a coastal settlement's damage exceeds a collapse threshold.
- **Touches:** game/rust/src/storms.rs, game/rust/src/trade.rs, game/rust/src/settlements.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** `diagnose economy` shows trade volume dips measurably in the month following a landfall event and recovers within the configured window on at least 90% of recorded storms, hash-stable.

### M80 — The Failed Year Named
- **Intent:** Turn drought from a single bad harvest roll into a multi-year event with a shape, a footprint, and a name history remembers.
- **Build:** Extend the drought field already used in `famine.rs` into a persistence model — once a cell's drought index crosses threshold it decays slowly across subsequent years rather than resetting, and a drought event spanning its worst-hit region and duration is emitted through `event.rs` and named via `chronicle.rs`'s naming bank.
- **Touches:** game/rust/src/famine.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs, game/rust/src/naming.rs
- **Gate:** `diagnose civ` shows drought events spanning a median of 2–5 consecutive years with a stable mapped extent, each surfaced once in the chronicle by name, reproducible at fixed seed.

### M81 — The River That Drowns and Gives
- **Intent:** Let good years turn dangerous — spates that drown the levees but leave richer fields behind.
- **Build:** Add a flood-year branch to the M72 discharge anomaly: when monthly discharge exceeds a settlement's levee-adjusted capacity, emit a flood event through `event.rs` that temporarily damages riverside population and, in the same stroke, boosts `fertility` in `agriculture.rs` for the following growing season via a silt bonus.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/agriculture.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose hydro` shows flood events firing on high-discharge-anomaly years only, with the subsequent season's fertility measurably above baseline on flooded cells and population damage bounded to a documented cap.

### M82 — Calibrated Against the Past
- **Intent:** Make sure droughts and floods strike at rates the paleoclimate record would recognize, not fantasy frequencies.
- **Build:** Add a return-time diagnostic computing empirical drought and flood recurrence intervals per climate zone and comparing them against literature-sourced Earth envelopes (multi-decade droughts rare, annual minor floods common), reporting deviation as a pass/warn/fail band.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/famine.rs, game/rust/src/hydrology.rs
- **Gate:** `diagnose civ` return-time table lands inside the configured Earth-analog envelope for at least 90% of climate zones across all sweep seeds.

### M83 — The Slow Drift
- **Intent:** Give each run its own multi-century climate history instead of a temperature that never moves.
- **Build:** Introduce a bounded secular drift curve — a slow random walk with reflecting bounds around the world's baseline `tmean`, seeded once at generation and evaluated per year — added as a global offset ahead of the M71 anomaly and M74 oscillation terms in `climate.rs`'s temperature pipeline.
- **Touches:** game/rust/src/climate.rs, game/rust/src/world.rs, game/rust/src/state.rs
- **Gate:** `diagnose climate` confirms the drift curve stays within its configured ±3°C excursion bound over a 1000-year run and its long-run mean sits within 0.2°C of baseline, hash-stable at fixed seed.

### M84 — Belts on the Move
- **Intent:** Let the wandering temperature drag rain belts and storm corridors with it, so ages have their own geography of weather.
- **Build:** Couple the M83 drift value into `climate.rs`'s ITCZ latitude offset and into `storms.rs`'s baroclinic-zone latitude, shifting both proportionally to drift magnitude so cold drift narrows the tropics and pulls storm tracks equatorward.
- **Touches:** game/rust/src/climate.rs, game/rust/src/storms.rs
- **Gate:** `diagnose climate` shows ITCZ latitude and storm-genesis band both shifting in the same direction as drift sign, with shift magnitude bounded to under 5° at maximum drift.

### M85 — No Runaway
- **Intent:** Prove the drift is drama, not doom — bounded excursion, stationary mean, no climate that spirals away.
- **Build:** Add a long-run drift diagnostic sampling temperature at 100-year intervals across a millennium-length synthetic run and checking excursion bounds, mean stationarity, and absence of monotonic trend beyond the configured envelope.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/climate.rs
- **Gate:** `diagnose climate` millennium sample shows global mean temperature drift under 0.5°C over the full run and zero excursions beyond the ±3°C bound on any sweep seed.

### M86 — The Cold Ages
- **Intent:** Let the world suffer real winters that last generations, arriving and receding on their own dated arcs.
- **Build:** New ages module layers a multidecadal cold-age process on top of the M83 drift — a Markov-style onset/release model with mean duration and depth drawn from a bounded distribution, dated at generation and named on onset through `chronicle.rs`; while active it deepens the global temperature offset beyond drift alone.
- **Touches:** new: game/rust/src/ages.rs, game/rust/src/climate.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** `diagnose climate` reports cold-age onsets with duration in the 20–80 year band and at most one active per world at a time, dated events appearing once each in the chronicle, hash-stable at fixed seed.

### M87 — The Generous Centuries
- **Intent:** Balance the cold ages with warm optima that open the uplands and reward the ages of plenty.
- **Build:** Extend `ages.rs`'s Markov model with a symmetric warm-optimum state, raising the effective growing-season temperature and, through `agriculture.rs`'s fertility term, upland cell productivity during its span, with onset and release dated the same way as cold ages.
- **Touches:** game/rust/src/ages.rs, game/rust/src/agriculture.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose civ` shows upland-cell fertility rising measurably during recorded warm optima and returning to baseline within one year of release, on every sweep seed.

### M88 — The Named Ages
- **Intent:** Let the chronicle christen the ages the way real history remembers its Little Ice Ages and Medieval Warm Periods.
- **Build:** Add an age-naming bank to `naming.rs` and wire `ages.rs` onset events to draw a unique name per age instance (Long Winter, Wine Years, and siblings), surfaced through `chronicle.rs` and persisted for later reference by other systems.
- **Touches:** game/rust/src/naming.rs, game/rust/src/ages.rs, game/rust/src/chronicle.rs
- **Gate:** every recorded age instance in a run carries a unique chronicle name with no repeats within a single world, reproducible at fixed seed.

### M89 — Margins on the Move
- **Intent:** Make treeline, snowline, and pack ice breathe with the ages instead of sitting fixed at generation.
- **Build:** Derive dynamic treeline and snowline latitude/altitude thresholds in `biomes.rs` from the current age state and drift offset, and extend polar sea-ice extent in `climate.rs` to expand during cold ages and retreat during warm optima, all computed per tick rather than baked at generation.
- **Touches:** game/rust/src/biomes.rs, game/rust/src/climate.rs, game/rust/src/ages.rs
- **Gate:** `diagnose climate` shows snowline altitude dropping during active cold ages and rising during warm optima by an amount proportional to age depth, within a bounded band, hash-stable.

### M90 — Fields at the Edge
- **Intent:** Let farmers gamble on the margins — upland and northern fields that open in good centuries and starve in bad ones.
- **Build:** Extend `crop_packages` in `agriculture.rs` to admit marginal cells (near the M89 treeline/snowline threshold) as farmable only when the current age state favors them, reverting to wildland or pastoral otherwise, with `famine.rs` treating a marginal field's reversion as a forced abandonment event.
- **Touches:** game/rust/src/agriculture.rs, game/rust/src/ages.rs, game/rust/src/famine.rs, game/rust/src/event.rs
- **Gate:** `diagnose civ` shows marginal-cell farmed area expanding during warm optima and contracting to zero during cold-age peaks, with abandonment events dated to age onset within one year.

### M91 — The Ice Remembers Time
- **Intent:** Bring Era I's glaciers into the flow of history, advancing and retreating with the ages rather than standing as generation-time relics.
- **Build:** Extend `erosion.rs`'s glacial mask with a per-tick advance/retreat term driven by the current age depth, moving the ice edge along the existing elevation-gradient logic without touching the immutable base terrain, and recording extent snapshots for the atlas.
- **Touches:** game/rust/src/erosion.rs, game/rust/src/geo.rs, game/rust/src/ages.rs, game/rust/src/world.rs
- **Gate:** `diagnose climate` shows glacial extent correlating with age depth (expanding in cold ages, contracting in warm optima) while base terrain height hashes remain unchanged across the run.

### M92 — Monsoon Fortune
- **Intent:** Make the monsoon a creature of drift and mode, so its failure years strike the paddies for a reason the sky can explain.
- **Build:** Couple the dynamic-ITCZ monsoon strength in `climate.rs` to both the M83 drift offset and the M74 oscillation phase, producing a monsoon-strength index per year and region; wire a failed-monsoon threshold into `famine.rs` so rice paddies (previously immune to rain-fed famine) can fail in matching years.
- **Touches:** game/rust/src/climate.rs, game/rust/src/oscillation.rs, game/rust/src/ages.rs, game/rust/src/famine.rs
- **Gate:** `diagnose civ` shows rice-paddy famine events firing only in years where the monsoon-strength index falls below threshold, at a rate consistent with the calibrated return-time band from M82.

### M93 — Lakes That Breathe
- **Intent:** Give endorheic basins a pulse — shores that rise in wet centuries and shrink in dry ones, leaving a strandline history behind.
- **Build:** Extend `hydrology.rs`'s endorheic-sink water balance to track a running lake-level state updated per year from inflow minus evaporation under the current climate state, and emit a dated strandline event through `event.rs` whenever the level crosses a recorded historical extreme.
- **Touches:** game/rust/src/hydrology.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs, game/rust/src/state.rs
- **Gate:** `diagnose hydro` shows endorheic lake levels tracking regional wet/dry years with a lag under two years, and strandline events dated and non-duplicated across a run.

### M94 — The Dry Edge
- **Intent:** Let the desert margin itself become an event — steppe creeping in, oases failing, with a mapped footprint and a date.
- **Build:** Add a steppe-encroachment detector comparing `biomes.rs` classification drift year over year against the pastoral aridity boundary from the ecology digest (below ~300 mm without irrigation), emitting a dated, extent-mapped event through `event.rs` when encroachment or oasis failure crosses a sustained threshold.
- **Touches:** game/rust/src/biomes.rs, game/rust/src/agriculture.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose civ` shows steppe-encroachment events firing only where precipitation has dropped below the 300 mm pastoral threshold for a sustained multi-year span, hash-stable at fixed seed.

### M95 — Hunger With a Cause
- **Intent:** Ground the harvest verdict in the sky it actually got, replacing dice with meteorology the player can trace.
- **Build:** Rewrite `famine_pass`'s drought check to read the M71–M92 composite anomaly (base drought field plus interannual anomaly, drift, oscillation, and monsoon terms) instead of its standalone fbm draw, so a famine's cause is now the same number `diagnose climate` reports for that year and cell.
- **Touches:** game/rust/src/famine.rs, game/rust/src/climate.rs, game/rust/src/explain.rs
- **Gate:** `diagnose civ` shows every famine event's recorded shortfall matching the corresponding cell-year's composite climate anomaly within rounding tolerance, on all sweep seeds.

### M96 — Granaries Against Lean Years
- **Intent:** Let towns that learned to store grain blunt the failed years, rewarding the storage techs with survival.
- **Build:** Extend `famine.rs`'s existing pottery-gated granary discount into a running per-settlement grain-reserve stock that accrues a fraction of surplus in fat years (via `agriculture.rs`'s fertility surplus) and is drawn down automatically during a recorded famine, gated by storage-tier techs in `society.rs`.
- **Touches:** game/rust/src/famine.rs, game/rust/src/agriculture.rs, game/rust/src/society.rs, game/rust/src/settlements.rs
- **Gate:** `diagnose civ` shows settlements with higher storage tech suffering measurably smaller population loss than untiered settlements under matched-shortfall famine years, on every sweep seed.

### M97 — Famine, Recalibrated
- **Intent:** Tune the whole hunger cadence — from drought to granary — against the historical record instead of gut feel.
- **Build:** Extend the M82 return-time diagnostic to famine frequency and severity specifically, checking event rate per settlement-century and mean population loss per famine against literature-derived pre-industrial famine cadences (Clark 2007-class density bands), adjusting `famine.rs` thresholds until the bands hold.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/famine.rs
- **Gate:** `diagnose civ` famine-cadence table lands inside its configured historical-envelope band on at least 90% of sweep seeds, with granary-tier settlements showing a distinct, smaller band.

### M98 — Off the Failing Margin
- **Intent:** When the sky fails a generation, send its people down the roads — climate migration with a date and a direction.
- **Build:** New migration module tracks each settlement's rolling decade-scale climate-shortfall average (drawing on the M95 composite anomaly and M90 marginal-field abandonment) and, when a sustained failed-decade threshold is crossed, redirects a fraction of population toward the nearest viable settlement of matching culture using the kin-town search already built for `famine.rs`, emitting a dated pulse event through `event.rs`.
- **Touches:** new: game/rust/src/migration.rs, game/rust/src/famine.rs, game/rust/src/settlements.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs
- **Gate:** `diagnose civ` shows migration pulses firing only after a sustained failed-decade climate average crosses threshold, arriving at destination settlements within the same recorded month, reproducible at fixed seed.

### M99 — Steppe Pressure
- **Intent:** Pastoral ranges are not fixed pasture but a moving grass-line, and the peoples who follow it now collide with farmers who cannot move at all.
- **Build:** Derive a herding-range field from the century drift curve and interannual rain anomaly (M71, M83): grass quality shifts the pastoral carrying capacity per cell, and a deterministic pressure score pushes nomadic settlement footprints outward in cold/dry decades and lets them retract in wet ones; where an advancing range overlaps a sedentary farm cell, raise the unrest ladder (society.rs) and log a `frontier-pressure` chronicle motif distinct from the M98 migration pulse.
- **Touches:** game/rust/src/climate.rs, game/rust/src/society.rs, game/rust/src/settlements.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs, new: game/rust/src/steppe.rs
- **Gate:** `diagnose sweep` shows pastoral range area tracking the drift curve within ±10% of the temperature-drift sign across a 300-year run, and frontier-pressure events never fire on seeds with flat drift (metamorphic check), determinism hash includes the new range field.

### M100 — Migration Diagnostics
- **Intent:** The climate-driven exodus of M98–M99 must prove itself statistical, not anecdotal — pulses belong to cold ages, not to noise.
- **Build:** Add a `diagnose sky migration` report that bins migration-pulse counts and displaced-population totals by climate-age phase (cold arc, warm optimum, neutral) across a multi-seed sweep, and assert pulse density is significantly higher inside cold arcs than the run-wide baseline; extend the property suite with a metamorphic check that lengthening a cold arc cannot lower its pulse count.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/famine.rs, game/rust/src/steppe.rs, game/rust/scripts/report.sh
- **Gate:** across a 20-seed sweep, cold-arc pulse density exceeds neutral-phase density on at least 17/20 seeds and the metamorphic lengthening check holds on every seed, `report.sh` records the new band.

### M101 — Campaign Seasons
- **Intent:** Armies now answer to mud and frost, so war stops being a year-round abstraction and starts keeping the calendar the farms already keep.
- **Build:** Gate active campaign months per region on month_precip and month_temperature (climate.rs): deep winter and peak wet-season months suspend offensive politics actions and pause active sieges, with a deterministic per-region campaign-window derived once per year from the climate series; sieges already underway lose progress at a rate tied to the besieging force's supply exposure to the bad month.
- **Touches:** game/rust/src/politics.rs, game/rust/src/climate.rs, game/rust/src/society.rs, new: game/rust/src/campaign.rs
- **Gate:** no offensive action or siege-progress tick fires inside a region's closed campaign months on any seed in a 50-seed sweep, and disabling the campaign gate (test-only) restores prior war cadence bit-for-bit, confirming the gate is additive.

### M102 — The Hungry Sword
- **Intent:** A lean year does not just starve a town, it hardens the temper of everyone who survives it, and that temper feeds directly into revolt.
- **Build:** Wire the famine severity computed in famine_pass (famine.rs) into the existing unrest ladder in society.rs as a bounded per-settlement opinion penalty scaled by consecutive lean years, decaying over a multi-year half-life once grain recovers; unrest crossing the existing revolt threshold during a lean stretch is tagged with a `hunger` cause in the chronicle rather than the generic unrest cause.
- **Touches:** game/rust/src/famine.rs, game/rust/src/society.rs, game/rust/src/politics.rs, game/rust/src/chronicle.rs
- **Gate:** metamorphic check confirms doubling consecutive lean years never lowers cumulative unrest penalty on any seed, hunger-tagged revolts are absent from every seed with famine disabled, determinism hash covers the new opinion term.

### M103 — Weather Turns Battles
- **Intent:** Storms and winters are not backdrop — they scatter fleets and break sieges, and the chronicle should say exactly which storm did it.
- **Build:** Cross-reference active storm-track and tropical-cyclone events (M77–M78) and winter campaign closures (M101) against in-flight fleet-movement and siege state: a fleet caught in a storm's path loses vessels by a deterministic function of storm intensity and route exposure, and a besieging army caught by an early hard winter breaks camp; both outcomes emit chronicle events naming the specific weather event as cause via a cross-reference id.
- **Touches:** game/rust/src/trade.rs, game/rust/src/politics.rs, game/rust/src/campaign.rs, game/rust/src/chronicle.rs, game/rust/src/event.rs
- **Gate:** every storm-caused fleet loss and camp-break chronicle entry carries a resolvable cross-reference to a real storm/winter event id, and disabling storm generation reduces such entries to zero on every seed in the sweep.

### M104 — Weather Enters the Record
- **Intent:** History remembers not just outcomes but the sky that caused them, so the sifter must expose weather as first-class dated fact.
- **Build:** Promote drought, flood, storm, and cold/warm-age transitions to sifter-visible chronicle events carrying region, extent polygon or bounding cells, severity, and duration fields, following the closed EventKind vocabulary discipline (event.rs); the chronicle UI's sifter (game/web/js) gains filters for the new weather-event kinds.
- **Touches:** game/rust/src/event.rs, game/rust/src/chronicle.rs, game/web/js, new: game/rust/src/weather_events.rs
- **Gate:** every generated weather event round-trips through pack/unpack byte-identical, sifter filter counts match raw event-table counts exactly on a reference seed, determinism hash includes weather-event payloads.

### M105 — The Calendar's Omens
- **Intent:** Eclipses and comets are deterministic astronomy, not superstition dressing, and the people of the world read them the way real chroniclers did.
- **Build:** Compute eclipse and comet-apparition dates from a closed-form orbital-period model seeded once at world creation, independent of weather RNG streams (ADR-0003 stream discipline), and feed each occurrence into the existing Omen event kind with a culture-specific reading drawn from the naming/culture tables rather than a fixed string.
- **Touches:** game/rust/src/event.rs, game/rust/src/culture.rs, game/rust/src/naming.rs, new: game/rust/src/astronomy.rs
- **Gate:** eclipse/comet dates are stable across reruns of the same seed and shift deterministically with a different seed, no two omen readings for the same seed/date pair diverge across pack/unpack, hash includes the astronomy stream.

### M106 — Living Memory
- **Intent:** "The worst winter in memory" should mean something falsifiable: what a population could plausibly still recall.
- **Build:** Give each settlement a rolling memory window sized by a generational constant (existing demographic turnover in society.rs) and have superlative chronicle phrasing ("worst in memory", "none alive recall") check the weather archive (M107) against that window rather than the run's full history; superlatives outside the window are suppressed or rephrased as "the old stories tell of…".
- **Touches:** game/rust/src/telling.rs, game/rust/src/society.rs, game/rust/src/chronicle.rs
- **Gate:** a scripted property test confirms no superlative phrase ever cites an event older than the settlement's computed memory window on any seed, and rephrased long-past events are counted separately in `diagnose telling`.

### M107 — The Weather Archive
- **Intent:** A region's climate history must survive as compact queryable data across a whole run's life, not be recomputed or forgotten between ticks.
- **Build:** Store per-region monthly temperature, precipitation, and active-event flags in a quantized ring-buffer-per-region structure appended each tick, capped at a fixed run-length budget and downsampled to decadal summaries beyond a recent window to bound memory; expose read accessors used by M104's weather events, M106's memory check, and M109's inspector.
- **Touches:** game/rust/src/climate.rs, game/rust/src/state.rs, game/rust/src/snapshot.rs, new: game/rust/src/climate_archive.rs
- **Gate:** archive memory footprint stays within the era's declared per-region byte budget at 500 simulated years, archived values round-trip pack/unpack byte-identical, and downsampled decadal summaries reproduce the same mean within quantization error as the raw series.

### M108 — Sky Layers in the Atlas
- **Intent:** The atlas should let an observer see the shape of climate history at a glance, the way it already shows terrain and biomes.
- **Build:** Add renderer layers for anomaly-magnitude heatmaps, age-timeline strips (cold arcs and warm optima as a horizontal ribbon), and event-overlay markers (storms, droughts, floods) drawn from the M107 archive, following the existing fullscreen-shader layer-toggle pattern in render.rs and its web-side layer controls.
- **Touches:** game/rust/src/render.rs, game/rust/src/climate_archive.rs, game/web/js
- **Gate:** each new atlas layer renders without frame-budget regression versus the pre-phase baseline on the reference bench scene, and layer toggling produces pixel-stable output for a fixed seed across repeated renders.

### M109 — The Explained Year
- **Intent:** Any given year's weather should be explainable in one card: the oscillation phase, the climate age, and the monsoon outcome that produced it.
- **Build:** Extend the inspector's provenance-chain pattern (explain.rs, following the M61 "why is this here" model) to a per-year "why this weather" card that reads oscillation phase (M74), drift/age state (M83–M87), and monsoon strength (M92) for the selected region and year, rendering them as a causal chain rather than a metric dump.
- **Touches:** game/rust/src/explain.rs, game/rust/src/climate_archive.rs, game/web/js
- **Gate:** for a fixed seed/region/year triple the explain card's cited causes match the underlying archived state fields exactly, and the card is generated identically across repeated queries within the same run.

### M110 — Metamorphic Weather
- **Intent:** The sky's logic must be provably directional — a colder age can shorten a growing season but must never lengthen one.
- **Build:** Add a metamorphic property suite over the climate pipeline: perturb only the century-drift or cold-age input and assert growing-season length, frost-date, and monsoon-onset outputs move in the physically required direction across paired seed runs, following the direction-of-effect pattern already used for rainfall↑⇒river-cells-not↓ (11-pcg-theory.md #6).
- **Touches:** game/rust/src/climate.rs, game/rust/src/agriculture.rs, new: game/rust/scripts/climate-metamorphic.sh
- **Gate:** all metamorphic direction assertions pass on every seed in a 30-seed battery with zero exceptions, and the check is wired into `report.sh` as a standing gate rather than an ad hoc script.

### M111 — Return-Time Honesty
- **Intent:** Droughts, storms, and floods must recur at rates a real climatologist would recognize, not at whatever cadence the generator happens to fall into.
- **Build:** Compute empirical return periods for drought, flood, and storm-landfall events per latitude/climate-zone bucket across a long multi-seed sweep and compare them against the paleoclimate envelope bands established in M82's calibration, tightening or documenting deviation in the diagnostics report.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/weather_events.rs, game/rust/scripts/report.sh
- **Gate:** measured return periods for each event class and zone fall inside the established Earth-analog envelope on at least 90% of sweep seeds, with `diagnose sky` reporting the miss rate explicitly.

### M112 — Spectral Honesty
- **Intent:** Variability must partition correctly across timescales — a century's variance is not just its years added up wrong.
- **Build:** Run a spectral decomposition (interannual, decadal, centennial bands) over the archived temperature and precipitation series from M107 and check that variance contributed by each band falls within literature-informed proportions (short-timescale noise dominant, secular drift bounded and small), reusing the existing bands-based PASS/WARN/FAIL harness idiom.
- **Touches:** game/rust/src/climate_archive.rs, game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** each of the three timescale bands' variance share stays within its declared PASS band on a 300-year, multi-seed sweep, and a seed with drift disabled shows its centennial-band share collapse toward zero as a metamorphic check.

### M113 — The Kept Sky
- **Intent:** A living climate ticking every month must not be the thing that breaks the tick budget the rest of the sim depends on.
- **Build:** Profile and optimize the monthly climate/weather tick path (oscillation update, storm-track step, archive append) against the era's declared per-tick time budget, applying incremental recomputation where the drift and mode state change slowly enough to amortize, without altering output values.
- **Touches:** game/rust/src/climate.rs, game/rust/src/climate_archive.rs, game/rust/src/systems.rs
- **Gate:** monthly tick wall-clock for the climate/weather subsystem stays within its declared budget band on the reference bench machine at 500-year scale, and the determinism hash is bit-identical before and after the optimization pass.

### M114 — Weather State Joins the Registry
- **Intent:** Every new field the living sky introduced this era must be declared once, not hand-mirrored across pack, snapshot, and hash code.
- **Build:** Move climate-archive ring buffers, oscillation phase/amplitude state, drift-curve parameters, and active weather-event tables into the field registry's declared-format discipline (pack.rs's FieldDecl/FieldSpec pattern, ADR-0015/0016), generating pack quantization, delta-tick encoding, and hash inclusion from the registry rather than hand-written code.
- **Touches:** game/rust/src/pack.rs, game/rust/src/state.rs, game/rust/src/climate_archive.rs, game/rust/src/snapshot.rs
- **Gate:** no climate/weather field is written to the pack or hash outside registry-generated code (grep-checkable), and pack round-trip plus determinism hash remain byte-identical to the pre-migration baseline on the reference seed.

### M115 — Climate-History Plates
- **Intent:** The report suite should let a human see an era's worth of climate history at a glance, the way ERA plots already do for terrain.
- **Build:** Add report-generation plates rendering per-seed age timelines (cold arcs, optima, drift curve) and anomaly atlases (spatial anomaly maps at chosen years) into the `report.sh` output bundle, following the existing plate-generation pattern used for terrain/hydrology reports.
- **Touches:** game/rust/scripts/report.sh, game/rust/src/bin/diagnose.rs, game/rust/src/climate_archive.rs
- **Gate:** `report.sh` emits climate-history plates for every sweep seed without manual steps, and plate generation completes within the suite's existing wall-clock budget for the full sweep.

### M116 — ERA Plots Gain Weather Axes
- **Intent:** Expressive-range analysis must now cover climate history, not just static terrain, to catch generator homogeneity in the sky itself.
- **Build:** Add 2D metric-pair histograms (per 11-pcg-theory.md's ERA technique) over climate-history metrics — e.g. drift amplitude × cold-arc frequency, oscillation period × teleconnection strength — exported per sweep alongside the existing terrain ERA plots, surfacing coverage gaps where climate histories cluster rather than spread.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/climate_archive.rs
- **Gate:** the new ERA plots are generated for every sweep run and flag when more than a declared fraction of seeds cluster within one histogram cell, with the threshold checked automatically by `report.sh`.

### M117 — Five-Century Runs
- **Intent:** The long sky must prove itself over the long run — drift, ages, and events cannot wander once the clock keeps turning for five hundred years.
- **Build:** Extend the standing sweep harness to a 500-year duration for the climate subsystem specifically, asserting the century-drift curve's running mean stays within its declared bound, cold-age/warm-optimum durations stay within literature-informed ranges, and no event class's frequency drifts monotonically across the run's five centuries.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/climate.rs, game/rust/scripts/report.sh
- **Gate:** on a 10-seed battery, five-century drift-curve running means stay within the declared bound throughout, and no seed shows a monotonic trend in event frequency across century buckets.

### M118 — Every Sky Its Own
- **Intent:** Two seeds must not produce recognizably the same climate history — the oatmeal problem applies to the sky as much as to terrain.
- **Build:** Implement structural distinctiveness metrics between seed pairs over climate history (cold-arc timing and duration sets, storm-track corridor shapes, drift-curve waveform correlation) following the oatmeal-detector pattern from 11-pcg-theory.md, and flag seed pairs whose climate histories correlate above a declared similarity threshold.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/src/climate_archive.rs, game/rust/scripts/report.sh
- **Gate:** across a 30-seed battery, no two seeds' climate-history similarity score exceeds the declared oatmeal threshold, and `report.sh` fails the sweep if a duplicate pair is found.

### M119 — `diagnose sky` Joins `report.sh`
- **Intent:** The era's climate work needs one standing runner the harness invokes every time, the way `diagnose earth` closed Era I.
- **Build:** Consolidate the phase's climate diagnostics — variability bands, oscillation spectra, storm/drought/flood return times, migration/famine/war coupling checks, ERA and oatmeal plates — into a single `diagnose sky` subcommand wired into `report.sh`'s standard invocation list.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** `report.sh` runs `diagnose sky` on every invocation and the aggregate report shows a single pass/warn/fail rollup for the climate subsystem, matching the sum of its constituent checks.

### M120 — Era II Gate
- **Intent:** The Long Sky closes: the living climate must hold across a full sweep with its human consequences in band before the forge recasts it.
- **Build:** Run the complete 300-year multi-seed sweep with `diagnose sky` and the full existing suite together, confirm famine, migration, and war-weather couplings (M95–M103) stay within their declared bands simultaneously, and record any superseded ADRs from era-long decisions in the ADR index.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, docs/adr/README.md
- **Gate:** the 300-year sweep is green across every subsystem check including the new sky diagnostics, famine/migration/war-weather coupling metrics all fall within their declared bands on every sweep seed, era close is recorded.

### M121 — Climate Stack Re-Cut
- **Intent:** Five years of climate work blurred the line between what generation decides once and what the tick recomputes every month; the forge draws that line cleanly.
- **Build:** Split climate.rs into a generation-time module producing the fixed per-cell baseline fields (temperature mean, continentality, precipitation baseline) and a sim-time weather module owning drift, oscillation, storms, and anomalies, landing the split with a superseding-or-extending ADR that documents the new boundary and its rationale.
- **Touches:** game/rust/src/climate.rs, game/rust/src/world.rs, docs/adr, new: game/rust/src/weather.rs
- **Gate:** determinism hash is bit-identical to the pre-refactor baseline across the standing sweep, the split compiles with no shared mutable state crossing the new module boundary except through explicit accessors, and the ADR is accepted before the phase closes.

### M122 — Time-Series State
- **Intent:** The climate archive and run-history buffers accumulated ad hoc during the era; the forge gives them one registry-declared shape.
- **Build:** Formalize ring-buffer and run-archive time-series formats as a registry-declared type family (fixed-capacity quantized ring buffer, decadal-summary tier) reused by climate_archive.rs and any future subsystem needing bounded history, generating pack/hash code from the declaration rather than per-module hand rolling.
- **Touches:** game/rust/src/climate_archive.rs, game/rust/src/pack.rs, game/rust/src/state.rs
- **Gate:** climate_archive.rs contains zero hand-written pack/hash code for its time-series fields after migration, and pack round-trip plus determinism hash remain byte-identical to the pre-forge baseline.

### M123 — Weather and Disaster Events Fold Into the Event Table
- **Intent:** Weather events grew their own ad hoc shapes during the era; the forge folds them into the one closed event-table discipline everything else already obeys.
- **Build:** Migrate weather_events.rs's storm, drought, flood, and age-transition records onto the shared EventKind/event-table structures (event.rs) used by every other chronicle event, retiring any bespoke serialization paths and ensuring the sifter and inspector query weather events the same way they query all others.
- **Touches:** game/rust/src/weather_events.rs, game/rust/src/event.rs, game/rust/src/chronicle.rs, game/web/js
- **Gate:** every weather event round-trips through the shared event-table pack/unpack path byte-identical, no bespoke weather-event serialization code remains (grep-checkable), determinism hash unchanged versus pre-migration baseline.

### M124 — Budgets Back in Band
- **Intent:** The living sky's five years of accretion must be re-measured against the engine's standing budgets with everything the era added switched on.
- **Build:** Re-run generation-time, tick, memory, and payload budget measurements with the full climate/weather stack — archive, oscillation, storms, campaign seasons, weather events — active, and tune or document any budget that has drifted out of its declared band since Era I's forge baseline.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh, game/rust/src/systems.rs
- **Gate:** generation time, per-tick time, peak memory, and pack payload size for a reference-seed run all fall within their declared bands with the complete Era II feature set active.

### M125 — Suite Refit
- **Intent:** The suite grew a weather-shaped tail across the era; the forge trims it back to fast before Era III adds people to name.
- **Build:** Consolidate the era's weather-specific property and metamorphic lanes (M110–M112, M117–M118) into shared harness infrastructure, parallelize or downsample redundant sweep repetitions, and hold the full sweep's wall-clock time against the pre-era baseline recorded at Forge I's close.
- **Touches:** game/rust/src/bin/diagnose.rs, game/rust/scripts/report.sh
- **Gate:** the full 300-year multi-seed sweep's wall-clock time is no worse than the Forge I baseline plus a declared small margin, full suite is green, determinism hash unchanged through the consolidation.
