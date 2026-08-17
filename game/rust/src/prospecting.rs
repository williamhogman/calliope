//! Prospecting and depletion — the discovery economy (ADR-0011), a module
//! like its peers (E11.2). Moved verbatim out of `world.rs`.

use crate::resources;
use crate::settlements;
use crate::trade;
use crate::world::{Event, EventKind, World};

impl World {
    /// Prospectors comb the hinterlands for hidden seams; worked mines thin
    /// toward exhaustion. Returns events and whether the deposit list changed.
    pub(crate) fn prospect_and_deplete(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        use rand::Rng;
        let mut events = Vec::new();
        let mut changed = false;
        let mods: Vec<society::Mods> = self.societies.iter().map(society::mods_for).collect();

        // --- discovery: hidden seams within a town's ranging distance may
        // come to light; rarer metals hide longer, better arts search wider.
        // Beyond the home range there is a second, thinner channel — the far
        // venture: prospecting parties that push into wild country, so even
        // mountain gold no town can reach is found in the fullness of time.
        //
        // E5.3: the deposit bucket grid inverts the old O(deposits × towns)
        // scan — each town only touches seams inside its own venture range.
        // Beyond 3.5 ranges the chance is zero, so an uncovered deposit and
        // a covered-but-too-far one are the same outcome (no rng drawn) and
        // the random stream is untouched.
        let deposit_buckets = crate::util::Buckets::build(
            self.deposits.iter().map(|d| (d.x as f64, d.y as f64)).collect(),
            32.0,
        );
        let mut best: Vec<Option<(f64, usize)>> = vec![None; self.deposits.len()];
        let mut cand: Vec<usize> = Vec::new();
        for (si, s) in self.settlements.iter().enumerate() {
            let reach = settlements::territory_radius(s.pop)
                * 2.4
                * mods.get(s.culture.idx()).map(|m| m.prospecting).unwrap_or(1.0);
            let reach = reach.max(1e-9);
            deposit_buckets.candidates(s.x as f64, s.y as f64, 3.5 * reach + 1.0, &mut cand);
            for &di in &cand {
                let d = &self.deposits[di];
                if d.known {
                    continue;
                }
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                let ratio = (dx * dx + dy * dy).sqrt() / reach;
                if best[di].map_or(true, |(b, _)| ratio < b) {
                    best[di] = Some((ratio, si));
                }
            }
        }
        let mut found: Vec<(usize, usize)> = Vec::new();
        for (di, d) in self.deposits.iter().enumerate() {
            if d.known {
                continue;
            }
            let Some((ratio, si)) = best[di] else { continue };
            let rarity = match d.r.abundance() {
                resources::Abundance::Uncommon => 0.6,
                resources::Abundance::Rare => 0.35,
                resources::Abundance::Legendary => 0.12,
                resources::Abundance::Common => 1.0,
            };
            let p = if ratio <= 1.0 {
                0.012 * rarity * (1.0 - 0.65 * ratio) // combing the home range
            } else if ratio <= 3.5 {
                0.0018 * rarity // a far venture into unclaimed country
            } else {
                0.0
            };
            if p > 0.0 && self.rng.gen::<f64>() < p {
                found.push((di, si));
            }
        }
        for (di, si) in found {
            self.deposits[di].known = true;
            changed = true;
            let kind = self.deposits[di].r;
            let rich = self.deposits[di].rich;
            let precious = matches!(
                kind,
                resources::Good::Silver | resources::Good::Gold | resources::Good::Mithril
            );
            // the market feels the strike before the first cart arrives —
            // hardest where the ore actually is (M5.2: local first)
            self.market.shock(kind, if precious { 0.88 } else { 0.84 });
            let ka = self.areas.area_of(si);
            if let Some(mk) = self.areas.markets.get_mut(ka) {
                mk.shock(kind, if precious { 0.80 } else { 0.75 });
            }
            let sname = self.settlements[si].name.clone();
            let strike = 12.0 + 45.0 * rich * economy::base_value(kind);
            self.settlements[si].wealth = round2(self.settlements[si].wealth + strike);
            if precious {
                // a rush: prospectors, chancers and mule-trains pour in
                let influx = ((self.settlements[si].pop as f64 * 0.05) as i64).max(10);
                self.settlements[si].pop += influx;
            }
            let text = match kind {
                resources::Good::Gold => format!(
                    "Gold! Panners out of {} lift bright dust from the gravels — word runs faster than horses.",
                    sname
                ),
                resources::Good::Silver => format!(
                    "A silver seam glitters by torchlight in the diggings above {}.",
                    sname
                ),
                resources::Good::Mithril => format!(
                    "Beneath the mountain-roots, miners of {} strike mithril — truesilver, the dream of every smith.",
                    sname
                ),
                _ => format!(
                    "Prospectors out of {} strike {} in the hills; the seam runs deep and true.",
                    sname, kind
                ),
            };
            events.push(Event {
                m: month_abs,
                s: sname,
                k: EventKind::Discovery,
                text,
                ..Default::default()
            });
            self.refresh_goods_near(di);
        }

        // --- depletion: a worked seam only lasts so many carts.
        // E5.3 inverted: each town spreads its crews over the seams inside
        // its own work radius. The outer loop stays in settlement order, so
        // every seam's crew sum accumulates in the same order as the old
        // per-deposit scan — float-identical totals.
        let mut crews_by: Vec<f64> = vec![0.0; self.deposits.len()];
        for s in &self.settlements {
            let r = settlements::work_radius(s.pop);
            deposit_buckets.candidates(s.x as f64, s.y as f64, r + 1.0, &mut cand);
            for &di in &cand {
                let d = &self.deposits[di];
                if !d.known || d.left <= 0.0 || !s.goods.iter().any(|g| *g == d.r) {
                    continue;
                }
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                if dx * dx + dy * dy <= r * r {
                    crews_by[di] += 1.0 + (s.pop as f64 / 9000.0).min(1.0);
                }
            }
        }
        let mut spent: Vec<usize> = Vec::new();
        for di in 0..self.deposits.len() {
            let crews = crews_by[di];
            if crews == 0.0 {
                continue;
            }
            let d = &mut self.deposits[di];
            d.left = round2((d.left - crews).max(0.0));
            if d.left == 0.0 {
                spent.push(di);
            }
        }
        for di in spent {
            changed = true;
            let kind = self.deposits[di].r;
            let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
            let near_i = self
                .settlements
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.x - dx0).pow(2) + (s.y - dy0).pow(2))
                .map(|(i, _)| i);
            let near = near_i
                .map(|i| self.settlements[i].name.clone())
                .unwrap_or_else(|| "the wilds".to_string());
            self.market.shock(kind, 1.22);
            // the pit's own market feels the silence first (M5.2)
            if let Some(i) = near_i {
                let ka = self.areas.area_of(i);
                if let Some(mk) = self.areas.markets.get_mut(ka) {
                    mk.shock(kind, 1.35);
                }
            }
            events.push(Event {
                m: month_abs,
                s: near.clone(),
                k: EventKind::Depletion,
                text: format!(
                    "The last good ore comes up from the {} pits near {}; the galleries fall silent.",
                    kind, near
                ),
                ..Default::default()
            });
            self.refresh_goods_near(di);
        }
        (events, changed)
    }

    /// Re-list goods for every settlement whose hinterland covers deposit `di`.
    fn refresh_goods_near(&mut self, di: usize) {
        let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
        for i in 0..self.settlements.len() {
            let s = &self.settlements[i];
            let r = settlements::work_radius(s.pop);
            let ddx = (dx0 - s.x) as f64;
            let ddy = (dy0 - s.y) as f64;
            if ddx * ddx + ddy * ddy <= r * r {
                trade::goods_for(&mut self.settlements[i], &self.deposits, &self.fertility);
            }
        }
    }
}
