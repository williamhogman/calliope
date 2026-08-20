//! M43 — the tides: the shore breathes daily, with a range that
//! answers to the shape of its sea.
//!
//! One law, three readings. The deep ocean carries a modest
//! equilibrium tide; what the coast feels is that signal reshaped by
//! basin geometry:
//!
//! - **Shoaling (Green's law)** — a wave entering shallow water
//!   steepens as `(H_ref/H)^¼`: shelf coasts run higher than the deep
//!   ocean that feeds them.
//! - **Funnel resonance (the Fundy law)** — a semi-enclosed arm whose
//!   penetration length sits near the quarter wavelength `¼·T·√(gH)`
//!   rings with the forcing; confinement gates the gain, so open
//!   straight coasts stay near base while gulfs and firths amplify.
//!   Past twice the quarter wave, friction eats the signal — the long
//!   shallow arm goes quiet.
//! - **The landlocked hush (the Mediterranean law)** — a sea with no
//!   path to the world ocean has no tidal wave to receive; it sits
//!   microtidal no matter its size. (The roadmap sketch's
//!   "enclosed-sea amplification" is read as the semi-enclosed
//!   resonance above: confinement amplifies only while the ocean can
//!   still get in.)
//!
//! Pure derived state off the final (widened) height field, like
//! `landform` and `currents`: recomputed at the dawn, folded into
//! `hash_state`, never ticked. The math is rational polynomials plus
//! IEEE-exact `sqrt` and the shared smoothing kernel — no
//! transcendentals, so every runtime computes the same shore
//! (ADR-0025 discipline). `landform::stamp_tidal` reads the field to
//! mint tidal flats and estuary mouths where range and low-slope
//! coast coincide.

use std::collections::VecDeque;

use ndarray::Array2;

use crate::constants::{KM_PER_CELL, METRES_PER_UNIT};
use crate::ndimage;
use crate::util::{fnv1a64, Band};

// ------------------------------------------------------------ constants

/// Open-ocean equilibrium range at the reference shelf depth, metres.
pub const TIDE_BASE: f64 = 2.0;
/// A landlocked sea's whole tide, metres — the Mediterranean law.
pub const TIDE_ENCLOSED: f64 = 0.3;
/// Green's-law reference depth, metres: the shelf depth where the
/// shoaling factor crosses 1.
pub const H_REF: f64 = 200.0;
/// Cap on `H_ref/H` in the shoaling factor — max amplification
/// `40^¼ ≈ 2.5×` at the mudline, so a 1-metre-deep cell cannot ring
/// the formula to absurdity.
pub const SHOAL_CAP: f64 = 40.0;
/// Depth floor, metres, for the same reason.
pub const DEPTH_MIN: f64 = 5.0;
/// The M2 lunar semidiurnal period, seconds (12.42 h).
pub const TIDE_PERIOD_S: f64 = 44_712.0;
pub const GRAVITY: f64 = 9.81;
/// Peak resonance gain at full confinement and exact quarter-wave
/// penetration — a resonant funnel runs up to `1 + RES_AMP` times its
/// shoaled base.
pub const RES_AMP: f64 = 2.5;
/// Confinement window radius, cells (Chebyshev): land fraction in the
/// (2R+1)² box around a water cell measures how funnel-like it sits.
pub const CONF_R: usize = 5;
/// Confinement gate: resonance wakes at `CONF_MIN` land fraction and
/// saturates at `CONF_SAT`. A straight shore reads ≈0.5 in the box
/// (half land), so the gate opens only past it — funnels, gulfs and
/// firths ring; the open shelf and the plain coast cannot (measured:
/// at 0.15 every coastal cell rang and the macrotidal share hit 26%).
pub const CONF_MIN: f64 = 0.55;
pub const CONF_SAT: f64 = 0.85;
/// Frictional decay per quarter-wave unit past resonance: the long
/// shallow arm loses its tide (rational, not exponential — the
/// no-transcendentals discipline).
pub const FRIC: f64 = 0.8;
/// Hard ceiling on the stored range, metres — the Fundy record.
pub const RANGE_MAX: f64 = 16.0;
/// One normalized smoothing pass (σ in cells) knits the per-cell
/// readings into a coherent shore signal without letting the land's
/// zeros dilute the coast.
pub const SMOOTH_SIGMA: f64 = 1.5;
/// Open-water seeds for the penetration march: nearly landless
/// neighborhoods over deep water.
pub const OPEN_CONF: f64 = 0.02;
/// Seed depth threshold in height units (−0.05 ≈ 200 m down).
pub const OPEN_DEPTH: f32 = -0.05;

/// Basin-enclosure classes, stored per cell.
pub const LAND: u8 = 0;
/// Water connected to the world ocean — the tide gets in.
pub const OPEN: u8 = 1;
/// Landlocked water — the tide never arrives.
pub const ENCLOSED: u8 = 2;

// --------------------------------------------------------------- struct

/// The tidal ledger: range in metres and enclosure class per cell,
/// zero/LAND on land.
pub struct Tides {
    /// Mean tidal range, metres. 0 on land.
    pub range: Array2<f32>,
    /// LAND / OPEN / ENCLOSED per cell.
    pub class: Array2<u8>,
}

impl Tides {
    pub fn empty() -> Self {
        Tides {
            range: Array2::zeros((0, 0)),
            class: Array2::zeros((0, 0)),
        }
    }

    /// FNV-1a over both grids — joins `hash_state` so the shore's
    /// breath holds still across regenerations and runtimes.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.range.len() * 4 + self.class.len());
        for v in &self.range {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        b.extend_from_slice(self.class.as_slice().expect("class grid is contiguous"));
        fnv1a64(&b)
    }

    /// Solve the tide off the final height field: label the water,
    /// call the largest component the world ocean, march penetration
    /// distance in from the open deep, then price every water cell by
    /// shoaling × resonance × friction. Landlocked components sit at
    /// `TIDE_ENCLOSED` flat.
    pub fn compute(height: &Array2<f32>) -> Self {
        let (rows, cols) = height.dim();
        if rows < 8 || cols < 8 {
            return Tides {
                range: Array2::zeros((rows, cols)),
                class: Array2::zeros((rows, cols)),
            };
        }
        let water = height.mapv(|h| h < 0.0);
        let lab = ndimage::label(&water, false);

        // The world ocean: the largest water component (the post-widen
        // margins guarantee it is the one that wraps the continents).
        let mut main = 0i32;
        let mut best = -1.0f64;
        for (i, &a) in lab.areas.iter().enumerate() {
            if a > best {
                best = a;
                main = (i + 1) as i32;
            }
        }

        let mut class = Array2::<u8>::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                let l = lab.lab[[y, x]];
                class[[y, x]] = if l == 0 {
                    LAND
                } else if l == main {
                    OPEN
                } else {
                    ENCLOSED
                };
            }
        }

        // Confinement: land fraction in the (2R+1)² box, via an
        // integer summed-area table — exact, order-free.
        let stride = cols + 1;
        let mut sat = vec![0u32; (rows + 1) * stride];
        for y in 0..rows {
            for x in 0..cols {
                let land = !water[[y, x]] as u32;
                sat[(y + 1) * stride + x + 1] =
                    land + sat[y * stride + x + 1] + sat[(y + 1) * stride + x]
                        - sat[y * stride + x];
            }
        }
        let conf = |y: usize, x: usize| -> f64 {
            let y0 = y.saturating_sub(CONF_R);
            let x0 = x.saturating_sub(CONF_R);
            let y1 = (y + CONF_R + 1).min(rows);
            let x1 = (x + CONF_R + 1).min(cols);
            let n = ((y1 - y0) * (x1 - x0)) as f64;
            let c = sat[y1 * stride + x1] + sat[y0 * stride + x0]
                - sat[y0 * stride + x1]
                - sat[y1 * stride + x0];
            c as f64 / n
        };

        // Penetration march: BFS through the world ocean from the open
        // deep (BFS distance is unique, so queue order cannot leak).
        let mut dist = Array2::<u32>::from_elem((rows, cols), u32::MAX);
        let mut q: VecDeque<(usize, usize)> = VecDeque::new();
        for y in 0..rows {
            for x in 0..cols {
                if lab.lab[[y, x]] == main
                    && height[[y, x]] <= OPEN_DEPTH
                    && conf(y, x) <= OPEN_CONF
                {
                    dist[[y, x]] = 0;
                    q.push_back((y, x));
                }
            }
        }
        if q.is_empty() {
            // A world with no open deep at all: the whole ocean is the
            // shore. No penetration, no resonance — base tide only.
            for y in 0..rows {
                for x in 0..cols {
                    if lab.lab[[y, x]] == main {
                        dist[[y, x]] = 0;
                    }
                }
            }
        }
        while let Some((y, x)) = q.pop_front() {
            let d = dist[[y, x]] + 1;
            let push = |ny: usize, nx: usize, q: &mut VecDeque<(usize, usize)>, dist: &mut Array2<u32>| {
                if lab.lab[[ny, nx]] == main && dist[[ny, nx]] == u32::MAX {
                    dist[[ny, nx]] = d;
                    q.push_back((ny, nx));
                }
            };
            if y > 0 {
                push(y - 1, x, &mut q, &mut dist);
            }
            if y + 1 < rows {
                push(y + 1, x, &mut q, &mut dist);
            }
            if x > 0 {
                push(y, x - 1, &mut q, &mut dist);
            }
            if x + 1 < cols {
                push(y, x + 1, &mut q, &mut dist);
            }
        }

        // Price every water cell: shoaling × resonance × friction.
        let cell_m = KM_PER_CELL * 1000.0;
        let mut range = Array2::<f64>::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                let l = lab.lab[[y, x]];
                if l == 0 {
                    continue;
                }
                if l != main {
                    range[[y, x]] = TIDE_ENCLOSED;
                    continue;
                }
                let depth_m = (-(height[[y, x]] as f64) * METRES_PER_UNIT).max(DEPTH_MIN);
                // Green's law: amplitude ∝ (H_ref/H)^¼, capped.
                let shoal = (H_REF / depth_m).min(SHOAL_CAP).sqrt().sqrt();
                // Quarter-wave penetration in cells at this depth.
                let dstar = 0.25 * TIDE_PERIOD_S * (GRAVITY * depth_m).sqrt() / cell_m;
                let xq = if dist[[y, x]] == u32::MAX {
                    0.0
                } else {
                    dist[[y, x]] as f64 / dstar
                };
                // Resonance bump (x(2−x))²: 0 at the mouth, 1 at the
                // quarter wave, 0 again at the half — polynomial.
                let bump = if xq < 2.0 {
                    let b = xq * (2.0 - xq);
                    b * b
                } else {
                    0.0
                };
                let ct = ((conf(y, x) - CONF_MIN) / (CONF_SAT - CONF_MIN)).clamp(0.0, 1.0);
                let res = 1.0 + RES_AMP * ct * bump;
                // Friction past twice the quarter wave: the over-long
                // arm goes quiet.
                let damp = 1.0 / (1.0 + FRIC * (xq - 2.0).max(0.0));
                range[[y, x]] = (TIDE_BASE * shoal * res * damp).min(RANGE_MAX);
            }
        }

        // Normalized smoothing over the world ocean only:
        // G(range·w)/G(w) with w = the OPEN mask, so the land's zeros
        // do not dilute the coast and no tide bleeds across an isthmus
        // into a landlocked sea. Enclosed cells stay exactly
        // TIDE_ENCLOSED — a constant needs no knitting.
        let wmask = Array2::<f64>::from_shape_fn((rows, cols), |(y, x)| {
            if lab.lab[[y, x]] == main {
                1.0
            } else {
                0.0
            }
        });
        let open_range = &range * &wmask;
        let num = ndimage::gaussian_filter(&open_range, SMOOTH_SIGMA);
        let den = ndimage::gaussian_filter(&wmask, SMOOTH_SIGMA);
        let mut out = Array2::<f32>::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                let l = lab.lab[[y, x]];
                if l == 0 {
                    continue;
                }
                if l != main {
                    out[[y, x]] = TIDE_ENCLOSED as f32;
                    continue;
                }
                let d = den[[y, x]];
                let v = if d > 1e-9 { num[[y, x]] / d } else { range[[y, x]] };
                out[[y, x]] = v.clamp(0.0, RANGE_MAX) as f32;
            }
        }

        Tides { range: out, class }
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E2.7): the tide census — range by enclosure
/// class against terrestrial analogs (open shelf coasts run 1–3 m
/// mean range; landlocked seas sit decimetric; macrotidal funnels are
/// a minority of any coast), plus the flats' scaling laws.
pub const BANDS: &[Band] = &[
    Band { name: "open-coast tidal range m", sweet: (1.0, 3.5), hard: (0.5, 6.0), target: "sweet 1–3.5 · hard 0.5–6 (M43: mean range over world-ocean coastal water — Earth's open shelf coasts run mesotidal; measured 2.89–3.09 ×3 seeds)" },
    Band { name: "macrotidal coast share %", sweet: (1.0, 20.0), hard: (0.2, 40.0), target: "sweet 1–20 · hard 0.2–40 (M43: share of coastal water at ≥4 m range — resonant funnels are a minority of any shore; measured 2.7–7.8)" },
    Band { name: "enclosed-sea tidal range m", sweet: (0.1, 0.8), hard: (0.0, 1.5), target: "sweet 0.1–0.8 · hard 0–1.5 (M43: mean range in landlocked seas — the Mediterranean law, decimetric; held at TIDE_ENCLOSED = 0.30 exactly, no bleed across isthmuses)" },
    Band { name: "tidal flats per 1000 coast", sweet: (5.0, 120.0), hard: (1.0, 300.0), target: "sweet 5–120 · hard 1–300 (M43: TIDEFLAT cells per 1000 coastal water cells — Wadden-scale presence without paving the shore; measured 25–111 cells/seed, mean 21.4/1000)" },
    Band { name: "estuary mouths per seed", sweet: (4.0, 80.0), hard: (1.0, 200.0), target: "sweet 4–80 · hard 1–200 (M43: river mouths on tidal water — every world keeps a few gateways; measured 4–103, mean 45.3)" },
    Band { name: "flat-cell range amplification", sweet: (1.05, 3.0), hard: (1.0, 5.0), target: "sweet 1.05–3 · hard 1–5 (M43 gate: mean range at flats over the coastal mean — flats crowd the high-range shore; measured 2.08–2.43)" },
    Band { name: "flat-cell relief fraction", sweet: (0.02, 0.75), hard: (0.0, 0.95), target: "sweet 0.02–0.75 · hard 0–0.95 (M43 gate: 3×3 relief at flats over the coastal mean — flats shun the steep shore; measured 0.16–0.19)" },
];
