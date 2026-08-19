//! M33 — permafrost and patterned ground: the cold rim carries its own
//! frozen-ground signature, distinct from ordinary tundra.
//!
//! **Extent.** Permafrost is where the ground's mean annual temperature
//! holds below freezing at depth — on Earth the sporadic limit tracks
//! the −2 °C mean-annual-air isotherm, with continental interiors
//! reaching warmer because thin snow and brutal winters chill the
//! ground harder than the annual mean suggests (Yakutia holds
//! continuous permafrost where maritime Norway at the same MAAT holds
//! none). We classify off the continentality-shifted MAAT
//! `t_adj = tmean − CONT_REACH · cont_norm`:
//!
//! - **continuous**    `t_adj ≤ −8 °C` — frozen ground wall to wall
//! - **discontinuous** `t_adj ≤ −4 °C` — frozen ground under most cells
//! - **sporadic**      `t_adj ≤ −2 °C` — patches in the coldest hollows
//!
//! Because the shift only *extends* the reach inland (`t_adj ≤ tmean`),
//! every cell inside the −2 °C isotherm qualifies and the extent's
//! frontier hugs that isotherm on the coasts while bowing equatorward
//! through the interiors — exactly the M33 gate.
//!
//! **Patterned ground.** Where real permafrost (discontinuous or
//! better) meets the surface, frost heave sorts the soil into
//! micro-texture: **ice-wedge polygons** on the flats (poorly drained
//! ground cracks into nets) and **solifluction stripes** on gentle
//! slopes (the active layer creeps downhill in stone-banked lanes).
//! Steeper ground sheds its mantle and stays bare. The classifier is a
//! pure threshold on the local height gradient — no RNG, replay-safe.
//!
//! Both grids are pure derived state — a function of the final height
//! and temperature fields — recomputed identically every generation,
//! folded into `hash_state` and the deep-earth identity line
//! (ADR-0025) so the classifier cannot drift between runtimes. The
//! landform vocabulary gains `PATTERNED`; the wire gains two CellFlags
//! bits (PERMAFROST, PATTERNED) at zero pack cost.

use ndarray::Array2;

use crate::climate;
use crate::state::CellFlags;
use crate::util::fnv1a64;

// ---------------------------------------------------------------- codes

pub const NONE: u8 = 0;
pub const SPORADIC: u8 = 1;
pub const DISCONTINUOUS: u8 = 2;
pub const CONTINUOUS: u8 = 3;

pub const PAT_NONE: u8 = 0;
/// Ice-wedge polygon nets on poorly drained flats.
pub const PAT_POLYGON: u8 = 1;
/// Solifluction stripes on gentle slopes.
pub const PAT_STRIPE: u8 = 2;

// ------------------------------------------------------------ constants

/// Class thresholds on the continentality-shifted MAAT, °C.
pub const T_SPORADIC: f64 = -2.0;
pub const T_DISCONTINUOUS: f64 = -4.0;
pub const T_CONTINUOUS: f64 = -8.0;

/// How many °C of extra reach the deepest continental interior gets:
/// the shift is `CONT_REACH · (cont − 0.35)/0.65`, zero on the coast,
/// full in the interior. 3 °C matches the Siberia-vs-Norway asymmetry
/// without letting the interior rim outrun the tundra entirely.
pub const CONT_REACH: f64 = 3.0;

/// Height-gradient ceilings for the micro-texture (height units per
/// cell; 1 unit ≈ 2000 m over 4 km). Flats crack into polygons, gentle
/// slopes creep into stripes, steeper ground stays bare.
pub const POLY_G: f32 = 0.004;
pub const STRIPE_G: f32 = 0.015;

/// The single extent law (M33, shared with the M38 biome pass): class
/// from the continentality-shifted MAAT. `cont` is the raw EDT
/// continentality (≈0 coast → 1 deep interior); the shift only ever
/// extends the reach inland, so `t_adj ≤ tmean` everywhere.
#[inline]
pub fn extent_class(tmean_c: f64, cont: f64) -> u8 {
    let cont_norm = ((cont - 0.35) / 0.65).clamp(0.0, 1.0);
    let t_adj = tmean_c - CONT_REACH * cont_norm;
    if t_adj <= T_CONTINUOUS {
        CONTINUOUS
    } else if t_adj <= T_DISCONTINUOUS {
        DISCONTINUOUS
    } else if t_adj <= T_SPORADIC {
        SPORADIC
    } else {
        NONE
    }
}

// --------------------------------------------------------------- struct

/// The frozen-ground ledger: extent class and surface pattern per cell.
/// Computed once at the dawn on the widened grids (like `landform`),
/// never ticked.
pub struct Permafrost {
    /// 0 none · 1 sporadic · 2 discontinuous · 3 continuous.
    pub extent: Array2<u8>,
    /// 0 none · 1 ice-wedge polygons · 2 solifluction stripes.
    pub pattern: Array2<u8>,
}

impl Permafrost {
    pub fn empty() -> Self {
        Permafrost {
            extent: Array2::zeros((0, 0)),
            pattern: Array2::zeros((0, 0)),
        }
    }

    /// FNV-1a over both grids — joins `hash_state` and the deep-earth
    /// identity line so the classifier holds still across runtimes.
    pub fn hash(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.extent.len() + self.pattern.len());
        b.extend_from_slice(self.extent.as_slice().expect("extent grid is contiguous"));
        b.extend_from_slice(self.pattern.as_slice().expect("pattern grid is contiguous"));
        fnv1a64(&b)
    }

    /// Classify the (widened) world. `flags` masks lakes out of the
    /// pattern pass — polygon nets crack soil, not open water.
    pub fn compute(height: &Array2<f32>, tmean: &Array2<f32>, flags: &Array2<u8>) -> Self {
        let (rows, cols) = height.dim();
        let water = height.mapv(|h| h < 0.0);
        let cont = climate::continentality(&water);

        let mut extent: Array2<u8> = Array2::zeros((rows, cols));
        for y in 0..rows {
            for x in 0..cols {
                if water[[y, x]] {
                    continue;
                }
                extent[[y, x]] = extent_class(tmean[[y, x]] as f64, cont[[y, x]]);
            }
        }

        // Micro-texture where real permafrost meets the surface:
        // polygons on the flats, stripes on the gentle slopes.
        let mut pattern: Array2<u8> = Array2::zeros((rows, cols));
        let lake = CellFlags::LAKE.bits();
        for y in 0..rows {
            for x in 0..cols {
                if extent[[y, x]] < DISCONTINUOUS || flags[[y, x]] & lake != 0 {
                    continue;
                }
                let h = height[[y, x]];
                let mut g = 0.0f32;
                if y > 0 {
                    g = g.max((height[[y - 1, x]] - h).abs());
                }
                if y + 1 < rows {
                    g = g.max((height[[y + 1, x]] - h).abs());
                }
                if x > 0 {
                    g = g.max((height[[y, x - 1]] - h).abs());
                }
                if x + 1 < cols {
                    g = g.max((height[[y, x + 1]] - h).abs());
                }
                pattern[[y, x]] = if g <= POLY_G {
                    PAT_POLYGON
                } else if g <= STRIPE_G {
                    PAT_STRIPE
                } else {
                    PAT_NONE
                };
            }
        }

        Permafrost { extent, pattern }
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7) — the M33 gate reads the frontier against
/// the −2 °C mean-annual isotherm. Because the continental reach only
/// *extends* the extent into warmer ground, the frontier is read in
/// two legs: the **maritime** frontier (continentality shift ≈ 0) must
/// hug −2 °C within tolerance, and the **continental** frontier must
/// sit warmer by the reach — the Siberia-vs-Norway asymmetry,
/// sign-correct. Ranges calibrated on the report seeds
/// (12345 · 777 · 90210).
pub const BANDS: &[Band] = &[
    Band { name: "permafrost share of land", sweet: (5.0, 30.0), hard: (2.0, 45.0), target: "sweet 5–30% · hard 2–45% (M33: the cold rim, not the whole tundra; measured 25.9–29.5)" },
    Band { name: "patterned share of permafrost", sweet: (10.0, 80.0), hard: (3.0, 95.0), target: "sweet 10–80% · hard 3–95% (M33: polygons on flats + stripes on slopes; measured 65.6–74.1)" },
    Band { name: "maritime frontier MAAT", sweet: (-2.75, -1.25), hard: (-4.0, -0.5), target: "sweet −2.75..−1.25 °C · hard −4..−0.5 (M33 gate: where the sea keeps the shift near zero the frontier tracks the −2° isotherm ±0.75)" },
    Band { name: "continental frontier offset", sweet: (0.25, 3.0), hard: (0.0, 4.0), target: "sweet +0.25..+3 °C · hard 0..+4 (M33: the interior frontier runs warmer than the maritime one by the continental reach)" },
    Band { name: "isotherm agreement", sweet: (70.0, 100.0), hard: (55.0, 100.0), target: "sweet 70–100% · hard 55–100 (M33 gate: Jaccard of extent vs tmean ≤ −2 °C on land; measured 87.7–90.1)" },
];
