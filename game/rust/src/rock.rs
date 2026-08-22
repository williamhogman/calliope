//! Rock provinces (M18) — the basement geology of the world.
//!
//! The ground differs by *history*, not just by height: every cell is
//! classified once at genesis into one of four provinces read straight
//! off the plate-history sketch (M16/ADR-0024) and the finished relief.
//!
//! - **Shield** — the cratonic cores: old continental interiors far from
//!   any seam, basement rock standing since the deep past.
//! - **Basin** — sedimentary cover: young or low interiors where the
//!   ages have laid down sand, silt and stone in beds.
//! - **Fold belt** — the orogeny country: rock bent and stacked within
//!   reach of a convergent seam, wide while the collision is young,
//!   narrowing to worn roots as it ages (M17).
//! - **Volcanic** — arc and rift terranes: cells riding young convergent
//!   seams, divergent/transform boundaries, and the rare hotspot blob.
//!
//! Everything is a pure function of the seed (ADR-0003): the noise
//! plane, the thresholds and the floor pass are all deterministic. The
//! grid is frozen at genesis — nothing here advances in tick time.

use ndarray::Array2;

use crate::noisegen::Perlin3;
use crate::plates::{Plates, B_CONVERGENT, B_NONE};
use crate::util::Band;

/// Province codes stored in `Fields::rock` (wire dtype u8).
pub const SHIELD: u8 = 0;
pub const BASIN: u8 = 1;
pub const FOLD_BELT: u8 = 2;
pub const VOLCANIC: u8 = 3;

/// Display names, indexed by code.
pub const NAMES: [&str; 4] = ["shield", "basin", "fold belt", "volcanic"];

/// M20 — regional stone: what the quarries of a province cut. Shields
/// yield granite from the old basement, basins limestone from their
/// sediments, fold belts marble from cooked carbonates, volcanic
/// terranes basalt. Pure function of the province code, so a town's
/// stone can never disagree with the rock under it.
pub fn quarry(province: u8) -> &'static str {
    match province {
        SHIELD => "granite",
        BASIN => "limestone",
        FOLD_BELT => "marble",
        VOLCANIC => "basalt",
        _ => "fieldstone",
    }
}

/// Every class must hold at least this share of the land — the M18 gate
/// checks 2%; the floor pass aims a little above it so the gate never
/// rides the boundary.
const FLOOR_SHARE: f64 = 0.025;

/// Classify every cell of the base-size grid. `height` is the eroded
/// relief (sea level at 0.0); the sketch supplies plate age, seam
/// distance and seam age. Margins added later by the widen pass are
/// open ocean and ride as `BASIN`.
pub fn classify(seed: i64, size: usize, plates: &Plates, height: &Array2<f32>) -> Array2<u8> {
    let hotspot = Perlin3::new(seed + 606);
    let n = size as f64;

    // Per-plate lookups the cell loop reads.
    let age_of: Vec<f64> = plates.plates.iter().map(|p| p.age).collect();
    let cont: Vec<bool> = plates.plates.iter().map(|p| p.continental).collect();

    // Affinity planes: kept so the floor pass can promote the *most
    // fitting* cells rather than arbitrary ones.
    let mut rock = Array2::from_elem((size, size), BASIN);
    let mut aff_volc = Array2::zeros((size, size));
    let mut aff_fold = Array2::zeros((size, size));
    let mut aff_shield = Array2::zeros((size, size));

    // E10.1 — the cell loop reads five plate grids and writes four
    // planes; on the flat row-major slices those ten accesses per cell
    // are one add each instead of a 2-D stride multiply. Every float
    // expression is untouched, so the province map is bit-identical.
    {
        let cells = plates.cell.as_slice().expect("cell is standard layout");
        let seam_ds = plates.seam_dist.as_slice().expect("seam_dist is standard layout");
        let seam_as = plates.seam_age.as_slice().expect("seam_age is standard layout");
        let edge_ds = plates.edge_dist.as_slice().expect("edge_dist is standard layout");
        let bnds = plates.boundary.as_slice().expect("boundary is standard layout");
        let hts = height.as_slice().expect("height is standard layout");
        let rocks = rock.as_slice_mut().expect("rock is standard layout");
        let avs = aff_volc.as_slice_mut().expect("aff_volc is standard layout");
        let afs = aff_fold.as_slice_mut().expect("aff_fold is standard layout");
        let ass = aff_shield.as_slice_mut().expect("aff_shield is standard layout");
        for y in 0..size {
            let row = y * size;
            let yc = y as f64 / n * 4.0;
            for x in 0..size {
                let i = row + x;
                let pid = cells[i] as usize;
                let seam_d = seam_ds[i] as f64;
                let seam_a = seam_as[i] as f64;
                let edge_d = edge_ds[i] as f64;
                let b = bnds[i];

                // Hotspot plumes: rare low-frequency blobs (the same deep
                // machinery that strings the archipelagos).
                let hs = hotspot.fbm(x as f64 / n * 4.0, yc, 2.5, 3);

                // Youth of the nearest collision — young seams stand wide.
                let youth = (-seam_a / 900.0).exp();

                let v_aff = (hs - 0.30)
                    .max(1.0 - seam_d / 4.0 + 0.6 * youth - 0.6)
                    .max(if b != B_NONE && b != B_CONVERGENT { 0.9 } else { -1.0 });
                let f_aff = (1.0 - seam_d / (5.0 + 11.0 * youth)).max(-1.0);
                let s_aff = if cont[pid] {
                    (age_of[pid] / 2400.0) * (edge_d / (0.06 * n)).min(1.5) - 0.55
                } else {
                    -1.0
                };
                avs[i] = v_aff;
                afs[i] = f_aff;
                ass[i] = s_aff;

                // Precedence: the loudest history wins the cell.
                rocks[i] = if hs > 0.62
                    || (seam_d <= 3.0 && seam_a <= 900.0)
                    || (b != B_NONE && b != B_CONVERGENT)
                {
                    VOLCANIC
                } else if seam_d <= 5.0 + 11.0 * youth {
                    FOLD_BELT
                } else if cont[pid] && age_of[pid] >= 1000.0 && edge_d >= 0.06 * n {
                    SHIELD
                } else if hts[i] > 0.5 {
                    // M21 — legibility of the heights: a mountain cannot read
                    // as trough country. Near a seam it is fold country; on an
                    // old continental interior it is exhumed shield basement;
                    // anywhere else it is a hotspot pile.
                    if seam_d <= 18.0 {
                        FOLD_BELT
                    } else if cont[pid] && age_of[pid] >= 1000.0 && edge_d >= 0.03 * n {
                        SHIELD
                    } else {
                        VOLCANIC
                    }
                } else {
                    BASIN
                };
            }
        }
    }

    // ---- the floor of fate (ADR-0013 house style) ----------------------
    // Every province must be *present* on land — a world can be dealt a
    // sketch whose seams all run under the sea. Promote the best-fitting
    // land cells, deterministically ranked, until each class holds its
    // floor share of the land. Only cells of over-represented classes
    // are taken, so one floor never starves another.
    let land: Vec<(usize, usize)> = (0..size)
        .flat_map(|y| (0..size).map(move |x| (y, x)))
        .filter(|&(y, x)| height[[y, x]] >= 0.0)
        .collect();
    if land.is_empty() {
        return rock;
    }
    let floor_n = ((land.len() as f64) * FLOOR_SHARE).ceil() as usize;
    let mut counts = [0usize; 4];
    for &(y, x) in &land {
        counts[rock[[y, x]] as usize] += 1;
    }
    for (class, aff) in [
        (SHIELD, &aff_shield),
        (FOLD_BELT, &aff_fold),
        (VOLCANIC, &aff_volc),
    ] {
        if counts[class as usize] >= floor_n {
            continue;
        }
        // Rank candidate land cells by affinity, ties broken by (y, x).
        let mut cands: Vec<(f64, usize, usize)> = land
            .iter()
            .filter(|&&(y, x)| {
                let c = rock[[y, x]] as usize;
                c != class as usize && counts[c] > floor_n * 2
            })
            .map(|&(y, x)| (aff[[y, x]], y, x))
            .collect();
        cands.sort_by(|a, b| {
            b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
        });
        for (_, y, x) in cands {
            if counts[class as usize] >= floor_n {
                break;
            }
            let old = rock[[y, x]] as usize;
            if counts[old] <= floor_n * 2 {
                continue;
            }
            counts[old] -= 1;
            rock[[y, x]] = class;
            counts[class as usize] += 1;
        }
    }
    rock
}

/// M21 — geologic legibility: does the province map read true to a
/// glance? Each class is sampled against an *independent* landform
/// correlate — not the classifier's own thresholds, but what a reader
/// of the map would expect:
///
/// - a **shield** sits on an old continental interior (cratonic plate,
///   ≥1000 Myr, not hugging the plate edge);
/// - a **basin** is low-relief trough country, never an alpine cap;
/// - a **fold belt** hugs an orogenic seam, never open interior.
///
/// Returns the mismatch share per class `[shield, basin, fold]` over
/// its own land cells. Volcanic terranes carry no correlate: hotspot
/// blobs land anywhere by design. All three grids must share one
/// frame — the widen pass grows plates and rock in lockstep, so the
/// shipped world qualifies.
pub fn legibility(
    rock: &Array2<u8>,
    plates: &crate::plates::Plates,
    height: &Array2<f32>,
) -> [f64; 3] {
    let rows = rock.dim().0 as f64; // base grid height survives the widen
    let age_of: Vec<f64> = plates.plates.iter().map(|p| p.age).collect();
    let cont: Vec<bool> = plates.plates.iter().map(|p| p.continental).collect();
    let mut n = [0usize; 3];
    let mut bad = [0usize; 3];
    for ((y, x), &r) in rock.indexed_iter() {
        if height[[y, x]] < 0.0 {
            continue;
        }
        match r {
            SHIELD => {
                n[0] += 1;
                let pid = plates.cell[[y, x]] as usize;
                let interior = cont[pid]
                    && age_of[pid] >= 1000.0
                    && (plates.edge_dist[[y, x]] as f64) >= 0.03 * rows;
                if !interior {
                    bad[0] += 1;
                }
            }
            BASIN => {
                n[1] += 1;
                if height[[y, x]] > 0.5 {
                    bad[1] += 1; // a basin capping a mountain reads false
                }
            }
            FOLD_BELT => {
                n[2] += 1;
                if (plates.seam_dist[[y, x]] as f64) > 18.0 {
                    bad[2] += 1; // a belt with no seam to hug reads false
                }
            }
            _ => {}
        }
    }
    [
        bad[0] as f64 / n[0].max(1) as f64,
        bad[1] as f64 / n[1].max(1) as f64,
        bad[2] as f64 / n[2].max(1) as f64,
    ]
}

/// Land share of each province, indexed by code.
pub fn land_shares(rock: &Array2<u8>, height: &Array2<f32>) -> [f64; 4] {
    let mut counts = [0usize; 4];
    let mut land = 0usize;
    for (r, h) in rock.iter().zip(height.iter()) {
        if *h >= 0.0 {
            land += 1;
            counts[*r as usize] += 1;
        }
    }
    let land = land.max(1) as f64;
    [
        counts[0] as f64 / land,
        counts[1] as f64 / land,
        counts[2] as f64 / land,
        counts[3] as f64 / land,
    ]
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (M18): the ground must differ by history.
pub const BANDS: &[Band] = &[
    Band { name: "shield share of land", sweet: (0.02, 0.60), hard: (0.02, 0.80), target: "M18 gate: every province present at ≥2% of land" },
    Band { name: "basin share of land", sweet: (0.02, 0.85), hard: (0.02, 0.92), target: "M18 gate: every province present at ≥2% of land" },
    Band { name: "fold-belt share of land", sweet: (0.02, 0.45), hard: (0.02, 0.60), target: "M18 gate: every province present at ≥2% of land" },
    Band { name: "volcanic share of land", sweet: (0.02, 0.30), hard: (0.02, 0.45), target: "M18 gate: every province present at ≥2% of land" },
];
