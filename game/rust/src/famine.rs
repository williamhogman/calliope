//! Famine — the harvest verdict (M2.6), a module like its peers (E11.2).
//!
//! Moved verbatim out of `world.rs`; behavior and event text unchanged.

use crate::agriculture;
use crate::climate;
use crate::resources;
use crate::society;
use crate::world::{Event, EventKind, World};

/// The standardized rainfall anomaly at which a year counts as a failed
/// one: SPI −1, moderate drought (McKee 1993). Shortfall runs from here
/// to SPI −2, extreme drought, where it saturates.
pub const DROUGHT_Z: f64 = -1.0;

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
            let pack = self.fields.crops[[y as usize, x as usize]];
            let rainfed = (pack == agriculture::CropPackage::Wheat.code()
                || pack == agriculture::CropPackage::Maize.code())
                && !river;
            // M92 — the paddies join the verdict: rice whose rain genuinely
            // leans into the monsoon faces the monsoon it actually got. The
            // channel does not exempt a riverine paddy — the pulse that
            // fills it is the monsoon over the whole basin, so it reads the
            // catchment's sky (the M81 gaussian) and fails only when the
            // wider sky does. Wheat and maize on rivers keep the old
            // immunity: channel irrigation is base flow, not the pulse.
            let lean = self.fields.pamp[[y as usize, x as usize]] as f64;
            let paddies = pack == agriculture::CropPackage::Rice.code()
                && lean.abs() >= climate::MONSOON_LEAN_MIN;
            if !(rainfed || paddies) || pop <= 90 {
                continue;
            }
            let z = town_spi[i];
            let (shortfall, msi) = if rainfed {
                if z >= DROUGHT_Z {
                    continue;
                }
                // saturates at SPI −2, the conventional edge of extreme drought
                ((((-z) - (-DROUGHT_Z)) / (-DROUGHT_Z)).min(1.0), 0.0)
            } else {
                // M92 — the failed monsoon: the year delivered this share
                // of a normal monsoon; the shortfall opens at MONSOON_FAIL
                // and saturates at MONSOON_SAT, where the paddies stand dry.
                let msi = self.monsoon_index(year, y as usize, x as usize, river);
                if msi >= climate::MONSOON_FAIL {
                    continue;
                }
                (
                    ((climate::MONSOON_FAIL - msi)
                        / (climate::MONSOON_FAIL - climate::MONSOON_SAT))
                        .min(1.0),
                    msi,
                )
            };
            worst = worst.max(shortfall);
            // granaries (pottery) blunt a failed year
            let granary = if self
                .peoples.societies
                .get(culture.0)
                .map_or(false, |so| so.knows(society::TechId::Pottery))
            {
                0.75
            } else {
                1.0
            };
            let hit = ((pop as f64) * (0.05 + 0.16 * shortfall) * granary) as i64;
            if hit < 4 {
                continue;
            }
            let dead = (hit as f64 * 0.55) as i64;
            let walked = hit - dead;
            self.peoples.settlements[i].pop = (pop - hit).max(30);
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
            });

            // the hungry walk to the nearest kin-town outside the blight —
            // ring search over the bucket grid (E5.3), same winner as the
            // old full scan: nearest by (distance², index)
            let target = town_buckets.nearest(x as f64, y as f64, |j| {
                let o = &self.peoples.settlements[j];
                j != i && o.people == culture && !(town_spi[j] < DROUGHT_Z)
            });
            let text = if let Some((j, _)) = target {
                migrations.push((j, walked));
                if paddies {
                    format!(
                        "The monsoon fails over {} — {} starve among the empty paddies, and {} take the road to {}.",
                        name, dead, walked, self.peoples.settlements[j].name
                    )
                } else {
                    format!(
                        "The rains fail over {} — {} starve, and {} take the road to {}.",
                        name, dead, walked, self.peoples.settlements[j].name
                    )
                }
            } else if paddies {
                format!(
                    "The monsoon fails over {} — {} starve among the empty paddies.",
                    name,
                    dead + walked
                )
            } else {
                format!(
                    "The rains fail over {} — {} starve in the dust of a dead harvest.",
                    name,
                    dead + walked
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
