//! Famine — the harvest verdict (M2.6), a module like its peers (E11.2).
//!
//! Moved verbatim out of `world.rs`; behavior and event text unchanged
//! through M94. M95 — *Hunger With a Cause* — makes the verdict speak
//! its sky: every famine row carries the numbers the verdict read
//! (`HarvestSky`), the chronicle line ends with the sentence those
//! numbers make, and the explain layer shows the same reading for a
//! town that has not failed. M97 — *Famine, Recalibrated* — is the one
//! time the law itself moved: the toll now opens at a floor of want
//! (`TOLL_FLOOR`), so a failed harvest is a dearth first and a famine
//! only when the want runs deep, and the harness judges the cadence and
//! severity that fall out of it against the pre-industrial record
//! (`BANDS`, per eligible settlement-century).

use crate::agriculture;
use crate::climate;
use crate::drought::MEMO_YEARS;
use crate::resources;
use crate::society;
use crate::world::{Event, EventKind, World};

/// The standardized rainfall anomaly at which a year counts as a failed
/// one: SPI −1, moderate drought (McKee 1993). Shortfall runs from here
/// to SPI −2, extreme drought, where it saturates.
pub const DROUGHT_Z: f64 = -1.0;

// ------------------------------------------------- M97 historical envelope
//
// *Famine, Recalibrated.* Through M96 the harness judged famine by a
// world-total count per century — a gut-feel band that scaled with the
// number of farming towns and said nothing about how often any one town
// starved. M97 replaces it with the quantity the historical record
// actually reports: **spoken famines per eligible settlement-century**
// (a spoken famine is a verdict whose toll reached the chronicle, ≥ 4
// souls struck; an eligible settlement-year is one the pass would have
// weighed — rain-fed grain off the river or monsoon-leaning paddies,
// more than 90 souls). The envelope is drawn from the pre-industrial
// record, low end to high:
//
// - Campbell & Ó Gráda 2011 (*Harvest shortfalls, grain prices and
//   famines in pre-industrial England*, J. Econ. Hist. 71): England
//   1268–1480 knew two true famines (1315–17, 1437–8; the 1290s a
//   near one) — ~1 per century on the least famine-prone farmed land of
//   the record, though back-to-back ≥10 % shortfalls came every ~15
//   years (weighted W-B-O probability 0.066) and ≥30 % ones every ~110
//   (0.009). Their reading: "most historical subsistence crises were the
//   product of back-to-back shortfalls". The floor of the envelope.
// - Ó Gráda 2009 (*Famine: A Short History*, ch. 1): in pre-industrial
//   Europe famine came "perhaps once a generation" — 3–4 per century;
//   Appleby's England 1586–1623 (three crises in forty years, then
//   none) sits there too. The middle of the envelope.
// - Farr 1846 (the "law" the same authors treat as a chronicle-derived
//   overcount): ten famine years per century before 1600; Mallory 1926
//   (*China: Land of Famine*): 1,828 famines in 2,019 years somewhere in
//   China — with ~18 provinces, ~5 per province-century. The ceiling.
// - Hoskins 1964 (*Harvest fluctuations and English economic history
//   1480–1619*): one harvest in six "bad" or "dearth" (≈17 per century).
//   That is the *harvest-failure* cadence, which famine mortality must
//   sit clearly under — the hard ceiling, not the sweet one.
//
// Severity is the mean share of the town that died in a spoken famine.
// Ó Gráda's ledger: 1315–17 England ~10 %, France 1693–4 ~6 %, 1709–10
// ~3 %, Ireland 1740–1 ~13 %, Finland 1696–7 25–33 %; Wrigley &
// Schofield's local "crisis mortality" (deaths ≥10 % above trend) adds a
// percent or two. A famine the chronicle names should take a few
// percent; the great ones a tenth.
//
// The granary tier's own band: Appleby (1978, *Famine in Tudor and
// Stuart England*) dates England's escape from famine to the storage and
// market integration of the 1620s–40s — the storehouse should show a
// band clearly below the bare law's, and below the world's.

/// Spoken famines per eligible settlement-century, all tiers: sweet.
pub const FAMINE_PER_CENTURY: (f64, f64) = (1.0, 12.0);
/// Spoken famines per eligible settlement-century, all tiers: hard.
pub const FAMINE_PER_CENTURY_HARD: (f64, f64) = (0.3, 20.0);
/// Spoken famines per storehouse-tier settlement-century: sweet — the
/// granary's own, smaller band.
pub const STORED_FAMINE_PER_CENTURY: (f64, f64) = (0.3, 8.0);
/// Spoken famines per storehouse-tier settlement-century: hard.
pub const STORED_FAMINE_PER_CENTURY_HARD: (f64, f64) = (0.0, 14.0);
/// Mean dead share of the town per spoken famine (one town-year): sweet.
pub const FAMINE_DEAD_SHARE: (f64, f64) = (0.02, 0.08);
/// Mean dead share of the town per spoken famine (one town-year): hard.
pub const FAMINE_DEAD_SHARE_HARD: (f64, f64) = (0.01, 0.15);
/// Mean dead share of the town per famine *episode* — consecutive famine
/// years at one town merged, the years' shares summed: the unit Ó
/// Gráda's ledger is written in (3 % France 1709 … 10 % England 1315–17,
/// Ireland 1740–1 the outlier at 13). Sweet.
pub const FAMINE_EPISODE_DEAD_SHARE: (f64, f64) = (0.03, 0.10);
/// Mean dead share per famine episode: hard.
pub const FAMINE_EPISODE_DEAD_SHARE_HARD: (f64, f64) = (0.01, 0.16);

/// Diagnostics bands (M97). Rates are per eligible settlement-century,
/// measured by the harness's yearly exposure census against the pass's
/// own ledgers; the world-total M2.6 band they replace is retired.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band {
        name: "famines per settlement-century",
        sweet: FAMINE_PER_CENTURY,
        hard: FAMINE_PER_CENTURY_HARD,
        target: "M97: spoken famines per eligible settlement-century — Campbell & Ó Gráda's England (~1) to Farr's ten-a-century and Mallory's China (~5 per province); Hoskins' one-bad-harvest-in-six (≈17) is the hard ceiling famine must sit under",
    },
    crate::util::Band {
        name: "storehouse famines per settlement-century",
        sweet: STORED_FAMINE_PER_CENTURY,
        hard: STORED_FAMINE_PER_CENTURY_HARD,
        target: "M97: the granary tier's own, smaller band — Appleby's England left famine behind on storage and markets; the storehouse must show it",
    },
    crate::util::Band {
        name: "famine dead share",
        sweet: FAMINE_DEAD_SHARE,
        hard: FAMINE_DEAD_SHARE_HARD,
        target: "M97: mean share of the town dead per spoken famine year — a single harvest's toll; the record's per-famine figures are the episode band below",
    },
    crate::util::Band {
        name: "famine episode dead share",
        sweet: FAMINE_EPISODE_DEAD_SHARE,
        hard: FAMINE_EPISODE_DEAD_SHARE_HARD,
        target: "M97: mean share of the town dead per famine episode (consecutive famine years at a town merged) — Ó Gráda's ledger runs 3 % (France 1709) to 10 % (England 1315–17); Ireland 1740–1 (13 %) is the outlier",
    },
];

// ----------------------------------------------------------------- M95 sky

/// The sky one harvest read, every number of it. `index` is the verdict's
/// own z (the M80 memory index the threshold is applied to); the rest is
/// the *structure* behind that number — this year alone, and how many of
/// the years behind it were dry — so a famine can say whether it was the
/// year that failed or the ground that had already been emptied.
///
/// Pure in seed × cell × year: nothing here reads the tick's memo, so the
/// harness re-derives every field from `climate::year_anomaly_at` alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HarvestSky {
    /// calendar year of the harvest
    pub year: i64,
    /// this year's own standardized rain anomaly (SPI)
    pub z1: f64,
    /// the M80 memory index — the z the verdict thresholds
    pub index: f64,
    /// this year's fractional rain anomaly (`precip * (1 + dp)`)
    pub dp: f64,
    /// consecutive years ending this year with SPI ≤ `DROUGHT_Z`
    pub dry_run: u8,
    /// consecutive years ending this year with SPI < 0
    pub lean_run: u8,
    /// failed years (SPI ≤ `DROUGHT_Z`) among the `MEMO_YEARS − 1`
    /// years behind this one, whether or not they ran unbroken
    pub dry_behind: u8,
}

impl HarvestSky {
    /// The neutral sky, for rows where no sky was read (never emitted by
    /// the pass; the `Default` the ledger needs).
    pub const NONE: HarvestSky = HarvestSky { year: 0, z1: 0.0, index: 0.0, dp: 0.0, dry_run: 0, lean_run: 0, dry_behind: 0 };
}

impl Default for HarvestSky {
    fn default() -> Self {
        HarvestSky::NONE
    }
}

/// What the harvest at a town reads, per `harvest_verdict`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarvestKind {
    /// wheat or maize under open sky — reads the SPI memory index
    RainFed,
    /// monsoon-leaning rice — reads the monsoon index
    Paddies,
    /// wheat or maize on a river — channel irrigation, base flow, immune
    Irrigated,
    /// no farming verdict: herders, fishers, rice without a monsoon lean
    NotFarming,
}

impl HarvestKind {
    pub fn name(self) -> &'static str {
        match self {
            HarvestKind::RainFed => "rain-fed",
            HarvestKind::Paddies => "paddies",
            HarvestKind::Irrigated => "irrigated",
            HarvestKind::NotFarming => "not-farming",
        }
    }
}

/// The harvest reading at one town in one year — the verdict's inputs
/// and what they would decide, for the explain layer and the harness.
/// `shortfall` and `hit` are the pass's own arithmetic (granary applied),
/// zero where the harvest holds. The pass never consults this; it is the
/// same law read without mutation.
#[derive(Clone, Copy, Debug)]
pub struct HarvestVerdict {
    pub kind: HarvestKind,
    pub sky: HarvestSky,
    /// monsoon-strength index on paddies (1.0 = normal), 0.0 elsewhere
    pub msi: f64,
    /// whether the paddies read the catchment's sky (riverine) or the point's
    pub catchment: bool,
    pub shortfall: f64,
    /// M96 — the toll multiplier: the share of the need the store could
    /// not cover (1.0 with no store or an empty one, 0.0 when it held)
    pub granary: f64,
    /// M96 — the town's storage tier (its people's craft)
    pub tier: society::StoreTier,
    /// M96 — grain in the store as the verdict reads it, person-years
    /// (after the year's turn: spoilage, accrual, the roof)
    pub store: f64,
    /// M96 — grain the store gives against the shortfall, person-years
    pub covered: f64,
    /// souls the verdict would take at the town's current population
    pub hit: i64,
    /// the verdict would fail *and* take enough to be spoken (`hit ≥ 4`)
    pub fails: bool,
}

// ------------------------------------------------------- M97 the toll law
//
// Through M96 the toll was `pop × (0.05 + 0.16 × shortfall) × granary`:
// the moment the memory index crossed SPI −1 a twentieth of the town was
// struck, whatever the depth, so every failed harvest was a famine and
// the rain-fed cadence ran at Hoskins' harvest-failure rate (measured
// 16.1 spoken per settlement-century on seed 12345 over 150 y, against
// a record whose famine counts top out near 10). The record keeps the
// two apart: a *dearth* — a bad harvest, high prices, belts tightened —
// is common; a *famine* — people die — needs the want to run deep.
// Campbell & Ó Gráda find true famines only where the net shortfall
// reached a quarter to a third of a year's bread, almost always
// back-to-back (and the M80 memory index already sums the years behind
// a harvest, so a run of thin years reads here as one deep want).
//
// M97 puts that floor in the law. Mortality opens once the town is short
// more than `TOLL_FLOOR` of a year's grain *after the store has given*
// (`shortfall × granary`, the want the granary left), and climbs
// linearly to `TOLL_RATE` of the town at a whole year's want. Below the
// floor the year is lean, not lethal: the store still draws, grain still
// spikes, the chronicle stays quiet. The granary therefore does what
// Appleby's storehouses did — it does not thin a famine, it prevents
// one, and the M96 gates measure exactly that.
//
// The rate was set from the record's own unit, the *episode*. The
// chronicle speaks a famine per town-year, but Ó Gráda's percentages are
// per famine — 1315–17 is one entry, three harvests long — so the
// harness merges consecutive famine years at a town and reads the dead
// share summed across them. At the old law's saturation (a fifth of the
// town struck, 11.5 % dead per year) the measured episode ran 10–12 %
// dead on every seed: the mean famine was England's Great Famine, the
// worst line in the ledger. At `TOLL_RATE` 0.15 the saturating year
// takes 8.25 % and the episode the ledger's middle (France 1693–4's
// six, back-to-back years reaching 1315–17's ten) — `FAMINE_EPISODE_
// DEAD_SHARE` holds it there.

/// The want, as a share of a year's grain after the store has given, at
/// which people begin to die. Half a year's bread: the deficit Campbell
/// & Ó Gráda's true famines reached (a quarter to a third, back-to-back).
pub const TOLL_FLOOR: f64 = 0.5;
/// The share of the town struck (dead or on the road) at a whole year's
/// want — the saturating famine year.
pub const TOLL_RATE: f64 = 0.15;
/// The share of the struck who die; the rest take the road to kin.
pub const DEAD_SHARE: f64 = 0.55;

/// The toll: the souls the want takes at a town of `pop`, the store's
/// uncovered share `granary` applied to the harvest `shortfall` before
/// the floor (M97). Zero below the floor — a lean year — and `TOLL_RATE`
/// of the town at a whole year's want.
pub fn toll(pop: i64, shortfall: f64, granary: f64) -> i64 {
    let want = (shortfall * granary - TOLL_FLOOR) / (1.0 - TOLL_FLOOR);
    if want <= 0.0 {
        return 0;
    }
    ((pop as f64) * TOLL_RATE * want.min(1.0)) as i64
}

/// The dead among the struck; the remainder walk.
pub fn dead_of(hit: i64) -> i64 {
    (hit as f64 * DEAD_SHARE) as i64
}

/// The want the store left, as a share of a year's grain: the number the
/// toll reads. `shortfall × granary`.
pub fn want_of(shortfall: f64, granary: f64) -> f64 {
    shortfall * granary
}

/// M97 — the dearth, spoken: a shortfall whose want stayed under the
/// floor. The inspector's sentence for the lean year that is not a
/// famine; `spoken` chooses the tense (verdict past or ahead).
pub fn dearth_sentence(shortfall: f64, want: f64, spoken: bool) -> String {
    let short = ((shortfall * 100.0).round() as i64).clamp(1, 100);
    let left = ((want * 100.0).round() as i64).clamp(0, 100);
    let floor = (TOLL_FLOOR * 100.0).round() as i64;
    if spoken {
        format!(
            "The harvest fell {} in every hundred short — a dearth, not a famine: the want the store left ({} in every hundred) stayed under the {} at which people die.",
            short, left, floor
        )
    } else {
        format!(
            "The harvest ahead falls {} in every hundred short — a dearth, not a famine: the want the store would leave ({} in every hundred) stays under the {} at which people die.",
            short, left, floor
        )
    }
}

// --------------------------------------------------------------- M96 store

/// M96 — the storehouse's turn, once a year before the verdict: what the
/// year did to a town's store. Spoilage first (the pile the winter left),
/// then the fat year's levy — the tier's share of the surplus over the
/// town's own need (`(yield − 1)⁺ × pop` person-years; a yield of 1.3 is
/// three parts in ten more grain than the town eats) — then the roof:
/// nothing above `cap_years × pop` keeps. A people with no craft of
/// keeping has no store: whatever was there when the craft was lost
/// (a town turned kindred to another people) spoils at once. Kept to a
/// thousandth of a person-year so the identity line is exact.
pub fn store_turn(store: f64, tier: society::StoreTier, pop: i64, harvest: f64) -> f64 {
    if tier == society::StoreTier::None {
        return 0.0;
    }
    let kept = store * (1.0 - tier.spoil());
    let surplus = (harvest - 1.0).max(0.0) * (pop as f64);
    let laid = kept + tier.share() * surplus;
    crate::util::round3(laid.min(tier.cap_years() * (pop as f64)))
}

/// M96 — the draw: against a shortfall the town needs `shortfall × pop`
/// person-years of grain; the store gives what it has up to that need,
/// and the toll is multiplied by the share it could *not* cover. Returns
/// `(covered, granary)`: covered in person-years, granary the M2.6
/// multiplier (exactly 0.0 when the store meets the need, exactly 1.0
/// when it is empty — no rounding at either end).
pub fn store_draw(store: f64, pop: i64, shortfall: f64) -> (f64, f64) {
    let need = shortfall * (pop as f64);
    if need <= 0.0 {
        return (0.0, 1.0);
    }
    if store >= need {
        (need, 0.0)
    } else if store <= 0.0 {
        (0.0, 1.0)
    } else {
        (store, (need - store) / need)
    }
}

/// Months of grain a store holds for a town: `12 × store / pop`, rounded
/// to the nearest month — the number the chronicle and the card speak.
pub fn store_months(store: f64, pop: i64) -> i64 {
    if pop <= 0 {
        return 0;
    }
    (12.0 * store / (pop as f64)).round() as i64
}

/// M96 — the clause a famine line carries when the store gave something
/// but not enough: how many months of grain it put against the need.
pub fn gave_sentence(tier: society::StoreTier, covered: f64, pop: i64) -> String {
    let months = store_months(covered, pop);
    if months <= 0 {
        format!("The {} gave what little they held.", tier.name())
    } else if months == 1 {
        format!("The {} gave a month of grain against it.", tier.name())
    } else {
        format!("The {} gave {} months of grain against it.", tier.name(), months)
    }
}

/// M96 — the line a held famine speaks: the sky failed, the store did not.
pub fn held_sentence(tier: society::StoreTier, covered: f64, pop: i64) -> String {
    let months = store_months(covered, pop);
    if months <= 1 {
        format!("but the {} hold — a month of grain sees the town through.", tier.name())
    } else {
        format!("but the {} hold — {} months of grain see the town through.", tier.name(), months)
    }
}

/// Rain-fed shortfall from the memory index: opens at SPI −1, saturates
/// at SPI −2. `None` where the harvest holds.
pub fn rainfed_shortfall(index: f64) -> Option<f64> {
    if index >= DROUGHT_Z {
        None
    } else {
        Some((((-index) - (-DROUGHT_Z)) / (-DROUGHT_Z)).min(1.0))
    }
}

/// Paddy shortfall from the monsoon index: opens at `MONSOON_FAIL`,
/// saturates at `MONSOON_SAT`. `None` where the monsoon came.
pub fn monsoon_shortfall(msi: f64) -> Option<f64> {
    if msi >= climate::MONSOON_FAIL {
        None
    } else {
        Some(((climate::MONSOON_FAIL - msi) / (climate::MONSOON_FAIL - climate::MONSOON_SAT)).min(1.0))
    }
}

const ORDINALS: [&str; 13] = ["zeroth", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh", "twelfth"];

fn ordinal(n: u8) -> &'static str {
    ORDINALS[(n as usize).min(ORDINALS.len() - 1)]
}

/// The rain this year, as the chronicle says it: whole parts in a hundred
/// short of (or above) the norm. Rounded half away from zero on the
/// magnitude so "−0.5 %" and "+0.5 %" both read as one part, not none.
pub fn rain_pct(dp: f64) -> i64 {
    (dp.abs() * 100.0).round() as i64
}

/// M95 — the sentence a rain-fed famine ends with: the reading of the
/// sky that failed it, in the chronicle's register. Pure in the sky, so
/// the harness can regenerate it from re-derived numbers and demand the
/// telling carry it verbatim.
///
/// Three shapes, by what actually failed:
/// - the year itself failed (SPI ≤ −1): how many dry years running, and
///   how short this one came;
/// - the year held but failed years behind it emptied the ground (the
///   M80 carry): the year near its norm, the count behind it;
/// - no single year failed outright, the memory summed lean years into
///   a drought: the lean run.
pub fn cause_sentence(sky: &HarvestSky) -> String {
    let pct = rain_pct(sky.dp);
    if sky.z1 <= DROUGHT_Z {
        let short = if pct == 0 {
            "the rains barely short of their norm".to_string()
        } else {
            format!("the rains {} in every hundred short of their norm", pct)
        };
        if sky.dry_run >= 2 {
            format!("It was the {} dry year running, {}.", ordinal(sky.dry_run), short)
        } else {
            format!("It was a dry year, {}.", short)
        }
    } else if sky.dry_behind >= 1 {
        let this_year = if sky.dp < 0.0 && pct > 0 {
            format!("The year itself came only {} in every hundred short", pct)
        } else {
            "The year itself came near its norm".to_string()
        };
        if sky.dry_behind == 1 {
            format!("{}, but a failed year behind it had emptied the ground.", this_year)
        } else {
            format!("{}, but {} failed years behind it had emptied the ground.", this_year, sky.dry_behind)
        }
    } else if sky.lean_run >= 2 {
        format!(
            "No single year failed outright, but {} lean years running had emptied the ground.",
            sky.lean_run
        )
    } else {
        // the memory index below −1 with no failed year behind and no
        // lean run: lean years scattered through the window did it
        "No single year failed outright, but the lean years behind it had emptied the ground.".to_string()
    }
}

/// M95 — the sentence a monsoon famine ends with: the share of a normal
/// monsoon the year delivered, and whether the basin or the point read it.
pub fn monsoon_sentence(msi: f64, catchment: bool) -> String {
    let share = (msi.max(0.0) * 100.0).round() as i64;
    if catchment {
        format!("The monsoon came at {} parts in a hundred of its norm over the whole basin.", share)
    } else {
        format!("The monsoon came at {} parts in a hundred of its norm.", share)
    }
}

impl World {
    /// M95 — the sky one harvest reads at a cell, from the law alone:
    /// this year's SPI, the memory index, and the run structure of the
    /// `MEMO_YEARS` window behind it. Pure in seed × cell × year; prior
    /// years are solved raw so the tick's year memo is never evicted.
    pub fn harvest_sky(&self, year: i64, y: usize, x: usize) -> HarvestSky {
        let sigma = self.spi_sigma(y);
        let dp = self.year_rain_anomaly_raw(year, y, x);
        let z1 = dp / sigma;
        let index = self.drought_index(year, y, x);
        let mut dry_run: u8 = 0;
        let mut lean_run: u8 = 0;
        let mut dry_behind: u8 = 0;
        let mut dry_unbroken = true;
        let mut lean_unbroken = true;
        for k in 0..MEMO_YEARS as i64 {
            let zk = if k == 0 { z1 } else { self.year_rain_anomaly_raw(year - k, y, x) / sigma };
            let dry = zk <= DROUGHT_Z;
            if dry_unbroken && dry {
                dry_run += 1;
            } else {
                dry_unbroken = false;
            }
            if lean_unbroken && zk < 0.0 {
                lean_run += 1;
            } else {
                lean_unbroken = false;
            }
            if k > 0 && dry {
                dry_behind += 1;
            }
        }
        HarvestSky { year, z1, index, dp, dry_run, lean_run, dry_behind }
    }

    /// M96 — the storage tier settlement `i` holds: its people's craft.
    pub fn store_tier_of(&self, i: usize) -> society::StoreTier {
        let s = &self.peoples.settlements[i];
        self.peoples.societies.get(s.people.0).map_or(society::StoreTier::None, |so| so.store_tier())
    }

    /// M96 — whether a town's fields fill a store at all: grain and rice
    /// packages do (open-sky, irrigated or paddy); herders and fishers
    /// have no harvest to lay by, whatever their people's craft.
    pub fn store_fills(&self, y: usize, x: usize) -> bool {
        let pack = self.fields.crops[[y, x]];
        pack == agriculture::CropPackage::Wheat.code()
            || pack == agriculture::CropPackage::Maize.code()
            || pack == agriculture::CropPackage::Rice.code()
    }

    /// M96 — the store as the verdict of `year` reads it, without moving
    /// it: the live store when the year's turn has already run, else the
    /// live store put through the turn read-only (the same `store_turn`
    /// the pass applies, at the same harvest). Years before the current
    /// one are not reconstructed — the twin speaks of the year at hand.
    pub fn store_at_verdict(&self, i: usize, year: i64) -> f64 {
        let s = &self.peoples.settlements[i];
        if self.store_year >= year {
            return s.store;
        }
        let tier = self.store_tier_of(i);
        let (y, x) = (s.y as usize, s.x as usize);
        let harvest = if self.store_fills(y, x) { self.year_yield(year, y, x) } else { 1.0 };
        store_turn(s.store, tier, s.pop, harvest)
    }

    /// M96 — the storehouse's turn for every town, once a year before the
    /// verdict: spoilage, the fat year's levy, the roof. Idempotent per
    /// year (`store_year` guards a chunked tick).
    pub(crate) fn granary_turn(&mut self, year: i64) {
        if self.store_year >= year {
            return;
        }
        self.store_year = year;
        for i in 0..self.peoples.settlements.len() {
            let tier = self.store_tier_of(i);
            let (y, x, pop, store) = {
                let s = &self.peoples.settlements[i];
                (s.y as usize, s.x as usize, s.pop, s.store)
            };
            let harvest = if tier != society::StoreTier::None && self.store_fills(y, x) {
                self.year_yield(year, y, x)
            } else {
                1.0
            };
            let next = store_turn(store, tier, pop, harvest);
            if next != store {
                self.peoples.settlements[i].store = next;
            }
        }
    }

    /// M97 — what the harvest at settlement `i` reads: the eligibility
    /// predicate the pass, the explain layer and the harness's exposure
    /// census all share. Wheat or maize under open sky reads the rain;
    /// monsoon-leaning rice reads the monsoon; grain on a river is
    /// irrigated and immune; everything else has no farming verdict.
    /// Population is not part of it — the pass's `> 90` floor is applied
    /// by whoever asks, so a town under the floor is still "rain-fed".
    pub fn harvest_kind(&self, i: usize) -> HarvestKind {
        let s = &self.peoples.settlements[i];
        let (y, x) = (s.y as usize, s.x as usize);
        let pack = self.fields.crops[[y, x]];
        let grain = pack == agriculture::CropPackage::Wheat.code() || pack == agriculture::CropPackage::Maize.code();
        let lean = self.fields.pamp[[y, x]] as f64;
        let paddies = pack == agriculture::CropPackage::Rice.code() && lean.abs() >= climate::MONSOON_LEAN_MIN;
        if grain && !s.river {
            HarvestKind::RainFed
        } else if paddies {
            HarvestKind::Paddies
        } else if grain {
            HarvestKind::Irrigated
        } else {
            HarvestKind::NotFarming
        }
    }

    /// M95 — the harvest reading at settlement `i` for `year`: the same
    /// predicate, the same sky and the same arithmetic as `famine_pass`,
    /// read without moving anyone. The explain layer's "Sky this year"
    /// and the harness's parity check both come through here.
    pub fn harvest_verdict(&self, i: usize, year: i64) -> HarvestVerdict {
        let s = &self.peoples.settlements[i];
        let (y, x) = (s.y as usize, s.x as usize);
        let kind = self.harvest_kind(i);
        let sky = self.harvest_sky(year, y, x);
        // M96 — the store the verdict reads: the people's craft sets the
        // tier, the town's own fat years set the pile.
        let tier = self.store_tier_of(i);
        let store = self.store_at_verdict(i, year);
        let (msi, shortfall) = match kind {
            HarvestKind::RainFed => (0.0, rainfed_shortfall(sky.index)),
            HarvestKind::Paddies => {
                let msi = self.monsoon_index(year, y, x, s.river);
                (msi, monsoon_shortfall(msi))
            }
            _ => (0.0, None),
        };
        let eligible = matches!(kind, HarvestKind::RainFed | HarvestKind::Paddies) && s.pop > 90;
        let sf = shortfall.unwrap_or(0.0);
        let (covered, granary) = if eligible && shortfall.is_some() { store_draw(store, s.pop, sf) } else { (0.0, 1.0) };
        let hit = if eligible && shortfall.is_some() { toll(s.pop, sf, granary) } else { 0 };
        HarvestVerdict {
            kind,
            sky,
            msi,
            catchment: kind == HarvestKind::Paddies && s.river,
            shortfall: sf,
            granary,
            tier,
            store,
            covered,
            hit,
            fails: eligible && shortfall.is_some() && hit >= 4,
        }
    }
}


impl World {
    /// M2.6 — the harvest verdict. Once a year, in the eighth month, every
    /// rain-fed farming town faces the sky it actually got: a deterministic
    /// standardized rain anomaly (SPI over the M71 sky) decides where the rains
    /// failed. Failure starves, spikes grain, and sends folk down the roads.
    /// Floodplains irrigate, herders walk to the grass and fishers never
    /// planted — wheat and maize under open sky fail on the SPI, and since
    /// M92 the monsoon-fed paddies fail with the monsoon itself.
    pub(crate) fn famine_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 7 {
            return events;
        }
        let year = month_abs / 12;
        // M96 — the storehouses turn before the verdict: the winter's
        // spoilage, the fat year's levy, the roof — so what the verdict
        // draws on is the store as it stands this harvest.
        self.granary_turn(year);
        // M72 — one sky. The failed year is no longer a private die: it is
        // *the year's own rain*, read as a standardized anomaly (SPI, McKee
        // 1993) against the interannual spread this latitude actually
        // carries. z ≤ −1 is meteorological drought anywhere on Earth, and
        // because the spread is latitude-shaped the same threshold means
        // the same thing in the tropics and on the steppe.
        // Read every inhabited point before populations move. Besides keeping
        // the later mutation walk borrow-clean, this pays for each town's sky
        // once and lets the kin search reuse the same standardized value.
        let town_spi: Vec<f64> = self
            .peoples
            .settlements
            .iter()
            .map(|s| {
                // M80 — the ground remembers. What decides the harvest is
                // no longer this year's rain alone but the accumulated
                // shortfall of the years behind it (`drought::MEM`),
                // renormalized so the SPI threshold keeps its meaning.
                self.drought_index(year, s.y as usize, s.x as usize)
            })
            .collect();
        let mut migrations: Vec<(usize, i64)> = Vec::new();
        let mut worst = 0.0f64;
        // settlement bucket grid for the kin-town search (E5.3) — positions
        // are fixed for the whole pass, only populations move
        let town_buckets = crate::util::Buckets::build(
            self.peoples.settlements.iter().map(|s| (s.x as f64, s.y as f64)).collect(),
            32.0,
        );
        for i in 0..self.peoples.settlements.len() {
            let (y, x, pop, culture, river, name) = {
                let s = &self.peoples.settlements[i];
                (s.y, s.x, s.pop, s.people, s.river, s.name.clone())
            };
            // M92 — the paddies join the verdict: rice whose rain genuinely
            // leans into the monsoon faces the monsoon it actually got. The
            // channel does not exempt a riverine paddy — the pulse that
            // fills it is the monsoon over the whole basin, so it reads the
            // catchment's sky (the M81 gaussian) and fails only when the
            // wider sky does. Wheat and maize on rivers keep the old
            // immunity: channel irrigation is base flow, not the pulse.
            // M97 — one predicate (`harvest_kind`), shared with the explain
            // layer and the harness's exposure census.
            let kind = self.harvest_kind(i);
            let rainfed = kind == HarvestKind::RainFed;
            let paddies = kind == HarvestKind::Paddies;
            if !(rainfed || paddies) || pop <= 90 {
                continue;
            }
            let z = town_spi[i];
            let (shortfall, msi) = if rainfed {
                // saturates at SPI −2, the conventional edge of extreme drought
                match rainfed_shortfall(z) {
                    Some(sf) => (sf, 0.0),
                    None => continue,
                }
            } else {
                // M92 — the failed monsoon: the year delivered this share
                // of a normal monsoon; the shortfall opens at MONSOON_FAIL
                // and saturates at MONSOON_SAT, where the paddies stand dry.
                let msi = self.monsoon_index(year, y as usize, x as usize, river);
                match monsoon_shortfall(msi) {
                    Some(sf) => (sf, msi),
                    None => continue,
                }
            };
            worst = worst.max(shortfall);
            // M96 — the store against the lean year. Through M95 a people
            // that knew Pottery paid a flat three-quarters of every toll,
            // whatever they had actually laid by; now the town's own store
            // (filled from its own fat years, thinned by its own winters,
            // roofed by its people's craft) gives what it holds against
            // the need, and the toll is multiplied by the share it could
            // not cover. A full store takes the year without a death; an
            // empty one is the bare law.
            let tier = self.store_tier_of(i);
            let store = self.peoples.settlements[i].store;
            let (covered, granary) = store_draw(store, pop, shortfall);
            let hit = toll(pop, shortfall, granary);
            let bare = toll(pop, shortfall, 1.0);
            if covered > 0.0 {
                self.peoples.settlements[i].store = crate::util::round3(store - covered);
            }
            let spoken = hit >= 4;
            // M96 — the lean years' ledger: every verdict that found a
            // shortfall, spoken or held, with the store's draw beside the
            // bare toll — the matched control the gate reads.
            self.store_ledger.push(crate::world::StoreRow {
                m: month_abs,
                x,
                y,
                tier: tier.code(),
                pop,
                shortfall,
                store,
                covered,
                granary,
                hit,
                bare,
                spoken,
            });
            if !spoken {
                // M96 — the year the store held: a shortfall the bare law
                // would have spoken (four or more souls), taken without a
                // death because the granary met it. Told once, as fortune,
                // with the same cause the famine would have carried.
                if bare >= 4 && covered > 0.0 {
                    let sky = self.harvest_sky(year, y as usize, x as usize);
                    let cause = if paddies { monsoon_sentence(msi, river) } else { cause_sentence(&sky) };
                    let held = held_sentence(tier, covered, pop);
                    let text = if paddies {
                        format!("The monsoon fails over {}, {} {}", name, held, cause)
                    } else {
                        format!("The rains fail over {}, {} {}", name, held, cause)
                    };
                    events.push(Event {
                        m: month_abs,
                        s: name,
                        k: EventKind::Granary,
                        text,
                        x,
                        y,
                        ..Default::default()
                    });
                }
                continue;
            }
            let dead = dead_of(hit);
            let walked = hit - dead;
            self.peoples.settlements[i].pop = (pop - hit).max(30);
            // M95 — the sky this verdict read, solved once the town has
            // actually failed (the run structure costs MEMO_YEARS raw
            // draws; a held harvest never pays it). Its `index` is the
            // very `z` above — same law, same arguments — and the harness
            // holds the two bit-equal.
            let sky = self.harvest_sky(year, y as usize, x as usize);
            // M72 — the pass's own ledger row: the numbers it actually used,
            // observed where they are computed. No behaviour rides on this.
            self.famine_ledger.push(crate::world::FamineRow {
                m: month_abs,
                x,
                y,
                pop,
                z,
                shortfall,
                granary,
                hit,
                dead,
                monsoon: paddies,
                msi,
                sky,
                tier: tier.code(),
                store,
                covered,
            });

            // the hungry walk to the nearest kin-town outside the blight —
            // ring search over the bucket grid (E5.3), same winner as the
            // old full scan: nearest by (distance², index)
            let target = town_buckets.nearest(x as f64, y as f64, |j| {
                let o = &self.peoples.settlements[j];
                j != i && o.people == culture && !(town_spi[j] < DROUGHT_Z)
            });
            // M95 — the telling ends with its cause: the sentence the sky's
            // own numbers make. The toll stays first (the harness reads the
            // first number of a famine line as its dead). M96 — when the
            // store gave something but not enough, the line says so after
            // the cause: what the jars put against the need.
            let mut cause = if paddies { monsoon_sentence(msi, river) } else { cause_sentence(&sky) };
            if covered > 0.0 {
                cause.push(' ');
                cause.push_str(&gave_sentence(tier, covered, pop));
            }
            let text = if let Some((j, _)) = target {
                migrations.push((j, walked));
                if paddies {
                    format!(
                        "The monsoon fails over {} — {} starve among the empty paddies, and {} take the road to {}. {}",
                        name, dead, walked, self.peoples.settlements[j].name, cause
                    )
                } else {
                    format!(
                        "The rains fail over {} — {} starve, and {} take the road to {}. {}",
                        name, dead, walked, self.peoples.settlements[j].name, cause
                    )
                }
            } else if paddies {
                format!(
                    "The monsoon fails over {} — {} starve among the empty paddies. {}",
                    name,
                    dead + walked,
                    cause
                )
            } else {
                format!(
                    "The rains fail over {} — {} starve in the dust of a dead harvest. {}",
                    name,
                    dead + walked,
                    cause
                )
            };
            events.push(Event {
                m: month_abs,
                s: name,
                k: EventKind::Famine,
                text,
                x,
                y,
                ..Default::default()
            });
        }
        for (j, souls) in migrations {
            self.peoples.settlements[j].pop += souls;
        }
        // scarcity is priced at once: one grain spike per failed year
        if worst > 0.0 && self.grain_shock_year != year {
            self.grain_shock_year = year;
            self.economy.market.shock(resources::Good::Grain, 1.0 + 0.30 * worst);
        }
        events
    }
}

// --------------------------------------------------------------- M82 zones
//
// Calibrated against the past: the return-time diagnostic judges drought
// and flood recurrence per climate zone against envelopes the paleoclimate
// record would recognize. The taxonomy lives here, beside the harvest
// verdict that consumes the dry side of it: six classes on the two fields
// every cell already carries — cold before dry (a polar desert is polar
// first; potential evapotranspiration, not rain, is its law), then the
// UNEP aridity cuts (arid < 250 mm, semi-arid 250–500 mm), then the warm
// split at 20 °C, the biome lattice's own tropical edge (TEMP_EDGES).

/// The six climate zones of the return-time table, in classifier order.
pub const ZONES: &[&str] = &["polar", "boreal", "arid", "semi-arid", "temperate", "tropical"];

/// The climate zone of a cell, from annual mean temperature (°C) and
/// annual precipitation (mm/y) — the same fields the biome lattice reads.
pub fn zone_of(tmean_c: f64, precip_mm: f64) -> usize {
    if tmean_c < -2.0 {
        0 // polar
    } else if tmean_c < 5.0 {
        1 // boreal
    } else if precip_mm < 250.0 {
        2 // arid
    } else if precip_mm < 500.0 {
        3 // semi-arid
    } else if tmean_c < 20.0 {
        4 // temperate
    } else {
        5 // tropical
    }
}

/// M82 — the Earth envelope for held droughts: acceptable per-place
/// return time in years, per zone (`ZONES` order), of a node on the
/// drought lattice crossing from free into held ground.
///
/// The event class is M80's: an accumulated multi-year deficit that
/// takes hold, keeps its footprint through hysteresis, and earns a
/// name — the *sustained regime* class of the paleo record, not the
/// single-season SPI dip. Anchors: tree-ring PDSI reconstructions put
/// multi-year drought regimes at 2–5 per century over the dry
/// mid-latitudes (Cook et al. 2007 — per-place return ~20–50 y),
/// SPI-run climatologies read 1–2 moderate-or-worse events per decade
/// where single seasons count (Spinoni et al. 2014 — return 5–10 y),
/// and held multi-year events sit between and beyond by zone: dry
/// lands slip into sustained deficit far more often than humid ones,
/// and the poleward zones' tiny precipitation totals make any
/// standardized index read wide. The envelopes span the class
/// honestly rather than pinning one paper's number.
pub const DROUGHT_RETURN: &[(f64, f64)] = &[
    (20.0, 400.0), // polar — thin totals, noisy index; wide by design
    (15.0, 250.0), // boreal
    (8.0, 100.0),  // arid — sustained deficit is the steppe's own weather
    (8.0, 100.0),  // semi-arid
    (12.0, 200.0), // temperate
    (12.0, 200.0), // tropical — monsoon failure clusters, then relents
];
