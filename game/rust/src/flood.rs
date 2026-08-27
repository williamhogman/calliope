//! Floods — "The River That Drowns and Gives" (M81).
//!
//! M72 gave every year its own realized discharge: a bounded multiplier on
//! the frozen mean-climate flow, catchment-integrated so a single dry cell
//! cannot empty a trunk river. Until now that multiplier could only ever
//! *help* — a wet year watered the canals and nothing else happened. Real
//! rivers do not work that way. Past a certain stage the water leaves the
//! channel, and the same water that drowns the levees lays down the silt
//! that makes the floodplain the richest ground a people can farm. The
//! Nile's flood-recession agriculture, the Mesopotamian levee breaches and
//! the Yellow River's centuries of "sorrow and gift" are all one mechanism
//! read twice.
//!
//! So a flood here is a threshold crossing on the year's own water, not a
//! die:
//!
//! ```text
//!   excess = year_flow_factor(year, cell) − capacity(town)
//!   capacity(town) = BANKFULL + Σ levee craft the town's people hold
//! ```
//!
//! A town floods in exactly the years where `excess > 0`. Nothing is
//! rolled, so the harness can re-derive every flood in the ledger from the
//! seed alone — the same discipline M80 holds for drought.
//!
//! It costs, and it pays. The cost is bounded population loss:
//! `DMG_GAIN · excess`, hard-capped at [`DMG_CAP`], so no single spate can
//! take more than a documented share of a town — a levee breach is a
//! disaster, never an extinction. The payment arrives the *following*
//! growing season as a silt bonus on the flooded ground and its
//! floodplain ring, decaying to nothing after one year: the harvest that
//! follows a flood is better than the harvest that preceded it.

use std::collections::HashMap;

use crate::society::TechId;

/// The bankfull stage, as a multiple of the mean-climate flow the frozen
/// `discharge` grid carries. A channel is cut by the water that usually
/// runs in it, so the flow it can hold sits modestly above its own mean —
/// the geomorphic bankfull recurrence on Earth is ~1.5 years (Leopold,
/// Wolman & Miller 1964), i.e. a stage an ordinary wet year already
/// reaches. 1.22 puts the crossing in that neighbourhood on this sky,
/// where the realized flow multiplier is clamped to
/// [`crate::climate::FLOW_FACTOR_MAX`].
pub const BANKFULL: f64 = 1.22;

/// What a levee craft adds to the stage the town's banks can hold. These
/// are earthworks, not miracles: together they buy roughly a third of the
/// mean flow again, and the clamp on the year's water means a
/// fully-engineered town still drowns in the worst years.
pub const LEVEE_MASONRY: f64 = 0.10;
pub const LEVEE_AQUEDUCT: f64 = 0.06;
pub const LEVEE_ENGINEERING: f64 = 0.14;

/// Souls lost per unit of excess stage, before the cap.
pub const DMG_GAIN: f64 = 0.45;

/// **The documented cap.** No flood may take more than this share of a
/// town's people, whatever the year does. The gate checks the realized
/// maximum against it directly.
pub const DMG_CAP: f64 = 0.06;

/// A town below this many souls is not modelled as drowning: the ledger
/// would be noise and the arithmetic would round to nothing.
pub const MIN_POP: i64 = 90;

/// Silt gain per unit of excess stage on the flooded cell, as a fraction
/// added to the following season's yield factor.
pub const SILT_GAIN: f64 = 0.55;

/// The most a silt layer may add to the following season's yield. One
/// flood is a good year on the floodplain, not a second harvest.
pub const SILT_CAP: f64 = 0.22;

/// **The drowned season.** The water that lays the silt is standing on
/// the fields *now*: a breach in the growing season destroys the crop it
/// covers, and the recompense arrives only with the next sowing. Without
/// this the spate is a pure gift — a town loses a bounded share of its
/// people once and is handed a richer harvest forever after, which made
/// riverside peoples advance faster *because* their river drowned them.
/// The loss is the mirror of the gift, on the same excess and the same
/// ground, so the law reads in one breath: the river takes the year it
/// stands in and pays the year that follows.
pub const DROWN_GAIN: f64 = SILT_GAIN;

/// The most a spate may take off the standing crop — the mirror of
/// [`SILT_CAP`]. A drowned field is a failed season on that ground, never
/// a salted one.
pub const DROWN_CAP: f64 = SILT_CAP;

/// How far off the drowned cell the silt sheet is laid, in grid cells.
/// One ring: the valley floor the water actually spread over (the same
/// reach `agriculture` calls the floodplain).
pub const SILT_REACH: i64 = 1;


/// One flood, as the ledger recorded it — everything the gate needs to
/// re-derive the event from the seed and to price its cost.
#[derive(Clone, Default)]
pub struct FloodRow {
    /// Absolute month it struck.
    pub m: i64,
    pub year: i64,
    pub x: i64,
    pub y: i64,
    /// Settlement id, as a plain integer for the ledger's hash line.
    pub sid: usize,
    /// Population before the water came.
    pub pop: i64,
    /// The year's realized flow multiplier at the town's cell.
    pub factor: f64,
    /// The stage its banks and levees could hold.
    pub cap: f64,
    /// Share of the town the water took (≤ [`DMG_CAP`], always).
    pub frac: f64,
    /// Souls lost.
    pub hit: i64,
    /// Silt strength laid on the ground for the following season.
    pub silt: f64,
}

/// The flood ledger: every spate the world has lived through, and the
/// silt those spates left on the ground for the season after.
#[derive(Default)]
pub struct Floods {
    /// Every flood, in the order they struck.
    pub rows: Vec<FloodRow>,
    /// `cell → (the year this silt feeds, its strength)`. A later, richer
    /// layer overwrites a thinner one; the entry is spent once the year
    /// it feeds has passed. Lookups only — iteration order never reaches
    /// an output (ADR-0003).
    pub(crate) silt: HashMap<(i64, i64), (i64, f64)>,
}

impl Floods {
    /// The silt bonus standing on a cell in `year`: a fraction to add to
    /// that season's yield factor, zero on unflooded or spent ground.
    pub fn silt_bonus(&self, year: i64, y: usize, x: usize) -> f64 {
        match self.silt.get(&(y as i64, x as i64)) {
            Some(&(yr, g)) if yr == year => g,
            _ => 0.0,
        }
    }

    /// Lay a silt sheet for `year` at `strength`, keeping the richer of
    /// two layers where two floods overlap.
    pub(crate) fn lay(&mut self, year: i64, y: i64, x: i64, strength: f64) {
        let e = self.silt.entry((y, x)).or_insert((year, 0.0));
        if e.0 != year || e.1 < strength {
            *e = (year, strength);
        }
    }

    /// Drop layers whose season has passed — the ledger stays bounded and
    /// the map only ever carries this year's and next year's silt.
    pub(crate) fn sweep(&mut self, year: i64) {
        self.silt.retain(|_, v| v.0 >= year);
    }

    /// Identity line (ADR-0003): floods are state — who drowned, when,
    /// how deep, and what the ground was given back.
    pub fn hash(&self) -> u64 {
        let mut s = String::new();
        for r in &self.rows {
            s.push_str(&format!(
                "{}|{}|{}|{}|{}|{:.4}|{:.4}|{:.4}|{}|{:.4}\n",
                r.m, r.sid, r.x, r.y, r.pop, r.factor, r.cap, r.frac, r.hit, r.silt
            ));
        }
        // The standing silt sheet rides the line too, in a fixed order —
        // a replay that lost a layer would farm a different next year.
        let mut cells: Vec<(&(i64, i64), &(i64, f64))> = self.silt.iter().collect();
        cells.sort_by_key(|(k, _)| **k);
        for ((y, x), (yr, g)) in cells {
            s.push_str(&format!("s{}|{}|{}|{:.4}\n", y, x, yr, g));
        }
        crate::util::fnv1a64(s.as_bytes())
    }
}

/// The stage a town's banks hold: bankfull plus whatever levee craft its
/// people have learned. Pure in the town's society, so the harness can
/// recompute the threshold it is judging.
pub fn capacity(knows: impl Fn(TechId) -> bool) -> f64 {
    let mut c = BANKFULL;
    if knows(TechId::Masonry) {
        c += LEVEE_MASONRY;
    }
    if knows(TechId::Aqueduct) {
        c += LEVEE_AQUEDUCT;
    }
    if knows(TechId::Engineering) {
        c += LEVEE_ENGINEERING;
    }
    c
}

/// Diagnostics bands (M81). Both are shape claims about the spate as an
/// event, not about any one world's luck.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band {
        name: "floods per river town per century",
        sweet: (2.0, 40.0),
        hard: (0.5, 90.0),
        target: "M81: a river town drowns now and then — a spate every few decades of its life, often enough to be a fact of life on the water, rare enough to be news",
    },
    crate::util::Band {
        name: "worst flood toll (share of town)",
        sweet: (0.0, DMG_CAP),
        hard: (0.0, DMG_CAP),
        target: "M81: the toll is capped by law — no spate may take more than DMG_CAP of a town",
    },
];
