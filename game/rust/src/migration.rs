//! M98 — Off the Failing Margin: climate migration with a date and a
//! direction.
//!
//! The famine pass (M72/M95/M97) answers a single failed year: some
//! starve, some walk to the nearest kin-town that same month. This module
//! answers a failed *generation*. Every settlement carries a ten-year
//! memory of what the land around it gave against a fair year — the
//! decade reading — and when the mean of those ten harvests crosses the
//! failed-decade threshold, a fraction of the town takes the road toward
//! the nearest viable settlement of its own people, in one dated pulse.
//!
//! The reading is the M95 composite sky as the harvest law reads it
//! (`World::year_yield_bare`: heat, rain, monsoon, the crop's own curve,
//! the irrigation lane) compounded with the M90 marginal-field
//! abandonment around the town (`World::abandoned_share`: the density
//! the dawn hinterland farmed that has since gone back to the wild). It
//! is a pure function of the year and the fields — no flood spate, no
//! store, no die — so the harness can re-derive every reading at the
//! year's close and every pulse from the readings before it.
//!
//! The historical anchors the constants are read against:
//!
//! - The Ancestral Puebloan departure from Mesa Verde during the Great
//!   Drought of 1276–1299 (tree-ring dated; Douglass 1929, Benson et al.
//!   2007): a region emptied over roughly two decades of failed maize
//!   harvests, its people arriving in the Rio Grande pueblos of their
//!   own kin — the road led to kin, not to strangers.
//! - The Dust Bowl (1930–1940): ~2.5 million left the Plains states over
//!   the decade; Oklahoma lost ~19 % of its population, the worst
//!   panhandle counties up to 40 % (Gregory 1989, Hornbeck 2012). The
//!   leaving share here runs 15 % at the threshold to 40 % at the worst
//!   decade the law can read.
//! - Norse Greenland's Western Settlement (~1350) and the Highland
//!   clearances-by-weather of the 1690s "ill years" (Cullen 2010): a
//!   sustained cold decade, then departure — never a single bad year.
//! - The Irish Famine emigration (1845–1852): ~1 million left in seven
//!   years, ~12 % of the island, with a strong bias toward places where
//!   kin already lived (Ó Gráda 1999). One generation, one pulse.
//!
//! Law: `reading = 1 − yield × (1 − lost)` in [0, 1]; a full ring of ten
//! readings averages to the decade mean; a pulse fires when the mean is
//! at or above `FAILED_DECADE`, the town has not pulsed inside
//! `PULSE_GAP_YEARS`, it is large enough to lose anyone (`POP_FLOOR`),
//! and a kin-town under a kinder sky (`REFUGE_MEAN`) stands anywhere on
//! the map. The walkers leave and arrive in the same month; nothing is
//! ever in transit across a tick boundary, so the chunking replay of the
//! determinism gate sees one world.
//!
//! Leaf module by the module-DAG law (`scripts/report.sh`): laws, the
//! memory, the ledger rows and the prose live here; the pass that reads
//! the world lives on `World` (`world.rs::migration_pass`).

use crate::constants as gc;

/// The memory's length in harvests: a generation of the land.
pub const DECADE: usize = 10;
/// The decade mean at or above which the sky is judged to have failed
/// the generation: the land gave three-quarters of a fair year, or
/// less, averaged over ten harvests. Read against the famine law's own
/// floor (`famine::TOLL_FLOOR`, want ≥ 0.5 in one year kills): a
/// single failed year does not move a town; ten years at a quarter
/// short do.
pub const FAILED_DECADE: f64 = 0.25;
/// One reading at or above this is a harvest that failed outright — the
/// telling counts them ("four of them failed outright").
pub const FAILED_YEAR: f64 = 0.5;
/// A refuge must carry a remembered mean under this: no one walks from
/// one failed sky into another, and a kinder sky is one clearly outside
/// the failing band, not a hair under its threshold (0.20 let waves
/// bounce between two towns of one failing valley six years apart).
/// Judged on the full decade when the town has one, on what it
/// remembers once it has `REFUGE_MIN_YEARS`; a town younger than that
/// has no sky record to hold against it.
pub const REFUGE_MEAN: f64 = 0.15;
/// Readings a young town needs before its own sky is held against it.
pub const REFUGE_MIN_YEARS: usize = 3;
/// One pulse a generation: the town does not empty year on year, it
/// sends one wave down the road and holds for another decade.
pub const PULSE_GAP_YEARS: i64 = 10;
/// Towns at or under this send no one — the famine law's own floor.
pub const POP_FLOOR: i64 = 90;
/// What a pulse always leaves behind: the hearth that keeps the name.
pub const KEEP: i64 = 30;
/// Fewer walkers than this are not a pulse: the famine law's spoken floor.
pub const SPOKEN_MIN: i64 = 4;
/// The share that leaves at the threshold, rising with the excess above
/// it (`LEAVE_SLOPE` per unit of mean), capped at `LEAVE_MAX`.
pub const LEAVE_BASE: f64 = 0.15;
pub const LEAVE_SLOPE: f64 = 1.0;
pub const LEAVE_MAX: f64 = 0.40;
/// The hinterland the abandonment share is read over: the same 12 km
/// (radius 3 cells) disc the town's field capacity is summed over.
pub const HINTERLAND_R: i64 = 3;

/// The year no pulse has fired yet.
pub const NEVER: i64 = -1;

/// One town's ten-year memory of the land. Rides the replay identity
/// line (hashed at the law's own resolution, a thousandth); never on the
/// wire.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    /// The last `DECADE` readings, each rounded to a thousandth.
    pub ring: [f64; DECADE],
    /// Readings taken so far, saturating at `DECADE`.
    pub filled: u8,
    /// Slot the next reading lands in.
    pub head: u8,
    /// Calendar year of the last pulse, or `NEVER`.
    pub last_pulse: i64,
}

impl Default for Memory {
    fn default() -> Self {
        Memory { ring: [0.0; DECADE], filled: 0, head: 0, last_pulse: NEVER }
    }
}

impl Memory {
    /// Take one year's reading into the ring.
    pub fn push(&mut self, reading: f64) {
        self.ring[self.head as usize] = crate::util::round3(reading.clamp(0.0, 1.0));
        self.head = ((self.head as usize + 1) % DECADE) as u8;
        if (self.filled as usize) < DECADE {
            self.filled += 1;
        }
    }

    /// A full generation remembered.
    pub fn full(&self) -> bool {
        self.filled as usize >= DECADE
    }

    /// The decade mean, once the ring is full; `None` while the town is
    /// too young to be judged. Summed in slot order — fixed, so the
    /// arithmetic replays exactly.
    pub fn mean(&self) -> Option<f64> {
        if !self.full() {
            return None;
        }
        Some(self.ring.iter().sum::<f64>() / DECADE as f64)
    }

    /// Readings taken so far, oldest first.
    pub fn readings(&self) -> Vec<f64> {
        let n = self.filled as usize;
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let slot = (self.head as usize + DECADE - n + k) % DECADE;
            out.push(self.ring[slot]);
        }
        out
    }

    /// How many remembered harvests failed outright.
    pub fn failed_years(&self) -> usize {
        self.readings().iter().filter(|&&r| r >= FAILED_YEAR).count()
    }

    /// The worst remembered harvest.
    pub fn worst(&self) -> f64 {
        self.readings().iter().cloned().fold(0.0, f64::max)
    }

    /// The memory has crossed the failed-decade threshold.
    pub fn failed(&self) -> bool {
        self.mean().map_or(false, |m| m >= FAILED_DECADE)
    }

    /// A pulse may fire this year as far as the gap is concerned.
    pub fn armed(&self, year: i64) -> bool {
        self.last_pulse == NEVER || year - self.last_pulse >= PULSE_GAP_YEARS
    }

    /// The mean of what the town remembers so far, once it remembers
    /// `REFUGE_MIN_YEARS`; `None` for a town younger than that. Equals
    /// `mean()` once the ring is full.
    pub fn remembered_mean(&self) -> Option<f64> {
        let n = self.filled as usize;
        if n < REFUGE_MIN_YEARS {
            return None;
        }
        if n >= DECADE {
            return self.mean();
        }
        Some(self.readings().iter().sum::<f64>() / n as f64)
    }

    /// The town can receive walkers: its own sky is not failing.
    pub fn refuge(&self) -> bool {
        self.remembered_mean().map_or(true, |m| m < REFUGE_MEAN)
    }

    /// Fold the memory into the replay identity line.
    pub fn hash_into(&self, s: &mut String) {
        s.push_str(&format!("g{}|{}|{}", self.filled, self.head, self.last_pulse));
        for r in &self.ring {
            s.push_str(&format!("|{:.3}", r));
        }
        s.push('\n');
    }
}

/// The law's reading of one year at one town: the share of a fair year
/// the land did not give, with the M90 abandonment compounded in.
pub fn reading(yield_factor: f64, lost: f64) -> f64 {
    // rounded here, once, so the ring, the ledger row and every
    // re-derivation hold the same three-decimal number
    crate::util::round3((1.0 - yield_factor * (1.0 - lost.clamp(0.0, 1.0))).clamp(0.0, 1.0))
}

/// The share of the town that leaves for a decade mean at `mean`.
pub fn leave_share(mean: f64) -> f64 {
    (LEAVE_BASE + LEAVE_SLOPE * (mean - FAILED_DECADE).max(0.0)).clamp(LEAVE_BASE, LEAVE_MAX)
}

/// How many walk: the share of the town, never below `KEEP` left behind.
pub fn walkers(pop: i64, mean: f64) -> i64 {
    if pop <= POP_FLOOR {
        return 0;
    }
    let n = (pop as f64 * leave_share(mean)).round() as i64;
    n.min(pop - KEEP).max(0)
}

/// Why a town whose decade has failed sends no wave this year. One
/// predicate, shared by the pass (`World::migration_pass`) and the
/// inspector's roads block, so the card can never promise a road the
/// law would not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// A wave left inside the generation gap; the town holds a decade.
    Walked,
    /// At or under `POP_FLOOR`: nobody is sent.
    Small,
    /// The famine has the town this year; its own toll moves people.
    Famine,
    /// Fewer than `SPOKEN_MIN` would go — not a wave the record speaks of.
    Unspoken,
}

/// The pass's eligibility, in the pass's own order. `None` means a wave
/// walks this year if a kin-town under a kinder sky can be found.
pub fn hold(mem: &Memory, year: i64, pop: i64, failing: bool, mean: f64) -> Option<Hold> {
    if !mem.armed(year) {
        Some(Hold::Walked)
    } else if pop <= POP_FLOOR {
        Some(Hold::Small)
    } else if failing {
        Some(Hold::Famine)
    } else if walkers(pop, mean) < SPOKEN_MIN {
        Some(Hold::Unspoken)
    } else {
        None
    }
}

/// Compass wind from the source toward the destination on the grid
/// (rows grow southward). Eight winds.
pub fn compass(dx: f64, dy: f64) -> &'static str {
    if dx == 0.0 && dy == 0.0 {
        return "nowhere";
    }
    // angle measured clockwise from north, in [0, 360)
    let ang = dx.atan2(-dy).to_degrees();
    let ang = if ang < 0.0 { ang + 360.0 } else { ang };
    const WINDS: [&str; 8] = [
        "north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west",
    ];
    let k = ((ang + 22.5) / 45.0).floor() as usize % 8;
    WINDS[k]
}

/// Road length in km between two grid positions.
pub fn road_km(sx: i64, sy: i64, dx: i64, dy: i64) -> f64 {
    (((dx - sx) as f64).powi(2) + ((dy - sy) as f64).powi(2)).sqrt() * gc::KM_PER_CELL
}

/// One pulse, observed at the mechanism. Diagnostics ledger; never
/// hashed, never packed. `dst` is `None` when the decade failed and the
/// town was ready to send, but no kin-town under a kinder sky stood
/// anywhere on the map — the road led nowhere, and nobody walked.
#[derive(Clone, Debug)]
pub struct ExodusRow {
    pub m: i64,
    pub year: i64,
    pub src_sid: i64,
    pub src: (i64, i64),
    pub src_name: String,
    pub dst_sid: Option<i64>,
    pub dst: Option<(i64, i64)>,
    pub dst_name: String,
    /// Decade mean the pulse read.
    pub mean: f64,
    /// The refuge's own remembered mean as the walkers arrived (`-1.0`
    /// for a town too young to have one).
    pub dst_mean: f64,
    /// Remembered harvests that failed outright.
    pub fails: usize,
    /// The M90 abandonment share at the source this year.
    pub lost: f64,
    pub walked: i64,
    pub km: f64,
    pub src_pop_before: i64,
    pub src_pop_after: i64,
    pub dst_pop_before: i64,
    pub dst_pop_after: i64,
}

/// One year's reading at one town, observed at the mechanism.
/// Diagnostics ledger; never hashed, never packed.
#[derive(Clone, Copy, Debug)]
pub struct ReadingRow {
    pub m: i64,
    pub year: i64,
    pub sid: i64,
    pub x: i64,
    pub y: i64,
    /// The town's head-count as the reading was taken (before any wave).
    pub pop: i64,
    /// The town was already failing as a town (no wave leaves a husk).
    pub failing: bool,
    pub yield_factor: f64,
    pub lost: f64,
    pub reading: f64,
    /// The decade mean after this reading, once the ring is full.
    pub mean: Option<f64>,
}

/// The chronicle's sentence for a pulse.
pub fn exodus_text(
    src: &str,
    dst: &str,
    walked: i64,
    mean: f64,
    fails: usize,
    lost: f64,
    km: f64,
    wind: &str,
) -> String {
    let gave = ((1.0 - mean) * 100.0).round() as i64;
    let mut s = format!(
        "Ten harvests have failed {} — the land gave {} in a hundred of a fair year",
        src, gave
    );
    if fails > 0 {
        s.push_str(&format!(", {} of them failing outright", fails));
    }
    if lost >= 0.02 {
        s.push_str(&format!(
            ", and {} in a hundred of the fields around it have gone back to the wild",
            (lost * 100.0).round() as i64
        ));
    }
    s.push_str(&format!(
        ". {} of its people take the road {} to {}, {:.0} km away.",
        walked, wind, dst, km
    ));
    s
}

/// The inspector's line for a town's decade memory. `hold` is the
/// pass's own verdict on why no wave walks (`None` = it would), and
/// `refuge` the kin-town the same search the pass runs would lead to.
pub fn decade_line(
    mem: &Memory,
    pop: i64,
    hold: Option<Hold>,
    refuge: Option<&str>,
) -> String {
    match mem.mean() {
        None => format!(
            "The land here has been read for {} harvest{}; ten make a generation, and no one is judged on less.",
            mem.filled,
            if mem.filled == 1 { "" } else { "s" }
        ),
        Some(m) => {
            let gave = ((1.0 - m) * 100.0).round() as i64;
            let fails = mem.failed_years();
            let mut s = format!(
                "Over the last ten harvests the land gave {} in a hundred of a fair year",
                gave
            );
            if fails > 0 {
                s.push_str(&format!(", {} of them failing outright", fails));
            }
            s.push('.');
            if m >= FAILED_DECADE {
                let n = walkers(pop, m);
                match hold {
                    Some(Hold::Walked) => s.push_str(&format!(
                        " The generation has failed; a wave already left in year {}, and the town holds for a decade before another.",
                        mem.last_pulse + 1
                    )),
                    Some(Hold::Small) => s.push_str(
                        " The generation has failed, but the town is too small to send anyone down the road.",
                    ),
                    Some(Hold::Famine) => s.push_str(
                        " The generation has failed, but this year the famine has the town; the road waits on its toll.",
                    ),
                    Some(Hold::Unspoken) => s.push_str(&format!(
                        " The generation has failed, but only {} would go — too few for the record to call a wave.",
                        n
                    )),
                    None => match refuge {
                        Some(r) => s.push_str(&format!(
                            " The generation has failed: {} in a hundred would take the road to {} — {} souls.",
                            (leave_share(m) * 100.0).round() as i64,
                            r,
                            n
                        )),
                        None => s.push_str(
                            " The generation has failed, and there is no kin-town under a kinder sky to walk to; the town holds on.",
                        ),
                    },
                }
            } else {
                s.push_str(&format!(
                    " A generation fails at {} in a hundred short.",
                    (FAILED_DECADE * 100.0).round() as i64
                ));
            }
            s
        }
    }
}

/// M98 — the design bands the civ and sweep lanes read.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band {
        name: "exodus pulses / settlement-century",
        sweet: (0.05, 2.0),
        hard: (0.0, 6.0),
        target: "M98: climate migration is rare and clustered — a few towns a century send a wave, in the failed ages; not none, not every year",
    },
    crate::util::Band {
        name: "exodus share of the town",
        sweet: (0.12, 0.42),
        hard: (0.05, 0.60),
        target: "M98: Dust Bowl Oklahoma lost ~19 %, its worst counties ~40 %; a wave is a fraction of the town, never the town",
    },
    crate::util::Band {
        name: "exodus road km",
        sweet: (16.0, 400.0),
        hard: (4.0, 1600.0),
        target: "M98: the road leads to the nearest kin-town under a kinder sky — days to weeks on foot, not across the world",
    },
];
