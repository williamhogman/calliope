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
//! **The legacy** (M30) is what the ice dropped rather than what it
//! took: broad **till sheets** under the thick slow interior (ground
//! rock smeared over gentle lowland — the fertile plains the farms
//! find later), **terminal moraines** strung along the former margin,
//! **drumlin swarms** combed down the old flow lines just inside it,
//! and **eskers** snaking where subglacial meltwater ran. Deposition
//! raises are small and land-only; the waterline never moves. Till is
//! not relief but soil: `agriculture::fertility` reads the sheet and
//! pays a bonus where the climate can use it.
//!
//! The **loess mantle** carries the legacy equatorward: the silt the
//! ice ground fine blows off the outwash aprons and settles as a
//! decaying plume toward warmer latitudes — on Earth this is the corn
//! belt, the chernozem, the Loess Plateau (research/08). It is the
//! mechanism that lets a tundra-margined sheet still feed temperate
//! farms; without it the till dividend dies inside the frost line.
//! Transport is a deterministic column-wise equatorward walk with
//! literal decay constants (multiplications only — IEEE-exact, no
//! libm in the gated ledger, ADR-0025 discipline).
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
/// higher than the scooped cell. Calibrated post-erosion: the fluvial
/// pass planes local relief, so per-cell rises top out near 0.05 —
/// 0.03 keeps cirques to the genuinely steep-backed heads.
const CIRQUE_WALL: f64 = 0.03;

/// A tributary hangs when the trunk's centerline was carved at least
/// this much deeper at the junction. Measured on the report seeds: the
/// trunk–tributary overdeepening differential runs 0.000–0.013, median
/// ~0.005; 0.002 keeps the marked steps and drops the flat junctions.
const HANG_MIN: f64 = 0.002;

/// Overdeepening floor: no trough is carved below this height — fjords
/// drown, the abyss stays tectonic.
const CARVE_FLOOR: f64 = -0.35;

/// A drowned cell counts as fjord country above this carve depth
/// (read by `landform::classify`).
pub const FJORD_MIN: f32 = 0.010;

// ---------------------------------------------------------- deposit laws

/// Ice thinner than this ground no rock worth the name — no till (m).
const TILL_TH_MIN: f64 = 250.0;

/// Full-strength till under this much ice (m); grinding saturates.
const TILL_TH_REF: f64 = 1500.0;

/// Till settles on gentle ground: steepest N4 rise below this
/// (height units per cell). Steeper slopes shed their drift.
const TILL_SLOPE_MAX: f64 = 0.015;

/// Till plains are lowland features; above this the ice quarried
/// rather than dropped (height units).
const TILL_H_MAX: f64 = 0.30;

/// Terminal-moraine ridge raise at the former margin (height units).
const MORAINE_H: f64 = 0.006;

/// Moraines survive on gentle forefield ground below this slope.
const MORAINE_SLOPE_MAX: f64 = 0.02;

/// Drumlin bump (height units), long axis down the old flow line.
const DRUMLIN_H: f64 = 0.004;

/// One eligible till cell in ~24 seeds a drumlin (before spacing).
const DRUMLIN_P: f64 = 1.0 / 24.0;

/// Accepted drumlins keep this Chebyshev spacing (cells).
const DRUMLIN_GAP: isize = 2;

/// Esker ridge raise (height units).
const ESKER_H: f64 = 0.004;

/// Esker channels: subglacial melt lines with real but sub-trough
/// catchment — at least this many drained cells (the carve's ACC_MIN
/// is the ceiling: bigger lines carved troughs instead).
const ESK_ACC_LO: f64 = 8.0;

/// Chains start on a draw at this rate over eligible cells.
const ESK_P: f64 = 1.0 / 40.0;

/// A chain runs at most this many cells downstream, and is discarded
/// under `ESK_MIN` — an esker is a ridge you can walk, not a dot.
const ESK_LEN: usize = 12;
const ESK_MIN: usize = 3;

/// Subglacial forms (drumlins, eskers) need this much ice overhead (m):
/// they are made well inside the sheet, not on the margin apron.
const SUBGLACIAL_TH: f64 = 400.0;

/// Fertility bonus per unit till strength, temperature-gated on the
/// agriculture side (`agriculture::fertility` reads this).
pub const TILL_FERT: f64 = 0.15;

// ------------------------------------------------------------ loess laws

/// Loess plume decay per land cell walked equatorward. A literal
/// (≈ e-fold over 22 cells ≈ 90 km at 4 km/cell): Earth's loess
/// belts run hundreds of km beyond the outwash aprons.
const LOESS_DECAY_LAND: f64 = 0.955;

/// Decay per water cell — the plume thins fast over open water and
/// deposits nothing there.
const LOESS_DECAY_WATER: f64 = 0.85;

/// Below this strength the mantle is dust, not soil: no deposit.
const LOESS_MIN: f64 = 0.08;

/// Fertility bonus per unit loess strength, temperature-gated like
/// till. Higher than TILL_FERT: loess is the premium farm soil —
/// chernozem and corn-belt ground (research/08).
pub const LOESS_FERT: f64 = 0.22;

// ------------------------------------------------------ proglacial laws

/// A flooded cell counts toward a proglacial basin when the flood fill
/// stands at least this far above the bed (height units): puddles are
/// soil, not lakes. ~6 m at the 4 km/cell scale — moraine-fringe
/// basins are shallow by nature; the area floor keeps out the ponds.
const PROG_DEP_MIN: f64 = 0.0015;

/// Minimum basin area, cells (4 km/cell → 4 cells ≈ 64 km²): the
/// proglacial story is about the giants, not kettle ponds.
const PROG_MIN_AREA: usize = 4;

/// A basin is moraine-dammed when any of its flooded cells — or any
/// cell of its shore rim — sits within this Chebyshev radius of a
/// terminal-moraine ridge cell. The dam holds the water from the
/// sill, so the shore counts as much as the lake.
const PROG_MORAINE_R: isize = 3;

/// Outburst cut below the old sill: a base notch plus a volume-scaled
/// term, so the great lakes cut the great channels.
const SPILL_CUT_BASE: f64 = 0.004;
const SPILL_CUT_K: f64 = 0.010;
const SPILL_VOL_REF: f64 = 2.0;

/// Width classes by impounded volume (height-units·cells): channels of
/// bigger lakes run wider — 1, 2 or 3 cells across.
const SPILL_VOL_W2: f64 = 0.5;
const SPILL_VOL_W3: f64 = 2.0;

/// Lateral shoulders of a widened channel sit this far above its floor.
const SPILL_LAT_LIFT: f64 = 0.002;

/// Per-step floor descent — the carved channel is a strict staircase,
/// so it drains instead of ponding.
const SPILL_STEP: f64 = 1e-4;

/// Give up after this many cells; stop early once the land runs
/// naturally below the floor for this many consecutive steps (the
/// channel has merged into lower ground).
const SPILL_MAX: usize = 240;
const SPILL_MERGE: usize = 3;

// ---- M32 — outwash plains and braided meltwater rivers ---------------

/// Meltwater accumulation (upstream ice column in km·cells) a cell
/// below the margin needs before it counts as a braided corridor.
const OUT_ACC_MIN: f64 = 40.0;

/// Local relief ceiling for a corridor cell — max |Δh| to any N8
/// neighbour, normalized height units. Braids wander only on flats.
const OUT_SLOPE_MAX: f64 = 0.010;

/// Lowland ceiling: outwash is a plains story (matches TILL_H_MAX).
const OUT_H_MAX: f64 = 0.30;

/// A neighbour standing within this band above a corridor cell gets
/// planed down into the apron.
const OUT_APRON: f64 = 0.020;

/// The planed neighbour keeps this much lift over the corridor floor.
const OUT_LIFT: f64 = 0.002;

/// Fertility bonus per unit outwash strength (agriculture.rs): glacial
/// silt over gravel — real, but leaner than till (0.15) or loess (0.22).
pub const OUT_FERT: f64 = 0.10;

/// Outwash strength at which a modern river over the plain reads as
/// braided (hydrology.rs) — corridors carry 1.0, aprons 0.5.
pub const OUT_BRAID_MIN: f32 = 0.9;


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
    /// M30 — till sheet strength, 0..1; > 0 marks the depositional
    /// footprint. Soil, not relief: agriculture pays a bonus on it.
    pub till: Array2<f32>,
    /// M30 — terminal-moraine ridge cells strung along the old margin.
    pub moraines: Vec<(u16, u16)>,
    /// M30 — drumlin seeds, long axis down the local flow line.
    pub drumlins: Vec<(u16, u16)>,
    /// M30 — esker ridge cells: subglacial meltwater lines, chained.
    pub eskers: Vec<(u16, u16)>,
    /// M30 — loess mantle strength, 0..1: wind-blown glacial silt
    /// settled equatorward of the outwash aprons. Soil, not relief;
    /// the warm end of the depositional footprint.
    pub loess: Array2<f32>,
    /// M31 — proglacial lake seeds (y, x): the deepest cell of each
    /// moraine-dammed basin the melt once filled.
    pub proglacial: Vec<(u16, u16)>,
    /// M31 — spillway channels, one chain per lake in `proglacial`
    /// order: the outburst valleys cut below the old sills, walked
    /// downstream in carve order.
    pub spillways: Vec<Vec<(u16, u16)>>,
    /// M31 — per-lake meta in `proglacial` order: (impounded volume
    /// ×1000, quantized; basin area in cells; channel width class 1–3).
    pub prog_meta: Vec<(u32, u16, u16)>,
    /// M31 — lake chains: groups of ≥2 proglacial lakes strung together
    /// by their spillways (one lake's channel ends in the next basin).
    pub chains: u32,
    /// M32 — outwash strength per cell: 1.0 on braided meltwater
    /// corridors below the former margin, 0.5 on the planed aprons
    /// beside them, 0 elsewhere. Read by fertility (`OUT_FERT`) and by
    /// hydrology's braided classification (`OUT_BRAID_MIN`).
    pub outwash: Array2<f32>,
    /// M34 — the ice that remains: modern mountain glaciers under
    /// today's climate. Annual surface mass balance in m w.e./yr where
    /// positive, 0 elsewhere; > 0 is the glacier mask. Since M35 it is
    /// computed at the climate stage (pre-widen, off the f64 grids) so
    /// hydrology can feed the melt to the rivers below, then rides
    /// `widen` with the rest of the ledger.
    pub modern: Array2<f32>,
    /// M35 — accumulated glacial meltwater discharge per cell, same
    /// units as `fields.discharge` (cells·metres of runoff): the melt
    /// lane of every river, flow-routed down the same drainage tree.
    /// Diagnostics and inspectors read melt/discharge for the
    /// glacier-fed regime; never on the wire.
    pub melt: Array2<f32>,
    /// M35 — signed month-0 harmonic of the melt lane, −1..1 (same
    /// convention as `flow_amp`): where the accumulated melt peaks in
    /// the year. 0 wherever no melt flows.
    pub melt_amp: Array2<f32>,
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
            till: Array2::zeros((0, 0)),
            moraines: Vec::new(),
            drumlins: Vec::new(),
            eskers: Vec::new(),
            loess: Array2::zeros((0, 0)),
            proglacial: Vec::new(),
            spillways: Vec::new(),
            prog_meta: Vec::new(),
            chains: 0,
            outwash: Array2::zeros((0, 0)),
            modern: Array2::zeros((0, 0)),
            melt: Array2::zeros((0, 0)),
            melt_amp: Array2::zeros((0, 0)),
        }
    }

    /// FNV-1a over the whole ledger — thickness, carve, till, loess,
    /// outwash, the modern glacier balance (M34), the meltwater
    /// discharge and its harmonic (M35),
    /// ELA profile, and every registry (cirques, hangs, moraines,
    /// drumlins, eskers; the M30 lists length-prefixed; the M31 lakes,
    /// chained spillways and meta). Joins `hash_state` (the I lane) and
    /// the deep-earth identity line (the M28 gate).
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(
            (self.thickness.len() + self.carved.len() + self.till.len() + self.loess.len()) * 4
                + self.ela_row.len() * 8,
        );
        for v in &self.thickness {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.carved {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.till {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.loess {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.outwash {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.modern {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in self.melt.iter().chain(self.melt_amp.iter()) {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in &self.ela_row {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for &(y, x) in self.cirques.iter().chain(self.hangs.iter()) {
            b.extend_from_slice(&y.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
        }
        for reg in [&self.moraines, &self.drumlins, &self.eskers, &self.proglacial] {
            b.extend_from_slice(&(reg.len() as u32).to_le_bytes());
            for &(y, x) in reg {
                b.extend_from_slice(&y.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        b.extend_from_slice(&(self.spillways.len() as u32).to_le_bytes());
        for ch in &self.spillways {
            b.extend_from_slice(&(ch.len() as u32).to_le_bytes());
            for &(y, x) in ch {
                b.extend_from_slice(&y.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        b.extend_from_slice(&(self.prog_meta.len() as u32).to_le_bytes());
        for &(v, a, w) in &self.prog_meta {
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&a.to_le_bytes());
            b.extend_from_slice(&w.to_le_bytes());
        }
        b.extend_from_slice(&self.chains.to_le_bytes());
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
        till: Array2::zeros((rows, cols)),
        moraines: Vec::new(),
        drumlins: Vec::new(),
        eskers: Vec::new(),
        loess: Array2::zeros((rows, cols)),
        proglacial: Vec::new(),
        spillways: Vec::new(),
        prog_meta: Vec::new(),
        chains: 0,
        outwash: Array2::zeros((rows, cols)),
        modern: Array2::zeros((0, 0)),
        melt: Array2::zeros((0, 0)),
        melt_amp: Array2::zeros((0, 0)),
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
    // Centerline depths, unspread — the hang test compares what the ice
    // did *along* each line, which the cross-valley kernel would blur.
    let mut line_cut: Array2<f64> = Array2::zeros((rows, cols));

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
            if depth > line_cut[[y, x]] {
                line_cut[[y, x]] = depth;
            }
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
            let ca = line_cut[[y, x]];
            if ca <= 0.0 || acc[[y, x]] >= ACC_REF {
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
            if acc[[ny, nx]] >= 4.0 * acc[[y, x]] && line_cut[[ny, nx]] - ca >= HANG_MIN {
                ice.hangs.push((y as u16, x as u16));
            }
        }
    }
}

/// M30 — lay down what the ice dropped. Runs after the carve, on the
/// post-carve surface: till sheets under the thick slow interior,
/// terminal moraines strung along the former margin, drumlin swarms
/// combed down the flow lines, eskers snaking where subglacial melt
/// ran. Raises are small and land-only — the waterline never moves.
/// Every draw is SplitMix64 fixed-width and every scan row-major, so
/// the legacy replays byte-identically (ADR-0025 discipline).
pub fn deposit(seed: i64, height: &mut Array2<f64>, ice: &mut Ice) {
    let (rows, cols) = height.dim();
    let water = height.mapv(|v| v < 0.0);
    const N4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    fn slope_at(h: &Array2<f64>, y: usize, x: usize) -> f64 {
        let (rows, cols) = h.dim();
        let mut s = 0.0f64;
        for (dy, dx) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                continue;
            }
            s = s.max((h[[ny as usize, nx as usize]] - h[[y, x]]).abs());
        }
        s
    }

    // 1. Till sheets: ground rock smeared over gentle lowland under
    // thick ice. Strength rises with the load and falls with slope.
    let mut till: Array2<f32> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            let t = ice.thickness[[y, x]] as f64;
            if t < TILL_TH_MIN || water[[y, x]] {
                continue;
            }
            if height[[y, x]] > TILL_H_MAX {
                continue;
            }
            let s = slope_at(height, y, x);
            if s > TILL_SLOPE_MAX {
                continue;
            }
            let tf = ((t - TILL_TH_MIN) / (TILL_TH_REF - TILL_TH_MIN)).min(1.0);
            let sf = 1.0 - s / TILL_SLOPE_MAX;
            till[[y, x]] = (tf * sf) as f32;
        }
    }

    // 2. Terminal moraines: the ice-free land cell just beyond the
    // margin catches the ridge. Mark on the pre-raise surface, then
    // raise in one row-major pass so tests never read their own edits.
    let mut msite: Array2<u8> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if ice.thickness[[y, x]] <= 0.0 || water[[y, x]] {
                continue;
            }
            for (dy, dx) in N4 {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                if water[[ny, nx]] || ice.thickness[[ny, nx]] > 0.0 {
                    continue;
                }
                if slope_at(height, ny, nx) <= MORAINE_SLOPE_MAX {
                    msite[[ny, nx]] = 1;
                }
            }
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            if msite[[y, x]] == 1 {
                height[[y, x]] += MORAINE_H;
                ice.moraines.push((y as u16, x as u16));
            }
        }
    }

    // 3. Drumlins: swarms on strong till under deep ice, long axis down
    // the local flow line (ice flows down its own thickness gradient).
    let base = (seed as u64) ^ 0xD30_D120_u64;
    let mut dgrid: Array2<u8> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if till[[y, x]] < 0.5 || (ice.thickness[[y, x]] as f64) < SUBGLACIAL_TH {
                continue;
            }
            let draw = splitmix64(base ^ ((y as u64) << 32) ^ (x as u64));
            if (draw >> 11) as f64 / (1u64 << 53) as f64 >= DRUMLIN_P {
                continue;
            }
            // spacing: no accepted drumlin within the gap (scan order)
            let mut near = false;
            'gap: for dy in -DRUMLIN_GAP..=DRUMLIN_GAP {
                for dx in -DRUMLIN_GAP..=DRUMLIN_GAP {
                    let ny = y as isize + dy;
                    let nx = x as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                        continue;
                    }
                    if dgrid[[ny as usize, nx as usize]] == 1 {
                        near = true;
                        break 'gap;
                    }
                }
            }
            if near {
                continue;
            }
            // flow line: the N8 neighbour with the steepest thickness drop
            let t0 = ice.thickness[[y, x]] as f64;
            let mut best: Option<(usize, usize)> = None;
            let mut drop = 0.0f64;
            for (dy, dx) in crate::hydrology::N8 {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                let d = t0 - ice.thickness[[ny, nx]] as f64;
                if d > drop {
                    drop = d;
                    best = Some((ny, nx));
                }
            }
            let Some((ny, nx)) = best else { continue }; // domes shed no drumlins
            dgrid[[y, x]] = 1;
            height[[y, x]] += DRUMLIN_H;
            if !water[[ny, nx]] {
                height[[ny, nx]] += DRUMLIN_H * 0.6;
            }
            ice.drumlins.push((y as u16, x as u16));
        }
    }

    // 4. Eskers: subglacial meltwater lines — small-catchment drainage
    // under deep ice, walked downstream as chains. The drainage tree is
    // the post-carve one: melt ran in the carved world.
    let filled = crate::hydrology::fill_depressions(height, &water);
    let dirs = crate::hydrology::flow_directions(&filled, &water);
    let ones = Array2::from_elem((rows, cols), 1000.0);
    let acc = crate::hydrology::flow_accumulation(&filled, &dirs, &ones, &water);
    let ebase = base ^ 0xE5_4E12_u64;
    let mut egrid: Array2<u8> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if water[[y, x]]
                || (ice.thickness[[y, x]] as f64) < SUBGLACIAL_TH
                || acc[[y, x]] < ESK_ACC_LO
                || acc[[y, x]] >= ACC_MIN
                || height[[y, x]] > TILL_H_MAX + 0.10
            {
                continue;
            }
            let draw = splitmix64(ebase ^ ((y as u64) << 32) ^ (x as u64));
            if (draw >> 11) as f64 / (1u64 << 53) as f64 >= ESK_P {
                continue;
            }
            // walk the chain downstream while it stays a small channel
            let mut chain: Vec<(usize, usize)> = Vec::new();
            let (mut cy, mut cx) = (y, x);
            while chain.len() < ESK_LEN {
                if water[[cy, cx]]
                    || ice.thickness[[cy, cx]] <= 0.0
                    || acc[[cy, cx]] >= ACC_MIN
                    || egrid[[cy, cx]] == 1
                {
                    break;
                }
                chain.push((cy, cx));
                let d = dirs[[cy, cx]];
                if d < 0 {
                    break;
                }
                let (dy, dx) = crate::hydrology::N8[d as usize];
                let ny = cy as isize + dy;
                let nx = cx as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    break;
                }
                cy = ny as usize;
                cx = nx as usize;
            }
            if chain.len() < ESK_MIN {
                continue;
            }
            for &(qy, qx) in &chain {
                egrid[[qy, qx]] = 1;
                height[[qy, qx]] += ESKER_H;
                ice.eskers.push((qy as u16, qx as u16));
            }
        }
    }

    ice.till = till;
}

/// M30 — the loess mantle. The silt the ice ground fine blows off the
/// outwash aprons (the ice-free land ringing the former margin) and
/// settles equatorward as a decaying plume: each column is walked from
/// the pole toward the equator, the plume recharging to full strength
/// at every apron cell, thinning by `LOESS_DECAY_LAND` per land cell
/// and `LOESS_DECAY_WATER` per water cell, depositing on ice-free land
/// while it stays above `LOESS_MIN`. Pure multiplications in row-major
/// order — no libm, no RNG — so the ledger replays byte-identically on
/// every runtime (ADR-0025). Soil, not relief: the surface never moves;
/// `agriculture::fertility` pays `LOESS_FERT` on the mantle where the
/// climate can use it.
pub fn loess_mantle(height: &Array2<f64>, ice: &mut Ice) {
    let (rows, cols) = height.dim();
    const N4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    // The apron: ice-free land cells touching the former margin.
    let mut src = Array2::<u8>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if ice.thickness[[y, x]] <= 0.0 || height[[y, x]] < 0.0 {
                continue;
            }
            for (dy, dx) in N4 {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                if height[[ny, nx]] >= 0.0 && ice.thickness[[ny, nx]] <= 0.0 {
                    src[[ny, nx]] = 1;
                }
            }
        }
    }

    // One plume step: recharge on the apron, decay otherwise, deposit
    // on ice-free land while the dust is still soil.
    let mut loess = Array2::<f64>::zeros((rows, cols));
    let step = |s: &mut f64, y: usize, x: usize, loess: &mut Array2<f64>| {
        if height[[y, x]] < 0.0 {
            *s *= LOESS_DECAY_WATER;
            return;
        }
        if src[[y, x]] == 1 {
            *s = 1.0;
        } else {
            *s *= LOESS_DECAY_LAND;
        }
        if ice.thickness[[y, x]] <= 0.0 && *s >= LOESS_MIN && *s > loess[[y, x]] {
            loess[[y, x]] = *s;
        }
    };

    // Southern hemisphere: rows walk north (ascending y) to the equator;
    // northern: rows walk south (descending y). Rows map to latitude as
    // everywhere else: lat(y) = −90 + y·180/(rows−1).
    let n = rows as f64;
    for x in 0..cols {
        let mut s = 0.0f64;
        for y in 0..rows {
            if -90.0 + (y as f64) * 180.0 / (n - 1.0) > 0.0 {
                break;
            }
            step(&mut s, y, x, &mut loess);
        }
        let mut s = 0.0f64;
        for y in (0..rows).rev() {
            if -90.0 + (y as f64) * 180.0 / (n - 1.0) < 0.0 {
                break;
            }
            step(&mut s, y, x, &mut loess);
        }
    }

    ice.loess = loess.mapv(|v| v as f32);
}

/// M31 — proglacial lakes and spillways. When the sheets melted, the
/// water ponded behind the moraines the ice itself had raised; where a
/// basin overtopped its rim, the outburst cut a channel below the old
/// sill — an oversized valley that outlives the lake that carved it.
///
/// Mechanics, all deterministic (row-major scans, fixed BFS order,
/// IEEE-exact arithmetic, no RNG):
///
/// 1. Priority-flood the post-deposit land; a depression component
///    (4-connected, fill ≥ `PROG_DEP_MIN` above the bed) inside the
///    LGM footprint that spans ≥ `PROG_MIN_AREA` cells is a proglacial
///    basin when a moraine stands within `PROG_MORAINE_R` of its
///    water OR its shore — the dam sits on the sill, not in the lake.
/// 2. Its pour point is the lowest rim cell on the flood surface. From
///    there the spillway walks the D8 descent of that surface, cutting
///    the floor to a strict staircase that starts `SPILL_CUT` below
///    the lake level — deeper and wider (`prog_meta` width class) the
///    more water the basin impounded.
/// 3. A channel that runs into another basin strings the two lakes
///    into a chain (`chains` counts groups of ≥2) — the great
///    staircase lakes of every deglaciation.
///
/// Relief only: the carve lowers ground, never raises it. Whatever
/// depression survives the notch refills later as an ordinary lake in
/// `hydrology`; beds left above the cut drain to lake plains for free.
pub fn proglacial(height: &mut Array2<f64>, ice: &mut Ice) {
    let (rows, cols) = height.dim();
    let water = height.mapv(|v| v < 0.0);
    let filled = crate::hydrology::fill_depressions(height, &water);
    let dirs = crate::hydrology::flow_directions(&filled, &water);

    // Moraine dams, dilated to PROG_MORAINE_R.
    let mut dam: Array2<u8> = Array2::zeros((rows, cols));
    for &(my, mx) in &ice.moraines {
        for dy in -PROG_MORAINE_R..=PROG_MORAINE_R {
            for dx in -PROG_MORAINE_R..=PROG_MORAINE_R {
                let ny = my as isize + dy;
                let nx = mx as isize + dx;
                if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize {
                    dam[[ny as usize, nx as usize]] = 1;
                }
            }
        }
    }

    // Label depression components inside the footprint (4-neigh BFS,
    // row-major discovery order).
    let is_dep = |y: usize, x: usize, water: &Array2<bool>| {
        !water[[y, x]]
            && ice.thickness[[y, x]] > 0.0
            && filled[[y, x]] - height[[y, x]] >= PROG_DEP_MIN
    };
    let mut label: Array2<i32> = Array2::from_elem((rows, cols), -1);
    // per component: (cells, level, volume, deepest, dammed)
    struct Basin {
        cells: Vec<(usize, usize)>,
        level: f64,
        vol: f64,
        deepest: (usize, usize),
        dammed: bool,
    }
    let mut basins: Vec<Basin> = Vec::new();
    const N4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for y in 0..rows {
        for x in 0..cols {
            if label[[y, x]] != -1 || !is_dep(y, x, &water) {
                continue;
            }
            let id = basins.len() as i32;
            let mut b = Basin {
                cells: Vec::new(),
                level: f64::NEG_INFINITY,
                vol: 0.0,
                deepest: (y, x),
                dammed: false,
            };
            let mut deep = f64::NEG_INFINITY;
            let mut queue: std::collections::VecDeque<(usize, usize)> =
                std::collections::VecDeque::new();
            label[[y, x]] = id;
            queue.push_back((y, x));
            while let Some((cy, cx)) = queue.pop_front() {
                let d = filled[[cy, cx]] - height[[cy, cx]];
                b.vol += d;
                if filled[[cy, cx]] > b.level {
                    b.level = filled[[cy, cx]];
                }
                if d > deep {
                    deep = d;
                    b.deepest = (cy, cx);
                }
                b.dammed |= dam[[cy, cx]] == 1;
                b.cells.push((cy, cx));
                for (dy, dx) in N4 {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if label[[ny, nx]] == -1 && is_dep(ny, nx, &water) {
                        label[[ny, nx]] = id;
                        queue.push_back((ny, nx));
                    }
                }
            }
            basins.push(b);
        }
    }

    // Qualify, in discovery order; map component id → lake index.
    // A basin missing a dam hit on its flooded cells gets a second
    // look at its shore rim — the moraine that impounds a lake stands
    // beside the water, not under it.
    let mut lake_of: Vec<i32> = vec![-1; basins.len()];
    let mut lakes: Vec<usize> = Vec::new();
    let (mut n_area, mut n_dam) = (0usize, 0usize);
    for (bi, b) in basins.iter_mut().enumerate() {
        if !b.dammed && b.cells.len() >= PROG_MIN_AREA {
            'rim: for &(cy, cx) in &b.cells {
                for (dy, dx) in crate::hydrology::N8 {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if label[[ny, nx]] != bi as i32 && dam[[ny, nx]] == 1 {
                        b.dammed = true;
                        break 'rim;
                    }
                }
            }
        }
        if b.cells.len() >= PROG_MIN_AREA {
            n_area += 1;
        }
        if b.dammed {
            n_dam += 1;
        }
        if b.dammed && b.cells.len() >= PROG_MIN_AREA {
            lake_of[bi] = lakes.len() as i32;
            lakes.push(bi);
        }
    }
    if std::env::var("CALLIOPE_PROG_DEBUG").is_ok() {
        eprintln!(
            "prog-debug: {} components · {} >= area · {} dammed · {} both",
            basins.len(), n_area, n_dam, lakes.len()
        );
    }

    // Union-find over lakes for the chain count.
    let mut parent: Vec<usize> = (0..lakes.len()).collect();
    fn find(p: &mut Vec<usize>, mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }

    for (li, &bi) in lakes.iter().enumerate() {
        let b = &basins[bi];

        // Pour point: the rim cell (8-neigh of the basin, outside it)
        // lowest on the flood surface; ties resolve by scan order.
        let mut pour: Option<(usize, usize)> = None;
        let mut pour_f = f64::INFINITY;
        for &(cy, cx) in &b.cells {
            for (dy, dx) in crate::hydrology::N8 {
                let ny = cy as isize + dy;
                let nx = cx as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                if label[[ny, nx]] == bi as i32 {
                    continue;
                }
                if filled[[ny, nx]] < pour_f {
                    pour_f = filled[[ny, nx]];
                    pour = Some((ny, nx));
                }
            }
        }
        let Some((mut cy, mut cx)) = pour else { continue };

        let cut = SPILL_CUT_BASE + SPILL_CUT_K * (b.vol / SPILL_VOL_REF).min(1.0);
        let width: u16 = 1 + (b.vol >= SPILL_VOL_W2) as u16 + (b.vol >= SPILL_VOL_W3) as u16;
        let mut cur = b.level - cut;
        let mut chain: Vec<(u16, u16)> = Vec::new();
        let mut below = 0usize;
        for _ in 0..SPILL_MAX {
            if water[[cy, cx]] {
                break; // the channel reached the sea
            }
            let lb = label[[cy, cx]];
            if lb >= 0 && lb != bi as i32 && lake_of[lb as usize] >= 0 {
                // ran into the next lake of the chain
                let a = find(&mut parent, li);
                let c = find(&mut parent, lake_of[lb as usize] as usize);
                if a != c {
                    parent[a] = c;
                }
                break;
            }
            if height[[cy, cx]] < cur {
                below += 1;
                if below >= SPILL_MERGE {
                    break; // merged into naturally lower ground
                }
            } else {
                below = 0;
                height[[cy, cx]] = cur;
            }
            chain.push((cy as u16, cx as u16));
            if width >= 2 {
                let lift = if width >= 3 { SPILL_LAT_LIFT } else { 2.0 * SPILL_LAT_LIFT };
                for (dy, dx) in N4 {
                    let ny = cy as isize + dy;
                    let nx = cx as isize + dx;
                    if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if !water[[ny, nx]] && height[[ny, nx]] > cur + lift {
                        height[[ny, nx]] = cur + lift;
                    }
                }
            }
            cur = height[[cy, cx]].min(cur) - SPILL_STEP;
            let d = dirs[[cy, cx]];
            if d < 0 {
                break;
            }
            let (dy, dx) = crate::hydrology::N8[d as usize];
            let ny = cy as isize + dy;
            let nx = cx as isize + dx;
            if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                break;
            }
            cy = ny as usize;
            cx = nx as usize;
        }

        ice.proglacial.push((b.deepest.0 as u16, b.deepest.1 as u16));
        ice.spillways.push(chain);
        ice.prog_meta.push((
            (b.vol * 1000.0).min(u32::MAX as f64) as u32,
            b.cells.len().min(u16::MAX as usize) as u16,
            width,
        ));
    }

    // Chains: union groups holding at least two lakes.
    let mut group_size = vec![0u32; lakes.len()];
    for li in 0..lakes.len() {
        let r = find(&mut parent, li);
        group_size[r] += 1;
    }
    ice.chains = group_size.iter().filter(|&&s| s >= 2).count() as u32;
}

/// M32 — outwash plains: route the melt off the sheets down the
/// deglacial relief; where enough ice drains onto low, flat land below
/// the margin the channels braid and the load planes the valley floor
/// into an apron. Runs after `proglacial` (the lakes and spillways are
/// part of the relief the melt reads) and before `loess_mantle` (the
/// silt blows off these very plains).
pub fn outwash(height: &mut Array2<f64>, ice: &mut Ice) {
    let (rows, cols) = height.dim();
    let water = height.mapv(|v| v < 0.0);
    let filled = crate::hydrology::fill_depressions(height, &water);
    let dirs = crate::hydrology::flow_directions(&filled, &water);
    let order = crate::hydrology::drainage_order(&filled);

    // Meltwater accumulation: every glaciated cell sheds its ice column
    // (in km) downstream; acc is the ice drained through a cell.
    let mut acc = Array2::<f64>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            acc[[y, x]] = ice.thickness[[y, x]] as f64 / 1000.0;
        }
    }
    for &idx in &order {
        let (y, x) = (idx / cols, idx % cols);
        let d = dirs[[y, x]];
        if d >= 0 {
            let (dy, dx) = crate::hydrology::N8[d as usize];
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny >= 0 && nx >= 0 && ny < rows as isize && nx < cols as isize {
                let v = acc[[y, x]];
                acc[[ny as usize, nx as usize]] += v;
            }
        }
    }

    // Corridors: heavy melt over low, flat, unglaciated land.
    let h0 = height.clone();
    let mut out = Array2::<f32>::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            let h = h0[[y, x]];
            if h < 0.0 || h >= OUT_H_MAX || ice.thickness[[y, x]] > 0.0 {
                continue;
            }
            if acc[[y, x]] < OUT_ACC_MIN {
                continue;
            }
            let mut relief = 0.0f64;
            for &(dy, dx) in crate::hydrology::N8.iter() {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                relief = relief.max((h - h0[[ny as usize, nx as usize]]).abs());
            }
            if relief <= OUT_SLOPE_MAX {
                out[[y, x]] = 1.0;
            }
        }
    }

    // Aprons: neighbours standing within OUT_APRON above a corridor are
    // planed down toward its floor. Reads come from the pre-pass
    // snapshot and writes are min()/max() — scan order cannot show.
    // M31 — outburst channels are off-limits: the flood is the last
    // hand on its own bed. The lakes drained *through* the retreating
    // margin's plains, recutting them; planing a channel cell toward a
    // sideways corridor floor would pit the staircase the carve built.
    let mut channel = Array2::<bool>::from_elem((rows, cols), false);
    for ch in &ice.spillways {
        for &(y, x) in ch {
            let (y, x) = (y as usize, x as usize);
            if y < rows && x < cols {
                channel[[y, x]] = true;
            }
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            if out[[y, x]] < 1.0 {
                continue;
            }
            let hc = h0[[y, x]];
            for &(dy, dx) in crate::hydrology::N8.iter() {
                let ny = y as isize + dy;
                let nx = x as isize + dx;
                if ny < 0 || nx < 0 || ny >= rows as isize || nx >= cols as isize {
                    continue;
                }
                let (ny, nx) = (ny as usize, nx as usize);
                let hn = h0[[ny, nx]];
                if hn <= hc || hn - hc > OUT_APRON || ice.thickness[[ny, nx]] > 0.0 {
                    continue;
                }
                if channel[[ny, nx]] {
                    continue;
                }
                height[[ny, nx]] = height[[ny, nx]].min(hc + OUT_LIFT);
                if out[[ny, nx]] < 0.5 {
                    out[[ny, nx]] = 0.5;
                }
            }
        }
    }
    ice.outwash = out;
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7). The M28 rows gate the footprint; the M29
/// rows gate the carve — U-valley and cirque density per glaciated
/// latitude belt, hanging valleys per world; the M30 rows gate the
/// legacy — till share, moraine/drumlin/esker density. Calibrated on
/// the report seeds; the alpine belt is 40–62° |lat|, sheet poleward.
pub const BANDS: &[Band] = &[
    Band { name: "ice share of land at LGM", sweet: (30.0, 48.0), hard: (20.0, 60.0), target: "sweet 30–48 · hard 20–60 (% of land under the LGM sheets; twin polar landmasses run this above Earth's ~25)" },
    Band { name: "lowland ice margin lat", sweet: (55.0, 65.0), hard: (50.0, 72.0), target: "sweet 55–65 · hard 50–72 (p5 |lat|° of glaciated lowland, h<0.10; the L0=62° law puts it near 60)" },
    Band { name: "ELA poleward monotone", sweet: (85.0, 100.0), hard: (70.0, 100.0), target: "sweet 85–100 · hard 70–100 (% of 3° bins where the lowest glaciated cell drops)" },
    Band { name: "peak ice thickness m", sweet: (1200.0, 4000.0), hard: (600.0, 4000.0), target: "sweet 1200–4000 · hard 600–4000 (dome height; parabolic profile, Antarctic cap)" },
    Band { name: "u-valley cells per 1000 iced, alpine", sweet: (20.0, 200.0), hard: (5.0, 400.0), target: "sweet 20–200 · hard 5–400 (carved cells per 1000 glaciated, 40–62°|lat|)" },
    Band { name: "u-valley cells per 1000 iced, sheet", sweet: (10.0, 200.0), hard: (2.0, 400.0), target: "sweet 10–200 · hard 2–400 (carved cells per 1000 glaciated, >62°|lat|)" },
    Band { name: "cirques per 1000 iced, alpine", sweet: (1.0, 40.0), hard: (0.2, 100.0), target: "sweet 1–40 · hard 0.2–100 (scooped bowls per 1000 glaciated alpine cells)" },
    Band { name: "hanging valleys per world", sweet: (10.0, 600.0), hard: (2.0, 2000.0), target: "sweet 10–600 · hard 2–2000 (tributaries left in the air at trunk junctions)" },
    Band { name: "till share of iced lowland %", sweet: (35.0, 90.0), hard: (10.0, 98.0), target: "sweet 35–90 · hard 10–98 (M30: till cells per 100 glaciated lowland cells, h<0.30; measured 59–80 on six seeds)" },
    Band { name: "moraine cells per 1000 iced", sweet: (10.0, 90.0), hard: (2.0, 200.0), target: "sweet 10–90 · hard 2–200 (M30: margin ridge cells per 1000 glaciated land cells; measured 39–54)" },
    Band { name: "drumlins per 1000 till", sweet: (1.5, 30.0), hard: (0.5, 100.0), target: "sweet 1.5–30 · hard 0.5–100 (M30: swarm seeds per 1000 till cells; measured 2.4–5.5)" },
    Band { name: "esker cells per world", sweet: (40.0, 800.0), hard: (10.0, 3000.0), target: "sweet 40–800 · hard 10–3000 (M30: chained subglacial ridge cells, chains ≥3; measured 159–213)" },
    Band { name: "loess share of land %", sweet: (4.0, 30.0), hard: (1.0, 45.0), target: "sweet 4–30 · hard 1–45 (M30: mantle cells per 100 land cells; Earth ~10; measured 24–28 on six seeds)" },
    Band { name: "proglacial lakes per world", sweet: (5.0, 120.0), hard: (1.0, 400.0), target: "sweet 5–120 · hard 1–400 (M31: moraine-dammed basins ≥4 cells inside the LGM footprint)" },
    Band { name: "spillway cells per world", sweet: (20.0, 1500.0), hard: (5.0, 5000.0), target: "sweet 20–1500 · hard 5–5000 (M31: outburst channel cells cut below the old sills)" },
    Band { name: "outwash cells per world", sweet: (150.0, 6000.0), hard: (30.0, 20000.0), target: "sweet 150–6000 · hard 30–20000 (M32: corridor + apron cells below the former margin; measured 853–1831 on four seeds)" },
    Band { name: "braided share of ice-fed rivers %", sweet: (2.0, 35.0), hard: (0.5, 60.0), target: "sweet 2–35 · hard 0.5–60 (M32: % of below-margin river cells running braided over outwash; measured 3.1–5.5)" },
    Band { name: "outwash fertility uplift", sweet: (0.01, 0.12), hard: (0.005, 0.20), target: "sweet +0.01–0.12 · hard +0.005–0.20 (M32: counterfactual — zeroing the outwash grid must cost the farmable plain; measured +0.027–0.033)" },
    Band { name: "modern glacier share of land %", sweet: (3.0, 18.0), hard: (0.5, 30.0), target: "sweet 3–18 · hard 0.5–30 (M34: cells with positive annual mass balance — polar caps plus alpine ice; Earth runs ~10 with its sheets; measured 12.4–15.4 on three seeds)" },
    Band { name: "glacier elev above snowline m", sweet: (0.0, 900.0), hard: (-200.0, 1600.0), target: "sweet 0–900 · hard −200–1600 (M34 gate: mean alpine-glacier elevation minus the belt snowline solved from belt-mean climate, cap belts excluded; measured +140–270)" },
    Band { name: "modern ice inside LGM footprint %", sweet: (70.0, 100.0), hard: (50.0, 100.0), target: "sweet 70–100 · hard 50–100 (M34: the LGM was colder everywhere — today's ice lives inside yesterday's; measured 100)" },
    Band { name: "fjord cells per 1000 polar coast", sweet: (3.0, 150.0), hard: (1.0, 400.0), target: "sweet 3–150 · hard 1–400 (M36: fjord cells per 1000 coast cells ≥55°|lat| — the Norway/Greenland analog coasts are riddled, not saturated; measured 4.9–56.4 ×3 seeds, mean 34.3)" },
    Band { name: "fjord cells poleward of 45° %", sweet: (90.0, 100.0), hard: (70.0, 100.0), target: "sweet 90–100 · hard 70–100 (M36: Earth's fjord coasts run 42–83°; the drowned carve stays poleward; measured 100 ×3 seeds)" },
    Band { name: "proglacial lakes per 1000 iced, margin belt", sweet: (0.03, 3.0), hard: (0.005, 10.0), target: "sweet 0.03–3 · hard 0.005–10 (M36: moraine-dammed giants per 1000 formerly iced land cells, 50–75°|lat| — the Laurentide fringe kept a handful of giants, not a lake district; measured 0–0.16 per seed, mean 0.10 — seed 777 keeps its giants poleward of the belt)" },
    Band { name: "moraine cells per 1000 iced, margin belt", sweet: (10.0, 120.0), hard: (2.0, 300.0), target: "sweet 10–120 · hard 2–300 (M36: margin ridge cells per 1000 formerly iced land cells in the 50–75° belt; measured 18.9–32.0, mean 24.6)" },
    Band { name: "lowland moraine cells near the margin %", sweet: (40.0, 100.0), hard: (25.0, 100.0), target: "sweet 40–100 · hard 25–100 (M36: share of lowland (h<0.30) moraine cells within ±6° of the measured lowland margin — alpine moraines follow their own snowlines; the lowland string hugs the line it records; measured 43.9–73.4, mean 55.7)" },
    Band { name: "fjord median latitude", sweet: (52.0, 72.0), hard: (45.0, 83.0), target: "sweet 52–72 · hard 45–83 (M39: median |lat|° of fjord cells; Earth's fjord coasts run 42–83° with the bulk 55–72 — Norway 58–71, Greenland 60–83, Chile 42–56; measured 67.6–68.5 ×3 seeds)" },
    Band { name: "fjord latitude IQR", sweet: (0.0, 20.0), hard: (0.0, 30.0), target: "sweet ≤20 · hard ≤30 (M39: interquartile |lat|° spread; Earth's fjords cluster on cold coasts in ~10–15° hemispheric bands, not smeared over the globe; measured 3.4–5.3)" },
    Band { name: "proglacial lakes per Mkm² iced", sweet: (0.5, 40.0), hard: (0.05, 120.0), target: "sweet 0.5–40 · hard 0.05–120 (M39: moraine-dammed giants per million km² formerly under ice; Earth's ~34 Mkm² of LGM sheets kept order 50–150 basins ≥64 km² ≈ 1.5–4.5 per Mkm²; measured 6.8–21.7 — a young world keeps its basins undrained)" },
    Band { name: "glacier curve descends poleward", sweet: (85.0, 100.0), hard: (60.0, 100.0), target: "sweet 85–100 · hard 60–100 (M39: % of adjacent 15°-belt pairs poleward of the crest where mean glacier elevation drops, 150 m slack; Earth's glaciation level falls ~5–6 km from the dry subtropics to the poles; measured 100 ×3 seeds)" },
    Band { name: "polar-belt glacier elevation", sweet: (0.0, 1000.0), hard: (0.0, 1800.0), target: "sweet ≤1000 · hard ≤1800 (M39: mean elevation of glaciered cells in the 75–90° belt; Earth's polar glaciation level sits at 0–800 m — the caps ride down to the sea; measured 169–276)" },
];

/// M34 — modern mountain glaciers: the ice that still lives on the
/// world's highest ground, responding to the climate the engine
/// already ships. A cell keeps a glacier when the annual surface mass
/// balance (`climate::ice_balance`: freezing-month snowfall minus
/// positive-degree-month melt) stays positive. Since M35 this runs at
/// the climate stage over the pre-widen f64 grids — hydrology reads
/// the result for glacier-fed discharge — and the grid then rides
/// `widen` with the rest of the ice ledger. The stored value is the
/// balance itself in m w.e./yr, so downstream systems can read
/// intensity, not just presence.
pub fn modern_glaciers(
    water: &Array2<bool>,
    tmean: &Array2<f64>,
    tamp: &Array2<f64>,
    precip: &Array2<f64>,
    pamp: &Array2<f64>,
) -> Array2<f32> {
    let (rows, cols) = water.dim();
    let mut out: Array2<f32> = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            if water[[y, x]] {
                continue;
            }
            let b = crate::climate::ice_balance(
                tmean[[y, x]],
                tamp[[y, x]],
                precip[[y, x]],
                pamp[[y, x]],
            );
            if b > 0.0 {
                out[[y, x]] = b as f32;
            }
        }
    }
    out
}
