//! Biome classification — the Whittaker table (geo.hy) with an honest
//! cold edge (M38).
//!
//! The warm rows classify as ever: annual mean temperature × annual
//! precipitation through a 6×6 Whittaker expansion. The **cold edge**
//! no longer trusts the annual mean: treeline on Earth is a
//! growing-season law — trees stand wherever the summer banks roughly
//! `GDD_TREELINE` growing-degree-days above 5 °C, however deep the
//! winter cuts (Yakutian larch taiga grows on continuous permafrost at
//! MAAT −9 °C; maritime tundra at −1 °C stays treeless because its
//! cool summers never bank the degree-days). So:
//!
//! - a Whittaker-forest cell whose GDD5 falls short of the treeline is
//!   demoted to tundra — cool maritime summers make honest tundra;
//! - a Whittaker-tundra cell whose continental summer clears the
//!   treeline is promoted one row into the cool-temperate lane — the
//!   Siberian paradox: forest poleward of where the annual mean says
//!   trees should quit.
//!
//! **Tundra splits wet and dry** on the permafrost table depth (M33's
//! extent classes, evaluated by the same `permafrost::extent_class`
//! law): where real permafrost (discontinuous or better) perches the
//! summer thaw over frozen ground and the land lies flat, the melt has
//! nowhere to drain — sedge-and-moss mire, the wet tundra of polygon
//! country. Steeper, drier or merely seasonal ground drains and reads
//! dry: fell-field, prostrate shrub, lichen heath.

use ndarray::Array2;

use crate::constants as gc;
use crate::{climate, permafrost};

// rows: coldest -> hottest, 6x6 expansion of geo.hy's table
const BIOME_TABLE: [[u8; 6]; 6] = [
    [gc::ICE; 6],
    [gc::TUNDRA; 6],
    [
        gc::GRASSLAND,
        gc::GRASSLAND,
        gc::WOODLAND,
        gc::BOREAL_FOREST,
        gc::BOREAL_FOREST,
        gc::BOREAL_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::WOODLAND,
        gc::WOODLAND,
        gc::SEASONAL_RAIN_FOREST,
        gc::TEMPERATE_RAIN_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::SAVANNA,
        gc::SAVANNA,
        gc::TROPICAL_RAIN_FOREST,
        gc::TROPICAL_RAIN_FOREST,
    ],
    [
        gc::DESERT,
        gc::DESERT,
        gc::SAVANNA,
        gc::SAVANNA,
        gc::TROPICAL_RAIN_FOREST,
        gc::TROPICAL_RAIN_FOREST,
    ],
];

const TEMP_EDGES: [f64; 5] = [-10.0, -2.0, 5.0, 13.0, 20.0];
const PRECIP_EDGES: [f64; 5] = [180.0, 420.0, 800.0, 1400.0, 2200.0];

/// M38 — degree-days above 5 °C a summer must bank before trees pay.
/// Paulsen & Körner (2014) put the global treeline near a 6.4 °C
/// season mean over ~94 days ≈ 640 °C·day, with GDD5 proxies quoted
/// 500–800 across the literature. We sit at the low edge: the E5
/// seasonal swing is capped near 22° (no Verkhoyansk winters, so no
/// Verkhoyansk summers either) and a 600 cut against that tame cosine
/// gutted the boreal belt to 0.2% of land — 500 keeps the maritime
/// demotion honest and leaves the taiga standing.
pub const GDD_TREELINE: f64 = 500.0;

/// M38 — wet tundra needs ground flat enough to pond the thaw: max
/// 4-neighbour height step (height units per cell; cf. the ice-wedge
/// polygon ceiling `permafrost::POLY_G` 0.004 — mire tolerates
/// noticeably more tilt than polygon nets: solifluction sheets and
/// string fens still pond between the risers).
pub const WET_G: f64 = 0.010;

/// M38 — and enough sky-water to pond: polar deserts read dry however
/// flat they lie (mm/yr).
pub const WET_PMIN: f64 = 220.0;

#[inline]
fn digitize(x: f64, edges: &[f64; 5]) -> usize {
    edges.iter().filter(|&&e| x >= e).count()
}

#[inline]
fn is_tree(b: u8) -> bool {
    b == gc::WOODLAND
        || b == gc::BOREAL_FOREST
        || b == gc::SEASONAL_RAIN_FOREST
        || b == gc::TEMPERATE_RAIN_FOREST
        || b == gc::TROPICAL_RAIN_FOREST
}

/// Classify the (pre-widen) world. `pf_extent` is the permafrost
/// extent class per cell — the same `permafrost::extent_class` law the
/// canonical hashed M33 ledger applies post-widen, evaluated here on
/// the pre-widen climate so the tundra can split on the table depth.
pub fn classify(
    height: &Array2<f64>,
    tmean: &Array2<f64>,
    tamp: &Array2<f64>,
    precip: &Array2<f64>,
    lakes: &Array2<bool>,
    pf_extent: &Array2<u8>,
) -> Array2<u8> {
    let (rows, cols) = height.dim();
    Array2::from_shape_fn((rows, cols), |(y, x)| {
        if height[[y, x]] < 0.0 || lakes[[y, x]] {
            return gc::WATER;
        }
        let trow = digitize(tmean[[y, x]], &TEMP_EDGES);
        let pcol = digitize(precip[[y, x]], &PRECIP_EDGES);
        let mut b = BIOME_TABLE[trow][pcol];

        // M38 — the treeline is a growing-season law (GDD5), not an
        // annual-mean cutoff: demote short-summer forest, promote
        // long-summer "tundra" into the cool-temperate row (dry columns
        // come up steppe, wet columns come up boreal). The demotion
        // lands where the annual mean does: cold rows make honest
        // tundra; milder maritime rows (MAAT ≥ 5 °C with summers too
        // cool for trees) make moorland — Faroe heath, not Yamal.
        let gdd = climate::gdd5(tmean[[y, x]], tamp[[y, x]]);
        if trow >= 2 && is_tree(b) && gdd < GDD_TREELINE {
            b = if trow >= 3 { gc::GRASSLAND } else { gc::TUNDRA };
        } else if trow == 1 && gdd >= GDD_TREELINE {
            b = BIOME_TABLE[2][pcol];
        }

        // M38 — wet/dry split on the permafrost table depth: a shallow
        // frozen table under flat, watered ground ponds the thaw.
        if b == gc::TUNDRA
            && pf_extent[[y, x]] >= permafrost::DISCONTINUOUS
            && precip[[y, x]] >= WET_PMIN
        {
            let h = height[[y, x]];
            let mut g = 0.0f64;
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
            if g <= WET_G {
                b = gc::WET_TUNDRA;
            }
        }
        b
    })
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): how the land is dressed. The M38 rows are
/// calibrated on the report seeds (12345 · 777 · 90210); the tracking
/// band reads the thermal treeline against the continuous-permafrost
/// lowland frontier column by column in `diagnose terrain`. In this
/// world the law is one-signed: with the E5 seasonal amplitude capped
/// near 22° and the M33-gated continental reach of 3 °C, the
/// continuous frontier always sits poleward of the GDD-600 treeline —
/// Earth's maritime rims (Norway: treeline ~70°N, continuous
/// permafrost only on Svalbard). The Siberian paradox — forest
/// standing ON the frozen ground — needs a stronger continental
/// reach, staged as a Ready item rather than smuggled past the M33
/// frontier gates here.
pub const BANDS: &[Band] = &[
    Band { name: "desert share of land", sweet: (0.12, 0.28), hard: (0.06, 0.38), target: "sweet 12–28% · hard 6–38%" },
    Band { name: "tundra+ice share of land", sweet: (0.05, 0.30), hard: (0.01, 0.45), target: "sweet 5–30% · hard 1–45%" },
    Band { name: "forest share of land", sweet: (0.25, 0.60), hard: (0.15, 0.75), target: "sweet 25–60% · hard 15–75%" },
    Band { name: "grass+savanna share of land", sweet: (0.10, 0.45), hard: (0.04, 0.60), target: "sweet 10–45% · hard 4–60%" },
    Band { name: "pastoral share of land", sweet: (0.02, 0.45), hard: (0.005, 0.65), target: "M2.1: the dry steppe carries herds" },
    Band { name: "treeline−permafrost offset", sweet: (-22.0, -8.0), hard: (-28.0, -2.0), target: "sweet −22..−8° · hard −28..−2 (M38 gate: median treeline latitude minus continuous-lowland-frontier latitude over cold-limited columns — negative, bounded, tracking; measured −16.9..−19.0)" },
    Band { name: "treeline tracking spread", sweet: (0.0, 12.0), hard: (0.0, 18.0), target: "sweet ≤12° · hard ≤18 (M38 gate: IQR of the per-column offset — tracks, not wanders; measured 8.5–10.6)" },
    Band { name: "treeline GDD discipline", sweet: (350.0, 850.0), hard: (200.0, 1100.0), target: "sweet 350–850 · hard 200–1100 °C·day (M38: median GDD5 on the treeline cells straddles the 600 threshold)" },
    Band { name: "wet share of the tundra", sweet: (8.0, 60.0), hard: (2.0, 85.0), target: "sweet 8–60% · hard 2–85% (M38: polygon-mire lowlands are a real minority of the tundra; measured 10.4–17.7)" },
];
