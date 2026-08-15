//! Soil fertility — port of agriculture.py.

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
