//! Soil fertility — port of agriculture.py — and the crop packages of
//! M2: every land cell gets the agriculture its temperature, rainfall
//! and irrigation actually support (research/08: GAEZ multiplicative
//! suitability, USDA climate profiles, the <300 mm pastoral boundary).

use ndarray::Array2;

use crate::ndimage;

pub fn fertility(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
) -> Array2<f64> {
    let size = height.dim().0;
    let hpos = height.mapv(|v| v.max(0.0));
    let (gy, gx) = ndimage::gradient(&hpos);

    let px = [0.0, 150.0, 450.0, 900.0, 1600.0, 2600.0, 4000.0];
    let py = [0.0, 0.08, 0.55, 1.0, 0.9, 0.5, 0.3];

    let mut fert = Array2::<f64>::zeros((size, size));
    let mut tgrid = Array2::<f64>::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            let t = (-(((tmean[[y, x]] - 17.0) / 11.0).powi(2))).exp();
            tgrid[[y, x]] = t;
            let p = crate::util::interp(precip[[y, x]], &px, &py);
            let slope = gy[[y, x]].hypot(gx[[y, x]]) * size as f64 / 8.0;
            let sp = 1.0 / (1.0 + (slope * 2.2).powi(2));
            fert[[y, x]] = 0.9 * t * p * sp;
        }
    }

    // alluvial floodplains: big rivers lay down silt as they wander
    let silt_src = Array2::from_shape_fn((size, size), |(y, x)| {
        if rivers[[y, x]] {
            (1.0 + discharge[[y, x]]).ln()
        } else {
            0.0
        }
    });
    let silt = ndimage::gaussian_filter(&silt_src, 2.2);

    // lakeshores hold moisture
    let shore_wide = ndimage::binary_dilation(lakes, 2);

    for y in 0..size {
        for x in 0..size {
            let mut v = fert[[y, x]];
            v += (silt[[y, x]] * 0.08).clamp(0.0, 0.35) * tgrid[[y, x]];
            if shore_wide[[y, x]] && !lakes[[y, x]] {
                v += 0.08;
            }
            if height[[y, x]] < 0.0 || lakes[[y, x]] {
                v = 0.0;
            }
            fert[[y, x]] = v.clamp(0.0, 1.0);
        }
    }
    fert
}

// ---------------------------------------------------------------- crops
// M2.1 — crop packages. Codes are stable (they ship in the pack).

pub const PACK_NONE: u8 = 0;
pub const PACK_WHEAT: u8 = 1;
pub const PACK_RICE: u8 = 2;
pub const PACK_MAIZE: u8 = 3;
pub const PACK_PASTORAL: u8 = 4;

pub const PACK_NAMES: [&str; 5] = ["wildland", "wheat", "rice", "maize", "pastoral"];

/// Souls per km² a package feeds at dawn-age arts (research/08:
/// hunter-gatherer 0.05–0.4 · pastoral 2 · wheat ~30 · rice ~90–120).
/// Kaplan's T^−0.5 (land per soul shrinks as arts accumulate) raises
/// these through `society::Mods::kaplan`.
pub const PACK_DENSITY: [f64; 5] = [0.4, 30.0, 90.0, 22.0, 2.0];

#[inline]
fn gauss(v: f64, mu: f64, sig: f64) -> f64 {
    (-((v - mu) / sig).powi(2)).exp()
}

/// Trapezoid suitability over precipitation: 0 at `lo`, 1 at `peak`,
/// easing to 0.35 at `hi`, 0 beyond — crops drown slower than they parch.
#[inline]
fn trap(p: f64, lo: f64, peak: f64, hi: f64) -> f64 {
    if p <= lo || p >= hi {
        0.0
    } else if p < peak {
        (p - lo) / (peak - lo)
    } else {
        1.0 - 0.65 * (p - peak) / (hi - peak)
    }
}

/// Classify every cell into the crop package that wins it. Deterministic,
/// pure function of climate + water adjacency; rivers and lakeshores count
/// as irrigable floodplain (paddies and canal-fed fields ignore a dry sky).
pub fn crop_packages(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
) -> Array2<u8> {
    let (rows, cols) = height.dim();
    let riv = ndimage::binary_dilation(rivers, 1);
    let lak = ndimage::binary_dilation(lakes, 1);
    Array2::from_shape_fn((rows, cols), |(y, x)| {
        if height[[y, x]] < 0.0 || lakes[[y, x]] {
            return PACK_NONE;
        }
        let t = tmean[[y, x]];
        let p = precip[[y, x]];
        let irrigated = riv[[y, x]] || lak[[y, x]];
        // no growing season at all: the high ice and the deep tundra
        if t < 1.5 {
            return if t >= -4.0 && p >= 130.0 {
                PACK_PASTORAL
            } else {
                PACK_NONE
            };
        }
        let mut wheat = gauss(t, 12.0, 8.0) * trap(p, 270.0, 700.0, 1400.0);
        let mut maize = gauss(t, 21.0, 6.0) * trap(p, 430.0, 950.0, 2000.0);
        let mut rice = gauss(t, 26.0, 5.5) * trap(p, 900.0, 1700.0, 4200.0);
        if irrigated {
            if t > 17.0 {
                rice = rice.max(0.75 * gauss(t, 26.0, 6.5));
            }
            wheat = wheat.max(0.70 * gauss(t, 12.0, 8.0));
            maize = maize.max(0.60 * gauss(t, 21.0, 6.0));
        } else if p < 300.0 {
            // the pastoral boundary: below ~300 mm farming fails (research/08)
            wheat = 0.0;
            maize = 0.0;
            rice = 0.0;
        }
        let pastoral = 0.32 * gauss(t, 12.0, 18.0) * trap(p, 110.0, 420.0, 1500.0);
        let best = wheat.max(maize).max(rice).max(pastoral);
        if best < 0.07 {
            PACK_NONE
        } else if rice >= best {
            PACK_RICE
        } else if maize >= best {
            PACK_MAIZE
        } else if wheat >= best {
            PACK_WHEAT
        } else {
            PACK_PASTORAL
        }
    })
}
