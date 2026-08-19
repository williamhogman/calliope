//! M28/M29 — the ice ages: the LGM footprint (M28) and the relief the
//! sheets carved into it (M29).
//!
//! **The footprint** is the standard mass-balance heuristic reduced to
//! its load-bearing quantity, the **equilibrium-line altitude** (ELA):
//! the elevation where a year's snowfall exactly matches a year's melt.
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
//! aspect, drift, shading — drawn with SplitMix64 fixed-width
//! arithmetic so the footprint replays byte-identically on every
//! runtime (ADR-0025 discipline, the same as the sea-level freeze).
//!
//! Thickness follows the classic parabolic sheet profile: a plastic
//! ice sheet's surface rises with the square root of the distance from
//! its margin (Vialov/Nye), so `H = TH_K · √(d cells)` with a physical
//! cap — thin aprons at the edge, kilometre domes deep inside.
//!
//! **The carve** (M29) runs mid-generation, after fluvial erosion and
//! before climate reads the land. Glacial erosion follows ice flux, and
//! ice flowed where water had already cut the lines, so the pass reads
//! the pre-carve drainage tree (the hydrology module's own fill → D8 →
//! accumulation machinery) and works three landform families:
//!
//! - **U-valleys** — under-ice drainage lines with real catchment get
//!   deepened in proportion to ice thickness and √flux, and the cut is
//!   spread across a cross-valley kernel whose quartic profile turns
//!   the fluvial V into the glacial U. Troughs may overdeepen below
//!   sea level: that is how fjords happen, and the landform classifier
//!   reads exactly that signature back out (`landform::FJORD`).
//! - **Cirques** — steep-backed headwater cells in the ELA band (the
//!   old ice-source elevations) get a scooped bowl and a registry
//!   entry, densest in the alpine belt where the textbooks put them.
//! - **Hanging valleys** — after the carve, a tributary whose trunk
//!   was cut markedly deeper hangs above it; the junction is recorded.
//!
//! Like the plate sketch and the sea-level history this is **frozen
//! prehistory** (ADR-0024): computed once at the dawn, folded into
//! `hash_state` (the I lane) and the deep-earth identity line, never
//! advanced in tick time. The LGM shelf (sheets grounding on exposed
//! shelf at lowstand) stays deliberately out of scope — land only.

use ndarray::Array2;
use std::collections::VecDeque;

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

// ------------------------------------------------------------ carve laws

/// A drainage line only carves as a glacial trough when it drains at
/// least this many cells — below it, ice creeps rather than streams.
const ACC_MIN: f64 = 24.0;

/// Flux normalizer: √(acc/ACC_REF), clamped to 1, is the flux factor.
/// A 240-cell catchment (≈3800 km²) carves at full strength.
const ACC_REF: f64 = 240.0;

/// Maximum trough deepening, height units, at full thickness and full
/// flux. Land p50 is ~0.20: a 0.055 cut is a real valley, not a scratch.
const K_C: f64 = 0.055;

/// Cirque scoop depth (height units) and the ELA window it forms in:
/// ice sources sat just above the line.
const CIRQUE_D: f64 = 0.020;
const CIRQUE_LO: f64 = -0.02;
const CIRQUE_HI: f64 = 0.10;

/// A cirque needs a back wall: some N8 neighbour at least this much
/// higher than the scooped cell.
const CIRQUE_WALL: f64 = 0.06;

/// A tributary hangs when its trunk was carved at least this much
/// deeper at the junction.
const HANG_MIN: f64 = 0.02;

/// Overdeepening floor: no trough is carved below this height — fjords
/// drown, the abyss stays tectonic.
const CARVE_FLOOR: f64 = -0.35;

/// A drowned cell counts as fjord country above this carve depth
/// (read by `landform::classify`).
pub const FJORD_MIN: f32 = 0.010;

/// SplitMix64 — one fixed-width draw per cell, identical on every
/// runtime (the noisegen/sealevel discipline, ADR-0025).
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The frozen ice-age ledger: LGM thickness, the carve it left in the
/// land, and the registries of cirques and hanging valleys.
#[derive(Clone, Debug)]
pub struct Ice {
    /// Peak-glaciation ice thickness, metres; > 0 is the LGM flag.
    pub thickness: Array2<f32>,
    /// M29 — how far the ice cut each cell down, height units; 0 =
    /// untouched. Fjords are the drowned tail of this grid.
    pub carved: Array2<f32>,
    /// LGM equilibrium-line altitude per generated row, height units.
    /// Negative rows are sheet country: ice to the waterline.
    pub ela_row: Vec<f64>,
    /// M29 — cirque bowls (y, x), scooped at the old source elevations.
    pub cirques: Vec<(u16, u16)>,
    /// M29 — hanging-valley junctions (y, x): tributary floors left in
    /// the air where the trunk cut deeper.
    pub hangs: Vec<(u16, u16)>,
}

impl Ice {
    /// Placeholder for the builder window before the glacial stage.
    pub fn empty() -> Self {
        Ice {
            thickness: Array2::zeros((0, 0)),
            carved: Array2::zeros((0, 0)),
            ela_row: Vec::new(),
            cirques: Vec::new(),
            hangs: Vec::new(),
        }
    }

    /// FNV-1a over the whole ledger — thickness, carve, ELA profile,
    /// cirque and hang registries. Joins `hash_state` (the I lane) and
    /// the deep-earth identity line (the M28 gate).
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(
            (self.thickness.len() + self.carved.len()) * 4 + self.ela_row.len() * 8,
        );
        for v in &self.thickness {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.carved {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.ela_row {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for &(y, x) in self.cirques.iter().chain(self.hangs.iter()) {
            b.extend_from_slice(&y.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
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

/// Cut the LGM footprint from the pre-carve height field (f64, the
/// mid-generation working grid). Rows map to latitude exactly as
/// `sealevel`/`climate` map them.
pub fn compute(seed: i64, height: &Array2<f64>) -> Ice {
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
            let h = height[[y, x]];
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
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
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

    Ice {
        thickness,
        carved: Array2::zeros((rows, cols)),
        ela_row,
        cirques: Vec::new(),
        hangs: Vec::new(),
    }
}

/// M29 — carve the relief the sheets left behind. Mutates the working
/// height field in place and fills the ledger's `carved`, `cirques`
/// and `hangs`. Every operation is IEEE-exact arithmetic over a
/// deterministic drainage order, so the carve replays byte-identically
/// (ADR-0025 discipline).
pub fn carve(height: &mut Array2<f64>, ice: &mut Ice) {
    let (rows, cols) = height.dim();

    // The pre-carve drainage tree: ice flowed where water had cut.
    let water = height.mapv(|v| v < 0.0);
    let filled = crate::hydrology::fill_depressions(height, &water);
    let dirs = crate::hydrology::flow_directions(&filled, &water);
    let ones = Array2::from_elem((rows, cols), 1000.0);
    let acc = crate::hydrology::flow_accumulation(&filled, &dirs, &ones, &water);

    let mut cut: Array2<f64> = Array2::zeros((rows, cols));

    // U-valleys: deepen under-ice drainage lines, spread the cut across
    // a cross-valley kernel — the quartic weight turns the V into a U.
    for y in 0..rows {
        for x in 0..cols {
            if ice.thickness[[y, x]] <= 0.0 || water[[y, x]] {
                continue;
            }
            let a = acc[[y, x]];
            if a < ACC_MIN {
                continue;
            }
            let tf = ice.thickness[[y, x]] as f64 / TH_CAP;
            let af = (a / ACC_REF).sqrt().min(1.0);
            let depth = K_C * tf * (0.35 + 0.65 * af);
            let r = 1 + (af * 2.0) as isize; // 1..=3 cells half-width
            for dy in -r..=r {
                for dx in -r..=r {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if water[[ny, nx]] {
                        continue;
                    }
                    let dd = dy.abs().max(dx.abs()) as f64 / (r + 1) as f64;
                    let w = 1.0 - dd * dd;
                    let w = w * w;
                    let c = depth * w;
                    if c > cut[[ny, nx]] {
                        cut[[ny, nx]] = c;
                    }
                }
            }
        }
    }

    // Cirques: steep-backed headwater cells in the ELA band.
    for y in 0..rows {
        for x in 0..cols {
            if ice.thickness[[y, x]] <= 0.0 || water[[y, x]] || acc[[y, x]] > 3.0 {
                continue;
            }
            let h = height[[y, x]];
            let e = ice.ela_row[y];
            if h < e + CIRQUE_LO || h > e + CIRQUE_HI {
                continue;
            }
            let mut wall = false;
            for (dy, dx) in [(-1isize, -1isize), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                if height[[ny as usize, nx as usize]] - h >= CIRQUE_WALL {
                    wall = true;
                    break;
                }
            }
            if !wall {
                continue;
            }
            if CIRQUE_D > cut[[y, x]] {
                cut[[y, x]] = CIRQUE_D;
            }
            ice.cirques.push((y as u16, x as u16));
        }
    }

    // Apply the cut: lower only, floored, clamped — NaN cannot happen.
    for y in 0..rows {
        for x in 0..cols {
            let c = cut[[y, x]];
            if c > 0.0 {
                let h = height[[y, x]];
                height[[y, x]] = (h - c).max(CARVE_FLOOR.min(h));
            }
        }
    }
    ice.carved = cut.mapv(|v| v as f32);

    // Hanging valleys: a carved tributary whose receiving trunk was cut
    // deeper hangs above it at the junction.
    for y in 0..rows {
        for x in 0..cols {
            let ca = cut[[y, x]];
            if ca <= 0.0 || acc[[y, x]] < ACC_MIN || acc[[y, x]] >= ACC_REF {
                continue;
            }
            let d = dirs[[y, x]];
            if d < 0 {
                continue;
            }
            let (dy, dx) = crate::hydrology::N8[d as usize];
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                continue;
            }
            let (ny, nx) = (ny as usize, nx as usize);
            if acc[[ny, nx]] >= 4.0 * acc[[y, x]] && cut[[ny, nx]] - ca >= HANG_MIN {
                ice.hangs.push((y as u16, x as u16));
            }
        }
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7). The M28 rows gate the footprint; the M29
/// rows gate the carve — U-valley and cirque density per glaciated
/// latitude belt, hanging valleys per world. Calibrated on the report
/// seeds; the alpine belt is 40–62° |lat|, the sheet belt poleward.
pub const BANDS: &[Band] = &[
    Band { name: "ice share of land at LGM", sweet: (30.0, 48.0), hard: (20.0, 60.0), target: "sweet 30–48 · hard 20–60 (% of land under the LGM sheets; twin polar landmasses run this above Earth's ~25)" },
    Band { name: "lowland ice margin lat", sweet: (55.0, 65.0), hard: (50.0, 72.0), target: "sweet 55–65 · hard 50–72 (p5 |lat|° of glaciated lowland, h<0.10; the L0=62° law puts it near 60)" },
    Band { name: "ELA poleward monotone", sweet: (85.0, 100.0), hard: (70.0, 100.0), target: "sweet 85–100 · hard 70–100 (% of 3° bins where the lowest glaciated cell drops)" },
    Band { name: "peak ice thickness m", sweet: (1200.0, 4000.0), hard: (600.0, 4000.0), target: "sweet 1200–4000 · hard 600–4000 (dome height; parabolic profile, Antarctic cap)" },
    Band { name: "u-valley cells per 1000 iced, alpine", sweet: (20.0, 200.0), hard: (5.0, 400.0), target: "sweet 20–200 · hard 5–400 (carved cells per 1000 glaciated, 40–62°|lat|)" },
    Band { name: "u-valley cells per 1000 iced, sheet", sweet: (10.0, 200.0), hard: (2.0, 400.0), target: "sweet 10–200 · hard 2–400 (carved cells per 1000 glaciated, >62°|lat|)" },
    Band { name: "cirques per 1000 iced, alpine", sweet: (1.0, 40.0), hard: (0.2, 100.0), target: "sweet 1–40 · hard 0.2–100 (scooped bowls per 1000 glaciated alpine cells)" },
    Band { name: "hanging valleys per world", sweet: (10.0, 600.0), hard: (2.0, 2000.0), target: "sweet 10–600 · hard 2–2000 (tributaries left in the air at trunk junctions)" },
];
