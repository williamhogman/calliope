//! Terrain generation — port of geo.py (itself ported from geo.hy).

use ndarray::Array2;

use crate::noisegen::Perlin3;

/// Two continental bulges — identical to geo.hy.
pub fn radial(size: usize) -> Array2<f64> {
    let n = size as f64;
    Array2::from_shape_fn((size, size), |(y, x)| {
        let xc = -std::f64::consts::PI
            + (x as f64) * (4.0 * std::f64::consts::PI) / (n - 1.0);
        let yc = (y as f64) * std::f64::consts::PI / (n - 1.0);
        xc.cos() * yc.sin()
    })
}

fn smoothstep(x: f64, lo: f64, hi: f64) -> f64 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn heightmap(seed: i64, size: usize) -> Array2<f64> {
    let base = Perlin3::new(seed);
    let warp = Perlin3::new(seed + 101);
    let ridge = Perlin3::new(seed + 202);
    let rad = radial(size);
    let n = size as f64;

    Array2::from_shape_fn((size, size), |(y, x)| {
        let fx = x as f64 / n * 5.0;
        let fy = y as f64 / n * 5.0;

        // Domain warp for organic coastlines
        let wx = warp.fbm(fx + 13.7, fy + 7.1, 0.5, 2);
        let wy = warp.fbm(fx + 3.3, fy + 11.9, 1.5, 2);
        let b = base.fbm(fx + 0.35 * wx, fy + 0.35 * wy, 0.0, 6);

        let mut h = (rad[[y, x]] + b * 1.15) / 2.0;

        // Mountain ranges: ridged noise, applied inland only so coasts stay clean
        let r = ridge.ridged(fx * 1.6 + 31.0, fy * 1.6 + 17.0, 3.3, 4);
        let inland = smoothstep(h, 0.05, 0.32);
        h += 0.55 * (r - 0.62).max(0.0) * inland;

        // Ocean frame: sink the height toward deep water near every edge so
        // no landmass is ever clipped by the border of the map.
        let ex = x.min(size - 1 - x) as f64 / n;
        let ey = y.min(size - 1 - y) as f64 / n;
        let frame = smoothstep(ex.min(ey), 0.012, 0.10);
        h = h * frame - (1.0 - frame) * 0.45;

        h.clamp(-1.0, 1.0)
    })
}
