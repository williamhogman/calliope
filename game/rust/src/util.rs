//! Shared helpers: clocks, seeded RNG, numpy-style scalar utilities.

use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;

/// Milliseconds since epoch — native and wasm.
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        * 1000.0
}

/// Deterministic RNG from a (possibly offset) world seed.
pub fn rng(seed: i64) -> Pcg64Mcg {
    Pcg64Mcg::seed_from_u64(seed as u64)
}

/// np.quantile with linear interpolation (numpy's default).
pub fn quantile(vals: &[f64], q: f64) -> f64 {
    let mut v: Vec<f64> = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if v.is_empty() {
        return 0.0;
    }
    let h = (v.len() - 1) as f64 * q;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let frac = h - lo as f64;
    v[lo] + (v[hi] - v[lo]) * frac
}

/// np.interp: piecewise-linear, clamped at the ends.
pub fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[xs.len() - 1] {
        return ys[ys.len() - 1];
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
            return ys[i - 1] + t * (ys[i] - ys[i - 1]);
        }
    }
    ys[ys.len() - 1]
}

pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
