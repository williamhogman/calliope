//! M86/M87/M88 — the ages: multidecadal winters with dated onsets and
//! slow releases (M86), the generous centuries — warm optima when the
//! uplands open (M87) — both layered on the M83 drift, and the names
//! the chronicle remembers them by (M88).
//!
//! **The law.** A renewal schedule drawn once from the seed: quiet gaps
//! alternate with arcs, and each arc draws its kind. A **cold age** is a
//! generation-scale winter — duration inside [`AGE_MIN_YEARS`,
//! `AGE_MAX_YEARS`], depth inside [`AGE_DEPTH_MIN`, `AGE_DEPTH_MAX`] °C
//! of cooling *beyond the drift*, [`AGE_RAMP_IN`] years of arrival and a
//! slower [`AGE_RAMP_OUT`]-year release — a winter that recedes more
//! reluctantly than it came. A **warm optimum** is longer and gentler —
//! duration inside [`OPT_MIN_YEARS`, `OPT_MAX_YEARS`] (the *centuries*
//! of the spec), warmth inside [`OPT_DEPTH_MIN`, `OPT_DEPTH_MAX`] °C,
//! and symmetric [`OPT_RAMP_IN`]/[`OPT_RAMP_OUT`]-year breathing: the
//! kindness comes slowly and goes slowly. Both kinds share one timeline
//! (the next onset draws *after* the previous release plus a bounded
//! gap), so at most one age is ever active and exclusivity stays a
//! property of the draw, not a patched-up invariant.
//!
//! **Derived law, not state** (ADR-0003, the M74/M83 pattern): the
//! schedule is a pure function of the seed on its own RNG stream,
//! nothing is stored, hashed or packed, and `World::year_forcing`
//! composes `drift + ages.offset(year)` in front of the anomaly lattice
//! exactly where the drift alone used to enter — a winter deepens the
//! sky, an optimum lifts it, and the M84 belts walk with the same term.
//! Prehistory (year ≤ 0) reads 0: the dawn is the baseline epoch. The
//! arithmetic is wall-clock-free and libm-free (multiplication,
//! comparison, floor only), so the schedule replays bit-identically
//! across runtimes and its [`Ages::probe`] rides the deep-earth
//! identity line.
//!
//! **The names (M88).** The chronicle christens every arc the way
//! history keeps its Little Ice Ages and Medieval Warm Periods: each
//! age draws a unique name — "The Long Winter", "The Wine Years" and
//! their siblings — composed from the per-kind banks in
//! [`crate::naming`] on a *separate* stream ([`AGE_NAME_STREAM_KEY`]),
//! so christening the schedule never moves a banked date. Uniqueness
//! is world-wide, enforced by a deterministic linear probe over the
//! kind's combo space, whose capacity stands above the hardest arc
//! count the cadence walls admit; the names join [`Ages::probe`], so a
//! bank or keying drift breaks replay identity, never quietly the
//! prose.
//!
//! **Calibration.** Earth's late-Holocene record carries a handful of
//! multidecadal cold epochs per millennium — the Spörer, Maunder and
//! Dalton minima inside the Little Ice Age alone span 20–80-year
//! troughs at a few tenths to ~1 °C of hemispheric cooling — and a
//! rarer, longer cadence of warm optima: the Minoan, Roman and Medieval
//! warm periods run one-to-two per millennium at a few tenths of a
//! degree, each a century or more. The gap draw (mean ≈ 160 y) with
//! [`WARM_SHARE`] of arcs turning warm yields ~2.8 winters and ~1.5
//! optima per millennium, winters holding ~14 % of the years and optima
//! ~17 % — each rare enough to stay an *age*, together leaving most
//! centuries quiet.

use std::collections::HashSet;

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;

use crate::util::fnv1a64;

// ------------------------------------------------------------ constants

/// Years — the shortest winter the law may write. The spec's band floor:
/// an age outlives a generation, or it was weather.
pub const AGE_MIN_YEARS: i64 = 20;
/// Years — the longest. Past this a "winter" would be a climate regime,
/// which is the drift's business, not an arc's.
pub const AGE_MAX_YEARS: i64 = 80;
/// °C — the shallowest full-plateau cooling an arc may carry.
pub const AGE_DEPTH_MIN: f64 = 0.4;
/// °C — the deepest. Beyond the drift's own walls this is the largest
/// term the composed forcing may add in either direction
/// (`OPT_DEPTH_MAX` stays below it), so the M85 walls row reads
/// `DRIFT_BOUND + AGE_DEPTH_MAX` and covers both signs.
pub const AGE_DEPTH_MAX: f64 = 1.0;
/// Years — no age begins sooner than this after the last released:
/// the floor that keeps exclusivity a property of the draw.
pub const AGE_GAP_MIN: i64 = 40;
/// Years — the span of the bounded gap draw above the floor. A 4-sum
/// uniform (bell-shaped, hard-bounded) over this span puts the mean gap
/// at `AGE_GAP_MIN + AGE_GAP_SPAN/2` = 160 y and the longest possible
/// gap at 280 y — so a 300-year leg always crosses the first onset.
pub const AGE_GAP_SPAN: i64 = 240;
/// Years of arrival: a winter's offset ramps linearly to full depth.
pub const AGE_RAMP_IN: i64 = 8;
/// Years of release: slower than the arrival, per the spec's "slow
/// releases" — the world thaws more reluctantly than it froze.
pub const AGE_RAMP_OUT: i64 = 12;

/// M87 — the share of arcs the draw turns warm. Optima are rarer than
/// winters (Minoan/Roman/Medieval against Spörer/Maunder/Dalton) but
/// each holds longer, so the *years* under kindness rival the years
/// under frost.
pub const WARM_SHARE: f64 = 0.35;
/// Years — the shortest optimum: the spec says *centuries*, so the
/// floor sits at the reach of three generations, well past the longest
/// weather and most winters.
pub const OPT_MIN_YEARS: i64 = 60;
/// Years — the longest optimum. The Medieval Warm Period's regional
/// spans reach ~150–200 y; past this the kindness would be a regime.
pub const OPT_MAX_YEARS: i64 = 160;
/// °C — the gentlest full-plateau warmth an optimum may carry.
pub const OPT_DEPTH_MIN: f64 = 0.3;
/// °C — the greatest. Held strictly below `AGE_DEPTH_MAX` so the M85
/// walls (`DRIFT_BOUND + AGE_DEPTH_MAX`, symmetric) bound both signs.
pub const OPT_DEPTH_MAX: f64 = 0.8;
/// Years of arrival for an optimum: the kindness comes slowly —
/// no single spring announces a generous century.
pub const OPT_RAMP_IN: i64 = 20;
/// Years of release: and it goes as slowly as it came — the uplands
/// are let go field by field, not abandoned in a season.
pub const OPT_RAMP_OUT: i64 = 20;

/// Years — the schedule's horizon. No diagnostic leg reads past it
/// (the longest lane scans 20 000 y); beyond it the sky is quiet.
pub const AGES_HORIZON: i64 = 30_000;
/// Stream key: the ages' draws share nothing with the drift, the
/// oscillation, the variability lattice or the famine die.
pub const AGES_STREAM_KEY: u64 = 0xC01D_A6E5_0B5E_55EDu64;
/// Stream key for the christening (M88): the names draw on their own
/// stream so the schedule's dates, kinds and depths stand exactly
/// where M86/M87 banked them.
pub const AGE_NAME_STREAM_KEY: u64 = 0x4E41_4D45_0FA6_E501u64;

// --------------------------------------------------------------- struct

/// One age: a dated winter or a dated optimum. `release` is exclusive —
/// the offset is nonzero on `onset..release`, exactly `release − onset`
/// years, which is the duration the gate bands read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgeArc {
    /// The dated onset year — the chronicle speaks in this year.
    pub onset: i64,
    /// The dated release year — the first year the sky owes nothing.
    pub release: i64,
    /// °C — the plateau's full magnitude, beyond the drift. Always
    /// positive; [`AgeArc::signed_depth`] carries the direction.
    pub depth: f64,
    /// M87 — true for a warm optimum, false for a cold age.
    pub warm: bool,
}

impl AgeArc {
    /// Years the age holds — 20–80 for a winter, 60–160 for an optimum.
    pub fn duration(&self) -> i64 {
        self.release - self.onset
    }

    /// The plateau offset with its sign: an optimum warms, a winter
    /// cools.
    pub fn signed_depth(&self) -> f64 {
        if self.warm {
            self.depth
        } else {
            -self.depth
        }
    }
}

/// The age schedule of one world — a law, not a table. Since M88 every
/// arc also carries its christened name, drawn beside the schedule on
/// its own stream.
pub struct Ages {
    arcs: Vec<AgeArc>,
    /// M88 — the christenings, parallel to `arcs`: unique within the
    /// world, composed from the per-kind banks in [`crate::naming`].
    names: Vec<String>,
}

impl Ages {
    /// Draw the schedule from the seed. Same seed ⇒ same winters and
    /// the same generous centuries, forever — dated at generation,
    /// exactly as the spec asks. Draw order per arc is fixed law:
    /// gap (four uniforms), kind, duration, depth. The christening
    /// (M88) runs after, on its own stream — two uniforms per arc,
    /// then a deterministic linear probe on collision — so a name can
    /// never move a date.
    pub fn new(seed: i64) -> Self {
        let mut rng = Pcg64Mcg::seed_from_u64((seed as u64) ^ AGES_STREAM_KEY);
        let mut arcs = Vec::new();
        let mut t = 0i64;
        loop {
            // Bounded bell gap: mean of four uniforms over the span.
            let mut g = 0.0f64;
            for _ in 0..4 {
                g += rng.gen::<f64>();
            }
            let gap = AGE_GAP_MIN + ((g / 4.0) * AGE_GAP_SPAN as f64).floor() as i64;
            let onset = t + gap;
            if onset > AGES_HORIZON {
                break;
            }
            let warm = rng.gen::<f64>() < WARM_SHARE;
            let (dur_min, dur_max, dep_min, dep_max) = if warm {
                (OPT_MIN_YEARS, OPT_MAX_YEARS, OPT_DEPTH_MIN, OPT_DEPTH_MAX)
            } else {
                (AGE_MIN_YEARS, AGE_MAX_YEARS, AGE_DEPTH_MIN, AGE_DEPTH_MAX)
            };
            let span = (dur_max - dur_min + 1) as f64;
            let dur = (dur_min + (rng.gen::<f64>() * span).floor() as i64).min(dur_max);
            let depth = dep_min + rng.gen::<f64>() * (dep_max - dep_min);
            arcs.push(AgeArc { onset, release: onset + dur, depth, warm });
            t = onset + dur;
        }

        // M88 — christen every arc. Banks are per kind; uniqueness is
        // world-wide, enforced by probing the kind's combo space from a
        // drawn start. Capacity stands above the hardest arc count the
        // cadence walls admit, so the probe always lands; the dated
        // fallback can only fire if a bank shrinks, and the season-fit
        // gate would call that name out as off-bank.
        let mut nrng = Pcg64Mcg::seed_from_u64((seed as u64) ^ AGE_NAME_STREAM_KEY);
        let mut taken: HashSet<String> = HashSet::with_capacity(arcs.len());
        let mut names: Vec<String> = Vec::with_capacity(arcs.len());
        for a in &arcs {
            let (adj, noun) = crate::naming::age_bank(a.warm);
            let total = adj.len() * noun.len();
            let ai = ((nrng.gen::<f64>() * adj.len() as f64).floor() as usize).min(adj.len() - 1);
            let ni = ((nrng.gen::<f64>() * noun.len() as f64).floor() as usize).min(noun.len() - 1);
            let start = ai * noun.len() + ni;
            let mut name = None;
            for step in 0..total {
                let idx = (start + step) % total;
                let cand = crate::naming::age_name(adj[idx / noun.len()], noun[idx % noun.len()]);
                if !taken.contains(&cand) {
                    name = Some(cand);
                    break;
                }
            }
            let name =
                name.unwrap_or_else(|| format!("The {} {} of Year {}", adj[ai], noun[ni], a.onset));
            taken.insert(name.clone());
            names.push(name);
        }
        Ages { arcs, names }
    }

    /// The whole schedule, in onset order.
    pub fn arcs(&self) -> &[AgeArc] {
        &self.arcs
    }

    /// M88 — the christened name of the arc at `idx` (parallel to
    /// [`Ages::arcs`]): what the chronicle calls it.
    pub fn name(&self, idx: usize) -> &str {
        &self.names[idx]
    }

    /// M88 — every christening, in onset order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The arc holding a given year, if any.
    pub fn active(&self, year: i64) -> Option<&AgeArc> {
        if year <= 0 {
            return None;
        }
        let idx = self.arcs.partition_point(|a| a.onset <= year);
        if idx == 0 {
            return None;
        }
        let a = &self.arcs[idx - 1];
        (year < a.release).then_some(a)
    }

    /// The age's offset at a given year, °C on the forcing: ≤ 0 under a
    /// winter, ≥ 0 under an optimum. Ramped per kind — winters arrive
    /// in [`AGE_RAMP_IN`] and release in [`AGE_RAMP_OUT`] years, optima
    /// breathe over [`OPT_RAMP_IN`]/[`OPT_RAMP_OUT`] — with a plateau at
    /// the full signed depth between. For the shortest arcs the two
    /// ramps meet in a triangle — the `min` is the shape.
    pub fn offset(&self, year: i64) -> f64 {
        let Some(a) = self.active(year) else {
            return 0.0;
        };
        let (rin, rout) = if a.warm {
            (OPT_RAMP_IN, OPT_RAMP_OUT)
        } else {
            (AGE_RAMP_IN, AGE_RAMP_OUT)
        };
        let arrive = ((year - a.onset + 1) as f64 / rin as f64).min(1.0);
        let leave = ((a.release - year) as f64 / rout as f64).min(1.0);
        a.signed_depth() * arrive.min(leave)
    }

    /// A fixed read of the schedule for the replay identity line, the
    /// M74/M83 pattern: the law's constants plus every arc's dates,
    /// depth, kind and — since M88 — its christened name. A schedule
    /// whose keying, arithmetic or naming banks move breaks replay
    /// here.
    pub fn probe(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(45 * self.arcs.len() + 128);
        for v in [AGE_DEPTH_MIN, AGE_DEPTH_MAX, WARM_SHARE, OPT_DEPTH_MIN, OPT_DEPTH_MAX] {
            b.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in [
            AGE_MIN_YEARS,
            AGE_MAX_YEARS,
            AGE_GAP_MIN,
            AGE_GAP_SPAN,
            AGE_RAMP_IN,
            AGE_RAMP_OUT,
            OPT_MIN_YEARS,
            OPT_MAX_YEARS,
            OPT_RAMP_IN,
            OPT_RAMP_OUT,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for (a, n) in self.arcs.iter().zip(self.names.iter()) {
            b.extend_from_slice(&a.onset.to_le_bytes());
            b.extend_from_slice(&a.release.to_le_bytes());
            b.extend_from_slice(&a.depth.to_bits().to_le_bytes());
            b.push(a.warm as u8);
            b.extend_from_slice(n.as_bytes());
            b.push(0);
        }
        fnv1a64(&b)
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7) — the M86/M87 lane reads the schedule
/// against the calibration the module doc states: a handful of winters
/// and one-to-two optima per millennium, winters a generation long and
/// optima a century or more, together leaving most years quiet. Ranges
/// set from the law's own draw (gap mean 160 y · winter mean 50 y ·
/// optimum mean 110 y · warm share 0.35 ⇒ cycle ≈ 231 y), with room
/// for seed luck, then confirmed on the report seeds.
pub const BANDS: &[Band] = &[
    Band { name: "cold share of the centuries", sweet: (10.0, 19.0), hard: (6.0, 26.0), target: "sweet 10–19% · hard 6–26% (M86, recalibrated at M87: years under an active winter over the horizon — the timeline now shares its arcs with the optima)" },
    Band { name: "warm share of the centuries", sweet: (11.0, 23.0), hard: (6.0, 30.0), target: "sweet 11–23% · hard 6–30% (M87: years under an active optimum — rarer arcs, each holding longer, so kindness rivals frost in years held)" },
    Band { name: "a winter's generation span", sweet: (42.0, 58.0), hard: (35.0, 65.0), target: "sweet 42–58 y · hard 35–65 (M86: mean cold-arc duration — the draw is uniform 20–80, so the schedule mean stands near 50)" },
    Band { name: "an optimum's century span", sweet: (95.0, 125.0), hard: (75.0, 145.0), target: "sweet 95–125 y · hard 75–145 (M87: mean warm-arc duration — the draw is uniform 60–160, the generous *centuries* of the spec)" },
    Band { name: "winters per millennium", sweet: (2.2, 3.6), hard: (1.6, 4.4), target: "sweet 2.2–3.6 · hard 1.6–4.4 (M86, recalibrated at M87: cold-onset rate over the horizon — the Spörer/Maunder/Dalton cadence on a shared timeline)" },
    Band { name: "optima per millennium", sweet: (1.0, 2.1), hard: (0.6, 2.8), target: "sweet 1.0–2.1 · hard 0.6–2.8 (M87: warm-onset rate — the Minoan/Roman/Medieval cadence, one-to-two a millennium)" },
];
