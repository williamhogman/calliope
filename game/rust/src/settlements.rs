//! Settlements — port of settlements.py: founding, tiers, colony siting.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::constants as gc;
use crate::naming;
use crate::ndimage;
use crate::resources::{isa_chain, Deposit};

pub const TIERS: [(i64, &str); 4] = [
    (0, "Camp"),
    (250, "Village"),
    (1000, "Town"),
    (5000, "City"),
];

pub fn tier(pop: i64) -> String {
    let mut name = TIERS[0].1;
    for (threshold, t) in TIERS {
        if pop >= threshold {
            name = t;
        }
    }
    name.to_string()
}

#[derive(Serialize, Clone)]
pub struct Settlement {
    pub id: i64,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub pop: i64,
    pub tier: String,
    pub food: f64,
    pub coastal: bool,
    pub river: bool,
    pub culture: usize,
    pub connections: i64,
    pub goods: Vec<String>,
    pub exports: Option<String>,
    pub wealth: f64,
    /// true when this town's trade takes to the sea at its own quays
    pub port: bool,
}

pub fn capacity(s: &Settlement) -> f64 {
    900.0 * s.food.max(0.3)
}

pub fn territory_radius(pop: i64) -> f64 {
    2.0 + 2.4 * (pop.max(10) as f64).log10()
}

pub fn site_food(
    food_grid: &Array2<f64>,
    fert: &Array2<f64>,
    near_fresh: &Array2<bool>,
    coast: &Array2<bool>,
    y: usize,
    x: usize,
) -> f64 {
    let v = food_grid[[y, x]]
        + 1.6 * fert[[y, x]]
        + 1.4 * (near_fresh[[y, x]] as u8 as f64)
        + (coast[[y, x]] as u8 as f64);
    crate::util::round2(v.max(0.35))
}

pub struct Founded {
    pub settlements: Vec<Settlement>,
    pub site_score: Array2<f64>,
    pub food_grid: Array2<f64>,
    pub near_fresh: Array2<bool>,
    pub coast: Array2<bool>,
    pub max_settlements: usize,
}

/// Score cells and greedily found settlements with min spacing.
#[allow(clippy::too_many_arguments)]
pub fn found_settlements(
    height: &Array2<f64>,
    biomes: &Array2<u8>,
    tmean: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
    deposits: &[Deposit],
    fert: &Array2<f64>,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
) -> Founded {
    let size = height.dim().0;
    let land = height.mapv(|h| h >= 0.0);
    let sea = height.mapv(|h| h < 0.0);

    let sea_adj = ndimage::binary_dilation(&sea, 2);
    let riv_adj = ndimage::binary_dilation(rivers, 2);
    let lake_adj = ndimage::binary_dilation(lakes, 2);
    let coast = Array2::from_shape_fn((size, size), |(y, x)| land[[y, x]] && sea_adj[[y, x]]);
    let near_fresh = Array2::from_shape_fn((size, size), |(y, x)| {
        land[[y, x]] && (riv_adj[[y, x]] || lake_adj[[y, x]])
    });

    // River deltas: where a great river meets the tide, the silt piles
    // deep and every keel and cart must meet. The bigger the river, the
    // harder its mouth pulls settlement toward it.
    let mut delta = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            if !rivers[[y, x]] || discharge[[y, x]] < 60.0 {
                continue;
            }
            let mut mouth = false;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (ny, nx) = (y as i64 + dy, x as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= size as i64 || nx >= size as i64 {
                        continue;
                    }
                    if height[[ny as usize, nx as usize]] < 0.0 {
                        mouth = true;
                    }
                }
            }
            if !mouth {
                continue;
            }
            let w = (discharge[[y, x]] / 300.0).sqrt().clamp(0.6, 1.8);
            let rr = 6i64;
            for dy in -rr..=rr {
                for dx in -rr..=rr {
                    let (ny, nx) = (y as i64 + dy, x as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= size as i64 || nx >= size as i64 {
                        continue;
                    }
                    let d = ((dy * dy + dx * dx) as f64).sqrt();
                    if d > rr as f64 {
                        continue;
                    }
                    let v = w * (1.0 - d / (rr as f64 + 1.0));
                    let cell = &mut delta[[ny as usize, nx as usize]];
                    if v > *cell {
                        *cell = v;
                    }
                }
            }
        }
    }

    // food kernel from deposits whose ISA chain reaches "food"
    let mut food = Array2::<f64>::zeros((size, size));
    for d in deposits {
        if isa_chain(&d.r).iter().any(|s| s == "food") {
            food[[d.y as usize, d.x as usize]] += d.rich;
        }
    }
    let mut food = ndimage::gaussian_filter(&food, 5.0).mapv(|v| (v * 60.0).clamp(0.0, 3.0));
    // delta silt and estuary fisheries feed towns for free
    food.zip_mut_with(&delta, |f, &d| *f += 0.9 * d);

    let mut score = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            if !land[[y, x]] {
                score[[y, x]] = -1e9;
                continue;
            }
            let comfort = (-(((tmean[[y, x]] - 12.0) / 14.0).powi(2))).exp();
            let b = biomes[[y, x]];
            score[[y, x]] = 2.2 * (near_fresh[[y, x]] as u8 as f64)
                + 1.6 * (coast[[y, x]] as u8 as f64)
                + 2.8 * delta[[y, x]]
                + food[[y, x]]
                + 2.0 * comfort
                + 2.6 * fert[[y, x]]
                - 2.5 * ((b == gc::DESERT) as u8 as f64)
                - 3.5 * ((b == gc::ICE) as u8 as f64)
                - 1.5 * ((b == gc::TUNDRA) as u8 as f64)
                - 2.0 * (height[[y, x]] - 0.5).clamp(0.0, 1.0) * 4.0;
        }
    }

    let mut settlements: Vec<Settlement> = Vec::new();
    let mut working = score.clone();
    let n_target = (size / 32).max(6);
    let min_dist = size as f64 / 18.0;
    let min_d2 = min_dist * min_dist;

    for _ in 0..n_target * 3 {
        if settlements.len() >= n_target {
            break;
        }
        // row-major first maximum, like np.argmax
        let mut best = f64::NEG_INFINITY;
        let (mut by, mut bx) = (0usize, 0usize);
        for y in 0..size {
            for x in 0..size {
                if working[[y, x]] > best {
                    best = working[[y, x]];
                    by = y;
                    bx = x;
                }
            }
        }
        if best < 2.0 {
            break;
        }
        let pop = rng.gen_range(40..140) as i64;
        settlements.push(Settlement {
            id: settlements.len() as i64,
            name: naming::make_word(rng, "hellenic", taken),
            x: bx as i64,
            y: by as i64,
            pop,
            tier: tier(pop),
            food: site_food(&food, fert, &near_fresh, &coast, by, bx),
            coastal: coast[[by, bx]],
            river: near_fresh[[by, bx]],
            culture: 0,
            connections: 0,
            goods: Vec::new(),
            exports: None,
            wealth: crate::util::round2(pop as f64 * 0.2),
            port: false,
        });
        for y in 0..size {
            for x in 0..size {
                let dy = y as f64 - by as f64;
                let dx = x as f64 - bx as f64;
                if dy * dy + dx * dx < min_d2 {
                    working[[y, x]] = -1e9;
                }
            }
        }
    }

    Founded {
        settlements,
        site_score: score,
        food_grid: food,
        near_fresh,
        coast,
        max_settlements: (n_target as f64 * 2.5) as usize,
    }
}

/// Best colony site in the ring around a parent, clear of others. The outer
/// edge of the ring widens as a people masters sail and star-charts.
pub fn colony_site(
    site_score: &Array2<f64>,
    settlements: &[Settlement],
    parent: &Settlement,
    max_d2: f64,
) -> Option<(usize, usize)> {
    let size = site_score.dim().0;
    let min_d2 = (size as f64 / 22.0).powi(2);
    let mut best = f64::NEG_INFINITY;
    let mut found: Option<(usize, usize)> = None;
    for y in 0..size {
        for x in 0..size {
            let s = site_score[[y, x]];
            if s <= 2.2 || s <= best {
                continue;
            }
            let dyp = y as f64 - parent.y as f64;
            let dxp = x as f64 - parent.x as f64;
            let d2p = dyp * dyp + dxp * dxp;
            if d2p < 256.0 || d2p > max_d2 {
                continue;
            }
            let mut clear = true;
            for o in settlements {
                let dy = y as f64 - o.y as f64;
                let dx = x as f64 - o.x as f64;
                if dy * dy + dx * dx < min_d2 {
                    clear = false;
                    break;
                }
            }
            if clear {
                best = s;
                found = Some((y, x));
            }
        }
    }
    found
}
