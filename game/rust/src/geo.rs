//! Terrain generation — port of geo.py (itself ported from geo.hy),
//! grown into a plate-and-plume model: continental bulges broken by
//! low-frequency plate noise, volcanic island arcs riding the deep
//! ocean, archipelago shoal-fields, and hotspot chains whose islands
//! sink with age. Every pass is deterministic in the seed.

use ndarray::Array2;
use rand::Rng;

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

/// One volcanic cone: pull the seabed toward `peak` with a Gaussian falloff.
/// The tails of the Gaussian leave a shallow skirt — the island's shelf.
fn splat_island(h: &mut Array2<f64>, cx: f64, cy: f64, peak: f64, sigma: f64) {
    let (rows, cols) = h.dim();
    let r = (sigma * 3.4).ceil() as isize;
    let x0 = ((cx as isize) - r).max(0);
    let x1 = ((cx as isize) + r).min(cols as isize - 1);
    let y0 = ((cy as isize) - r).max(0);
    let y1 = ((cy as isize) + r).min(rows as isize - 1);
    let inv = 1.0 / (2.0 * sigma * sigma);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            let g = (-(dx * dx + dy * dy) * inv).exp();
            let cell = &mut h[[y as usize, x as usize]];
            if *cell > 0.08 {
                continue; // leave standing coasts untouched
            }
            let target = *cell + g * (peak - *cell);
            if target > *cell {
                *cell = target.min(0.45);
            }
        }
    }
}

pub fn heightmap(seed: i64, size: usize) -> Array2<f64> {
    let base = Perlin3::new(seed);
    let warp = Perlin3::new(seed + 101);
    let ridge = Perlin3::new(seed + 202);
    let arcn = Perlin3::new(seed + 303);
    let arch = Perlin3::new(seed + 404);
    let rad = radial(size);
    let n = size as f64;

    // ---- pass 1: continents — the old bulges, but plate noise lets the
    // shorelines wander so the two hemispheres stop mirroring each other.
    let mut h = Array2::from_shape_fn((size, size), |(y, x)| {
        let fx = x as f64 / n * 5.0;
        let fy = y as f64 / n * 5.0;

        // Domain warp for organic coastlines
        let wx = warp.fbm(fx + 13.7, fy + 7.1, 0.5, 2);
        let wy = warp.fbm(fx + 3.3, fy + 11.9, 1.5, 2);
        let b = base.fbm(fx + 0.35 * wx, fy + 0.35 * wy, 0.0, 6);

        let plate = base.fbm(fx * 0.45 + 91.0, fy * 0.45 + 47.0, 8.0, 2);
        let mut hh = (rad[[y, x]] * (0.82 + 0.42 * plate) + b * 1.15) / 2.0;

        // Mountain ranges: ridged noise, applied inland only so coasts stay clean
        let r = ridge.ridged(fx * 1.6 + 31.0, fy * 1.6 + 17.0, 3.3, 4);
        let inland = smoothstep(hh, 0.05, 0.32);
        hh += 0.55 * (r - 0.62).max(0.0) * inland;

        // Foothill belts: a finer, weaker ridged pass gives the great
        // ranges their aprons and raises stand-alone hill country the
        // primary orogeny never touched.
        let r2 = ridge.ridged(fx * 3.4 + 77.7, fy * 3.4 + 5.5, 6.6, 3);
        let hills = smoothstep(hh, 0.03, 0.20) * (1.0 - smoothstep(hh, 0.38, 0.55));
        hh += 0.12 * (r2 - 0.72).max(0.0) * hills;
        hh
    });

    // ---- pass 2: island arcs and archipelago fields, strictly offshore.
    for y in 0..size {
        for x in 0..size {
            let h0 = h[[y, x]];
            if h0 >= -0.04 {
                continue;
            }
            let fx = x as f64 / n * 5.0;
            let fy = y as f64 / n * 5.0;
            let oceanic = smoothstep(-h0, 0.05, 0.20);

            // Volcanic arcs: thin curved crests of ridged noise, broken
            // into separate isles by a bead modulation along the crest.
            let a = arcn.ridged(fx * 1.05 + 71.3, fy * 1.05 + 43.9, 5.5, 3);
            let crest = ((a - 0.87) / 0.13).max(0.0).min(1.0);
            let beads =
                smoothstep(arcn.noise(fx * 3.4 + 17.0, fy * 3.4 + 5.0, 2.5), -0.12, 0.30);
            let arc_lift = crest * (0.20 + 0.80 * beads);

            // Archipelago fields: gated regions of ocean where a finer
            // noise breaks the surface as shoals and scattered isles.
            let region = smoothstep(
                arch.fbm(fx * 0.55 + 55.0, fy * 0.55 + 21.0, 3.0, 2),
                0.14,
                0.40,
            );
            let sp = arch.fbm(fx * 3.1 + 9.0, fy * 3.1 + 33.0, 6.0, 3);
            let speck = smoothstep(sp, 0.22, 0.46);
            let arch_lift = region * speck;

            let lift = arc_lift.max(arch_lift);
            if lift > 0.01 {
                // rise from the local seabed toward island hills; partial
                // lifts stay submerged and read as banks and shelves.
                let t = (oceanic * lift).min(1.0);
                h[[y, x]] += t * (0.24 - h0) * 0.92;
            }
        }
    }

    // ---- pass 3: hotspot chains — a plume drifts under the plate and
    // leaves a trail of volcanoes, tallest at the young end, the elders
    // worn down to shoals and seamounts.
    let mut rng = crate::util::rng(seed * 7 + 4040);
    let chains = 2 + (rng.gen::<f64>() * 2.0) as usize;
    for _ in 0..chains {
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut ok = false;
        for _ in 0..400 {
            let cx = rng.gen_range(0.14..0.86) * n;
            let cy = rng.gen_range(0.14..0.86) * n;
            if h[[cy as usize, cx as usize]] < -0.20 {
                sx = cx;
                sy = cy;
                ok = true;
                break;
            }
        }
        if !ok {
            continue;
        }
        let dir = rng.gen_range(0.0..std::f64::consts::TAU);
        let (mut dx, mut dy) = (dir.cos(), dir.sin());
        let step = n * 0.030 + rng.gen_range(0.0..n * 0.014);
        let count = 4 + rng.gen_range(0..4);
        let (mut px, mut py) = (sx, sy);
        for i in 0..count {
            let age = i as f64 / count.max(1) as f64;
            let peak = (0.34 * (1.0 - 0.78 * age) + rng.gen_range(-0.03..0.03)).max(-0.06);
            let sigma = (n * 0.0055 + n * 0.0045 * (1.0 - age)).max(2.0);
            splat_island(&mut h, px, py, peak, sigma);
            let turn: f64 = rng.gen_range(-0.35..0.35);
            let (c, s) = (turn.cos(), turn.sin());
            let (ndx, ndy) = (dx * c - dy * s, dx * s + dy * c);
            dx = ndx;
            dy = ndy;
            px += dx * step;
            py += dy * step;
            if px < n * 0.08 || px > n * 0.92 || py < n * 0.08 || py > n * 0.92 {
                break;
            }
        }
    }

    // ---- pass 4: ocean frame — sink the height toward deep water near
    // every edge so no landmass is ever clipped by the border of the map.
    for y in 0..size {
        for x in 0..size {
            let ex = x.min(size - 1 - x) as f64 / n;
            let ey = y.min(size - 1 - y) as f64 / n;
            let frame = smoothstep(ex.min(ey), 0.012, 0.10);
            let v = h[[y, x]];
            h[[y, x]] = (v * frame - (1.0 - frame) * 0.45).clamp(-1.0, 1.0);
        }
    }
    h
}
