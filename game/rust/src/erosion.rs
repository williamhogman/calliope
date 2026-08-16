//! Erosion — the carved land. Raw tectonic noise gives mountains their
//! bones; erosion gives them their faces. Three processes, run in the
//! order nature runs them: thermal collapse knocks the impossible
//! spikes down to talus slopes, rivers cut valleys with the stream
//! power they actually carry, and soil creep softens what remains.
//! Everything is deterministic — no randomness, pure functions of the
//! heightfield — so the same seed still carves the same world.

use ndarray::Array2;

use crate::hydrology::{fill_depressions, flow_directions, DIST, N8};

/// Slopes steeper than this (height units per cell of run) shed rock.
/// One height unit is ~4 km over a ~4 km cell, so 0.05 ≈ 200 m of rise
/// per cell — about the steepest a mean 4 km tile can honestly hold.
const TALUS: f64 = 0.05;
/// Fraction of the excess relief that lets go per pass.
const TALUS_K: f64 = 0.5;
const TALUS_PASSES: usize = 3;

/// Stream-power constant: how hard a river of unit drainage area cuts.
const SPI_K: f64 = 0.014;
/// Implicit incision solves toward the receiver, so any K is stable;
/// two passes let the first valleys steer the second's drainage.
const SPI_PASSES: usize = 2;

/// Soil-creep diffusion strength and passes (land-only, coast-safe).
const DIFF_D: f64 = 0.12;
const DIFF_PASSES: usize = 2;

/// Thermal erosion: where a cell towers over a neighbour beyond the
/// angle of repose, the excess lets go and comes to rest below. Moves
/// material (conserves it) rather than just planing peaks off.
fn talus_pass(h: &mut Array2<f64>) {
    let (rows, cols) = h.dim();
    let mut delta = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            let hc = h[[y, x]];
            if hc <= 0.0 {
                continue; // the seabed keeps its trenches
            }
            // steepest downhill neighbour
            let mut best = 0.0f64;
            let mut bi: Option<(usize, usize, f64)> = None;
            for (&(dy, dx), &dist) in N8.iter().zip(DIST.iter()) {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                let s = (hc - h[[ny, nx]]) / dist;
                if s > best {
                    best = s;
                    bi = Some((ny, nx, dist));
                }
            }
            if let Some((ny, nx, dist)) = bi {
                if best > TALUS {
                    let move_amt = TALUS_K * 0.5 * (best - TALUS) * dist;
                    delta[[y, x]] -= move_amt;
                    delta[[ny, nx]] += move_amt;
                }
            }
        }
    }
    *h += &delta;
}

/// Stream-power incision, implicit in the manner of Braun & Willett:
/// walk the drainage tree from mouth to source and relax every cell
/// toward its receiver with strength K·√A. Unconditionally stable —
/// a cell can approach its receiver but never dig below it, so no
/// pass ever creates a pit the next fill has to paper over.
fn fluvial_pass(h: &mut Array2<f64>) {
    let size = h.dim().0;
    let water = h.mapv(|v| v < 0.0);
    let filled = fill_depressions(h, &water);
    let dirs = flow_directions(&filled, &water);

    // drainage area in cells, accumulated down the tree. E5.11 — the
    // comparator is a total order (index tie-break), so the unstable sort
    // returns the identical permutation without the stable sort's
    // half-array scratch allocation.
    let mut order: Vec<usize> = (0..size * size).collect();
    order.sort_unstable_by(|&a, &b| {
        let fa = filled[[a / size, a % size]];
        let fb = filled[[b / size, b % size]];
        fb.partial_cmp(&fa).unwrap().then(a.cmp(&b)) // high to low
    });
    let mut area = Array2::<f64>::from_elem((size, size), 1.0);
    for &idx in &order {
        let (y, x) = (idx / size, idx % size);
        let d = dirs[[y, x]];
        if d >= 0 {
            let (dy, dx) = N8[d as usize];
            let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
            let v = area[[y, x]];
            area[[ny, nx]] += v;
        }
    }

    // receivers first: walk the order back-to-front (low to high)
    for &idx in order.iter().rev() {
        let (y, x) = (idx / size, idx % size);
        if water[[y, x]] {
            continue;
        }
        let d = dirs[[y, x]];
        if d < 0 {
            continue; // terminal cells (pit bottoms) keep their floor
        }
        if filled[[y, x]] - h[[y, x]] > 1e-4 {
            continue; // under standing water: deposition country, not erosion
        }
        let (dy, dx) = N8[d as usize];
        let (ny, nx) = ((y as isize + dy) as usize, (x as isize + dx) as usize);
        let dist = DIST[d as usize];
        // rivers never drag the coast below the tideline
        let hr = h[[ny, nx]].max(0.0015);
        let hc = h[[y, x]];
        if hc <= hr {
            continue;
        }
        let f = SPI_K * area[[y, x]].sqrt() / dist;
        h[[y, x]] = (hc + f * hr) / (1.0 + f);
    }
}

/// Soil creep: a gentle land-only diffusion. Cells average with their
/// land neighbours; the coastline itself never moves, so beaches stay
/// where the tectonics put them. E5.11 — the pre-pass snapshot lands in
/// a caller-owned scratch grid instead of a fresh clone per pass.
fn diffuse_pass(h: &mut Array2<f64>, src: &mut Array2<f64>) {
    let (rows, cols) = h.dim();
    src.assign(h);
    for y in 0..rows {
        for x in 0..cols {
            let hc = src[[y, x]];
            if hc <= 0.0 {
                continue;
            }
            let mut sum = 0.0;
            let mut n = 0usize;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let hn = src[[ny as usize, nx as usize]];
                if hn > 0.0 {
                    sum += hn;
                    n += 1;
                }
            }
            if n > 0 {
                let mean = sum / n as f64;
                h[[y, x]] = hc + DIFF_D * (mean - hc);
            }
        }
    }
}

/// The full carving sequence, applied to the raw tectonic heightmap
/// before climate ever sees it.
pub fn erode(h: &mut Array2<f64>) {
    for _ in 0..TALUS_PASSES {
        talus_pass(h);
    }
    for _ in 0..SPI_PASSES {
        fluvial_pass(h);
    }
    let mut src = Array2::<f64>::zeros(h.dim());
    for _ in 0..DIFF_PASSES {
        diffuse_pass(h, &mut src);
    }
}
