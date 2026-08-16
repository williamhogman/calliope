//! Biome classification — port of biomes.py (geo.hy's Whittaker table).

use ndarray::Array2;

use crate::constants as gc;

// rows: coldest -> hottest, 6x6 expansion of geo.hy's table
const BIOME_TABLE: [[u8; 6]; 6] = [
    [gc::ICE; 6],
    [gc::TUNDRA; 6],
    [
        gc::GRASSLAND,
        gc::GRASSLAND,
        gc::WOODLAND,
        gc::BOREAL_FOREST,
        gc::BOREAL_FOREST,
        gc::BOREAL_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::WOODLAND,
        gc::WOODLAND,
        gc::SEASONAL_RAIN_FOREST,
        gc::TEMPERATE_RAIN_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::SAVANNA,
        gc::SAVANNA,
        gc::TROPICAL_RAIN_FOREST,
        gc::TROPICAL_RAIN_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::SAVANNA,
        gc::SAVANNA,
        gc::TROPICAL_RAIN_FOREST,
        gc::TROPICAL_RAIN_FOREST,
    ],
];

const TEMP_EDGES: [f64; 5] = [-10.0, -2.0, 5.0, 13.0, 20.0];
const PRECIP_EDGES: [f64; 5] = [180.0, 420.0, 800.0, 1400.0, 2200.0];

#[inline]
fn digitize(x: f64, edges: &[f64; 5]) -> usize {
    edges.iter().filter(|&&e| x >= e).count()
}

pub fn classify(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    lakes: &Array2<bool>,
) -> Array2<u8> {
    Array2::from_shape_fn(height.dim(), |(y, x)| {
        if height[[y, x]] < 0.0 || lakes[[y, x]] {
            gc::WATER
        } else {
            let trow = digitize(tmean[[y, x]], &TEMP_EDGES);
            let pcol = digitize(precip[[y, x]], &PRECIP_EDGES);
            BIOME_TABLE[trow][pcol]
        }
    })
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): how the land is dressed.
pub const BANDS: &[Band] = &[
    Band { name: "desert share of land", sweet: (0.12, 0.28), hard: (0.06, 0.38), target: "sweet 12–28% · hard 6–38%" },
    Band { name: "tundra+ice share of land", sweet: (0.05, 0.30), hard: (0.01, 0.45), target: "sweet 5–30% · hard 1–45%" },
    Band { name: "forest share of land", sweet: (0.25, 0.60), hard: (0.15, 0.75), target: "sweet 25–60% · hard 15–75%" },
    Band { name: "grass+savanna share of land", sweet: (0.10, 0.45), hard: (0.04, 0.60), target: "sweet 10–45% · hard 4–60%" },
];
