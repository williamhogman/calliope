//! Drought with a memory (M80) — "The Failed Year Named".
//!
//! Before this milestone a failed year was a single roll of the sky: the
//! standardized rain anomaly (SPI, McKee 1993) of *that* year decided who
//! starved, and the next year started from nothing. Real droughts do not
//! work that way. Soil moisture, aquifers and granaries carry a shortfall
//! forward, so a severe year leaves the ground primed and an ordinary year
//! that follows it can still fail. Meteorology handles this by
//! *accumulating* the standardized anomaly over a window (SPI-24, SPI-48
//! are the standard multi-year indices).
//!
//! So the drought field the harvest verdict reads is no longer one year's
//! z but an exponentially weighted accumulation of the last
//! [`MEMO_YEARS`] years,
//!
//! ```text
//!   D(year, cell) = sqrt(1 − MEM²) · Σ_{k=0..K}  MEMᵏ · z(year−k, cell)
//! ```
//!
//! with `MEM` = 0.5, so a year's weight halves annually and the window's
//! tail is 2.4·10⁻⁴. The `sqrt(1 − MEM²)` factor renormalizes the sum back
//! to unit variance for independent years, which is what keeps the
//! threshold meaning what it always meant: `D ≤ −1` is moderate drought,
//! `D ≤ −2` extreme, exactly as `famine::DROUGHT_Z` declares.
//!
//! The index is a **pure function of seed × cell × year** — no stored
//! per-cell state, nothing to desynchronize, re-derivable from the sky at
//! any point by anyone (the harness does exactly that, independently).
//!
//! On top of the field sits the thing history remembers: a drought
//! *event*. Once a year the index is read over a coarse lattice of the
//! land ([`STRIDE`] cells apart), the cells at or below the threshold are
//! grouped into connected regions, and each region is matched to last
//! year's regions by overlap. A region with no ancestor is a new drought:
//! it is named once, from the ground it withers, and announced once in the
//! chronicle. A region that inherits keeps the same name and the same id
//! for as long as the ground stays dry — that span, not the single bad
//! harvest, is what the gate measures.

use std::collections::HashSet;


/// How much of last year's shortfall the ground still carries. 0.5: the
/// weight of a year halves annually — a two-year memory in the sense that
/// matters, an eleven-year tail in the sense that is measurable.
pub const MEM: f64 = 0.5;

/// The accumulation window. `MEM^12` = 2.4·10⁻⁴ σ: past it the sum cannot
/// move a threshold test, so truncating here keeps the index a *finite,
/// exactly reproducible* expression rather than an unbounded recursion.
pub const MEMO_YEARS: usize = 12;

/// Renormalization back to unit variance under the *independence*
/// assumption, `sqrt(1 − MEM²)`: the weighted sum of independent
/// unit-variance years has variance `1/(1 − MEM²)`.
///
/// M80 follow-up — this constant is the textbook baseline, and it is not
/// what this sky needs. The years are not independent: the M74/M76
/// seesaw gives the interannual anomaly a real lag structure (measured
/// ρ(1) = −0.19 on seed 12345, −0.30 on 90210, +0.04 on 777), so the
/// weighted sum's variance is a property of the *world*, not of `MEM`.
/// Applied blind it narrowed the index against the single year it
/// replaced — sd(D)/sd(z) measured 0.938 / 0.983 / 0.880 across the three
/// suite seeds, i.e. the same numeric threshold silently meant a *rarer*
/// event than SPI −1, exactly the failure this factor exists to prevent
/// (P(z ≤ −1) 0.2055 → P(D ≤ −1) 0.1923 on seed 12345). M80's contract is
/// that memory changes drought *persistence*, not its severity
/// distribution, so the shipped normalization is calibrated per world
/// against its own sky ([`Droughts::norm`], set once at generation from a
/// fixed deterministic sample — pure in seed × size). This constant
/// remains the fallback for an uncalibrated ledger and the documented
/// independence baseline.
pub const NORM: f64 = 0.866_025_403_784_438_6;

/// The calibration sample for [`Droughts::norm`]: how many years of the
/// sky's own prehistory the world reads to measure what its weighted sum
/// actually does to the variance. Fixed, seed-independent and read from
/// negative years, so the scalar is a pure function of seed × size and
/// never moves as the world ages.
pub const CAL_YEARS: i64 = 96;

/// Lattice spacing of the calibration sample, in grid cells — coarser
/// than [`STRIDE`] because a variance ratio needs breadth, not detail.
pub const CAL_STRIDE: usize = 16;


/// Spacing of the lattice the extent is mapped on, in grid cells. A cell
/// is 4 km (ADR-0004), so one lattice node stands for 32 × 32 km — finer
/// than a drought's grain, coarse enough that three centuries of yearly
/// mapping costs a few million noise draws, not a few hundred million.
pub const STRIDE: usize = 8;

/// The holding contour. A drought ENTERS at [`DROUGHT_Z`] (SPI −1, the
/// same line the harvest verdict reads) but HOLDS while the ground is
/// merely parched. Without the second contour a region flickering across
/// one line dies and is re-named every other year — the ledger blinks.
pub const DRY_HOLD: f64 = -0.45;

/// The smallest failing core that earns a new name, in lattice nodes:
/// 24 nodes ≈ 24 500 km², a country's worth of failed harvest, not a
/// bad week over three valleys.
pub const MIN_CORE: usize = 32;

/// The smallest region worth mapping at all, in lattice nodes, once a
/// drought is alive and only holding.
pub const MIN_NODES: usize = 12;


/// Area one lattice node stands for, in km².
pub const NODE_KM2: f64 = (STRIDE * STRIDE * 16) as f64;

/// Name forms. `{P}` is the ground the drought sits on — the nearest
/// named feature or town, so the name is the map's, not a bank's.
pub(crate) const FORMS: [&str; 6] = [
    "the Withering of {P}",
    "the Long Thirst of {P}",
    "the Dust Years of {P}",
    "the Failing of {P}",
    "the Dry Years of {P}",
    "the Great Drought of {P}",
];

/// One drought, from the year it took hold to the last year it held.
#[derive(Clone, Default)]
pub struct DroughtEvent {
    pub id: usize,
    pub name: String,
    /// The ground it was named for.
    pub place: String,
    pub start_year: i64,
    pub last_year: i64,
    /// Deepest index reached anywhere in it (most negative).
    pub peak: f64,
    /// Widest extent it ever reached, in lattice nodes.
    pub peak_nodes: usize,
    /// Extent the year it was named.
    pub onset_nodes: usize,
    /// Fine-grid anchor: the centroid at onset.
    pub x: i64,
    pub y: i64,
    /// The deepest node at onset, in fine-grid cells — a point the sky's
    /// own arithmetic must call dry, so the harness can re-derive the
    /// event from the seed without trusting this ledger.
    pub ax: i64,
    pub ay: i64,
    /// Per-year ledger: `(year, nodes, cx, cy, jaccard-with-last-year,
    /// anchor-x, anchor-y)`. The first row carries jaccard 1.0 by
    /// definition. The anchor is *that year's* deepest node in fine-grid
    /// cells: a drought walks, so a single onset anchor stops being dry
    /// ground the moment the region moves off it, and the re-derivation
    /// must probe the year it is judging.
    pub years: Vec<(i64, usize, f64, f64, f64, i64, i64)>,

    /// Whether the chronicle has spoken its name (it does so once).
    pub announced: bool,
    /// Last year's node set — the matcher's working memory, never hashed.
    pub prev: Vec<usize>,
}

impl DroughtEvent {
    /// Years the ground stayed dry, inclusive.
    pub fn duration(&self) -> i64 {
        self.last_year - self.start_year + 1
    }
    /// Median year-on-year footprint overlap (Jaccard) after the onset
    /// year — how much the same ground it was, year after year.
    pub fn stability(&self) -> Option<f64> {
        let mut js: Vec<f64> = self.years.iter().skip(1).map(|r| r.4).collect();
        if js.is_empty() {
            return None;
        }
        js.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some(js[js.len() / 2])
    }
}

/// The drought ledger: the lattice, the window of standardized years it
/// accumulates, and every drought the world has lived through.
#[derive(Default)]
pub struct Droughts {
    /// Lattice dimensions.
    pub rows: usize,
    pub cols: usize,
    /// Lattice node is land (the index is meaningless over open sea).
    pub land: Vec<bool>,
    /// The last `MEMO_YEARS` standardized rain years over the lattice,
    /// newest first. Derived state: a pure read of the sky, rebuilt from
    /// the seed alone, never hashed and never packed.
    pub(crate) hist: Vec<Vec<f32>>,
    /// The accumulated index over the lattice, this year.
    pub index: Vec<f32>,
    /// Which event owns each node this year; `-1` = dry-free ground.
    pub owner: Vec<i32>,
    /// Every drought, in the order they took hold.
    pub events: Vec<DroughtEvent>,
    /// The last year mapped (`i64::MIN` before the first pass).
    pub year: i64,
    /// M80 follow-up — this world's own renormalization of the weighted
    /// sum, measured once at generation against the sky it will actually
    /// live in (see [`NORM`]). `0.0` means uncalibrated: the index then
    /// falls back to the independence baseline.
    pub norm: f64,

    pub(crate) taken: HashSet<String>,
}

impl Droughts {
    pub fn new(height: &ndarray::Array2<f32>) -> Droughts {
        let (h, w) = height.dim();
        let rows = h.div_ceil(STRIDE);
        let cols = w.div_ceil(STRIDE);
        let mut land = vec![false; rows * cols];
        for cy in 0..rows {
            for cx in 0..cols {
                land[cy * cols + cx] = height[[cy * STRIDE, cx * STRIDE]] >= 0.0;
            }
        }
        Droughts {
            rows,
            cols,
            land,
            hist: Vec::new(),
            index: vec![0.0; rows * cols],
            owner: vec![-1; rows * cols],
            events: Vec::new(),
            year: i64::MIN,
            taken: HashSet::new(),
        }
    }

    /// Droughts alive in `year` (the ones the map is showing).
    pub fn active(&self, year: i64) -> impl Iterator<Item = &DroughtEvent> {
        self.events.iter().filter(move |e| e.last_year == year)
    }

    /// The event covering a fine-grid cell this year, if any.
    pub fn at(&self, y: usize, x: usize) -> Option<&DroughtEvent> {
        let (cy, cx) = (y / STRIDE, x / STRIDE);
        if cy >= self.rows || cx >= self.cols {
            return None;
        }
        let o = self.owner[cy * self.cols + cx];
        if o < 0 {
            None
        } else {
            self.events.get(o as usize)
        }
    }

    /// Identity line (ADR-0003): the ledger is state a replay must
    /// reproduce — same names, same spans, same ground.
    pub fn hash(&self) -> u64 {
        let mut s = String::new();
        for e in &self.events {
            s.push_str(&format!(
                "{}|{}|{}|{}|{:.4}|{}|{}|{}|{}|{}|{}\n",
                e.id, e.name, e.start_year, e.last_year, e.peak, e.peak_nodes, e.onset_nodes,
                e.x, e.y, e.ax, e.ay
            ));
        }
        crate::util::fnv1a64(s.as_bytes())
    }
}



/// Diagnostics bands (M80). Both are shape claims about the failed year
/// as an *event*, not about any one world's luck.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band {
        name: "drought carry-over share of famines",
        sweet: (0.05, 0.60),
        hard: (0.01, 0.85),
        target: "M80: memory is load-bearing but not sovereign — some harvests fail on the years behind them, most still on the year itself",
    },
    crate::util::Band {
        name: "named droughts per century",
        sweet: (3.0, 40.0),
        hard: (1.0, 90.0),
        target: "M80: a drought is a generational event the chronicle can carry, not annual weather",
    },
];
