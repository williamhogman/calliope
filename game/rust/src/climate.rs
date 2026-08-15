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
pub fn temperature_mean(height: &Array2<f64>, lat_deg: &Array2<f64>) -> Array2<f64> {
    Array2::from_shape_fn(height.dim(), |(y, x)| {
        let lat = lat_deg[[y, x]] / 90.0;
        let t_sea = 28.0 - 53.0 * lat.powf(1.7);
        t_sea - 26.0 * height[[y, x]].max(0.0) // 6.5 C/km * 4 km per unit
    })
}

/// 0.35 (maritime) .. 1.0 (deep continental interior).
pub fn continentality(water: &Array2<bool>) -> Array2<f64> {
    let land = water.mapv(|w| !w);
    let d = ndimage::distance_transform_edt(&land);
    d.mapv(|v| 0.35 + 0.65 * (v / 70.0).clamp(0.0, 1.0))
}

/// Signed seasonal swing: southern hemisphere positive (warm in Gamelion).
pub fn temperature_amplitude(lat_deg: &Array2<f64>, water: &Array2<bool>) -> Array2<f64> {
    let cont = continentality(water);
    let size = lat_deg.dim().0;
    Array2::from_shape_fn(lat_deg.dim(), |(y, x)| {
        let lat = lat_deg[[y, x]] / 90.0;
        let amp = (3.0 + 19.0 * lat.powf(1.2)) * cont[[y, x]];
        if y >= size / 2 {
            amp
        } else {
            -amp
        }
    })
}

pub fn month_temperature(tmean: f64, tamp_signed: f64, month: i64) -> f64 {
    tmean + tamp_signed * (2.0 * std::f64::consts::PI * month as f64 / 12.0).cos()
}

/// Wind-advected moisture -> annual precipitation in mm/yr.
pub fn precipitation(
    height: &Array2<f64>,
    water: &Array2<bool>,
    tmean: &Array2<f64>,
    lat_deg: &Array2<f64>,
) -> Array2<f64> {
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
            let evap = if wat {
                0.018 + 0.030 * t.clamp(0.0, 30.0) / 30.0
            } else {
                0.0035
            };
            w += evap;
            let hcur = height[[y, xcur]].max(0.0);
            let hprev = height[[y, xprev]].max(0.0);
            let uplift = ((hcur - hprev) * size as f64 / 40.0).clamp(0.0, 3.0);
            let rate = if wat {
                0.012
            } else {
                (0.030 + 0.40 * uplift).clamp(0.0, 0.65)
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

    // ITCZ convective boost, subtropical subsidence suppression, cold-air factor
    for y in 0..size {
        for x in 0..size {
            let lat = lat_deg[[y, x]];
            let t = tmean[[y, x]];
            let mut v = p[[y, x]];
            v *= 1.0 + 0.9 * (-(lat / 10.0).powi(2)).exp();
            v *= 1.0 - 0.38 * (-(((lat - 25.0) / 8.0).powi(2))).exp();
            v *= (0.25 + (t + 20.0) / 40.0).clamp(0.25, 1.0);
            p[[y, x]] = v;
        }
    }

    let mut p = ndimage::gaussian_filter(&p, 1.4);

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
    p
}
