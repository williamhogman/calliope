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
/// Over land the marine memory decays toward neutral — except where a
/// warm rim keeps the boundary layer convective: the land target is
/// pulled up by the local positive bias at this fraction of the sea
/// gain. Cold bias never stabilizes land air (the inversion is a
/// marine artifact that breaks on landfall heating).
pub const STAB_LAND_WARM_PULL: f64 = 0.75;

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
                stab += STAB_LAND_RELAX * (1.0 - stab);
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
            rain += 0.5 * (w - cap).max(0.0);
            w -= rain;
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
            let c0 = 1.0 + 1.7 * (-((lat_s - 10.0) / 12.0).powi(2)).exp();
            let c6 = 1.0 + 1.7 * (-((lat_s + 10.0) / 12.0).powi(2)).exp();
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

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): temperature, rain and the seasons.
pub const BANDS: &[Band] = &[
    Band { name: "land mean temperature", sweet: (5.0, 20.0), hard: (-2.0, 28.0), target: "sweet 5–20°C · hard -2–28°C" },
    Band { name: "land mean precipitation", sweet: (500.0, 1500.0), hard: (250.0, 2400.0), target: "sweet 500–1500 · hard 250–2400" },
    Band { name: "mean seasonal swing", sweet: (4.0, 14.0), hard: (2.0, 20.0), target: "sweet 4–14°C · hard 2–20°C" },
    Band { name: "tropical monsoon amplitude", sweet: (0.12, 0.55), hard: (0.05, 0.85), target: "sweet .12–.55 · hard .05–.85" },
    Band { name: "warm-coast heat delta", sweet: (0.75, 6.0), hard: (0.3, 10.0), target: "sweet +0.75..+6 °C · hard +0.3..+10 (M41: mean bias over land the warm rims reach (≥ +0.5); Gulf-Stream coasts run a few degrees over their zonal law)" },
    Band { name: "cold-coast heat delta", sweet: (-6.0, -0.75), hard: (-10.0, -0.3), target: "sweet −6..−0.75 °C · hard −10..−0.3 (M41: mean bias over land the cold rims reach (≤ −0.5); Humboldt/Benguela coasts run a few degrees under)" },
    Band { name: "heat transport net bias", sweet: (0.0, 0.3), hard: (0.0, 0.6), target: "sweet ≤0.3 °C · hard ≤0.6 (M41: |world-mean bias| — advection redistributes heat, it must not mint it)" },
    Band { name: "cold-rim rain suppression", sweet: (0.30, 0.90), hard: (0.10, 0.98), target: "sweet 0.30–0.90 · hard 0.10–0.98 (M42: cold-current coastal land rains at this ratio of its latitude's land mean — Atacama/Namib run far under)" },
    Band { name: "warm-rim rain boost", sweet: (1.02, 2.20), hard: (0.98, 3.50), target: "sweet 1.02–2.20 · hard 0.98–3.50 (M42: warm-current coastal land over its latitude's land mean — Gulf-Stream coasts run wet)" },
];
