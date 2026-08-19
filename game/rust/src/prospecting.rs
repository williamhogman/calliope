//! Prospecting and depletion — the discovery economy (ADR-0011), a module
//! like its peers (E11.2). Moved verbatim out of `world.rs`.

use crate::economy;
use crate::resources;
use crate::society;
use crate::settlements;
use crate::trade;
use crate::util::round2;
use crate::world::{Event, EventKind, World};

impl World {
    /// Prospectors comb the hinterlands for hidden seams; worked mines thin
    /// toward exhaustion. Returns events and whether the deposit list changed.
    pub(crate) fn prospect_and_deplete(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        use rand::Rng;
        let mut events = Vec::new();
        let mut changed = false;
        let mods: Vec<society::Mods> = self.peoples.societies.iter().map(society::mods_for).collect();

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
        for (si, s) in self.peoples.settlements.iter().enumerate() {
            let reach = settlements::territory_radius(s.pop)
                * 2.4
                * mods.get(s.people.idx()).map(|m| m.prospecting).unwrap_or(1.0);
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
            self.economy.market.shock(kind, if precious { 0.88 } else { 0.84 });
            let ka = self.economy.areas.area_of(si);
            if let Some(mk) = self.economy.areas.markets.get_mut(ka) {
                mk.shock(kind, if precious { 0.80 } else { 0.75 });
            }
            let sname = self.peoples.settlements[si].name.clone();
            let strike = 12.0 + 45.0 * rich * economy::base_value(kind);
            self.peoples.settlements[si].wealth = round2(self.peoples.settlements[si].wealth + strike);
            if precious {
                // a rush: prospectors, chancers and mule-trains pour in
                let influx = ((self.peoples.settlements[si].pop as f64 * 0.05) as i64).max(10);
                self.peoples.settlements[si].pop += influx;
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
                // anchor the ground (M9.2): a same-tick rename must not
                // orphan the entry
                x: self.peoples.settlements[si].x,
                y: self.peoples.settlements[si].y,
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
        for s in &self.peoples.settlements {
            let r = settlements::work_radius(s.pop);
            deposit_buckets.candidates(s.x as f64, s.y as f64, r + 1.0, &mut cand);
            for &di in &cand {
                let d = &self.deposits[di];
                // M14.8 — wild grounds (left < 0, alive) now count crews
                // too: their pressure drives the stock, not the reserve.
                // Mineral sums are untouched, so totals stay float-identical.
                if !d.live() || !s.goods.iter().any(|g| *g == d.r) {
                    continue;
                }
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                if dx * dx + dy * dy <= r * r {
                    crews_by[di] += 1.0 + (s.pop as f64 / 9000.0).min(1.0);
                }
            }
        }
        // M15.6 — defensive re-size: the meters are pure bookkeeping and
        // deposits are only ever created at generation, so this fires once.
        if self.flows.extracted.len() != self.deposits.len() {
            self.flows = resources::Flows::for_deposits(self.deposits.len());
        }
        let mut spent: Vec<usize> = Vec::new();
        for di in 0..self.deposits.len() {
            let crews = crews_by[di];
            if crews == 0.0 {
                continue;
            }
            let d = &mut self.deposits[di];
            if d.left < 0.0 {
                continue; // renewables spend stock, not reserve (M14.8)
            }
            let before = d.left;
            d.left = round2((d.left - crews).max(0.0));
            let drawn = before - d.left;
            if d.left == 0.0 {
                spent.push(di);
            }
            self.flows.extracted[di] += drawn; // M15.6 flow meter
        }
        for di in spent {
            changed = true;
            let kind = self.deposits[di].r;
            let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
            let near_i = self
                .peoples.settlements
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.x - dx0).pow(2) + (s.y - dy0).pow(2))
                .map(|(i, _)| i);
            let near = near_i
                .map(|i| self.peoples.settlements[i].name.clone())
                .unwrap_or_else(|| "the wilds".to_string());
            // M6.1 — name the town by its ground, not its name: the event
            // anchors at the pit, so a same-tick rename would orphan it if
            // we left the id for resolve_events' name lookup to find.
            let near_ent = self.near_settlement_ent(near_i);
            self.economy.market.shock(kind, 1.22);
            // the pit's own market feels the silence first (M5.2)
            if let Some(i) = near_i {
                let ka = self.economy.areas.area_of(i);
                if let Some(mk) = self.economy.areas.markets.get_mut(ka) {
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
                // the pit itself is the ground — present even when no
                // settlement stands near
                x: dx0,
                y: dy0,
                ids: near_ent,
                ..Default::default()
            });
            self.refresh_goods_near(di);
        }

        // --- M14.8 — the wild stocks breathe. Timber, fish and game carry
        // memory: logistic regrowth against this month's harvest pressure,
        // with a hysteresis latch so the chronicle speaks at the thresholds
        // and not every month. Collapse withdraws the good (live() says no)
        // until the stock stands past half again; a stripped timber ground
        // marks the biome map and recovery unmarks it.
        for di in 0..self.deposits.len() {
            let d = &self.deposits[di];
            let rate = match resources::regrow_rate(d.r) {
                Some(r) => r,
                None => continue,
            };
            if !d.known {
                continue;
            }
            let pressure = 0.0025 * crews_by[di];
            let d = &mut self.deposits[di];
            let s0 = d.stock;
            let next = (s0 + rate * s0 * (1.0 - s0) - pressure).clamp(0.0, 1.0);
            d.stock = (next * 1e4).round() / 1e4;
            let (phase, stock, kind, dx0, dy0) = (d.phase, d.stock, d.r, d.x, d.y);
            self.flows.dstock[di] += stock - s0; // M15.6 flow meter
            // latch transitions — at most one per ground per month
            let transition: Option<u8> = match phase {
                0 if stock <= 0.06 => Some(2),
                0 if stock < 0.35 => Some(1),
                1 if stock <= 0.06 => Some(2),
                1 if stock >= 0.60 => Some(0),
                2 if stock >= 0.50 => Some(0),
                _ => None,
            };
            let Some(to) = transition else { continue };
            let near_i = self
                .peoples
                .settlements
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.x - dx0).pow(2) + (s.y - dy0).pow(2))
                .map(|(i, _)| i);
            let near = near_i
                .map(|i| self.peoples.settlements[i].name.clone())
                .unwrap_or_else(|| "the wilds".to_string());
            // M6.1 — deposit-anchored entries carry the town's id from the
            // start; the ground fallback in resolve_events can't find a
            // settlement standing on a stand of timber or a fishing shoal.
            let near_ent = self.near_settlement_ent(near_i);
            use resources::Good;
            let text = {
                match (kind, to) {
                    (Good::Timber, 1) => format!("The old groves near {near} go over to stumps; the loggers walk farther every year."),
                    (Good::Timber, 2) => format!("The last tall timber falls near {near}; bare hills stand where the forest stood."),
                    (Good::Timber, 0) => format!("Green returns to the logged-out hills near {near}; the woods stand tall again."),
                    (Good::Fish, 1) => format!("The shoals off {near} come up thin; the nets rise half empty."),
                    (Good::Fish, 2) => format!("The fishery off {near} fails; the boats turn home empty."),
                    (Good::Fish, 0) => format!("The shoals return off {near}; the water silvers again."),
                    (_, 1) => format!("The trap-lines near {near} run empty more seasons than not."),
                    (_, 2) => format!("The game is hunted out of the country around {near}."),
                    _ => format!("Game moves back into the country around {near}."),
                }
            };
            match to {
                1 => {
                    self.deposits[di].phase = 1;
                    if let Some(i) = near_i {
                        let ka = self.economy.areas.area_of(i);
                        if let Some(mk) = self.economy.areas.markets.get_mut(ka) {
                            mk.shock(kind, 1.12);
                        }
                    }
                    if kind == Good::Timber {
                        self.mark_timber_scar(di, 2);
                    }
                    events.push(Event {
                        m: month_abs,
                        s: near,
                        k: EventKind::Depletion,
                        text: text.clone(),
                        x: dx0,
                        y: dy0,
                        ..Default::default()
                    });
                }
                2 => {
                    self.deposits[di].phase = 2;
                    changed = true;
                    self.economy.market.shock(kind, 1.18);
                    if let Some(i) = near_i {
                        let ka = self.economy.areas.area_of(i);
                        if let Some(mk) = self.economy.areas.markets.get_mut(ka) {
                            mk.shock(kind, 1.30);
                        }
                    }
                    if kind == Good::Timber {
                        self.mark_timber_scar(di, 4);
                    }
                    events.push(Event {
                        m: month_abs,
                        s: near,
                        k: EventKind::Depletion,
                        text: text.clone(),
                        x: dx0,
                        y: dy0,
                        ..Default::default()
                    });
                    self.refresh_goods_near(di);
                }
                _ => {
                    let was_collapsed = phase == 2;
                    self.deposits[di].phase = 0;
                    if kind == Good::Timber {
                        self.restore_timber_scar(di);
                    }
                    if was_collapsed {
                        changed = true;
                        events.push(Event {
                            m: month_abs,
                            s: near,
                            k: EventKind::Discovery,
                            text: text.clone(),
                            x: dx0,
                            y: dy0,
                            ..Default::default()
                        });
                        self.refresh_goods_near(di);
                    }
                }
            }
        }
        (events, changed)
    }

    /// M14.8 — a stripped timber ground shows on the map: forest-family
    /// biome cells within `radius` of the deposit go over to grassland,
    /// with the original code remembered in `self.scars` for restoration.
    fn mark_timber_scar(&mut self, di: usize, radius: i64) {
        use crate::constants as gc;
        let forest = |b: u8| {
            b == gc::WOODLAND
                || b == gc::BOREAL_FOREST
                || b == gc::SEASONAL_RAIN_FOREST
                || b == gc::TEMPERATE_RAIN_FOREST
                || b == gc::TROPICAL_RAIN_FOREST
        };
        let (cy, cx) = (self.deposits[di].y, self.deposits[di].x);
        let (rows, cols) = self.fields.biomes.dim();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dy * dy + dx * dx > radius * radius {
                    continue;
                }
                let (y, x) = (cy + dy, cx + dx);
                if y < 0 || x < 0 || y as usize >= rows || x as usize >= cols {
                    continue;
                }
                let (yu, xu) = (y as usize, x as usize);
                let b = self.fields.biomes[[yu, xu]];
                if forest(b) {
                    self.scars.push((di, y, x, b));
                    self.fields.biomes[[yu, xu]] = gc::GRASSLAND;
                    self.dirty.mark(crate::world::Dirty::DEPOSITS);
                }
            }
        }
    }

    /// M14.8 — recovery unmarks: every remembered cell of this ground's
    /// scar gets its original biome back.
    fn restore_timber_scar(&mut self, di: usize) {
        let mut i = 0;
        while i < self.scars.len() {
            let (sdi, y, x, orig) = self.scars[i];
            if sdi == di {
                self.fields.biomes[[y as usize, x as usize]] = orig;
                self.dirty.mark(crate::world::Dirty::DEPOSITS);
                self.scars.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Re-list goods for every settlement whose hinterland covers deposit `di`.
    fn refresh_goods_near(&mut self, di: usize) {
        let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
        for i in 0..self.peoples.settlements.len() {
            let s = &self.peoples.settlements[i];
            let r = settlements::work_radius(s.pop);
            let ddx = (dx0 - s.x) as f64;
            let ddy = (dy0 - s.y) as f64;
            if ddx * ddx + ddy * ddy <= r * r {
                trade::goods_for(&mut self.peoples.settlements[i], &self.deposits, &self.fields.fertility, &self.fields.rock);
            }
        }
    }
}
