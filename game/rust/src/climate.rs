//! Climate — port of climate.py: temperature, seasonal swing, precipitation.

use ndarray::Array2;

use crate::ndimage;

/// Degrees from equator; equator at the middle row (as in geo.hy).
pub fn latitude_deg(size: usize) -> Array2<f64> {
    let n = size as f64;
    Array2::from_shape_fn((size, size), |(y, _)| {
        (-90.0 + (y as f64) * 180.0 / (n - 1.0)).abs()
    })
}

/// Annual-mean sea-level temperature by latitude, minus altitude lapse.
/// E5.11 — the sea-level term depends only on the row, so the `powf`
/// hoists out of the inner loop; per-cell arithmetic is unchanged.
pub fn temperature_mean(height: &Array2<f64>, lat_deg: &Array2<f64>) -> Array2<f64> {
    let (rows, cols) = height.dim();
    let mut out = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        let lat = lat_deg[[y, 0]] / 90.0;
        let t_sea = 28.0 - 53.0 * lat.powf(1.7);
        for x in 0..cols {
            out[[y, x]] = t_sea - 26.0 * height[[y, x]].max(0.0); // 6.5 C/km * 4 km per unit
        }
    }
    out
}

// ------------------------------------------------------ heat transport

/// M41 — heat transport: rows of remembered journey per unit current.
pub const HEAT_RES: f64 = 25.0;
/// Clamp on the remembered journey, rows.
pub const HEAT_MAX_DISP: f64 = 60.0;
/// °C cap on the open-sea anomaly.
pub const HEAT_ANOM_CAP: f64 = 8.0;
/// Per-ring decay walking the anomaly inland.
pub const HEAT_COAST_DECAY: f64 = 0.55;
/// How far inland the sea reaches, rings of cells (×4 km).
pub const HEAT_COAST_RINGS: usize = 6;

// ------------------------------------------- M50 metamorphic harness

/// M50 — the synthetic warm current the metamorphic harness injects:
/// a poleward ribbon hugging every shore in the subtropical/temperate
/// band. Not a world field — a probe, built to be switched off again,
/// so the climate's *response* is what gets measured rather than the
/// happenstance strength of any one seed's real gyre.
pub const META_WARM_V: f64 = 3.0;
/// How far offshore the ribbon runs, cells (×4 km).
pub const META_STRIP: f64 = 16.0;
/// Latitude window the ribbon occupies, degrees |lat|.
pub const META_LAT: (f64, f64) = (10.0, 70.0);
/// M50 gate: killing the ribbon must cool the coast it touched by at
/// least this much, mean over the touched land, every seed.
pub const META_COOL_MIN: f64 = 2.0;
/// M50 gate: share of the touched coast whose rain must fall with the
/// current — the direction has to be the rule, not the average.
pub const META_RAIN_SHARE_MIN: f64 = 0.70;

// ------------------------------------------------- current-aware rain

/// M42 — marine-layer stability response per °C of SST anomaly: cold
/// water caps the air (subsidence inversion — Atacama, Namib), warm
/// water destabilizes it (Gulf-Stream storm coasts).
pub const STAB_GAIN: f64 = 0.16;
/// Stability floor: over the coldest rims rain falls at this share.
pub const STAB_MIN: f64 = 0.45;
/// Stability ceiling on warm rims — wetter, not monsoon-mad.
pub const STAB_MAX: f64 = 1.30;
/// Per-step relaxation of the parcel's stability over water.
pub const STAB_SEA_RELAX: f64 = 0.30;
/// Per-step decay of the marine memory over land — the inversion
/// breaks a few hundred km inland and the interior forgets the sea.
pub const STAB_LAND_RELAX: f64 = 0.12;
/// Sea-evaporation response per °C of anomaly: cold seas breathe less.
pub const EVAP_GAIN: f64 = 0.05;
/// Warm-rim onshore moisture feed, per °C of positive land bias: the
/// storm-track transients a zonal march cannot carry — what keeps
/// Earth's warm-current east coasts humid even leeward of a continent.
/// Asymmetric by design: cold rims dry through stability, they do not
/// steal moisture twice.
pub const WARM_INJECT: f64 = 0.020;
/// Direct lateral rain off a warm rim, per °C of positive land bias:
/// the share of the onshore transient feed that falls at the coast
/// itself. Routed through `w` it would blow 98% past the rim (rain
/// rates are a few % per cell); Earth's humid-subtropical east coasts
/// are watered exactly by this import, so it rains where it lands and
/// never debits the parcel — the sea paid for it, not the march.
pub const WARM_RAIN: f64 = 0.012;
/// Over land the marine memory decays toward neutral — except where a
/// warm rim keeps the boundary layer convective: the land target is
/// pulled up by the local positive bias at this fraction of the sea
/// gain. Cold bias never stabilizes land air (the inversion is a
/// marine artifact that breaks on landfall heating).
pub const STAB_LAND_WARM_PULL: f64 = 1.0;

/// M41 — heat transport: the current-driven sea-surface anomaly and
/// its coastal reach. Water remembers the latitude it came from: each
/// ocean cell's meridional current displaces its origin `HEAT_RES`
/// rows upstream, and the anomaly is the zonal sea-surface law read
/// at the origin minus at home — poleward flow warms (Gulf Stream),
/// equatorward flow cools (Humboldt), no sign rule beyond that
/// subtraction. Smoothed with the shared kernel over open water, then
/// walked inland in decaying rings so the coasts the current touches
/// bend with it while the interior keeps its continental truth.
/// Returned over the whole grid: ocean cells carry the SST anomaly
/// (the sea-ice calendar obeys the currents too), land cells carry
/// the coastal reach, the far interior carries zero.
pub fn current_bias(water: &Array2<bool>, cur_v: &Array2<f32>) -> Array2<f64> {
    let (rows, cols) = water.dim();
    if rows < 8 || cols < 8 {
        return Array2::zeros((rows, cols));
    }
    let nf = rows as f64;
    let t_sea = |yy: f64| -> f64 {
        let lat = ((-90.0 + yy * 180.0 / (nf - 1.0)) / 90.0).abs();
        28.0 - 53.0 * lat.powf(1.7)
    };
    let mut a = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        let here = t_sea(y as f64);
        for x in 0..cols {
            if !water[[y, x]] {
                continue;
            }
            let disp =
                (cur_v[[y, x]] as f64 * HEAT_RES).clamp(-HEAT_MAX_DISP, HEAT_MAX_DISP);
            let y0 = (y as f64 - disp).clamp(0.0, nf - 1.0);
            a[[y, x]] = (t_sea(y0) - here).clamp(-HEAT_ANOM_CAP, HEAT_ANOM_CAP);
        }
    }
    // knit the anomaly along the flow; the land carries none of it yet
    let mut a = ndimage::gaussian_filter(&a, 2.0);
    for y in 0..rows {
        for x in 0..cols {
            if !water[[y, x]] {
                a[[y, x]] = 0.0;
            }
        }
    }
    // walk inland ring by ring: each ring reads the mean of already-
    // reached 8-neighbors and decays — raster order never matters
    // because every ring reads only the previous ring's snapshot.
    let mut reached = water.clone();
    for _ in 0..HEAT_COAST_RINGS {
        let prev = reached.clone();
        let pa = a.clone();
        for y in 0..rows {
            for x in 0..cols {
                if prev[[y, x]] {
                    continue;
                }
                let mut s = 0.0f64;
                let mut n = 0usize;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dy == 0 && dx == 0 {
                            continue;
                        }
                        let yy = y as i64 + dy;
                        let xx = x as i64 + dx;
                        if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                            continue;
                        }
                        let (yy, xx) = (yy as usize, xx as usize);
                        if prev[[yy, xx]] {
                            s += pa[[yy, xx]];
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    a[[y, x]] = HEAT_COAST_DECAY * s / n as f64;
                    reached[[y, x]] = true;
                }
            }
        }
    }
    a
}

// ------------------------------------------------------- M47 upwelling

/// Weight of the offshore-wind term in the upwelling index.
pub const UPWELL_WIND: f64 = 0.55;
/// Weight of the cold-current-adjacency term (M41's heat anomaly).
pub const UPWELL_COLD: f64 = 0.45;
/// Latitude window: full credit equatorward of this |latitude| —
/// Earth's eastern-boundary systems (Canary, Benguela, Humboldt,
/// California) all live inside ~35°.
pub const UPWELL_LAT_FULL: f64 = 35.0;
/// …fading to nothing by here: polar offshore winds ride sea ice and
/// light-starved water, not fisheries.
pub const UPWELL_LAT_NONE: f64 = 60.0;
/// The nutrient_rich mark: a coastal cell whose upwelling index clears
/// this threshold is the honest ground Era IV's fisheries will harvest.
/// Derived from the measured coastline share across the seed sweep so
/// the marked fraction lands in the M47 gate band (3–10%).
pub const NUTRIENT_RICH: f32 = 0.34;

/// M47 — the upwelling index: 0..1 on ocean cells touching land, zero
/// everywhere else. The wind field is zonal by construction
/// (`currents::wind_stress`), so the honest Ekman proxy available is
/// the wind component crossing the coast seaward — and only the
/// **trades** mint it: Earth's eastern-boundary systems (Canary,
/// Benguela, Humboldt, California) are all trade-driven subtropical
/// west coasts. The westerlies' own offshore shores are the mid-latitude
/// east coasts — Gulf-Stream country, the warm western-boundary rims —
/// and marking those was measured to invert the analogue (marked rims
/// ran +0.85°C over unmarked), so eastward stress contributes nothing.
/// Cold-current adjacency (M41's anomaly, the same field that writes
/// the Atacama rains) adds the Humboldt/Benguela signature, and a
/// smoothstep latitude window keeps the polar easterlies from minting
/// fisheries under the pack ice. Pure function of the ocean mask and
/// the meridional current — raster order, no iteration-order hazards.
pub fn upwelling(water: &Array2<bool>, cur_v: &Array2<f32>) -> Array2<f32> {
    let (rows, cols) = water.dim();
    let mut up = Array2::<f32>::zeros((rows, cols));
    if rows < 8 || cols < 8 {
        return up;
    }
    let nf = rows as f64;
    let heat = current_bias(water, cur_v);
    // the trades' own peak normalizes the wind term: they are the engine
    let tau_max = crate::currents::TAU_TRADES;
    for y in 0..rows {
        let lat_abs = (-90.0 + y as f64 * 180.0 / (nf - 1.0)).abs();
        let tau = crate::currents::wind_stress(lat_abs);
        // algebraic smoothstep window — no transcendentals
        let t = ((lat_abs - UPWELL_LAT_FULL) / (UPWELL_LAT_NONE - UPWELL_LAT_FULL))
            .clamp(0.0, 1.0);
        let lat_favor = 1.0 - t * t * (3.0 - 2.0 * t);
        if lat_favor <= 0.0 {
            continue;
        }
        for x in 0..cols {
            if !water[[y, x]] {
                continue;
            }
            // offshore normal from the 8-neighborhood land census
            let (mut nx, mut ny) = (0.0f64, 0.0f64);
            let mut nland = 0usize;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dy == 0 && dx == 0 {
                        continue;
                    }
                    let yy = y as i64 + dy;
                    let xx = x as i64 + dx;
                    if yy < 0 || xx < 0 || yy >= rows as i64 || xx >= cols as i64 {
                        continue;
                    }
                    if !water[[yy as usize, xx as usize]] {
                        let len = (((dy * dy + dx * dx) as f64)).sqrt();
                        nx -= dx as f64 / len;
                        ny -= dy as f64 / len;
                        nland += 1;
                    }
                }
            }
            if nland == 0 {
                continue; // open ocean — the scalar is a coastal reading
            }
            let nl = (nx * nx + ny * ny).sqrt();
            if nl < 1e-9 {
                continue; // land on all sides cancels: no defined offshore
            }
            let nxu = nx / nl;
            // wind crossing the coast seaward (zonal wind ⇒ dot with n̂x);
            // easterly stress only — the westerlies mint no upwelling
            let wind_off = if tau < 0.0 {
                (tau * nxu / tau_max).max(0.0)
            } else {
                0.0
            };
            // cold rim adjacency, 0..1 (Humboldt over Gulf Stream)
            let cold = (-heat[[y, x]]).max(0.0) / HEAT_ANOM_CAP;
            let v = (UPWELL_WIND * wind_off + UPWELL_COLD * cold) * lat_favor;
            up[[y, x]] = v as f32;
        }
    }
    up
}

/// 0.35 (maritime) .. 1.0 (deep continental interior).
/// E5.11 — computed once per generation in world.rs and shared by
/// `temperature_amplitude` and `precipitation`; the EDT is the expensive
/// part and the two consumers used to run it twice on the same mask.
pub fn continentality(water: &Array2<bool>) -> Array2<f64> {
    let land = water.mapv(|w| !w);
    let d = ndimage::distance_transform_edt(&land);
    d.mapv(|v| 0.35 + 0.65 * (v / 70.0).clamp(0.0, 1.0))
}

/// Signed seasonal swing: southern hemisphere positive (warm in Gamelion).
pub fn temperature_amplitude(lat_deg: &Array2<f64>, cont: &Array2<f64>) -> Array2<f64> {
    let (rows, cols) = lat_deg.dim();
    let mut out = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        let lat = lat_deg[[y, 0]] / 90.0;
        let base = 3.0 + 19.0 * lat.powf(1.2);
        let sign = if y >= rows / 2 { 1.0 } else { -1.0 };
        for x in 0..cols {
            out[[y, x]] = sign * (base * cont[[y, x]]);
        }
    }
    out
}

pub fn month_temperature(tmean: f64, tamp_signed: f64, month: i64) -> f64 {
    tmean + tamp_signed * (2.0 * std::f64::consts::PI * month as f64 / 12.0).cos()
}

/// Monthly rainfall from the annual total and the signed seasonal
/// share. Positive amplitude peaks in Gamelion (month 0, southern
/// summer) to match the sign convention of `temperature_amplitude`.
pub fn month_precip(p_annual: f64, pamp_signed: f64, month: i64) -> f64 {
    let phase = (2.0 * std::f64::consts::PI * month as f64 / 12.0).cos();
    (p_annual / 12.0 * (1.0 + pamp_signed * phase)).max(0.0)
}

/// M34 — the twelve-month cosine cycle as exact literals: cos(2πm/12)
/// takes only algebraic values (±1, ±√3/2, ±1/2, 0), so the glacier
/// mass balance stays libm-free and replay-identical across runtimes
/// (ADR-0025 discipline; the loess plume set the precedent).
pub const COS12: [f64; 12] = [
    1.0,
    0.866_025_403_784_438_6,
    0.5,
    0.0,
    -0.5,
    -0.866_025_403_784_438_6,
    -1.0,
    -0.866_025_403_784_438_6,
    -0.5,
    0.0,
    0.5,
    0.866_025_403_784_438_6,
];

/// M38 — the treeline currency's floor: degree-days count above 5 °C.
pub const GDD_BASE: f64 = 5.0;
/// M38 — mean month length (365.25/12): degree-months → degree-days.
pub const DAYS_PER_MONTH: f64 = 30.4375;

/// M38 — growing-degree-days above 5 °C from the two-parameter
/// seasonal climate, through the exact COS12 table (libm-free,
/// replay-identical, ADR-0025 discipline). GDD5 is the ecologist's
/// treeline currency: trees pay in summer warmth regardless of how
/// brutal the winter gets, which is why larch taiga stands on Yakutian
/// permafrost while maritime tundra at the same annual mean stays
/// bare. Amplitude enters through |phase| symmetry, so the sign
/// convention washes out and both hemispheres read the same law.
#[inline]
pub fn gdd5(tmean: f64, tamp_signed: f64) -> f64 {
    let mut g = 0.0;
    for phase in COS12 {
        let t = tmean + tamp_signed * phase;
        if t > GDD_BASE {
            g += (t - GDD_BASE) * DAYS_PER_MONTH;
        }
    }
    g
}

/// M34 — snow falls when the month sits at or below this air temperature (°C).
pub const SNOW_T: f64 = 1.0;
/// M34 — degree-month melt factor, mm w.e. per positive °C·month
/// (≈4 mm/°C/day PDD for a snow–ice mix, ×30 days — Hock 2003 range).
pub const DDF_MELT: f64 = 120.0;

/// M34 — annual surface mass balance for permanent ice, in metres of
/// water equivalent per year: snowfall accumulated over the freezing
/// months minus positive-degree-month ablation, both read off the same
/// seasonal cycle the rest of the engine ticks (`month_temperature` /
/// `month_precip` semantics, but through the exact COS12 table so the
/// result is a pure function of its four inputs on every runtime).
/// Positive balance is glacier country.
pub fn ice_balance(tmean: f64, tamp_signed: f64, p_annual: f64, pamp_signed: f64) -> f64 {
    let mut acc = 0.0;
    let mut pdd = 0.0;
    for phase in COS12 {
        let t = tmean + tamp_signed * phase;
        let p = (p_annual / 12.0 * (1.0 + pamp_signed * phase)).max(0.0);
        if t <= SNOW_T {
            acc += p;
        }
        if t > 0.0 {
            pdd += t;
        }
    }
    (acc - DDF_MELT * pdd) / 1000.0
}

/// M89 — the balance-zero elevation ("snowline") for one latitude belt's
/// mean climate under a composed forcing `dt` (°C on `tmean`): the
/// height `h ∈ (0, 2.5]` where `ice_balance(t0 + dt − 26·h, ta, pr, pa)`
/// crosses zero, solved by the same 48-step bisection the M34 lane has
/// always run — `dt = 0` reproduces the banked M34 arithmetic bit for
/// bit (`t0 + 0.0 ≡ t0` in IEEE). `t0` is the belt's sea-level-equivalent
/// mean temperature; 26 °C per height unit is the standing lapse
/// (6.5 °C/km over the 4 km unit). `None` when the belt holds no alpine
/// snowline: the balance never turns positive below the ceiling, or the
/// shore itself accumulates (cap country — ice at sea level).
pub fn belt_snowline(t0: f64, ta: f64, pr: f64, pa: f64, dt: f64) -> Option<f64> {
    let bal = |h: f64| ice_balance(t0 + dt - 26.0 * h, ta, pr, pa);
    if bal(2.5) <= 0.0 {
        return None; // no snowline below the ceiling in this belt
    }
    if bal(0.0) > 0.0 {
        return None; // cap country: ice at the shore, no alpine snowline
    }
    let mut hi = 2.5f64;
    let mut lo = 0.0f64;
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if bal(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

/// M35 — how a glacier cell hands its water to the rivers below.
/// Splits the cell's year into the snow bank and the rain lane over
/// the same COS12 cycle as `ice_balance`, and phases the bank's
/// release by positive-degree months. Returns, all in metres/yr:
///
/// * `melt`      — annual meltwater throughput: the freezing-month
///   accumulation given back in the warm season (steady state — what
///   the ice takes it returns; a cap with no melt months returns 0
///   and banks the snow forever).
/// * `melt_amp`  — signed month-0 cosine projection of the melt-month
///   weights, −1..1, same sign convention as `temperature_amplitude`:
///   the melt lane's first harmonic for the seasonal-swing ledger.
/// * `rain`      — the non-snow months' precipitation, which still
///   runs off immediately like everywhere else.
/// * `rain_amp_mass` — that rain's signed cosine mass (unnormalized
///   harmonic), so the caller can keep the rain lane's true phase
///   instead of pretending the banked snow ran off in winter.
pub fn melt_throughput(
    tmean: f64,
    tamp_signed: f64,
    p_annual: f64,
    pamp_signed: f64,
) -> (f64, f64, f64, f64) {
    let mut acc = 0.0;
    let mut rain = 0.0;
    let mut rharm = 0.0;
    let mut pdd = 0.0;
    let mut proj = 0.0;
    for phase in COS12 {
        let t = tmean + tamp_signed * phase;
        let p = (p_annual / 12.0 * (1.0 + pamp_signed * phase)).max(0.0);
        if t <= SNOW_T {
            acc += p;
        } else {
            rain += p;
            rharm += p * phase;
        }
        if t > 0.0 {
            pdd += t;
            proj += t * phase;
        }
    }
    let amp = if pdd > 0.0 { (proj / pdd).clamp(-1.0, 1.0) } else { 0.0 };
    let melt = if pdd > 0.0 { acc / 1000.0 } else { 0.0 };
    (melt, amp, rain / 1000.0, rharm / 1000.0)
}


/// Wind-advected moisture -> annual precipitation in mm/yr, plus the
/// signed monsoon amplitude: how strongly the year's rain leans into
/// the local summer as the ITCZ marches between the tropics.
///
/// M42 — the march is current-aware: each parcel carries a marine-layer
/// stability alongside its moisture. Over water it relaxes toward the
/// local SST anomaly's verdict (cold rim → capped inversion, warm rim →
/// convective); over land it decays back to neutral while scaling the
/// rain rate — so a Humboldt coast starves its downwind desert, a
/// Gulf-Stream coast feeds its downwind green, and the deep interior
/// answers only to its own uplift and warmth, exactly as before.
pub fn precipitation(
    height: &Array2<f64>,
    water: &Array2<bool>,
    tmean: &Array2<f64>,
    lat_deg: &Array2<f64>,
    cont: &Array2<f64>,
    heat: &Array2<f64>,
) -> (Array2<f64>, Array2<f64>) {
    let size = height.dim().0;
    let mut p = Array2::<f64>::zeros((size, size));
    let wraps = 3usize;

    for y in 0..size {
        let lat = lat_deg[[y, 0]];
        // trades (<30) E->W: dx=-1; westerlies (30-60): +1; polar easterlies: -1
        let d: isize = if lat < 30.0 {
            -1
        } else if lat < 60.0 {
            1
        } else {
            -1
        };
        let mut w = 0.4f64;
        let mut stab = 1.0f64;
        for step in 0..wraps * size {
            let xcur = (d * step as isize).rem_euclid(size as isize) as usize;
            let xprev = (xcur as isize - d).rem_euclid(size as isize) as usize;
            let wat = water[[y, xcur]];
            let t = tmean[[y, xcur]];
            // M42 — the marine layer remembers the water it crossed.
            if wat {
                let target = (1.0 + STAB_GAIN * heat[[y, xcur]]).clamp(STAB_MIN, STAB_MAX);
                stab += STAB_SEA_RELAX * (target - stab);
            } else {
                let target = (1.0
                    + STAB_GAIN * STAB_LAND_WARM_PULL * heat[[y, xcur]].max(0.0))
                .clamp(1.0, STAB_MAX);
                stab += STAB_LAND_RELAX * (target - stab);
            }
            // Land evapotranspiration recycles a real share of moisture —
            // without it every continental interior turns to bone-dry waste.
            let evap = if wat {
                (0.018 + 0.030 * t.clamp(0.0, 30.0) / 30.0)
                    * (1.0 + EVAP_GAIN * heat[[y, xcur]]).clamp(0.75, 1.25)
            } else {
                0.009
                    + 0.004 * t.clamp(0.0, 30.0) / 30.0
                    + WARM_INJECT * heat[[y, xcur]].max(0.0)
            };
            w += evap;
            let hcur = height[[y, xcur]].max(0.0);
            let hprev = height[[y, xprev]].max(0.0);
            let uplift = ((hcur - hprev) * size as f64 / 40.0).clamp(0.0, 3.0);
            let rate = if wat {
                0.012
            } else {
                ((0.023 + 0.40 * uplift) * stab).clamp(0.0, 0.65)
            };
            let cap = (1.0 + t / 22.0).clamp(0.15, 2.3); // warm air holds more
            let mut rain = w * rate;
            // M42 — over land the inversion caps every rain mode: the
            // spill dump obeys the marine memory too, or polar cold rims
            // would grow wetter from their own lowered cap.
            rain += 0.5 * (w - cap).max(0.0) * if wat { 1.0 } else { stab };
            w -= rain;
            if !wat {
                // M42 — the warm-rim import falls here, off the sea's
                // account: recorded as rain, never subtracted from w.
                rain += WARM_RAIN * heat[[y, xcur]].max(0.0);
            }
            if step >= (wraps - 1) * size {
                // record only the settled final wrap
                p[[y, xcur]] += rain;
            }
        }
    }

    // The ITCZ is not a line but a march: it camps at ~10°S in the
    // southern summer (month 0) and ~10°N half a year later. Each cell
    // gets its convective boost from both camps; the *difference*
    // between the two visits is the monsoon. Continentality arrives
    // precomputed (E5.11) — same values, one EDT per generation.
    let mut pamp = Array2::<f64>::zeros((size, size));
    let n = size as f64;
    for y in 0..size {
        for x in 0..size {
            let lat = lat_deg[[y, x]];
            // signed latitude: negative north (y=0), positive south —
            // matching the sign convention of temperature_amplitude.
            let lat_s = -90.0 + (y as f64) * 180.0 / (n - 1.0);
            let t = tmean[[y, x]];
            let mut v = p[[y, x]];
            let c0 = itcz_camp(lat_s, ITCZ_CAMP_LAT);
            let c6 = itcz_camp(lat_s, -ITCZ_CAMP_LAT);
            v *= 0.5 * (c0 + c6);
            v *= 1.0 - 0.30 * (-(((lat - 25.0) / 8.0).powi(2))).exp();
            v *= (0.25 + (t + 20.0) / 40.0).clamp(0.25, 1.0);
            p[[y, x]] = v;

            // signed seasonal share: positive = wet when the south warms
            let mut a = (c0 - c6) / (c0 + c6);
            // continental summer convection: interiors pull their rain
            // into the warm half of the year even outside the tropics
            if t > 8.0 && !water[[y, x]] {
                let hemi = if y >= size / 2 { 1.0 } else { -1.0 };
                a += hemi
                    * 0.22
                    * ((cont[[y, x]] - 0.35) / 0.65).clamp(0.0, 1.0)
                    * ((t - 8.0) / 20.0).clamp(0.0, 1.0);
            }
            pamp[[y, x]] = a.clamp(-0.85, 0.85);
        }
    }

    let mut p = ndimage::gaussian_filter(&p, 1.4);
    let pamp = ndimage::gaussian_filter(&pamp, 1.4);

    // normalise to mm/yr: land mean ~900 mm
    let mut sum = 0.0;
    let mut cnt = 0usize;
    for y in 0..size {
        for x in 0..size {
            if !water[[y, x]] {
                sum += p[[y, x]];
                cnt += 1;
            }
        }
    }
    let mean_land = if cnt > 0 { sum / cnt as f64 } else { 1.0 };
    let k = 900.0 / mean_land.max(1e-9);
    p.mapv_inplace(|v| (v * k).clamp(0.0, 4500.0));
    (p, pamp)
}

// ------------------------------------------------- M71 · the year stops repeating
//
// Every simulated year gets its own weather instead of the climate mean.
// The anomaly is a pure function of (seed, x, y, year): a two-octave fbm
// over space × year, scaled by a latitude-shaped amplitude — the tropics
// barely move, the poles swing wide, which is the variance shape Earth's
// instrumental record carries (interannual σ of annual-mean temperature
// runs a few tenths of a degree in the deep tropics and well over a
// degree in the high-latitude interiors). Temperature is additive in °C;
// precipitation is fractional, because a dry year takes a *share* of the
// rain, not a fixed millimetre count.
//
// Derived state, deliberately: nothing here is stored, hashed or packed.
// The field is recomputed from the seed and the year whenever it is asked
// for, so determinism is structural rather than checked.

/// 1σ of the annual temperature anomaly at the equator, °C.
pub const ANOM_T_EQ: f64 = 0.30;
/// 1σ of the annual temperature anomaly at the pole, °C.
pub const ANOM_T_POLE: f64 = 1.85;
/// 1σ of the annual precipitation anomaly at the equator, as a fraction.
pub const ANOM_P_EQ: f64 = 0.09;
/// 1σ of the annual precipitation anomaly at the pole, as a fraction.
pub const ANOM_P_POLE: f64 = 0.24;
/// How sharply the swing grows poleward (>1: the mid-latitudes stay
/// closer to the tropics than to the poles).
pub const ANOM_LAT_POW: f64 = 1.55;
/// Spatial frequency of the anomaly cells — ~30 cells (120 km) across,
/// so a bad year covers a region, never a single farm.
pub const ANOM_SPACE: f64 = 0.033;
/// Step through the noise's third axis per year. Large enough that
/// consecutive years are effectively independent draws.
pub const ANOM_YEAR_STEP: f64 = 1.7;
/// Offset that hands the rain its own slice of the same field, so a hot
/// year is not mechanically a wet one.
pub const ANOM_RAIN_LANE: f64 = 137.0;
/// Two-octave fbm on this lattice measures 0.2076 population σ over the
/// land of a 512-world (measured by `diagnose climate`, which then holds
/// the realized amplitude to the declared one ±20%). Dividing
/// by it makes the declared amplitudes mean what they say in °C.
pub const ANOM_FBM_SIGMA: f64 = 0.2076;
/// A year may not take more than this share of the rain away — total
/// failure of the rains is famine's verdict (M2.6), not the sky's noise.
pub const ANOM_P_FLOOR: f64 = -0.85;

/// M72 — how far the rain anomaly is smeared before the rivers read it.
/// A 4 km cell's cloud is not a catchment: what a trunk river carries is
/// the year averaged over its basin, so the flow lane reads the rain
/// field through a gaussian of roughly basin scale (~24 km 1σ).
pub const CATCHMENT_SIGMA: f64 = 6.0;
/// M72 — how strongly the catchment-integrated rain anomaly moves flow.
/// Rainfall-runoff is elastic, not proportional: a wet year swells rivers
/// by more than its rain surplus once soils saturate (elasticity ~1.5–2.5
/// in the empirical literature; the conservative end is taken here).
pub const FLOW_ANOM_GAIN: f64 = 1.5;
/// M72 — the year's flow multiplier is bounded: rivers rise and fall, they
/// do not vanish or become a different river.
pub const FLOW_FACTOR_MIN: f64 = 0.35;
pub const FLOW_FACTOR_MAX: f64 = 2.20;

// ---- M75 — the tilted belts (teleconnection) -------------------------------
//
// A basin that leans does not lean alone. When the warm pool shifts, the
// Walker circulation shifts with it: the trades slacken on one side and
// stiffen on the other, and the rain belts on *opposite* hemispheres tilt
// in opposite directions. That antisymmetry is the signature — not a
// global wet year or a global dry one, but a see-saw in the rain itself,
// arriving on the far side a season or two after the index moves.
//
// The bias is a pure function of `(index, latitude)`: it is added to the
// fractional rain anomaly after the ITCZ gaussian the precipitation pass
// already applies, so it strengthens or weakens trade-wind moisture
// delivery without touching the mean climate a world was generated with.

/// How long the far side takes to answer the index, in months. Real
/// teleconnections propagate through the atmosphere in a season, not a
/// year (~4 months, the canonical ENSO-to-remote lag).
pub const TELE_LAG_MONTHS: i64 = 4;
/// Fractional rain shifted per unit index at the belt core. A strong
/// basin (index ≈ 2σ) therefore moves the trade belt's rain by roughly a
/// third — large enough to be felt in harvests, far short of the −0.85
/// floor that famine, not the sky, is allowed to reach.
pub const TELE_GAIN: f64 = 0.16;
/// Latitude of the trade-belt core, degrees. The tilt lives in the
/// trades, not at the ITCZ itself and not in the westerlies.
pub const TELE_BELT_LAT: f64 = 15.0;
/// Width of the belt, degrees (1σ of the gaussian either side).
pub const TELE_BELT_SIGMA: f64 = 12.0;

/// M75 — the rain bias the oscillation's phase lays on this latitude.
///
/// Antisymmetric by construction: the northern trade belt is wet exactly
/// when the southern one is dry, and the two swap when the index changes
/// sign. It vanishes at the equator (the ITCZ is not tilted, it is
/// straddled) and outside the trades, so the westerlies and the poles
/// keep the variability M73 measured.
pub fn teleconnection_bias(index: f64, lat_signed: f64) -> f64 {
    let g = |c: f64| (-0.5 * ((lat_signed - c) / TELE_BELT_SIGMA).powi(2)).exp();
    TELE_GAIN * index * (g(TELE_BELT_LAT) - g(-TELE_BELT_LAT))
}

// ---- M83 — the slow drift (the century's own temperature) ------------------
//
// A world's climate mean is not a constant it never leaves: Earth's
// Holocene wandered by tenths of a degree over centuries — the Roman warm
// spell, the medieval optimum, the little ice age — without ever running
// away. M83 gives every run that history: a slow, bounded, mean-reverting
// walk around the baseline `tmean`, drawn once from the seed and evaluated
// per year, entering the temperature pipeline as a global offset *ahead of*
// the M71 anomaly draw (the year's weather rides on the century's back).
//
// The law is an exact-discretization Ornstein–Uhlenbeck step with
// reflecting walls: v_k = φ·v_{k−1} + σ_step·ξ_k, φ = 1 − 1/DRIFT_TAU,
// σ_step chosen so the stationary σ is DRIFT_SIGMA, then reflected into
// ±DRIFT_BOUND. Mean reversion is what keeps the long-run mean at the
// baseline (the M83/M85 stationarity gates); the walls are the configured
// hard stop the "no runaway" law names — at 5.5σ they almost never fire,
// which is exactly what a wall should do. The step noise is an Irwin–Hall
// 12-sum (bounded at ±6σ), so no single year can jump: the drift is slow
// by construction, not by luck.
//
// Derived state, like the rest of the sky (ADR-0003): nothing stored,
// hashed or packed. The curve is a pure function of (seed, year) — the
// walk is re-run from the dawn on demand, and `World::year_drift` memoizes
// the year's value so the sim pays the walk once per year, not per site.
// Prehistory (year ≤ 0) reads 0: the dawn *is* the baseline epoch, and the
// generated fields are the mean climate the walk wanders around.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;

/// °C — the reflecting walls. The configured excursion bound the M83/M85
/// gates check: no year's drift may stand beyond it, ever.
pub const DRIFT_BOUND: f64 = 3.0;
/// Years — the walk's memory (mean-reversion e-folding time). Century
/// scale: a warm age is generations long, not a bad decade.
pub const DRIFT_TAU: f64 = 140.0;
/// °C — the walk's stationary σ. Earth's Holocene multicentennial
/// global-mean swing runs a few tenths of a degree; ~0.5 is lively enough
/// to be felt in harvests and far short of the walls.
pub const DRIFT_SIGMA: f64 = 0.55;
/// Stream key: the drift's draws share nothing with the famine die, the
/// oscillation or the variability lattice.
pub const DRIFT_STREAM_KEY: u64 = 0xD21F_7C01_5EC0_1A2u64;

/// The slow drift of one world's climate — a law, not a table.
pub struct Drift {
    seed: i64,
}

impl Drift {
    /// Draw the walk's identity from the seed. Same seed ⇒ same century
    /// history, forever.
    pub fn new(seed: i64) -> Self {
        Self { seed }
    }

    /// One step of the walk. Kept as the single site of the arithmetic so
    /// `value` and `scan` cannot drift apart: both call this, in order.
    #[inline]
    fn step(v: f64, rng: &mut Pcg64Mcg) -> f64 {
        let phi = 1.0 - 1.0 / DRIFT_TAU;
        let sigma_step = DRIFT_SIGMA * (1.0 - phi * phi).sqrt();
        // Irwin–Hall 12-sum: mean 6, variance 1 — a bounded gaussian.
        let mut g = 0.0f64;
        for _ in 0..12 {
            g += rng.gen::<f64>();
        }
        let mut next = phi * v + sigma_step * (g - 6.0);
        // Reflecting walls — the configured hard stop.
        if next > DRIFT_BOUND {
            next = 2.0 * DRIFT_BOUND - next;
        }
        if next < -DRIFT_BOUND {
            next = -2.0 * DRIFT_BOUND - next;
        }
        next
    }

    fn rng(&self) -> Pcg64Mcg {
        Pcg64Mcg::seed_from_u64((self.seed as u64) ^ DRIFT_STREAM_KEY)
    }

    /// The drift at a given year, °C on the baseline `tmean`. O(year):
    /// the walk is re-run from the dawn — sim callers go through
    /// `World::year_drift`, which memoizes the year.
    pub fn value(&self, year: i64) -> f64 {
        if year <= 0 {
            return 0.0;
        }
        let mut rng = self.rng();
        let mut v = 0.0f64;
        for _ in 0..year {
            v = Self::step(v, &mut rng);
        }
        v
    }

    /// The whole curve in one pass: index = year, `scan(n)[0] = 0.0` (the
    /// dawn), `scan(n)[k] = value(k)` bit-for-bit. For the diagnostics
    /// that read millennia.
    pub fn scan(&self, years: usize) -> Vec<f64> {
        let mut out = Vec::with_capacity(years + 1);
        out.push(0.0);
        let mut rng = self.rng();
        let mut v = 0.0f64;
        for _ in 0..years {
            v = Self::step(v, &mut rng);
            out.push(v);
        }
        out
    }

    /// A fixed, world-independent read of the walk for the replay identity
    /// line, exactly as M73/M74 probe their sources: the law's constants
    /// plus the curve at spaced years. A drift whose keying or arithmetic
    /// moves breaks replay here.
    pub fn probe(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(8 * 10);
        for v in [DRIFT_BOUND, DRIFT_TAU, DRIFT_SIGMA] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for year in [1i64, 37, 211, 997, 4001] {
            b.extend_from_slice(&self.value(year).to_bits().to_le_bytes());
        }
        crate::util::fnv1a64(&b)
    }
}

// ------------------------------------------------------- M85 · no runaway
//
// The drift's discipline, declared where the law lives so the millennium
// lane in `diagnose climate` reads the same envelope the law promises.
// M83 gates the walk in isolation; M85 gates the *composed* sky — the
// forced temperature field the world actually breathes, drift plus the
// year's anomaly lattice plus the oscillation's shape — sampled at
// century cadence across a millennium. A runaway would show here even if
// every isolated term behaved: a coupling that rectifies (a term whose
// grid mean rides the drift's sign, say) would walk the global mean off
// the baseline while each law's own lane stayed green.

/// M85 — the millennium probe's horizon, years.
pub const MILLEN_YEARS: usize = 1000;
/// M85 — the probe's cadence: the composed sky is sampled every
/// this-many years (the spec's 100-year intervals).
pub const MILLEN_STEP: usize = 100;
/// M85 — °C: the envelope on the millennium's *mean* — "global mean
/// temperature drift under 0.5 °C over the full run" is a statement
/// about where the run's mean stands, and this is that 0.5.
pub const MILLEN_TREND_BOUND: f64 = 0.5;
/// M85 — how many of the law's own σ the fitted trend may reach. The
/// envelope for the *trend* row is not a hand-picked °C figure: it is
/// derived from the declared law itself (`millen_trend_sigma`), because
/// the banked M83 constants (σ 0.55, τ 140) mathematically imply that a
/// perfectly lawful, stationary millennium carries an OLS-trend
/// dispersion of ~0.83 °C — a fixed 0.5 °C bound on that estimator
/// would contradict the law the M83 lane already proved. 3σ contains
/// every lawful realization; a genuine runaway grows without bound and
/// fails it regardless.
pub const MILLEN_TREND_Z: f64 = 3.0;

/// M85 — the analytic dispersion (°C per millennium) of the OLS trend
/// fitted through the century samples, under the declared drift law:
/// AR(1) with per-year φ = 1 − 1/DRIFT_TAU and stationary σ =
/// DRIFT_SIGMA, so cov(sample_i, sample_j) = σ²·φ^|Δyears|. The
/// reflecting walls only ever *shrink* this dispersion (they fold the
/// tails inward), so a bound stated on the unreflected law contains the
/// walled walk a fortiori. Pure arithmetic on the declared constants —
/// change the law and the envelope moves with the declaration.
pub fn millen_trend_sigma() -> f64 {
    let n = MILLEN_YEARS / MILLEN_STEP;
    let phi = 1.0 - 1.0 / DRIFT_TAU;
    let years: Vec<f64> = (1..=n).map(|k| (k * MILLEN_STEP) as f64).collect();
    let mean_y = years.iter().sum::<f64>() / n as f64;
    let den: f64 = years.iter().map(|y| (y - mean_y) * (y - mean_y)).sum();
    let mut s = 0.0f64;
    for i in 0..n {
        for j in 0..n {
            let cov = DRIFT_SIGMA * DRIFT_SIGMA * phi.powf((years[i] - years[j]).abs());
            s += (years[i] - mean_y) * (years[j] - mean_y) * cov;
        }
    }
    (s / (den * den)).sqrt() * MILLEN_YEARS as f64
}



// -------------------------------------------------- M84 · belts on the move
//
// The drift is not just a thermometer reading — a warmer world carries its
// tropics wider and its storm tracks poleward; a colder one pulls both in.
// M84 couples the M83 drift into the two belt geometries the world already
// owns: the ITCZ camps (the ±ITCZ_CAMP_LAT convective gaussians the
// baseline precipitation marched between) and the storm corridors (whose
// fuel lines — sea-freeze for the frontal engine, TROP_SST_MIN for the
// warm-sea one — move when the sea under them warms or cools). The rain
// side is exact: `belt_anomaly` is the fractional change a camp shifted by
// `BELT_SHIFT_DEG_PER_C · drift` degrees poleward makes against the
// unshifted climatology, entering the `dp` lane beside the M75 tilt. The
// storm side names no latitude (M77's law): `storms.rs` re-reads its own
// fuel ramps at the drifted SST, and the band moves because the fuel line
// does. Both couplings are pure in (seed, year); drift 0 is exactly the
// unshifted world, to the bit.

/// The ITCZ camp latitude, degrees off the equator — the two convective
/// gaussians the baseline precipitation marches between (one per solstice).
pub const ITCZ_CAMP_LAT: f64 = 10.0;
/// The camp gaussian's width, degrees of latitude.
pub const ITCZ_CAMP_WIDTH: f64 = 12.0;
/// The camp gaussian's convective boost at its centre.
pub const ITCZ_CAMP_BOOST: f64 = 1.7;
/// M84 — degrees of latitude the belts walk per °C of drift, poleward when
/// warm. Earth's models put Hadley-edge/ITCZ migration at roughly 1–2° per
/// °C of global mean; 1.4 lands the maximum excursion at the ±3 °C walls
/// on 4.2° — inside the 5° bound the M84 gate holds.
pub const BELT_SHIFT_DEG_PER_C: f64 = 1.4;

/// One ITCZ camp's convective factor at a signed latitude.
#[inline]
pub fn itcz_camp(lat_s: f64, camp_lat: f64) -> f64 {
    1.0 + ITCZ_CAMP_BOOST * (-((lat_s - camp_lat) / ITCZ_CAMP_WIDTH).powi(2)).exp()
}

/// M84 — the fractional rain change at a signed latitude when the drift
/// walks both ITCZ camps `BELT_SHIFT_DEG_PER_C · drift` degrees poleward
/// (equatorward when the drift is cold). Exactly zero at drift 0: the
/// moved climatology over the standing one, minus one. A row law — it
/// joins the `dp` lane beside the M75 teleconnection tilt.
#[inline]
pub fn belt_anomaly(lat_s: f64, drift: f64) -> f64 {
    if drift == 0.0 {
        return 0.0;
    }
    let d = BELT_SHIFT_DEG_PER_C * drift;
    let base = 0.5 * (itcz_camp(lat_s, ITCZ_CAMP_LAT) + itcz_camp(lat_s, -ITCZ_CAMP_LAT));
    let moved = 0.5
        * (itcz_camp(lat_s, ITCZ_CAMP_LAT + d) + itcz_camp(lat_s, -(ITCZ_CAMP_LAT + d)));
    moved / base - 1.0
}

// -------------------------------------------------- M92 · monsoon fortune
//
// The monsoon is the seasonal lean the dawn already measured (`pamp`, the
// signed share of the year's rain that arrives with the local summer as
// the ITCZ marches). M92 names the year's fortune for that lean: the
// composed rain anomaly — the M71 draw, the M75 tilt (the mode's grip),
// the M84 belt walk (the drift's) — read as the delivery of the monsoon
// against the gale-grade scale the trade law already fixed (M48's
// MONSOON_GALE: |pamp| 0.40 is a full monsoon in this world — one scale
// for land and sea, the one-lattice-law discipline of ADR-0026). A paddy
// on a river reads the catchment's sky, not its own cell: the flood
// pulse that fills it is the monsoon over the whole basin (the Nile
// fails when Ethiopia's rains do), so the channel widens the sky that
// must fail — it does not exempt the paddy from it.

/// A cell is monsoon-fed when at least this share of its rain leans into
/// the local summer (|pamp|). The scale is the trade law's own: M48's
/// MONSOON_LANE (0.12) is where a sea lane starts sailing the monsoon
/// calendar, and a paddy needs a slightly firmer lean than a sail.
/// Below it there is no monsoon to fail — the paddy drinks steady rain
/// and keeps its old immunity.
pub const MONSOON_LEAN_MIN: f64 = 0.15;

/// The lean at which the index reads the year's anomaly at full
/// strength — M48's gale grade (MONSOON_GALE = 0.40), the deepest
/// monsoon this world carries. Weaker leans measure their fortune on
/// the same scale: their monsoon is a smaller share of the rain, and
/// the steadier remainder buffers the paddy in exactly that proportion.
pub const MONSOON_REF: f64 = 0.40;

/// The failed-monsoon threshold: below this share of a normal year's
/// monsoon the paddies do not fill, and the shortfall saturates at
/// MONSOON_SAT, where the harvest is simply gone. Calibrated the M82
/// way — against the record's cadence, not the eye: the pair must land
/// the per-place return time of a failed monsoon inside the tropical
/// drought envelope (DROUGHT_RETURN: 12–200 y) on the report seeds.
/// 0.55 read 13%/y on seed 12345, whose paddies sit in a
/// teleconnection hot zone (dry-forced mean index 0.704) — an ordinary
/// dry-mode year is not a famine; 0.45 keeps "failed" famine-grade.
pub const MONSOON_FAIL: f64 = 0.45;
pub const MONSOON_SAT: f64 = 0.20;

/// M92 — the monsoon-strength index at one cell in one year: the share
/// of its normal monsoon the year delivered, 1.0 = a normal year. Pure
/// in (seed, cell, year): the same composed sky every rain reader
/// consumes (`year_anomaly_at` — draw + tilt + belt), read against the
/// gale-grade lean (`MONSOON_REF`); a deeper lean than the reference
/// steadies the core, exactly as the record has it — the famine belt
/// is the margin of the monsoon, never its heart.
#[inline]
pub fn monsoon_index(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    x: usize,
    y: usize,
    year: i64,
    osc: f64,
    drift: f64,
    lean: f64,
) -> f64 {
    let dp = rain_anomaly_at(noise, rows, x, y, year, osc, drift);
    monsoon_of(dp, lean)
}

/// The monsoon index from a rain anomaly already in hand — the one
/// expression both the point and the catchment readings end in, so the
/// tick's memo-served basin read (`World::catchment_rain_anomaly`) and the
/// raw law below cannot drift apart.
#[inline]
pub fn monsoon_of(dp: f64, lean: f64) -> f64 {
    1.0 + dp / lean.abs().max(MONSOON_REF)
}

/// M92 — the riverine paddy's index: the same law, but the rain is the
/// basin's (`catchment_anomaly_at`, the exact gaussian the M81 floods
/// read) — the flood pulse integrates the monsoon over the catchment,
/// so a delta paddy fails only when the wider sky does.
#[inline]
pub fn monsoon_index_catchment(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    cols: usize,
    x: usize,
    y: usize,
    year: i64,
    osc: f64,
    drift: f64,
    lean: f64,
) -> f64 {
    let dp = catchment_anomaly_at(noise, rows, cols, x, y, year, osc, drift);
    monsoon_of(dp, lean)
}

/// 1σ of the year-to-year temperature swing at this latitude, °C.
pub fn anomaly_amp_t(lat_abs: f64) -> f64 {
    ANOM_T_EQ + (ANOM_T_POLE - ANOM_T_EQ) * (lat_abs.abs() / 90.0).clamp(0.0, 1.0).powf(ANOM_LAT_POW)
}

/// 1σ of the year-to-year rainfall swing at this latitude, as a fraction.
pub fn anomaly_amp_p(lat_abs: f64) -> f64 {
    ANOM_P_EQ + (ANOM_P_POLE - ANOM_P_EQ) * (lat_abs.abs() / 90.0).clamp(0.0, 1.0).powf(ANOM_LAT_POW)
}

/// The raw (unscaled) anomaly draw for one cell in one year — the shared
/// lattice both lanes read, exposed so diagnostics can measure its σ
/// rather than trust the constant above.
pub fn anomaly_draw(noise: &crate::noisegen::Perlin3, x: usize, y: usize, year: i64, lane: f64) -> f64 {
    noise.fbm(
        x as f64 * ANOM_SPACE,
        y as f64 * ANOM_SPACE,
        year as f64 * ANOM_YEAR_STEP + lane,
        2,
    )
}

/// M71 — the year's weather: `(dt, dp)` over the whole grid, where `dt`
/// is degrees added to `tmean` and `dp` is the fractional change applied
/// to `precip` (`precip * (1 + dp)`). `rows` carries the latitude, which
/// is a property of the row alone (margins widen columns, never rows).
/// M83 — `drift` is the century's global offset, entering the temperature
/// lane ahead of the year's draw; 0.0 asks for the unforced amplitude law
/// alone (the quantity M71/M73 declare and gate). M84 — the same drift
/// moves the belts: the rain lane carries `belt_anomaly`, the fractional
/// change of ITCZ camps walked poleward with the warmth, beside the M75
/// tilt. Drift 0 is the unshifted rain law to the bit.
pub fn year_anomaly(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    cols: usize,
    year: i64,
    osc: f64,
    drift: f64,
) -> (Array2<f64>, Array2<f64>) {
    let mut dt = Array2::<f64>::zeros((rows, cols));
    let mut dp = Array2::<f64>::zeros((rows, cols));
    let n = rows as f64;
    for y in 0..rows {
        let lat_signed = -90.0 + (y as f64) * 180.0 / (n - 1.0);
        let lat = lat_signed.abs();
        // M95 — one sky, to the bit. The row's amplitudes are hoisted (a
        // powf each), but the per-cell arithmetic must be *the same
        // operations in the same order* as `year_anomaly_at`: the harvest
        // reads `draw * amp / σ`, so the grid does too. The earlier form
        // pre-divided the amplitude (`draw * (amp / σ)`) and landed one
        // ulp off the pointwise law on ~15% of cells — two skies, which
        // the M95 audit caught (291/343 famine rows equal on seed 12345).
        // The extra division per cell is noise against the fbm draws.
        let amp_t = anomaly_amp_t(lat);
        // M75: the tilt is a property of the row, drawn once per row.
        // M84: so is the belt — the camps move with the century, not the cell.
        // Both live in `RowSky` now, with the amplitude; the per-cell rain
        // expression is `rain_anomaly_row`, shared with every pointwise path.
        let row = RowSky::at(rows, y, osc, drift);
        for x in 0..cols {
            dt[[y, x]] = drift + anomaly_draw(noise, x, y, year, 0.0) * amp_t / ANOM_FBM_SIGMA;
            dp[[y, x]] = rain_anomaly_row(noise, x, y, year, &row);
        }
    }
    (dt, dp)
}

/// One cell of `year_anomaly`, for simulation consumers that only need the
/// weather where people live. This is the same law as the full diagnostic
/// grid; avoiding hundreds of thousands of unobserved cells is a material
/// part of the tick budget once weather changes every year.
/// `drift` as in `year_anomaly`: since M84 the rain lane carries the belt
/// term too, so rain-reading callers must pass the year's real drift; 0.0
/// asks for the unforced twin on both lanes.
#[inline]
pub fn year_anomaly_at(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    x: usize,
    y: usize,
    year: i64,
    osc: f64,
    drift: f64,
) -> (f64, f64) {
    let n = rows as f64;
    let lat_signed = -90.0 + (y as f64) * 180.0 / (n - 1.0);
    let lat = lat_signed.abs();
    let dt = drift + anomaly_draw(noise, x, y, year, 0.0) * anomaly_amp_t(lat) / ANOM_FBM_SIGMA;
    let dp = rain_anomaly_row(noise, x, y, year, &RowSky::at(rows, y, osc, drift));
    (dt, dp)
}

/// The rain lane of `year_anomaly_at` alone — the same expression to the
/// bit, without drawing the temperature lane nobody asked for. Every
/// rain-only reader (the drought ledger's memory walk, the monsoon index,
/// the catchment kernel) comes through here; the tick's own per-site
/// weather memo still takes both lanes through `year_anomaly_at`.
#[inline]
pub fn rain_anomaly_at(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    x: usize,
    y: usize,
    year: i64,
    osc: f64,
    drift: f64,
) -> f64 {
    rain_anomaly_row(noise, x, y, year, &RowSky::at(rows, y, osc, drift))
}

/// The three terms of the rain lane that belong to the *row*, not the
/// cell: the latitude amplitude (a powf), the M75 tilt and the M84 belt
/// walk (four exps). `year_anomaly` hoists them per row already; the
/// catchment kernel below reads forty-nine rows and used to re-solve them
/// at every one of its ~2,400 taps. Hoisting changes no value — each term
/// is a pure function of the row and the year's forcing — and the per-cell
/// arithmetic in `rain_anomaly_row` keeps the operation order the M95
/// audit pinned (`draw * amp / σ + tilt + belt`).
#[derive(Clone, Copy, Debug)]
pub struct RowSky {
    pub amp_p: f64,
    pub tilt: f64,
    pub belt: f64,
}

impl RowSky {
    #[inline]
    pub fn at(rows: usize, y: usize, osc: f64, drift: f64) -> RowSky {
        let n = rows as f64;
        let lat_signed = -90.0 + (y as f64) * 180.0 / (n - 1.0);
        RowSky {
            amp_p: anomaly_amp_p(lat_signed.abs()),
            tilt: teleconnection_bias(osc, lat_signed),
            belt: belt_anomaly(lat_signed, drift),
        }
    }
}

/// The rain lane at one cell given its row's hoisted terms. This is *the*
/// per-cell rain expression: `year_anomaly` (full grid), `year_anomaly_at`
/// (one cell), `rain_anomaly_at` and the catchment kernel all evaluate it,
/// so there is one sky to the bit whichever path reads it.
#[inline]
pub fn rain_anomaly_row(
    noise: &crate::noisegen::Perlin3,
    x: usize,
    y: usize,
    year: i64,
    row: &RowSky,
) -> f64 {
    (anomaly_draw(noise, x, y, year, ANOM_RAIN_LANE) * row.amp_p / ANOM_FBM_SIGMA + row.tilt + row.belt)
        .max(ANOM_P_FLOOR)
}

/// The separable Gaussian catchment reading at one cell. The arithmetic and
/// reflect boundary are deliberately identical to `ndimage::gaussian_filter`:
/// diagnostics may ask for the full field, while ticks pay only for inhabited
/// cells and receive the same value bit-for-bit. `drift` as everywhere since
/// M84: the catchment must read the same belt-carrying rain the sky rains.
///
/// Cost shape (M95 perf pass): the kernel is (8σ+1)² ≈ 2,400 taps of the
/// rain draw. The row terms are hoisted once per kernel row and the
/// temperature lane is never drawn — the tap is one 2-octave fbm plus the
/// three-term sum, which is what the law actually costs. Values are
/// unchanged: same taps, same weights, same summation order.
pub fn catchment_anomaly_at(
    noise: &crate::noisegen::Perlin3,
    rows: usize,
    cols: usize,
    x: usize,
    y: usize,
    year: i64,
    osc: f64,
    drift: f64,
) -> f64 {
    let sigma = CATCHMENT_SIGMA;
    let radius = (4.0 * sigma + 0.5) as isize;
    let s2 = 2.0 * sigma * sigma;
    let mut kernel: Vec<f64> = (-radius..=radius)
        .map(|d| (-(d * d) as f64 / s2).exp())
        .collect();
    let sum: f64 = kernel.iter().sum();
    for value in &mut kernel {
        *value /= sum;
    }

    let mut out = 0.0;
    for (jy, ky) in kernel.iter().enumerate() {
        let yy = crate::ndimage::reflect(y as isize + jy as isize - radius, rows as isize);
        let row = RowSky::at(rows, yy, osc, drift);
        let mut horizontal = 0.0;
        for (jx, kx) in kernel.iter().enumerate() {
            let xx = crate::ndimage::reflect(x as isize + jx as isize - radius, cols as isize);
            // Rain lane, belt included (M84) — bit-equal to the full filter.
            horizontal += kx * rain_anomaly_row(noise, xx, yy, year, &row);
        }
        out += ky * horizontal;
    }
    out
}

// ---------------------------------------------------------------- bands


use crate::util::Band;

/// Diagnostics bands (E2.7): temperature, rain and the seasons.
pub const BANDS: &[Band] = &[
    Band { name: "land mean temperature", sweet: (5.0, 20.0), hard: (-2.0, 28.0), target: "sweet 5–20°C · hard -2–28°C" },
    Band { name: "land mean precipitation", sweet: (500.0, 1500.0), hard: (250.0, 2400.0), target: "sweet 500–1500 · hard 250–2400" },
    Band { name: "mean seasonal swing", sweet: (4.0, 14.0), hard: (2.0, 20.0), target: "sweet 4–14°C · hard 2–20°C" },
    Band { name: "tropical monsoon amplitude", sweet: (0.12, 0.55), hard: (0.05, 0.85), target: "sweet .12–.55 · hard .05–.85" },
    Band { name: "warm-coast heat delta", sweet: (0.75, 6.0), hard: (0.3, 10.0), target: "sweet +0.75..+6 °C · hard +0.3..+10 (M41: mean bias over land the warm rims reach (≥ +0.5); Gulf-Stream coasts run a few degrees over their zonal law)" },
    // M89 — the margins respond: sensitivities of the three margin laws
    // to a ±1 °C composed forcing, measured on the dawn fields. Bands
    // calibrated on the report seeds after measurement (ADR-0009 loop).
    Band { name: "snowline walk m per degC", sweet: (80.0, 260.0), hard: (40.0, 400.0), target: "M89: belt-mean balance-zero elevation rise per +1 °C — Earth's glaciers read ~120–200 m/°C ELA shift" },
    Band { name: "treeline breathing pp per degC", sweet: (0.5, 6.0), hard: (0.15, 12.0), target: "M89: percentage points of land crossing GDD-500 tree eligibility per +1 °C — the cold margin, not the whole forest" },
    Band { name: "pack breathing pp per degC", sweet: (0.3, 5.0), hard: (0.1, 10.0), target: "M89: percentage points of ocean area (cos-weighted) leaving the ever-frozen pack per +1 °C of warming" },
    Band { name: "cold-coast heat delta", sweet: (-6.0, -0.75), hard: (-10.0, -0.3), target: "sweet −6..−0.75 °C · hard −10..−0.3 (M41: mean bias over land the cold rims reach (≤ −0.5); Humboldt/Benguela coasts run a few degrees under)" },
    Band { name: "heat transport net bias", sweet: (0.0, 0.3), hard: (0.0, 0.6), target: "sweet ≤0.3 °C · hard ≤0.6 (M41: |world-mean bias| — advection redistributes heat, it must not mint it)" },
    Band { name: "cold-rim rain suppression", sweet: (0.25, 0.80), hard: (0.10, 0.95), target: "sweet 0.25–0.80 · hard 0.10–0.95 (M42: sub-polar cold-rim coastal land against aspect-matched neutral coasts at its latitude — the Atacama law)" },
    Band { name: "warm-rim rain boost", sweet: (1.02, 2.20), hard: (0.98, 3.50), target: "sweet 1.02–2.20 · hard 0.98–3.50 (M42: sub-polar warm-rim coastal land against aspect-matched neutral coasts at its latitude — the Gulf-Stream law)" },
    Band { name: "current-coast warm anomaly", sweet: (0.1, 6.0), hard: (0.0, 12.0), target: "sweet +0.1..+6 °C · hard 0..+12 (M49: mean tmean of warm-rim coastal land against its own row's coastal mean)" },
    Band { name: "current-coast cold anomaly", sweet: (-6.0, -0.1), hard: (-12.0, 0.0), target: "sweet −6..−0.1 °C · hard −12..0 (M49: mean tmean of cold-rim coastal land against its own row's coastal mean)" },
    Band { name: "upwelling median latitude", sweet: (5.0, 60.0), hard: (0.0, 75.0), target: "sweet 5–60° · hard 0–75° (M49: median |latitude| of nutrient-rich coast — Earth's eastern-boundary systems run 5–45°)" },
    Band { name: "upwelling share of coastline", sweet: (0.03, 0.10), hard: (0.015, 0.15), target: "sweet 3–10% · hard 1.5–15% (M47: nutrient-rich share of coastal ocean cells — Earth's eastern-boundary analogues)" },
];
