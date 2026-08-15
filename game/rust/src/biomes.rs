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
