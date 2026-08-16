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

/// Wind-advected moisture -> annual precipitation in mm/yr, plus the
/// signed monsoon amplitude: how strongly the year's rain leans into
/// the local summer as the ITCZ marches between the tropics.
pub fn precipitation(
    height: &Array2<f64>,
    water: &Array2<bool>,
    tmean: &Array2<f64>,
    lat_deg: &Array2<f64>,
    cont: &Array2<f64>,
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
        for step in 0..wraps * size {
            let xcur = (d * step as isize).rem_euclid(size as isize) as usize;
            let xprev = (xcur as isize - d).rem_euclid(size as isize) as usize;
            let wat = water[[y, xcur]];
            let t = tmean[[y, xcur]];
            // Land evapotranspiration recycles a real share of moisture —
            // without it every continental interior turns to bone-dry waste.
            let evap = if wat {
                0.018 + 0.030 * t.clamp(0.0, 30.0) / 30.0
            } else {
                0.009 + 0.004 * t.clamp(0.0, 30.0) / 30.0
            };
            w += evap;
            let hcur = height[[y, xcur]].max(0.0);
            let hprev = height[[y, xprev]].max(0.0);
            let uplift = ((hcur - hprev) * size as f64 / 40.0).clamp(0.0, 3.0);
            let rate = if wat {
                0.012
            } else {
                (0.023 + 0.40 * uplift).clamp(0.0, 0.65)
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
];
