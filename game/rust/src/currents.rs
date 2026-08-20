//! M40 — wind-driven gyres: the ocean circulates the way its winds
//! already say it must.
//!
//! The climate's rain marches under three zonal wind bands (easterly
//! trades below 30°, westerlies 30–60°, polar easterlies beyond —
//! `climate::precipitation`). Those same bands drag on the sea
//! surface. Depth-integrated, the steady response of a basin is the
//! Sverdrup balance — β·v = curl(τ) — closed by a narrow western
//! boundary current (Stommel 1948). We solve exactly that, cheaply:
//! a streamfunction ψ integrated westward from each ocean run's
//! eastern shore, pressed against the western wall by a rational
//! boundary factor, knit across coastline steps by one smoothing
//! pass, then differentiated into a surface current field. The
//! subtropical gyres come out anticyclonic — clockwise viewed
//! north-up in the northern hemisphere, counterclockwise in the
//! southern — and the subpolar gyres cyclonic, not by decree but
//! because the curl of the wind bands says so; the M40 gate measures
//! the rotation sense per basin instead of trusting this derivation.
//!
//! Pure derived state off the final (widened) height field, like
//! `landform` and `permafrost`: recomputed at the dawn, folded into
//! `hash_state`, never ticked. The wind-stress profile is polynomial
//! and the boundary factor rational — no transcendentals beyond the
//! smoothing kernel the whole pipeline already leans on — so every
//! runtime computes the same circulation (ADR-0025 discipline).

use ndarray::Array2;

use crate::ndimage;
use crate::util::{fnv1a64, Band};

// ------------------------------------------------------------ constants

/// Peak zonal wind stress per band (dimensionless index — the pattern
/// steers the gyres; the unit cancels into `SPEED_SCALE`).
pub const TAU_TRADES: f64 = 0.8;
pub const TAU_WESTERLIES: f64 = 1.0;
pub const TAU_POLAR: f64 = 0.4;

/// Stommel western-boundary layer width, cells. The closure factor is
/// rational — d/(d+W) — not exponential, by the no-transcendentals
/// discipline above.
pub const WBL_CELLS: f64 = 3.0;

/// Floor on cos(lat) in the β term: β → 0 at the poles would blow the
/// streamfunction up, and polar basins are boundary-trapped anyway.
pub const COS_FLOOR: f64 = 0.15;

/// One smoothing pass (σ in cells) knits the row-wise Sverdrup
/// interior into basin-shaped gyres across coastline steps.
pub const SMOOTH_SIGMA: f64 = 2.0;

/// Scales ψ-gradients into the stored current index so a typical
/// world's p95 open-ocean speed lands near 1.0 (banded below).
/// Calibrated on the flagship seed: raw p95 ran 0.237 at scale 1.
pub const SPEED_SCALE: f64 = 4.0;

// ---------------------------------------------------------- wind stress

/// Zonal wind stress vs |latitude|, matching the three rain-march
/// bands: easterly trades to 30°, westerlies to 60°, polar easterlies
/// beyond. Positive = eastward (westerly). Polynomial by design.
pub fn wind_stress(lat_abs: f64) -> f64 {
    if lat_abs < 30.0 {
        let s = lat_abs / 30.0;
        -TAU_TRADES * (1.0 - s * s)
    } else if lat_abs < 60.0 {
        let s = (lat_abs - 30.0) / 30.0;
        TAU_WESTERLIES * 4.0 * s * (1.0 - s)
    } else {
        let s = ((lat_abs - 60.0) / 30.0).min(1.0);
        -TAU_POLAR * s * (2.0 - s)
    }
}

// --------------------------------------------------------------- struct

/// The circulation ledger: streamfunction and surface current per
/// cell, zero on land. Grid conventions: `u` is grid-eastward (+x),
/// `v` is grid-southward (+y); flow runs along ψ contours, and a
/// positive ψ cell turns clockwise on screen (north-up).
pub struct Currents {
    /// Streamfunction, dimensionless index. Sign is rotation sense.
    pub psi: Array2<f32>,
    /// Grid-eastward current component (+x).
    pub u: Array2<f32>,
    /// Grid-southward current component (+y).
    pub v: Array2<f32>,
}

impl Currents {
    pub fn empty() -> Self {
        Currents {
            psi: Array2::zeros((0, 0)),
            u: Array2::zeros((0, 0)),
            v: Array2::zeros((0, 0)),
        }
    }

    /// FNV-1a over all three grids — joins `hash_state` so the
    /// circulation holds still across regenerations.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> =
            Vec::with_capacity((self.psi.len() + self.u.len() + self.v.len()) * 4);
        for g in [&self.psi, &self.u, &self.v] {
            for v in g {
                b.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
        fnv1a64(&b)
    }

    /// Solve the gyres off the ocean mask: Sverdrup interior per
    /// ocean run, Stommel western closure, one smoothing pass,
    /// central-difference currents. Takes the mask (not the height)
    /// so the dawn's post-widen ledger and M41's pre-widen climate
    /// coupling run the very same law on their own coastlines.
    pub fn compute(water: &Array2<bool>) -> Self {
        let (rows, cols) = water.dim();
        if rows < 8 || cols < 8 {
            return Currents {
                psi: Array2::zeros((rows, cols)),
                u: Array2::zeros((rows, cols)),
                v: Array2::zeros((rows, cols)),
            };
        }
        let nf = rows as f64;

        // Per-row wind stress and the Sverdrup source term
        // S(y) = −(∂τx/∂y_grid)/β. Grid y points south; the sign is
        // chosen so a positive ψ cell turns clockwise on screen, which
        // the band structure then makes anticyclonic in each
        // hemisphere's subtropics — the sense the M40 gate measures.
        let mut tau = vec![0.0f64; rows];
        for (y, t) in tau.iter_mut().enumerate() {
            let lat = (-90.0 + y as f64 * 180.0 / (nf - 1.0)).abs();
            *t = wind_stress(lat);
        }
        let mut src = vec![0.0f64; rows];
        for y in 0..rows {
            let ym = y.saturating_sub(1);
            let yp = (y + 1).min(rows - 1);
            let dtau = (tau[yp] - tau[ym]) / (yp - ym).max(1) as f64;
            let lat = -90.0 + y as f64 * 180.0 / (nf - 1.0);
            let beta = lat.to_radians().cos().max(COS_FLOOR);
            src[y] = -dtau / beta;
        }

        // Sverdrup interior, integrated westward from each ocean run's
        // eastern shore; Stommel closure against the western wall.
        let mut psi = Array2::<f64>::zeros((rows, cols));
        for y in 0..rows {
            let mut x = 0usize;
            while x < cols {
                if !water[[y, x]] {
                    x += 1;
                    continue;
                }
                let x0 = x;
                while x < cols && water[[y, x]] {
                    x += 1;
                }
                let x1 = x - 1; // inclusive eastern end of the run
                for xi in x0..=x1 {
                    let interior = (x1 - xi) as f64;
                    let d_west = (xi - x0) as f64 + 1.0;
                    let wbl = d_west / (d_west + WBL_CELLS);
                    psi[[y, xi]] = src[y] * interior * wbl;
                }
            }
        }

        // Knit the rows into gyres; the land stays ψ = 0.
        let mut psi = ndimage::gaussian_filter(&psi, SMOOTH_SIGMA);
        for y in 0..rows {
            for x in 0..cols {
                if !water[[y, x]] {
                    psi[[y, x]] = 0.0;
                }
            }
        }

        // u = ∂ψ/∂y_grid · v = −∂ψ/∂x_grid: flow along the contours.
        let mut u = Array2::<f32>::zeros((rows, cols));
        let mut v = Array2::<f32>::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                if !water[[y, x]] {
                    continue;
                }
                let ym = y.saturating_sub(1);
                let yp = (y + 1).min(rows - 1);
                let xm = x.saturating_sub(1);
                let xp = (x + 1).min(cols - 1);
                let du = (psi[[yp, x]] - psi[[ym, x]]) / (yp - ym).max(1) as f64;
                let dv = -(psi[[y, xp]] - psi[[y, xm]]) / (xp - xm).max(1) as f64;
                u[[y, x]] = (SPEED_SCALE * du) as f32;
                v[[y, x]] = (SPEED_SCALE * dv) as f32;
            }
        }
        Currents {
            psi: psi.mapv(|x| x as f32),
            u,
            v,
        }
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E2.7): the gyre census.
pub const BANDS: &[Band] = &[
    Band { name: "gyre basins per seed", sweet: (1.0, 6.0), hard: (1.0, 8.0), target: "sweet 1–6 · hard 1–8 (M40: labeled ocean basins ≥2500 cells carrying a qualifying subtropical gyre)" },
    Band { name: "surface current speed p95", sweet: (0.3, 3.0), hard: (0.1, 10.0), target: "sweet 0.3–3 · hard 0.1–10 (M40: p95 of the current index over subtropical-band ocean; SPEED_SCALE pins a typical world near 1)" },
    // Measured, not guessed: the five probe seeds run 30.7k–34.1k cells
    // per qualifying gyre at 512² — one ocean basin wide enough that each
    // hemisphere's subtropical band is a third of a grid. Sweet brackets
    // that envelope with room for a world whose land splits the basin;
    // hard only insists the region is a gyre and not a puddle.
    Band { name: "gyre cells per gyre", sweet: (2000.0, 60000.0), hard: (250.0, 120000.0), target: "sweet 2000–60000 · hard 250–120000 (M49: mean subtropical-band cell count of a qualifying gyre — topology, not sign: a gyre is a region)" },
    Band { name: "western boundary intensification", sweet: (1.5, 12.0), hard: (1.2, 30.0), target: "sweet 1.5–12 · hard 1.2–30 (M40: p95 speed in the 4-cell western strip over p95 in the interior — the Stommel signature, Gulf-Stream-side crowding)" },
];
