//! The plate-history sketch (M16, ADR-0024) — the deep past as a
//! *generative input*, never a simulation.
//!
//! A coarse set of warped-Voronoi polygons covers the grid. Each plate
//! carries a drift vector, a drift-age in megayears, and a continental
//! flag; each boundary between two plates is classified once — convergent,
//! divergent, or transform — from the relative drift the two plates were
//! dealt. Nothing here advances in tick time: the sketch is frozen at
//! world-genesis and consumed by `geo::heightmap` (collision seams gate
//! the orogeny belts, plate interiors bias the coastline) and, from M17
//! on, by the age-decay of relief. Everything is a pure function of the
//! seed (ADR-0003).

use std::collections::VecDeque;

use ndarray::Array2;
use rand::Rng;

use crate::noisegen::Perlin3;
use crate::util::{fnv1a64, Band};

/// Boundary-kind codes stored in `Plates::boundary` (0 = plate interior).
pub const B_NONE: u8 = 0;
pub const B_CONVERGENT: u8 = 1;
pub const B_DIVERGENT: u8 = 2;
pub const B_TRANSFORM: u8 = 3;

/// One plate of the sketch: a Voronoi seed, a drift it was dealt at the
/// dawn of deep time, an age, and whether it carries a continent.
#[derive(Clone)]
pub struct Plate {
    pub id: u8,
    /// Voronoi seed point, cell coordinates.
    pub cx: f64,
    pub cy: f64,
    /// Drift vector (unitless; only directions and ratios matter).
    pub vx: f64,
    pub vy: f64,
    /// Drift age in megayears — how long this plate has ridden its course.
    pub age: f64,
    /// Cratonic core vs oceanic floor: biases the heightmap base.
    pub continental: bool,
}

/// The frozen sketch: the plate table plus the per-cell derived grids the
/// generation passes read. Grids are base-size at generation and widened
/// with the world's ocean margins afterwards.
#[derive(Clone)]
pub struct Plates {
    pub plates: Vec<Plate>,
    /// Owning plate per cell (index into `plates`).
    pub cell: Array2<u8>,
    /// Boundary kind per cell: `B_NONE` in the interior, else the kind of
    /// the seam this cell sits on.
    pub boundary: Array2<u8>,
    /// Chebyshev distance (cells) to the nearest plate boundary of any kind.
    pub edge_dist: Array2<f32>,
    /// Chebyshev distance (cells) to the nearest *convergent* seam.
    pub seam_dist: Array2<f32>,
    /// Drift-age (Myr) of the collision that raised the nearest convergent
    /// seam — young collisions stand sharp, old ones lie worn (M17).
    pub seam_age: Array2<f32>,
}

impl Plates {
    /// Mean drift-age across the plate table, Myr.
    pub fn mean_age(&self) -> f64 {
        if self.plates.is_empty() {
            return 0.0;
        }
        self.plates.iter().map(|p| p.age).sum::<f64>() / self.plates.len() as f64
    }

    /// FNV-1a over the whole sketch — table and grids. Two generations of
    /// the same seed must agree byte-for-byte (the M16 gate).
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.cell.len() * 2 + 256);
        for p in &self.plates {
            b.push(p.id);
            b.extend_from_slice(&p.cx.to_bits().to_le_bytes());
            b.extend_from_slice(&p.cy.to_bits().to_le_bytes());
            b.extend_from_slice(&p.vx.to_bits().to_le_bytes());
            b.extend_from_slice(&p.vy.to_bits().to_le_bytes());
            b.extend_from_slice(&p.age.to_bits().to_le_bytes());
            b.push(p.continental as u8);
        }
        b.extend(self.cell.iter().copied());
        b.extend(self.boundary.iter().copied());
        for v in self.seam_age.iter() {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        fnv1a64(&b)
    }

    /// Sub-hashes for cross-runtime bisection: (table, cell, boundary).
    /// Debug instrumentation for the M22 gate — not part of the wire.
    pub fn debug_parts(&self) -> (u64, u64, u64) {
        let mut tb: Vec<u8> = Vec::new();
        for p in &self.plates {
            tb.push(p.id);
            tb.extend_from_slice(&p.cx.to_bits().to_le_bytes());
            tb.extend_from_slice(&p.cy.to_bits().to_le_bytes());
            tb.extend_from_slice(&p.vx.to_bits().to_le_bytes());
            tb.extend_from_slice(&p.vy.to_bits().to_le_bytes());
            tb.extend_from_slice(&p.age.to_bits().to_le_bytes());
            tb.push(p.continental as u8);
        }
        let cb: Vec<u8> = self.cell.iter().copied().collect();
        let bb: Vec<u8> = self.boundary.iter().copied().collect();
        (fnv1a64(&tb), fnv1a64(&cb), fnv1a64(&bb))
    }
}

/// Multi-source Chebyshev BFS: distance to the nearest source cell plus a
/// payload carried outward from that source (used for the seam age).
/// FIFO order and fixed scan order keep ties deterministic.
fn distance_field(
    size: usize,
    sources: &[(usize, usize, f32)],
) -> (Array2<f32>, Array2<f32>) {
    let mut dist = Array2::from_elem((size, size), f32::INFINITY);
    let mut val = Array2::from_elem((size, size), 0.0f32);
    let mut q: VecDeque<(usize, usize)> = VecDeque::new();
    for &(y, x, v) in sources {
        if dist[[y, x]] > 0.0 {
            dist[[y, x]] = 0.0;
            val[[y, x]] = v;
            q.push_back((y, x));
        }
    }
    while let Some((y, x)) = q.pop_front() {
        let d = dist[[y, x]] + 1.0;
        for dy in -1isize..=1 {
            for dx in -1isize..=1 {
                if dy == 0 && dx == 0 {
                    continue;
                }
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                if dist[[ny, nx]] > d {
                    dist[[ny, nx]] = d;
                    val[[ny, nx]] = val[[y, x]];
                    q.push_back((ny, nx));
                }
            }
        }
    }
    // A sketch with no seams of the sought kind: report "far" everywhere.
    let far = size as f32;
    for d in dist.iter_mut() {
        if !d.is_finite() {
            *d = far;
        }
    }
    (dist, val)
}

/// Generate the sketch for one seed. Deterministic: the RNG stream, the
/// candidate order, the warp noise and every classification depend only
/// on the seed and size.
pub fn generate(seed: i64, size: usize) -> Plates {
    let mut rng = crate::util::rng(seed.wrapping_mul(31).wrapping_add(1616));
    let n = size as f64;

    // ---- the table: 9–13 plates, greedy max–min seed points ------------
    let count = 9 + (rng.gen::<f64>() * 5.0) as usize;
    let cands: Vec<(f64, f64)> = (0..count * 4)
        .map(|_| {
            (
                rng.gen_range(0.04..0.96) * n,
                rng.gen_range(0.04..0.96) * n,
            )
        })
        .collect();
    // Cross-runtime float discipline (M22): everything feeding the
    // boundary grid uses only IEEE-exact ops (+ − × ÷ √) — no libm
    // transcendentals, whose last bits differ between native and wasm.
    // Squared distances replace hypot; drift directions come from a
    // normalized 2-vector draw instead of sin/cos of an angle.
    let d2 = |a: (f64, f64), p: (f64, f64)| {
        let (dx, dy) = (a.0 - p.0, a.1 - p.1);
        dx * dx + dy * dy
    };
    let mut pts: Vec<(f64, f64)> = vec![cands[0]];
    while pts.len() < count {
        let far = cands
            .iter()
            .max_by(|a, b| {
                let da = pts.iter().map(|p| d2(**a, *p)).fold(f64::INFINITY, f64::min);
                let db = pts.iter().map(|p| d2(**b, *p)).fold(f64::INFINITY, f64::min);
                da.partial_cmp(&db).unwrap()
            })
            .copied()
            .unwrap();
        pts.push(far);
    }
    let plates: Vec<Plate> = pts
        .iter()
        .enumerate()
        .map(|(i, &(cx, cy))| {
            // Unit drift direction from two draws + √-normalization: the
            // slight square-corner bias is irrelevant at 9–13 plates, the
            // bit-identity across runtimes is not.
            let ux: f64 = rng.gen_range(-1.0..1.0);
            let uy: f64 = rng.gen_range(-1.0..1.0);
            let norm = (ux * ux + uy * uy).sqrt();
            let (ux, uy) = if norm > 1e-6 { (ux / norm, uy / norm) } else { (1.0, 0.0) };
            let speed = 0.35 + rng.gen::<f64>() * 0.75;
            let age = 120.0 + rng.gen::<f64>() * 2400.0;
            let continental = rng.gen::<f64>() < 0.58;
            Plate {
                id: i as u8,
                cx,
                cy,
                vx: ux * speed,
                vy: uy * speed,
                age,
                continental,
            }
        })
        .collect();

    // ---- ownership: warped-Voronoi so the seams wander organically -----
    let warp = Perlin3::new(seed + 515);
    let amp = 0.055 * n;
    let cell = Array2::from_shape_fn((size, size), |(y, x)| {
        let fx = x as f64 / n * 3.0;
        let fy = y as f64 / n * 3.0;
        let px = x as f64 + amp * warp.fbm(fx + 41.7, fy + 9.3, 0.7, 3);
        let py = y as f64 + amp * warp.fbm(fx + 7.7, fy + 27.1, 4.2, 3);
        let mut best = 0u8;
        let mut bd = f64::INFINITY;
        for p in &plates {
            let (dx, dy) = (px - p.cx, py - p.cy);
            let d = dx * dx + dy * dy; // squared: exact, order-preserving
            if d < bd {
                bd = d;
                best = p.id;
            }
        }
        best
    });

    // ---- pair classification: what the relative drift says -------------
    // approach > 0 toward each other ⇒ convergent; near-tangent ⇒ transform.
    let k = plates.len();
    let mut pair_kind = vec![B_TRANSFORM; k * k];
    let mut pair_age = vec![0.0f32; k * k];
    for i in 0..k {
        for j in 0..k {
            if i == j {
                continue;
            }
            let (a, b) = (&plates[i], &plates[j]);
            let (nx, ny) = (b.cx - a.cx, b.cy - a.cy);
            let len = (nx * nx + ny * ny).sqrt().max(1e-9);
            let (nx, ny) = (nx / len, ny / len);
            let (rx, ry) = (a.vx - b.vx, a.vy - b.vy);
            let rel = (rx * rx + ry * ry).sqrt().max(1e-9);
            let approach = rx * nx + ry * ny;
            let kind = if approach.abs() < 0.35 * rel {
                B_TRANSFORM
            } else if approach > 0.0 {
                B_CONVERGENT
            } else {
                B_DIVERGENT
            };
            // Collision age: the younger partner set the seam's clock,
            // jittered per pair (order-free) so belts differ in wear.
            let (lo, hi) = (i.min(j) as u64, i.max(j) as u64);
            let h = fnv1a64(&[seed as u64, lo, hi].map(u64::to_le_bytes).concat());
            let jit = 0.55 + 0.45 * ((h % 1000) as f64 / 1000.0);
            pair_kind[i * k + j] = kind;
            pair_age[i * k + j] = (a.age.min(b.age) * jit) as f32;
        }
    }

    // ---- boundary grid + seam sources -----------------------------------
    let mut boundary = Array2::from_elem((size, size), B_NONE);
    let mut edge_src: Vec<(usize, usize, f32)> = Vec::new();
    let mut conv_src: Vec<(usize, usize, f32)> = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let me = cell[[y, x]] as usize;
            let mut kind = B_NONE;
            let mut age = 0.0f32;
            for (dy, dx) in [(0isize, 1isize), (1, 0), (0, -1), (-1, 0)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= size as isize || nx >= size as isize {
                    continue;
                }
                let other = cell[[ny as usize, nx as usize]] as usize;
                if other == me {
                    continue;
                }
                let pk = pair_kind[me * k + other];
                // Convergent wins the cell, then divergent, then transform:
                // a triple junction reads as its most consequential seam.
                if kind == B_NONE
                    || pk == B_CONVERGENT
                    || (pk == B_DIVERGENT && kind == B_TRANSFORM)
                {
                    kind = pk;
                    age = pair_age[me * k + other];
                }
            }
            if kind != B_NONE {
                boundary[[y, x]] = kind;
                edge_src.push((y, x, 0.0));
                if kind == B_CONVERGENT {
                    conv_src.push((y, x, age));
                }
            }
        }
    }

    let (edge_dist, _) = distance_field(size, &edge_src);
    let (seam_dist, seam_age) = distance_field(size, &conv_src);

    Plates {
        plates,
        cell,
        boundary,
        edge_dist,
        seam_dist,
        seam_age,
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E2.7): the shape of the deep past.
pub const BANDS: &[Band] = &[
    Band { name: "plate count", sweet: (8.0, 16.0), hard: (6.0, 20.0), target: "M16: a coarse sketch — continents, not crazy paving" },
    Band { name: "plate mean drift-age (Myr)", sweet: (700.0, 1900.0), hard: (400.0, 2300.0), target: "M16: deep time, neither newborn nor exhausted" },
    Band { name: "convergent share of boundary", sweet: (0.15, 0.65), hard: (0.05, 0.85), target: "M16: some seams close, some open, some slide" },
];
