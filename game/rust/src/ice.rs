//! M28 — ice-sheet extent at the last glacial maximum: the ice age's
//! footprint, earned from latitude and elevation rather than painted on.
//!
//! The model is the standard mass-balance heuristic reduced to its
//! load-bearing quantity, the **equilibrium-line altitude** (ELA): the
//! elevation where a year's snowfall exactly matches a year's melt.
//! Ground above the ELA accumulates ice; ground below loses it. The
//! ELA falls poleward (colder ablation seasons) and the last glacial
//! maximum depressed it globally, so the LGM footprint is
//!
//! ```text
//!   glaciated(y, x)  ⇔  land  ∧  h ≥ ela(|lat|) + jitter(seed, y, x)
//!   ela(l) = ELA_EQ · (1 − (l / L0)²),   l = |lat| / 90
//! ```
//!
//! — a parabola that starts above every summit at the equator, dips
//! through the mountain tops in the mid-latitudes (alpine ice caps),
//! and goes below sea level past `L0·90°` (continental sheets swallow
//! the lowlands whole). The per-cell jitter is a small ELA wobble —
//! aspect, drift, shading — that keeps the margin ragged over flat
//! ground; it is drawn with SplitMix64 fixed-width arithmetic so the
//! footprint replays byte-identically on every runtime (ADR-0025
//! discipline, the same as the sea-level freeze).
//!
//! Thickness follows the classic parabolic sheet profile: a plastic
//! ice sheet's surface rises with the square root of the distance from
//! its margin (Vialov/Nye), so `H = TH_K · √(d cells)` with a physical
//! cap — thin aprons at the edge, kilometre domes deep inside. The
//! distance is a 4-neighbour BFS from every margin cell, scan-order
//! seeded and therefore deterministic.
//!
//! Like the plate sketch and the sea-level history this is **frozen
//! prehistory** (ADR-0024): computed once at the dawn from the final
//! height field, folded into `hash_state` and the deep-earth identity
//! line, never advanced in tick time. M29 reads it to carve the relief
//! the ice left behind; the LGM shelf (sheets grounding on exposed
//! shelf at lowstand) is deliberately out of scope — land only.

use ndarray::Array2;

use crate::util::fnv1a64;

/// LGM equilibrium-line altitude at the equator, height units. Sits
/// just above the p95 land height (~0.55) so equatorial ice is a
/// rarity reserved for freak summits, exactly as on Earth.
const ELA_EQ: f64 = 0.62;

/// Normalized latitude where the LGM ELA reaches sea level — poleward
/// of `L0 · 90°` the sheets take the lowlands. 62°, the same edge the
/// isostatic rebound belt remembers (`sealevel::ICE_EDGE..ICE_FULL`).
const L0: f64 = 62.0 / 90.0;

/// Per-cell ELA wobble amplitude (height units): aspect, snowdrift,
/// shading. Keeps the margin ragged over flat ground.
const JITTER: f64 = 0.02;

/// Thickness scale, metres per √cell (cells are 4 km, ADR-0004). The
/// parabolic profile `H = TH_K·√d` puts a ~300 m apron one cell in and
/// a ~2.9 km dome eighty cells (320 km) from the margin.
const TH_K: f64 = 320.0;

/// Physical dome cap, metres — no sheet outgrows the Antarctic.
const TH_CAP: f64 = 4000.0;

/// SplitMix64 — one fixed-width draw per cell, identical on every
/// runtime (the noisegen/sealevel discipline, ADR-0025).
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The frozen LGM footprint: thickness in metres (0 = never under the
/// ice) plus the ELA row profile the mask was cut against.
#[derive(Clone, Debug)]
pub struct Ice {
    /// Peak-glaciation ice thickness, metres; > 0 is the LGM flag.
    pub thickness: Array2<f32>,
    /// LGM equilibrium-line altitude per generated row, height units.
    /// Negative rows are sheet country: ice to the waterline.
    pub ela_row: Vec<f64>,
}

impl Ice {
    /// Placeholder for the builder window before the height field is
    /// final; replaced by `compute` at the dawn.
    pub fn empty() -> Self {
        Ice { thickness: Array2::zeros((0, 0)), ela_row: Vec::new() }
    }

    /// FNV-1a over the thickness grid and the ELA profile — joins
    /// `hash_state` and the deep-earth identity line (the M28 gate).
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> =
            Vec::with_capacity(self.thickness.len() * 4 + self.ela_row.len() * 8);
        for v in &self.thickness {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.ela_row {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        fnv1a64(&b)
    }
}

/// The LGM equilibrium-line altitude at normalized |latitude| `l`.
#[inline]
pub fn ela(l: f64) -> f64 {
    let r = l / L0;
    ELA_EQ * (1.0 - r * r)
}

/// Compute the LGM footprint from the final height field. Rows map to
/// latitude exactly as `sealevel`/`climate` map them; the widened grid
/// adds columns only, so the row profile is unchanged by the margins.
pub fn compute(seed: i64, height: &Array2<f32>) -> Ice {
    let (rows, cols) = height.dim();
    let n = rows as f64;

    // ELA per row — pure IEEE-exact arithmetic.
    let ela_row: Vec<f64> = (0..rows)
        .map(|y| {
            let lat = (-90.0 + (y as f64) * 180.0 / (n - 1.0)).abs();
            ela(lat / 90.0)
        })
        .collect();

    // The mask: land at or above the wobbled ELA.
    let base = (seed as u64) ^ 0x1CE_A6E_F007_u64;
    let mut mask: Array2<u8> = Array2::zeros((rows, cols));
    for y in 0..rows {
        let e = ela_row[y];
        for x in 0..cols {
            let h = height[[y, x]] as f64;
            if h < 0.0 {
                continue; // land only — the LGM shelf is out of scope
            }
            let draw = splitmix64(base ^ ((y as u64) << 32) ^ (x as u64));
            let jit = (((draw >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0) * JITTER;
            if h >= e + jit {
                mask[[y, x]] = 1;
            }
        }
    }

    // Thickness: multi-source BFS from the margin (any glaciated cell
    // touching bare ground or open water), parabolic profile in the
    // BFS distance. Scan-order seeding keeps it deterministic.
    let mut dist: Array2<i32> = Array2::from_elem((rows, cols), -1);
    let mut queue: std::collections::VecDeque<(usize, usize)> = std::collections::VecDeque::new();
    for y in 0..rows {
        for x in 0..cols {
            if mask[[y, x]] == 0 {
                continue;
            }
            let mut edge = false;
            for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue; // the map edge is sheet interior, not margin
                }
                if mask[[ny as usize, nx as usize]] == 0 {
                    edge = true;
                    break;
                }
            }
            if edge {
                dist[[y, x]] = 1;
                queue.push_back((y, x));
            }
        }
    }
    while let Some((y, x)) = queue.pop_front() {
        let d = dist[[y, x]];
        for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if mask[[ny, nx]] == 1 && dist[[ny, nx]] < 0 {
                dist[[ny, nx]] = d + 1;
                queue.push_back((ny, nx));
            }
        }
    }

    let mut thickness: Array2<f32> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if mask[[y, x]] == 1 {
                // an ice patch with no margin (a fully iced map would
                // have dist -1 everywhere) still gets the cap profile
                let d = if dist[[y, x]] > 0 { dist[[y, x]] as f64 } else { 1.0 };
                thickness[[y, x]] = (TH_K * d.sqrt()).min(TH_CAP) as f32;
            }
        }
    }

    Ice { thickness, ela_row }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7) — the M28 gate: the footprint's share, its
/// lowland margin latitude, the ELA's poleward march read back off the
/// mask itself, and the dome ceiling. Calibrated on the report seeds.
pub const BANDS: &[Band] = &[
    Band { name: "ice share of land at LGM", sweet: (8.0, 32.0), hard: (4.0, 45.0), target: "sweet 8–32 · hard 4–45 (% of land under the LGM sheets; Earth ran ~25)" },
    Band { name: "lowland ice margin lat", sweet: (53.0, 63.0), hard: (48.0, 70.0), target: "sweet 53–63 · hard 48–70 (p5 |lat|° of glaciated lowland, h<0.10; law says ~57)" },
    Band { name: "ELA poleward monotone", sweet: (85.0, 100.0), hard: (70.0, 100.0), target: "sweet 85–100 · hard 70–100 (% of 3° bins where the lowest glaciated cell drops)" },
    Band { name: "peak ice thickness m", sweet: (1200.0, 4000.0), hard: (600.0, 4000.0), target: "sweet 1200–4000 · hard 600–4000 (dome height; parabolic profile, Antarctic cap)" },
];
