//! M26 — drowned and raised coasts: the sea-level history (M25) leaves
//! a legible vocabulary of coastal landforms.
//!
//! The freeze-time offset `dz(y) = isostasy(y) − eustatic` moved the
//! land against the waterline; this module reads the *signature* of
//! that move back out of the pair (final height, pre-offset height):
//!
//! - **Raised beach** — land that stood below the old waterline and
//!   was carried above it (`h0 < 0 ≤ h`): the emerged shelf strip,
//!   densest where the land rose most (the rebound belt — Scandinavia's
//!   strandlines).
//! - **Ria** — sea that was land before the waterline rose over it
//!   (`h0 ≥ 0 > h`) under steep walls: a drowned valley, a firth.
//! - **Skerry** — the same drowned ground in low relief: a scatter of
//!   flooded flats and islets, a skerry field.
//!
//! The grid is pure derived state — a function of the height field and
//! the frozen `SeaLevel` — recomputed identically every generation and
//! folded into `hash_state` to hold the classifier still (the M26
//! gate). Naming reads it to mint firths, skerry fields and strands;
//! the label layer draws them like any other coastal detail.

use ndarray::Array2;

use crate::sealevel::SeaLevel;
use crate::util::fnv1a64;

pub const NONE: u8 = 0;
pub const RAISED: u8 = 1;
pub const RIA: u8 = 2;
pub const SKERRY: u8 = 3;

/// A drowned cell counts as a ria when land at least this tall stands
/// within `WALL_R` cells — valley walls, not open flats.
const RIA_WALL: f32 = 0.12;
const WALL_R: isize = 2;

/// Classify every cell of the (possibly widened) grid. Rows map 1:1 to
/// the sea-level row profile — the widen adds columns only.
pub fn classify(height: &Array2<f32>, sl: &SeaLevel) -> Array2<u8> {
    let (h, w) = height.dim();
    let mut out: Array2<u8> = Array2::zeros((h, w));
    let last = sl.row.len().saturating_sub(1);
    for y in 0..h {
        let dz = (sl.row[y.min(last)] - sl.eustatic) as f32;
        if dz == 0.0 {
            continue;
        }
        for x in 0..w {
            let hv = height[[y, x]];
            let h0 = hv - dz;
            if hv >= 0.0 && h0 < 0.0 {
                out[[y, x]] = RAISED;
            } else if hv < 0.0 && h0 >= 0.0 {
                // drowned ground: walls nearby make it a ria, open
                // low relief makes it a skerry field
                let mut walled = false;
                'scan: for ddy in -WALL_R..=WALL_R {
                    for ddx in -WALL_R..=WALL_R {
                        let ny = y as isize + ddy;
                        let nx = x as isize + ddx;
                        if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                            continue;
                        }
                        if height[[ny as usize, nx as usize]] >= RIA_WALL {
                            walled = true;
                            break 'scan;
                        }
                    }
                }
                out[[y, x]] = if walled { RIA } else { SKERRY };
            }
        }
    }
    out
}

/// FNV-1a over the tag grid — joins `hash_state` so the classifier
/// cannot drift silently between generations or runtimes.
pub fn hash(grid: &Array2<u8>) -> u64 {
    fnv1a64(grid.as_slice().expect("landform grid is contiguous"))
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): coastal-landform frequency, normalized by
/// the sea-level-curve amplitude that made it (the M26 gate) — a world
/// that moved its waterline twice as far should show roughly twice the
/// coast rewritten. Ranges calibrated on the three report seeds.
pub const BANDS: &[Band] = &[
    Band { name: "raised coast per stand", sweet: (70.0, 150.0), hard: (40.0, 250.0), target: "sweet 70–150 · hard 40–250 (share of coast per mean emergence, ≈1/coastal slope)" },
    Band { name: "drowned coast per stand", sweet: (60.0, 150.0), hard: (25.0, 300.0), target: "sweet 60–150 · hard 25–300 (share of coast per mean submergence)" },
];
