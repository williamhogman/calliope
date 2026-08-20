//! M44 — longshore drift: the coast stops being a static outline and
//! starts moving sediment along itself.
//!
//! One law, walked a few times. Waves arrive with the prevailing wind
//! (the same zonal bands the precipitation march sweeps: trades below
//! 30°, westerlies to 60°, polar easterlies beyond). Where a wave
//! strikes the shore at an angle, the surf carries sand alongshore —
//! the CERC rule: transport goes as `sin α · cos α`, the onshore
//! component times the alongshore one. Where that flux *converges*
//! (an embayment neck, the downdrift shadow of a headland), sand has
//! nowhere left to go and the sea shallows into new ground:
//!
//! - a chain rooted on the old shore is a **spit** — the hook the
//!   current builds off a headland;
//! - a chain that stands free of it is a **barrier** — the offshore
//!   bar grown to daylight;
//! - world-ocean water the new ground pinches off is a **lagoon** —
//!   the quiet water every barrier owes its name to.
//!
//! The pass runs at the glacial stage's close, on the final pre-widen
//! f64 height — the last hand to touch the land before climate reads
//! it — so rivers, biomes, tides and settlements all see the grown
//! coast, not an overlay. Deposition is iterative (each new cell bends
//! the shoreline the next reading answers), capped per pass, and every
//! deposit records the seabed it buried, so the whole walk replays
//! byte-identically from the same height field. The math is rational
//! arithmetic plus IEEE-exact `sqrt` and the shared smoothing kernel —
//! no transcendentals (ADR-0025 discipline).

use ndarray::Array2;

use crate::ndimage;
use crate::util::{fnv1a64, Band};

// ------------------------------------------------------------ constants

/// Deepest seabed a spit can grow onto, height units (−0.0075 ≈ 30 m):
/// longshore sand stays on the shoreface.
pub const SHELF: f64 = -0.0075;
/// Offshore-bar nucleation depth, height units (−0.004 ≈ 16 m): a bar
/// needs a shallower bank to daylight without a land anchor.
pub const BAR_DEPTH: f64 = -0.004;
/// The new ground's elevation, height units (+0.00075 ≈ +3 m): barrier
/// islands are low — a storm surge away from the sea that built them.
pub const DEPOSIT_H: f64 = 0.00075;
/// Land-fraction smoothing (σ, cells) for the shore-normal reading.
pub const NORMAL_SIGMA: f64 = 2.0;
/// Fetch saturation, cells (32 · 4 km = 128 km of open water gives the
/// full wave); sheltered inner seas drift little.
pub const FETCH_MAX: usize = 32;
/// Growth passes: each deposit bends the shoreline the next pass reads.
/// Many small passes beat few large ones — the feedback (tip migrates,
/// convergence follows) is what elongates a spit.
pub const N_ITER: usize = 12;
/// Deposits per pass — convergence is ranked and only the strongest
/// sites build, so the shore gains features, not a pavement (the M44
/// share gate holds the total under 4% of the coast).
pub const CAP_PER_ITER: usize = 8;
/// Growth continuation: a site touching already-deposited ground scores
/// its convergence up (×1 + BONUS·min(adj,2)). Once a spit starts, the
/// transport along it keeps feeding the tip — chains elongate instead
/// of the map collecting dust.
pub const GROW_BONUS: f64 = 1.5;
/// Minimum flux convergence (−div q) to qualify as a deposition site.
pub const CONV_MIN: f64 = 0.02;
/// Minimum nearby transport magnitude: convergence without supply
/// builds nothing (a still bay has no sand in motion).
pub const FLUX_MIN: f64 = 0.08;
/// Keep the walk off the map frame.
pub const BORDER: usize = 2;
/// Barrier nucleation sites per world: the breaker line seeds the bar,
/// the drift builds it. A minority feature everywhere on Earth.
pub const BAR_SEEDS: usize = 5;
/// Minimum Chebyshev spacing between nucleation sites, cells.
pub const BAR_SPACING: usize = 10;
/// Minimum smoothed shallow-bank fraction at a nucleation site — bars
/// are born on wide gentle shelves, never off a plunging coast.
pub const BAR_SHELF_MIN: f64 = 0.35;
/// Open water required to windward, cells: no wave, no breaker.
pub const BAR_OPEN: usize = 6;
/// Nucleation depth floor, height units (−0.02 ≈ 80 m). At 4 km cells
/// the coastal slope drops tens of metres per cell, so Earth's literal
/// breaker depth is subgrid; the bank the grid can actually resolve is
/// the bank the bar daylights from. This is the deepest seabed any
/// deposit may bury (the diagnose must reads it).
pub const BAR_FLOOR: f64 = -0.02;


/// CoastForm classes, stored per cell.
pub const OPEN: u8 = 0;
/// Deposited ground rooted on the pre-drift shore.
pub const SPIT: u8 = 1;
/// Deposited ground standing free of it.
pub const BARRIER: u8 = 2;
/// World-ocean water the deposits pinched off.
pub const LAGOON: u8 = 3;

// --------------------------------------------------------------- struct

/// The drift ledger: CoastForm per cell, plus the undo record — every
/// deposited cell with the seabed it buried, in deposition order.
/// `drift()` is a pure function of the height field, so the ledger
/// replays byte-identically (the M44 regen gate).
///
/// The buried depth is recorded at **f32** precision — the engine's
/// portable currency (cf. `tides`). The f64 height field carries
/// runtime-local ULP noise from upstream transcendentals; the coast
/// bisection instrument proved it (native vs wasm: positions and form
/// agree, raw f64 bits diverge). Never hash raw f64 derived from the
/// height field.
pub struct Coast {
    /// OPEN / SPIT / BARRIER / LAGOON per cell.
    pub form: Array2<u8>,
    /// (y, x, pre-drift height as f32 bits), in deposition order.
    pub deposits: Vec<(u32, u32, u32)>,
}

impl Coast {
    pub fn empty() -> Self {
        Coast {
            form: Array2::zeros((0, 0)),
            deposits: Vec::new(),
        }
    }

    /// FNV-1a over the form grid and the deposit ledger — joins
    /// `hash_state` and the deep-earth identity line.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> =
            Vec::with_capacity(self.form.len() + self.deposits.len() * 12);
        b.extend_from_slice(self.form.as_slice().expect("form grid is contiguous"));
        for &(y, x, h) in &self.deposits {
            b.extend_from_slice(&y.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&h.to_le_bytes());
        }
        fnv1a64(&b)
    }

    /// Bisection instrument (M44, cf. `seismic_debug`): the coast hash
    /// split into its constituents, so a cross-runtime divergence names
    /// the part it lives in — deposit positions, pre-height bits, or
    /// the form grid.
    pub fn debug_parts(&self) -> (u64, u64, u64) {
        let mut pos: Vec<u8> = Vec::with_capacity(self.deposits.len() * 8);
        let mut bits: Vec<u8> = Vec::with_capacity(self.deposits.len() * 4);
        for &(y, x, h) in &self.deposits {
            pos.extend_from_slice(&y.to_le_bytes());
            pos.extend_from_slice(&x.to_le_bytes());
            bits.extend_from_slice(&h.to_le_bytes());
        }
        let form = fnv1a64(self.form.as_slice().expect("form grid is contiguous"));
        (fnv1a64(&pos), fnv1a64(&bits), form)
    }

    /// Ride the ocean-margin widening: margins are open sea (no form),
    /// deposit coordinates shift into shipped map space.
    pub fn widen(&mut self, pad: usize) {
        if pad == 0 || self.form.is_empty() {
            return;
        }
        let (h, w) = self.form.dim();
        let p = pad as isize;
        self.form = Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
            let xi = x as isize - p;
            if xi >= 0 && (xi as usize) < w {
                self.form[[y, xi as usize]]
            } else {
                OPEN
            }
        });
        for d in &mut self.deposits {
            d.1 += pad as u32;
        }
    }
}

// ------------------------------------------------------------- the pass

/// The wind's zonal direction for a grid row: −1 east→west (trades,
/// polar easterlies), +1 west→east (westerlies). The very bands the
/// precipitation march sweeps (`climate::precipitation`).
fn wind_dx(y: usize, rows: usize) -> isize {
    let lat = (-90.0 + y as f64 * 180.0 / (rows as f64 - 1.0)).abs();
    if lat < 30.0 {
        -1
    } else if lat < 60.0 {
        1
    } else {
        -1
    }
}

/// Run the longshore-drift pass on the final pre-widen height field.
/// Mutates `h` (deposition raises seabed to `DEPOSIT_H`) and returns
/// the ledger. Pure function of `h` — no seed, no clock.
pub fn drift(h: &mut Array2<f64>) -> Coast {
    let (rows, cols) = h.dim();
    if rows < 16 || cols < 16 {
        return Coast {
            form: Array2::zeros((rows, cols)),
            deposits: Vec::new(),
        };
    }

    // The pre-drift record: original land and the original world ocean,
    // for attachment and lagoon reads after the walk.
    let pre_water = h.mapv(|v| v < 0.0);
    let pre_lab = ndimage::label(&pre_water, false);
    let pre_main = main_label(&pre_lab);

    let mut deposits: Vec<(u32, u32, u32)> = Vec::new();
    // Deposited-ground mask, maintained across passes: feeds the
    // growth-continuation bonus and the final chain classification.
    let mut dep_mask = Array2::<bool>::from_elem((rows, cols), false);

    // ------------------------------------------------- bar nucleation
    // Barrier islands are not born of convergence: on a wide gentle
    // shelf the waves break before they reach the beach, and the
    // breaker line piles a bar parallel to the shore. Seed up to
    // BAR_SEEDS such sites — offshore, shallow, shelf-backed, open sea
    // to windward — and let the drift walk elongate them like any tip.
    {
        let pre_land = pre_water.mapv(|w| !w);
        let shallow01 = Array2::from_shape_fn((rows, cols), |(y, x)| {
            let d = h[[y, x]];
            if d < 0.0 && d > BAR_FLOOR {
                1.0
            } else {
                0.0
            }
        });
        let shelf = ndimage::gaussian_filter(&shallow01, 2.0);
        let near2 = ndimage::maximum_filter(&pre_land, 5); // touches land ≤2
        let near8 = ndimage::maximum_filter(&pre_land, 17); // shore zone ≤8
        let mut seeds: Vec<(f64, usize, usize)> = Vec::new();
        for y in BORDER..rows - BORDER {
            for x in BORDER..cols - BORDER {
                if pre_lab.lab[[y, x]] != pre_main {
                    continue;
                }
                let d = h[[y, x]];
                if d <= BAR_FLOOR || d >= 0.0 {
                    continue;
                }
                if near2[[y, x]] || !near8[[y, x]] {
                    continue;
                }
                if shelf[[y, x]] < BAR_SHELF_MIN {
                    continue;
                }
                // open sea to windward: no wave, no breaker
                let w = wind_dx(y, rows);
                let mut open = 0usize;
                while open < BAR_OPEN {
                    let ux = x as isize - w * (open as isize + 1);
                    if ux < 0 || ux >= cols as isize {
                        open = BAR_OPEN;
                        break;
                    }
                    if !pre_water[[y, ux as usize]] {
                        break;
                    }
                    open += 1;
                }
                if open < BAR_OPEN {
                    continue;
                }
                // wider shelf and shallower bank rank higher
                let score = shelf[[y, x]] * (1.0 - d / BAR_FLOOR);
                seeds.push((score, y, x));
            }
        }
        seeds.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .expect("score is NaN-free")
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        let mut placed: Vec<(usize, usize)> = Vec::new();
        for &(_, y, x) in &seeds {
            if placed.len() >= BAR_SEEDS {
                break;
            }
            if placed
                .iter()
                .any(|&(py, px)| py.abs_diff(y).max(px.abs_diff(x)) < BAR_SPACING)
            {
                continue;
            }
            deposits.push((y as u32, x as u32, (h[[y, x]] as f32).to_bits()));
            h[[y, x]] = DEPOSIT_H;
            dep_mask[[y, x]] = true;
            placed.push((y, x));
        }
    }


    for _pass in 0..N_ITER {
        let water = h.mapv(|v| v < 0.0);
        let lab = ndimage::label(&water, false);
        let main = main_label(&lab);

        // Shore normal off the smoothed land fraction: n points landward.
        let landf = Array2::from_shape_fn((rows, cols), |(y, x)| {
            if water[[y, x]] {
                0.0
            } else {
                1.0
            }
        });
        let lsm = ndimage::gaussian_filter(&landf, NORMAL_SIGMA);
        let (gy, gx) = ndimage::gradient(&lsm);

        // Signed alongshore flux per world-ocean cell with a defined
        // shore direction; zero elsewhere. CERC: Q = (w·n)⁺ · (w·t) · fetch.
        let mut qy = Array2::<f64>::zeros((rows, cols));
        let mut qx = Array2::<f64>::zeros((rows, cols));
        let mut qmag = Array2::<f64>::zeros((rows, cols));
        for y in 0..rows {
            let dx = wind_dx(y, rows);
            let w = dx as f64; // wave vector: (0, w) in (y, x) components
            for x in 0..cols {
                if lab.lab[[y, x]] != main {
                    continue;
                }
                let (ny, nx) = (gy[[y, x]], gx[[y, x]]);
                let g = (ny * ny + nx * nx).sqrt();
                if g < 1e-6 {
                    continue; // open ocean: no shore to drift along
                }
                let (ny, nx) = (ny / g, nx / g);
                let onshore = w * nx; // w·n
                if onshore <= 0.0 {
                    continue; // leeward coast: the wave never arrives
                }
                // Fetch: consecutive water upwind (where the wave came
                // from), saturating at FETCH_MAX. Off-map is open sea.
                let mut f = 0usize;
                while f < FETCH_MAX {
                    let ux = x as isize - w as isize * (f as isize + 1);
                    if ux < 0 || ux >= cols as isize {
                        f = FETCH_MAX;
                        break;
                    }
                    if !water[[y, ux as usize]] {
                        break;
                    }
                    f += 1;
                }
                let fetch = f as f64 / FETCH_MAX as f64;
                // Tangent t = (−nx, ny): alongshore component w·t.
                let along = w * ny;
                let q = onshore * along * fetch;
                qy[[y, x]] = q * (-nx);
                qx[[y, x]] = q * ny;
                qmag[[y, x]] = q.abs();
            }
        }

        // Convergence: sand piles where the alongshore flux dies.
        let (dqy_dy, _) = ndimage::gradient(&qy);
        let (_, dqx_dx) = ndimage::gradient(&qx);
        // Supply: strongest transport in the 3×3 around the site.
        let supply = ndimage::maximum_filter(&qmag, 3);

        let mut cand: Vec<(f64, usize, usize)> = Vec::new();
        for y in BORDER..rows - BORDER {
            for x in BORDER..cols - BORDER {
                if lab.lab[[y, x]] != main {
                    continue;
                }
                let conv = -(dqy_dy[[y, x]] + dqx_dx[[y, x]]);
                if conv < CONV_MIN || supply[[y, x]] < FLUX_MIN {
                    continue;
                }
                let depth = h[[y, x]];
                let near_land = (y.saturating_sub(1)..=(y + 1).min(rows - 1)).any(|yy| {
                    (x.saturating_sub(1)..=(x + 1).min(cols - 1))
                        .any(|xx| !water[[yy, xx]])
                });
                // On-shore growth works the whole shoreface; a free bar
                // needs a genuinely shallow bank to daylight on.
                let ok = if near_land { depth > SHELF } else { depth > BAR_DEPTH };
                if !ok {
                    continue;
                }
                // Growth continuation: sites touching deposited ground
                // score up — the tip keeps feeding, the chain elongates.
                let adj = (y.saturating_sub(1)..=(y + 1).min(rows - 1))
                    .flat_map(|yy| {
                        (x.saturating_sub(1)..=(x + 1).min(cols - 1))
                            .map(move |xx| (yy, xx))
                    })
                    .filter(|&(yy, xx)| dep_mask[[yy, xx]])
                    .count()
                    .min(2);
                cand.push((conv * (1.0 + GROW_BONUS * adj as f64), y, x));
            }
        }
        // Strongest score first; (y, x) breaks exact ties. The
        // arithmetic is NaN-free, so the total order is real.
        cand.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .expect("convergence is NaN-free")
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        let mut took = 0usize;
        for &(_, y, x) in cand.iter() {
            if took >= CAP_PER_ITER {
                break;
            }
            deposits.push((y as u32, x as u32, (h[[y, x]] as f32).to_bits()));
            h[[y, x]] = DEPOSIT_H;
            dep_mask[[y, x]] = true;
            took += 1;
        }
        if took == 0 {
            break; // the shore has settled
        }
    }

    // ------------------------------------------------ classification
    let mut form = Array2::<u8>::zeros((rows, cols));

    // Chains: 8-connected components of deposited ground. Rooted on
    // the pre-drift shore → spit; standing free → barrier.
    let chains = ndimage::label(&dep_mask, true);
    let mut attached = vec![false; chains.n + 1];
    for y in 0..rows {
        for x in 0..cols {
            let c = chains.lab[[y, x]];
            if c == 0 {
                continue;
            }
            let rooted = (y.saturating_sub(1)..=(y + 1).min(rows - 1)).any(|yy| {
                (x.saturating_sub(1)..=(x + 1).min(cols - 1))
                    .any(|xx| !pre_water[[yy, xx]])
            });
            if rooted {
                attached[c as usize] = true;
            }
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            let c = chains.lab[[y, x]];
            if c != 0 {
                form[[y, x]] = if attached[c as usize] { SPIT } else { BARRIER };
            }
        }
    }

    // Lagoons: water that belonged to the world ocean and no longer
    // reaches it — the deposits closed the door.
    let post_water = h.mapv(|v| v < 0.0);
    let post_lab = ndimage::label(&post_water, false);
    let post_main = main_label(&post_lab);
    for y in 0..rows {
        for x in 0..cols {
            if post_water[[y, x]]
                && pre_lab.lab[[y, x]] == pre_main
                && post_lab.lab[[y, x]] != post_main
            {
                form[[y, x]] = LAGOON;
            }
        }
    }

    Coast { form, deposits }
}

/// The largest water component's label — the world ocean.
fn main_label(lab: &ndimage::Labeled) -> i32 {
    let mut main = 0i32;
    let mut best = -1.0f64;
    for (i, &a) in lab.areas.iter().enumerate() {
        if a > best {
            best = a;
            main = (i + 1) as i32;
        }
    }
    main
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E2.7): the drift census. The M44 gate fixes the
/// share band — spits, barriers and lagoons together claim a real but
/// minority slice of any coast (0.5–4% of coastal cells).
pub const BANDS: &[Band] = &[
    Band { name: "coastform share of coastal cells %", sweet: (0.5, 4.0), hard: (0.2, 8.0), target: "sweet 0.5–4 · hard 0.2–8 (M44 gate: spit+barrier+lagoon cells over coastal water cells — features, not pavement; measured 3.30–3.73 ×3 seeds)" },
    Band { name: "spit chains per seed", sweet: (5.0, 50.0), hard: (1.0, 100.0), target: "sweet 5–50 · hard 1–100 (M44: drift-built hooks rooted on the shore — every windward coast grows a few; measured 22–39)" },
    Band { name: "barrier chains per seed", sweet: (1.0, 12.0), hard: (0.0, 30.0), target: "sweet 1–12 · hard 0–30 (M44: free-standing bars grown to daylight — rarer than spits, absent only on steep coasts; measured 3–4)" },
    Band { name: "lagoon cells per seed", sweet: (1.0, 100.0), hard: (0.0, 300.0), target: "sweet 1–100 · hard 0–300 (M44: world-ocean water the deposits pinched off — the quiet water behind the bar; measured 2–20)" },
];
