//! Famine — the harvest verdict (M2.6), a module like its peers (E11.2).
//!
//! Moved verbatim out of `world.rs`; behavior and event text unchanged.

use crate::agriculture;
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
    /// Floodplains irrigate, paddies flood, herders walk to the grass and
    /// fishers never planted — only wheat and maize under open sky can fail.
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
        let rows = self.fields.tmean.dim().0 as f64;
        let dry = |x: i64, y: i64| -> f64 {
            let lat = (-90.0 + (y as f64) * 180.0 / (rows - 1.0)).abs();
            let sigma = crate::climate::anomaly_amp_p(lat).max(1e-6);
            self.with_year_weather(year, |_, dp| dp[[y as usize, x as usize]] / sigma)
        };
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
            if !rainfed || pop <= 90 {
                continue;
            }
            let z = dry(x, y);
            if z >= DROUGHT_Z {
                continue;
            }
            // saturates at SPI −2, the conventional edge of extreme drought
            let shortfall = (((-z) - (-DROUGHT_Z)) / (-DROUGHT_Z)).min(1.0);
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
            // the hungry walk to the nearest kin-town outside the blight —
            // ring search over the bucket grid (E5.3), same winner as the
            // old full scan: nearest by (distance², index)
            let target = town_buckets.nearest(x as f64, y as f64, |j| {
                let o = &self.peoples.settlements[j];
                j != i && o.people == culture && !(dry(o.x, o.y) < DROUGHT_Z)
            });
            let text = if let Some((j, _)) = target {
                migrations.push((j, walked));
                format!(
                    "The rains fail over {} — {} starve, and {} take the road to {}.",
                    name, dead, walked, self.peoples.settlements[j].name
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
