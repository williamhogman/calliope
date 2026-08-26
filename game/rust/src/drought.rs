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

use crate::climate;
use crate::event::{Event, EventKind};
use crate::famine::DROUGHT_Z;
use crate::world::World;

/// How much of last year's shortfall the ground still carries. 0.5: the
/// weight of a year halves annually — a two-year memory in the sense that
/// matters, an eleven-year tail in the sense that is measurable.
pub const MEM: f64 = 0.5;

/// The accumulation window. `MEM^12` = 2.4·10⁻⁴ σ: past it the sum cannot
/// move a threshold test, so truncating here keeps the index a *finite,
/// exactly reproducible* expression rather than an unbounded recursion.
pub const MEMO_YEARS: usize = 12;

/// Renormalization back to unit variance, `sqrt(1 − MEM²)`: the weighted
/// sum of independent unit-variance years has variance `1/(1 − MEM²)`.
/// Without it the same numeric threshold would silently mean a rarer
/// event than SPI −1 does.
pub const NORM: f64 = 0.866_025_403_784_438_6;

/// Spacing of the lattice the extent is mapped on, in grid cells. A cell
/// is 4 km (ADR-0004), so one lattice node stands for 32 × 32 km — finer
/// than a drought's grain, coarse enough that three centuries of yearly
/// mapping costs a few million noise draws, not a few hundred million.
pub const STRIDE: usize = 8;

/// The smallest region worth calling a drought, in lattice nodes: three
/// nodes ≈ 3 000 km². Below that the sky is having a bad week somewhere,
/// not a failed year over a country.
pub const MIN_NODES: usize = 3;

/// Area one lattice node stands for, in km².
pub const NODE_KM2: f64 = (STRIDE * STRIDE * 16) as f64;

/// Name forms. `{P}` is the ground the drought sits on — the nearest
/// named feature or town, so the name is the map's, not a bank's.
const FORMS: [&str; 6] = [
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
    hist: Vec<Vec<f32>>,
    /// The accumulated index over the lattice, this year.
    pub index: Vec<f32>,
    /// Which event owns each node this year; `-1` = dry-free ground.
    pub owner: Vec<i32>,
    /// Every drought, in the order they took hold.
    pub events: Vec<DroughtEvent>,
    /// The last year mapped (`i64::MIN` before the first pass).
    pub year: i64,
    taken: HashSet<String>,
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

impl World {
    /// The drought index at one cell in one year — the law itself
    /// (see the module header). Pure in seed × cell × year.
    pub fn drought_index(&self, year: i64, y: usize, x: usize) -> f64 {
        let rows = self.fields.tmean.dim().0;
        let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
        let mut acc = 0.0;
        let mut w = 1.0;
        for k in 0..MEMO_YEARS as i64 {
            let yr = year - k;
            let (_, dp) =
                climate::year_anomaly_at(self.variability(), rows, x, y, yr, self.year_osc(yr));
            acc += w * dp / sigma;
            w *= MEM;
        }
        acc * NORM
    }

    /// The single year's standardized anomaly, kept public because the
    /// harness and the explain layer both want to show the year apart
    /// from the memory it lands on.
    pub fn year_spi(&self, year: i64, y: usize, x: usize) -> f64 {
        let rows = self.fields.tmean.dim().0;
        let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
        self.year_rain_anomaly_site(year, y, x) / sigma
    }

    /// M80 — the yearly mapping pass: read the index over the lattice,
    /// group the dry ground, carry names forward, name what is new.
    /// Runs once a year, in the same month as the harvest verdict, so a
    /// famine and the drought it belongs to always agree.
    pub(crate) fn drought_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let year = month_abs / 12;
        let mut d = std::mem::take(&mut self.droughts);
        if d.rows == 0 {
            d = Droughts::new(&self.fields.height);
        }
        let out = self.drought_map(&mut d, year, month_abs);
        self.droughts = d;
        out
    }

    fn lattice_year(&self, d: &Droughts, year: i64) -> Vec<f32> {
        let rows = self.fields.tmean.dim().0;
        let osc = self.year_osc(year);
        let mut z = vec![0.0f32; d.rows * d.cols];
        for cy in 0..d.rows {
            let y = cy * STRIDE;
            let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
            for cx in 0..d.cols {
                if !d.land[cy * d.cols + cx] {
                    continue;
                }
                let x = cx * STRIDE;
                let (_, dp) = climate::year_anomaly_at(self.variability(), rows, x, y, year, osc);
                z[cy * d.cols + cx] = (dp / sigma) as f32;
            }
        }
        z
    }

    fn drought_map(&self, d: &mut Droughts, year: i64, month_abs: i64) -> Vec<Event> {
        // The window: newest first. On the first pass the whole window is
        // filled from the sky's own prehistory (years before the founding
        // exist on the lattice), so year 0 reads the same law as year 300.
        if d.hist.is_empty() {
            for k in (1..MEMO_YEARS as i64).rev() {
                d.hist.insert(0, self.lattice_year(d, year - k));
            }
            d.hist.insert(0, self.lattice_year(d, year));
        } else {
            d.hist.insert(0, self.lattice_year(d, year));
            d.hist.truncate(MEMO_YEARS);
        }
        let n = d.rows * d.cols;
        for i in 0..n {
            let mut acc = 0.0f64;
            let mut w = 1.0f64;
            for g in d.hist.iter() {
                acc += w * g[i] as f64;
                w *= MEM;
            }
            d.index[i] = (acc * NORM) as f32;
        }

        // Two thresholds, not one (hysteresis). A drought must ENTER on
        // genuinely failed ground — a core of `MIN_CORE` nodes at or past
        // SPI −1 — but it HOLDS while the ground stays merely parched
        // (`DRY_HOLD`). Mapping both edges at the same line was what made
        // the ledger blink: a region flickering across a single contour
        // dies and is re-named every other year, which reads as a
        // one-year median span and a footprint that never matches
        // yesterday's. The extent is grown on the holding contour; the
        // core decides only whether a *new* name is owed.
        let prev_owner = std::mem::replace(&mut d.owner, vec![-1; n]);
        let hold: Vec<bool> =
            (0..n).map(|i| d.land[i] && d.index[i] as f64 <= DRY_HOLD).collect();
        let mut seen = vec![false; n];
        let mut regions: Vec<Vec<usize>> = Vec::new();
        for start in 0..n {
            if !hold[start] || seen[start] {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut cells = Vec::new();
            while let Some(i) = stack.pop() {
                cells.push(i);
                let (cy, cx) = (i / d.cols, i % d.cols);
                let push = |ny: usize, nx: usize, stack: &mut Vec<usize>, seen: &mut Vec<bool>| {
                    let j = ny * d.cols + nx;
                    if hold[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                };
                if cy > 0 {
                    push(cy - 1, cx, &mut stack, &mut seen);
                }
                if cy + 1 < d.rows {
                    push(cy + 1, cx, &mut stack, &mut seen);
                }
                if cx > 0 {
                    push(cy, cx - 1, &mut stack, &mut seen);
                }
                if cx + 1 < d.cols {
                    push(cy, cx + 1, &mut stack, &mut seen);
                }
            }
            cells.sort_unstable();
            if cells.len() < MIN_NODES {
                continue;
            }
            // A core past SPI −1 earns a new name; ground already owned by
            // a living drought keeps it without re-earning the core.
            let core = cells.iter().filter(|&&i| d.index[i] as f64 <= DROUGHT_Z).count();
            let inherited = cells.iter().any(|&i| prev_owner[i] >= 0);
            if core >= MIN_CORE || inherited {
                regions.push(cells);
            }
        }
        // Deterministic order: by the region's lowest node.
        regions.sort_by_key(|r| r[0]);

        // Inheritance: a region belongs to last year's drought it overlaps
        // most. One ancestor can only be claimed once — where a drought
        // splits, the larger half keeps the name and the other half is a
        // new failed year of its own.

        let mut claimed: HashSet<usize> = HashSet::new();
        let mut assign: Vec<Option<usize>> = vec![None; regions.len()];
        let mut order: Vec<usize> = (0..regions.len()).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(regions[i].len()), regions[i][0]));
        for &ri in &order {
            let mut tally: std::collections::BTreeMap<usize, usize> = Default::default();
            for &cell in &regions[ri] {
                let o = prev_owner[cell];
                if o >= 0 {
                    *tally.entry(o as usize).or_default() += 1;
                }
            }
            let best = tally
                .iter()
                .filter(|(e, _)| !claimed.contains(*e))
                .max_by_key(|(e, n)| (**n, std::cmp::Reverse(**e)))
                .map(|(e, _)| *e);
            if let Some(e) = best {
                claimed.insert(e);
                assign[ri] = Some(e);
            }
        }

        let mut events = Vec::new();
        for (ri, cells) in regions.iter().enumerate() {
            let nodes = cells.len();
            let (mut sx, mut sy) = (0.0f64, 0.0f64);
            let mut peak = 0.0f64;
            let mut deep = cells[0];
            for &cell in cells {
                sx += ((cell % d.cols) * STRIDE) as f64;
                sy += ((cell / d.cols) * STRIDE) as f64;
                if (d.index[cell] as f64) < peak {
                    deep = cell;
                }
                peak = peak.min(d.index[cell] as f64);
            }
            let (cx, cy) = (sx / nodes as f64, sy / nodes as f64);
            // This year's anchor: the deepest node of *this* year's
            // footprint, in fine-grid cells.
            let deep_x = ((deep % d.cols) * STRIDE) as i64;
            let deep_y = ((deep / d.cols) * STRIDE) as i64;

            let idx = match assign[ri] {
                Some(e) => {
                    let prev: HashSet<usize> = d.events[e].prev.iter().copied().collect();
                    let inter = cells.iter().filter(|c| prev.contains(c)).count();
                    let union = prev.len() + nodes - inter;
                    let jac = if union == 0 { 0.0 } else { inter as f64 / union as f64 };
                    let ev = &mut d.events[e];
                    ev.last_year = year;
                    ev.peak = ev.peak.min(peak);
                    ev.peak_nodes = ev.peak_nodes.max(nodes);
                    ev.years.push((year, nodes, cx, cy, jac, deep_x, deep_y));
                    ev.prev = cells.clone();
                    e
                }
                None => {
                    let id = d.events.len();
                    let (name, place) = self.name_drought(&mut d.taken, id, cx, cy);
                    d.events.push(DroughtEvent {
                        id,
                        name: name.clone(),
                        place: place.clone(),
                        start_year: year,
                        last_year: year,
                        peak,
                        peak_nodes: nodes,
                        onset_nodes: nodes,
                        x: cx.round() as i64,
                        y: cy.round() as i64,
                        ax: deep_x,
                        ay: deep_y,
                        years: vec![(year, nodes, cx, cy, 1.0, deep_x, deep_y)],
                        announced: true,
                        prev: cells.clone(),
                    });
                    // The chronicle speaks a drought's name exactly once:
                    // in the year it takes hold.
                    events.push(Event {
                        m: month_abs,
                        s: name.clone(),
                        k: EventKind::Drought,
                        text: format!(
                            "The rains withdraw from {}: {} begins, and {:.0} thousand square leagues of field and pasture go dry.",
                            place,
                            name,
                            (nodes as f64 * NODE_KM2 / 1000.0).max(1.0)
                        ),
                        x: cx.round() as i64,
                        y: cy.round() as i64,
                        ..Default::default()
                    });
                    id
                }
            };
            for &cell in cells {
                d.owner[cell] = idx as i32;
            }
        }
        d.year = year;
        events
    }

    /// A drought is named for the ground it withers: the nearest named
    /// feature, or failing that the nearest town, or failing that the
    /// world itself. The form is picked by the event's own ordinal, so no
    /// die is rolled and no other stream is disturbed.
    fn name_drought(
        &self,
        taken: &mut HashSet<String>,
        id: usize,
        cx: f64,
        cy: f64,
    ) -> (String, String) {
        let mut best: Option<(f64, String)> = None;
        let mut consider = |x: i64, y: i64, name: &str| {
            if name.is_empty() {
                return;
            }
            let dd = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
            if best.as_ref().is_none_or(|(bd, _)| dd < *bd) {
                best = Some((dd, name.to_string()));
            }
        };
        for f in &self.features {
            consider(f.x, f.y, &f.name);
        }
        for s in &self.peoples.settlements {
            consider(s.x, s.y, &s.name);
        }
        let place = best.map(|(_, n)| n).unwrap_or_else(|| self.world_name.clone());
        for k in 0..FORMS.len() {
            let cand = FORMS[(id + k) % FORMS.len()].replace("{P}", &place);
            if taken.insert(cand.clone()) {
                return (cand, place);
            }
        }
        let cand = format!("{} of the year {}", FORMS[id % FORMS.len()].replace("{P}", &place), id);
        taken.insert(cand.clone());
        (cand, place)
    }
}

fn row_lat(rows: usize, y: usize) -> f64 {
    (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs()
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
