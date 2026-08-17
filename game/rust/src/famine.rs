//! Famine — the harvest verdict (M2.6), a module like its peers (E11.2).
//!
//! Moved verbatim out of `world.rs`; behavior and event text unchanged.

use crate::agriculture;
use crate::resources;
use crate::society;
use crate::world::{Event, EventKind, World};

impl World {
    /// M2.6 — the harvest verdict. Once a year, in the eighth month, every
    /// rain-fed farming town faces the sky it actually got: a deterministic
    /// drought field (seeded noise over space × year) decides where the rains
    /// failed. Failure starves, spikes grain, and sends folk down the roads.
    /// Floodplains irrigate, paddies flood, herders walk to the grass and
    /// fishers never planted — only wheat and maize under open sky can fail.
    pub(crate) fn famine_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 7 {
            return events;
        }
        let year = month_abs / 12;
        let dry = |x: i64, y: i64| -> f64 {
            self.drought
                .fbm(x as f64 * 0.045, y as f64 * 0.045, year as f64 * 0.83, 2)
        };
        let mut migrations: Vec<(usize, i64)> = Vec::new();
        let mut worst = 0.0f64;
        // settlement bucket grid for the kin-town search (E5.3) — positions
        // are fixed for the whole pass, only populations move
        let town_buckets = crate::util::Buckets::build(
            self.settlements.iter().map(|s| (s.x as f64, s.y as f64)).collect(),
            32.0,
        );
        for i in 0..self.settlements.len() {
            let (y, x, pop, culture, river, name) = {
                let s = &self.settlements[i];
                (s.y, s.x, s.pop, s.culture, s.river, s.name.clone())
            };
            let pack = self.crops[[y as usize, x as usize]];
            let rainfed = (pack == agriculture::CropPackage::Wheat.code()
                || pack == agriculture::CropPackage::Maize.code())
                && !river;
            if !rainfed || pop <= 90 {
                continue;
            }
            let d = dry(x, y);
            if d >= -0.30 {
                continue;
            }
            let shortfall = (((-d) - 0.30) / 0.30).min(1.0);
            worst = worst.max(shortfall);
            // granaries (pottery) blunt a failed year
            let granary = if self
                .societies
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
            self.settlements[i].pop = (pop - hit).max(30);
            // the hungry walk to the nearest kin-town outside the blight —
            // ring search over the bucket grid (E5.3), same winner as the
            // old full scan: nearest by (distance², index)
            let target = town_buckets.nearest(x as f64, y as f64, |j| {
                let o = &self.settlements[j];
                j != i && o.culture == culture && !(dry(o.x, o.y) < -0.30)
            });
            let text = if let Some((j, _)) = target {
                migrations.push((j, walked));
                format!(
                    "The rains fail over {} — {} starve, and {} take the road to {}.",
                    name, dead, walked, self.settlements[j].name
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
                ..Default::default()
            });
        }
        for (j, souls) in migrations {
            self.settlements[j].pop += souls;
        }
        // scarcity is priced at once: one grain spike per failed year
        if worst > 0.0 && self.grain_shock_year != year {
            self.grain_shock_year = year;
            self.market.shock(resources::Good::Grain, 1.0 + 0.30 * worst);
        }
        events
    }
}
