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
/// M29 — a drowned glacial trough: the ice overdeepened the valley
/// below the waterline and the sea took it.
pub const FJORD: u8 = 4;
/// M30 — the depositional legacy: ridge of the former ice margin,
/// flow-combed swarm hill, subglacial meltwater ridge.
pub const MORAINE: u8 = 5;
pub const DRUMLIN: u8 = 6;
pub const ESKER: u8 = 7;
/// M31 — an outburst spillway: the oversized abandoned valley a
/// proglacial lake cut below its moraine sill.
pub const SPILLWAY: u8 = 8;
/// M32 — a braided outwash corridor: the flat gravel plain the
/// meltwater planed below the former ice margin.
pub const OUTWASH: u8 = 9;
/// M33 — patterned ground: frost-sorted polygon nets and solifluction
/// stripes where real permafrost meets the surface.
pub const PATTERNED: u8 = 10;
/// M43 — an intertidal flat: ground the tide uncovers and re-covers
/// daily, where a real range meets a low-slope shore.
pub const TIDEFLAT: u8 = 11;
/// M43 — an estuary mouth: a river meeting tidal water.
pub const ESTUARY: u8 = 12;

/// A drowned cell counts as a ria when land at least this tall stands
/// within `WALL_R` cells — valley walls, not open flats.
const RIA_WALL: f32 = 0.12;
const WALL_R: isize = 2;

/// Classify every cell of the (possibly widened) grid. Rows map 1:1 to
/// the sea-level row profile — the widen adds columns only.
pub fn classify(height: &Array2<f32>, sl: &SeaLevel, ice: &crate::ice::Ice) -> Array2<u8> {
    let (h, w) = height.dim();
    let mut out: Array2<u8> = Array2::zeros((h, w));
    // M29 — fjords first: a drowned cell the ice carved is a fjord no
    // matter what the sea-level ledger says about it.
    if ice.carved.dim() == (h, w) {
        for y in 0..h {
            for x in 0..w {
                if height[[y, x]] < 0.0 && ice.carved[[y, x]] >= crate::ice::FJORD_MIN {
                    out[[y, x]] = FJORD;
                }
            }
        }
    }
    let last = sl.row.len().saturating_sub(1);
    for y in 0..h {
        let dz = (sl.row[y.min(last)] - sl.eustatic) as f32;
        if dz == 0.0 {
            continue;
        }
        for x in 0..w {
            let hv = height[[y, x]];
            let h0 = hv - dz;
            if out[[y, x]] == FJORD {
                continue;
            }
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
    // M30 — the depositional legacy joins the vocabulary: land cells
    // only, and the coastal story wins where the two overlap.
    for (reg, tag) in [
        (&ice.moraines, MORAINE),
        (&ice.drumlins, DRUMLIN),
        (&ice.eskers, ESKER),
    ] {
        for &(y, x) in reg.iter() {
            let (y, x) = (y as usize, x as usize);
            if y < h && x < w && height[[y, x]] >= 0.0 && out[[y, x]] == NONE {
                out[[y, x]] = tag;
            }
        }
    }
    // M31 — the spillways: outburst valleys on land, same precedence.
    for ch in &ice.spillways {
        for &(y, x) in ch.iter() {
            let (y, x) = (y as usize, x as usize);
            if y < h && x < w && height[[y, x]] >= 0.0 && out[[y, x]] == NONE {
                out[[y, x]] = SPILLWAY;
            }
        }
    }
    // M32 — the outwash corridors: braided plains on land, same
    // precedence (the coastal story and the ridge registries win).
    if ice.outwash.dim() == (h, w) {
        for y in 0..h {
            for x in 0..w {
                if height[[y, x]] >= 0.0
                    && out[[y, x]] == NONE
                    && ice.outwash[[y, x]] >= crate::ice::OUT_BRAID_MIN
                {
                    out[[y, x]] = OUTWASH;
                }
            }
        }
    }
    out
}

/// M33 — patterned ground joins the vocabulary after the permafrost
/// pass (which runs post-classify): land cells only, and the coastal
/// story and the glacial registries win where they overlap.
pub fn stamp_patterned(out: &mut Array2<u8>, pattern: &Array2<u8>, height: &Array2<f32>) {
    let (h, w) = out.dim();
    if pattern.dim() != (h, w) {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            if height[[y, x]] >= 0.0 && out[[y, x]] == NONE && pattern[[y, x]] != 0 {
                out[[y, x]] = PATTERNED;
            }
        }
    }
}

/// M43 — the tide must reach mesotidal before it can build a flat or
/// mark a mouth as an estuary.
pub const FLAT_MIN_RANGE: f64 = 2.0;
pub const EST_MIN_RANGE: f64 = 2.0;
/// M43 — a flat forms where the intertidal outcrop spans real ground:
/// range (m) divided by the local slope must stretch at least this
/// many metres of shore. A quarter map cell — a flat you could draw
/// (at 2000 m the fjord-coast seed 777 kept just 2 flat cells; the
/// law's scaling held but the shore read barren).
pub const FLAT_WIDTH_M: f64 = 1000.0;
/// M43 — vertical proximity to the waterline, metres: a candidate
/// cell's mean elevation magnitude must sit within reach of the tide.
pub const FLAT_VERT_M: f64 = 16.0;

/// M43 — the tides join the vocabulary after the tide field is solved
/// (post-widen, like everything coastal). Estuaries first — the mouth
/// outranks the flat on the same cell — then the formation law: the
/// tide builds a flat where its vertical range, spread over the local
/// slope, spans at least `FLAT_WIDTH_M` of shore near the waterline.
/// (Flats are depositional — the tide manufactures them — so the rule
/// reads formation capacity, not pre-existing bathymetry: the strict
/// intertidal-band criterion left 0–6 cells per world at 4 km
/// resolution.) The earlier stories (coastal history, glacial
/// registries, patterned ground) win where they already spoke.
pub fn stamp_tidal(
    out: &mut Array2<u8>,
    tides: &crate::tides::Tides,
    height: &Array2<f32>,
    flags: &Array2<u8>,
) {
    let (h, w) = out.dim();
    if tides.range.dim() != (h, w) || height.dim() != (h, w) || flags.dim() != (h, w) {
        return;
    }
    let river = crate::state::CellFlags::RIVER.bits();
    // Estuary mouths: a river cell on land, touching open tidal water.
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE || height[[y, x]] < 0.0 || flags[[y, x]] & river == 0 {
                continue;
            }
            let mut tidal = false;
            for (ny, nx) in [
                (y.wrapping_sub(1), x),
                (y + 1, x),
                (y, x.wrapping_sub(1)),
                (y, x + 1),
            ] {
                if ny < h
                    && nx < w
                    && tides.class[[ny, nx]] == crate::tides::OPEN
                    && tides.range[[ny, nx]] as f64 >= EST_MIN_RANGE
                {
                    tidal = true;
                    break;
                }
            }
            if tidal {
                out[[y, x]] = ESTUARY;
            }
        }
    }
    // Intertidal flats: a waterline cell (open water touching land, or
    // land touching open water) near sea level, where range over slope
    // spans at least FLAT_WIDTH_M of shore.
    let cell_m = crate::constants::KM_PER_CELL * 1000.0;
    for y in 0..h {
        for x in 0..w {
            if out[[y, x]] != NONE {
                continue;
            }
            // Waterline test and the governing range: own range for an
            // open-water cell with a land neighbor, the wettest open
            // neighbor's range for a land cell on the shore.
            let is_open = tides.class[[y, x]] == crate::tides::OPEN;
            let mut r = 0.0f64;
            let mut waterline = false;
            for (ny, nx) in [
                (y.wrapping_sub(1), x),
                (y + 1, x),
                (y, x.wrapping_sub(1)),
                (y, x + 1),
            ] {
                if ny >= h || nx >= w {
                    continue;
                }
                if is_open {
                    if height[[ny, nx]] >= 0.0 {
                        waterline = true;
                    }
                } else if tides.class[[ny, nx]] == crate::tides::OPEN {
                    waterline = true;
                    r = r.max(tides.range[[ny, nx]] as f64);
                }
            }
            if is_open {
                r = tides.range[[y, x]] as f64;
            } else if height[[y, x]] < 0.0 {
                // enclosed water is never a tidal flat
                continue;
            }
            if !waterline || r < FLAT_MIN_RANGE {
                continue;
            }
            // Near the waterline vertically...
            let hv_m = height[[y, x]] as f64 * crate::constants::METRES_PER_UNIT;
            if hv_m.abs() > FLAT_VERT_M {
                continue;
            }
            // ...and gentle enough that the intertidal band spans real
            // ground: slope from the 3×3 relief over 2 cells.
            let mut lo = f32::INFINITY;
            let mut hi = f32::NEG_INFINITY;
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                        continue;
                    }
                    let v = height[[ny as usize, nx as usize]];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            let slope = ((hi - lo) as f64 * crate::constants::METRES_PER_UNIT / (2.0 * cell_m))
                .max(1e-6);
            if r / slope >= FLAT_WIDTH_M {
                out[[y, x]] = TIDEFLAT;
            }
        }
    }
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
