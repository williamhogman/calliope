//! Economy — a real market over the goods the land yields. Every good has
//! a base worth by rarity; monthly prices move with supply (who produces
//! it, weighted by workforce) against demand (mouths, categories, and the
//! taste for luxury that wealth brings).
//!
//! M5 makes the market local: the route web is carved into MARKET AREAS,
//! each with its own price list (M5.2). Trade income comes from the price
//! gaps between areas — arbitrage, not geography (M5.3) — and every load
//! carried drags the two prices toward each other. Towns with ore, fuel,
//! smiths and the right arts work RECIPES into finished goods (M5.1), and
//! named MERCHANTS ride the widest gaps for profit (M5.5).
//!
//! Deterministic: goods are `Copy` enums (E1); every map over goods is an
//! `EnumMap` whose iteration order is the enum's alphabetical variant
//! order — the exact order the old `BTreeMap<String, _>` gave. No strings,
//! no wall-clock.

use smallvec::smallvec;
use std::collections::{BTreeMap, HashMap, HashSet};

use enum_map::EnumMap;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ids::{PeopleId, EntityId, RealmId, SettlementId};
use crate::entity::Registry;
use crate::naming;
use crate::resources::{Abundance, Good, GoodSet};
use crate::settlements::Settlement;
use crate::society::{self, TechId};
use crate::trade::Route;
use crate::util::round2;
use crate::event::EventKind;
use crate::event::Event;

/// Base worth of one unit of a good, from its rarity in the world.
pub fn base_value(good: Good) -> f64 {
    let mut v = match good.abundance() {
        Abundance::Uncommon => 1.8,
        Abundance::Rare => 3.4,
        Abundance::Legendary => 7.0,
        Abundance::Common => 1.0,
    };
    if good.is_food() {
        v *= 0.85;
    }
    round2(v)
}

// pub(crate): explain.rs mirrors the price math for its term ledger.
pub(crate) fn demand_weight(good: Good, luxury: f64) -> f64 {
    if good.is_food() {
        1.15
    } else if good == Good::Salt {
        // M14.2: every table needs it, rich or poor — demand sits just
        // under food and does not ride luxury; scarcity alone moves it
        1.00
    } else if good.is_luxury() {
        // M14.3: furs and their kin — nothing but taste; the poor world
        // barely wants them, the rich world cannot get enough
        0.25 + 1.6 * luxury
    } else if good.is_craft() {
        // M5.1: finished goods are bought with surplus — a taste that
        // sharpens as the world grows rich
        0.50 + 1.3 * luxury
    } else if good.is_metal() {
        // M2.7: metal hunger grows fast with wealth — smiths, arms, coin
        0.60 + 1.1 * luxury
    } else if good.is_material() {
        // M2.7: timber and stone are bulk goods; even rich folk buy few
        0.45 + 0.35 * luxury
    } else {
        0.45 + luxury
    }
}

// ---------------------------------------------------------------- market

/// Price book over the goods actually traded here. `None` = the market has
/// never priced that good (the old "key absent" state).
#[derive(Default, Clone)]
pub struct Market {
    pub prices: EnumMap<Good, Option<f64>>,
    prev: EnumMap<Good, Option<f64>>,
}

impl Market {
    pub fn price(&self, good: Good) -> f64 {
        self.prices[good].unwrap_or_else(|| base_value(good))
    }

    #[inline]
    pub fn contains(&self, good: Good) -> bool {
        self.prices[good].is_some()
    }

    /// Priced goods, alphabetical (variant) order — the old BTreeMap keys.
    pub fn keys(&self) -> impl Iterator<Item = Good> + '_ {
        self.prices
            .iter()
            .filter_map(|(g, p)| p.map(|_| g))
    }

    /// (good, price) rows in alphabetical order.
    pub fn iter_some(&self) -> impl Iterator<Item = (Good, f64)> + '_ {
        self.prices.iter().filter_map(|(g, p)| p.map(|p| (g, p)))
    }

    pub fn set(&mut self, good: Good, price: f64) {
        self.prices[good] = Some(price);
    }

    /// Rows for the client: {g, p, b, t} sorted dearest first.
    pub fn snapshot(&self) -> Value {
        let mut rows: Vec<(Good, f64, f64, f64)> = self
            .iter_some()
            .map(|(g, p)| {
                let prev = self.prev[g].unwrap_or(p);
                (g, p, base_value(g), round2(p - prev))
            })
            .collect();
        rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Value::Array(
            rows.into_iter()
                .map(|(g, p, b, t)| json!({ "g": g, "p": round2(p), "b": b, "t": t }))
                .collect(),
        )
    }

    /// A sudden strike or a dead mine jolts the price at once, before the
    /// slow-moving supply average has time to catch up.
    pub fn shock(&mut self, good: Good, factor: f64) {
        let p = self.price(good);
        let b = base_value(good);
        self.prices[good] = Some(round2((p * factor).clamp(0.3 * b, 5.0 * b)));
    }
}

/// The shared price core: relative scarcity against the market's geometric
/// mean, eased 25 % per month toward target. `catalogue`, when given, adds
/// goods the members do NOT produce — in a local market a good nobody makes
/// is dear, and that gap is exactly what the caravans live on (M5.2).
fn compute_prices<'a, I>(
    market: &mut Market,
    members: I,
    catalogue: Option<GoodSet>,
    people_style: &[usize],
) where
    I: Iterator<Item = &'a Settlement>,
{
    let mut supply: EnumMap<Good, Option<f64>> = EnumMap::default();
    let mut total_pop: i64 = 0;
    let mut total_wealth = 0.0;
    // M14.9 — who is buying: population by culture style, so the demand
    // side can lean toward the tastes of the people actually here.
    let mut pop_by_style = [0.0f64; crate::culture::N_STYLES];
    for s in members {
        total_pop += s.pop;
        total_wealth += s.wealth;
        let sty = people_style.get(s.people.0).copied().unwrap_or(0);
        pop_by_style[sty] += s.pop as f64;
        for (i, &g) in s.goods.iter().enumerate() {
            let w = (s.pop as f64 / 1000.0) * 0.7f64.powi(i as i32);
            *supply[g].get_or_insert(0.0) += w;
        }
    }
    let style_pop: f64 = pop_by_style.iter().sum();
    let mix = |g: Good| -> f64 {
        if style_pop <= 0.0 {
            return 1.0;
        }
        pop_by_style
            .iter()
            .enumerate()
            .map(|(k, p)| p * crate::culture::taste(k, g))
            .sum::<f64>()
            / style_pop
    };
    if let Some(cat) = catalogue {
        for g in cat.iter() {
            supply[g].get_or_insert(0.0);
        }
    }
    let luxury = (total_wealth / (total_pop.max(1) as f64) / 4.0).min(0.5);

    market.prev = market.prices.clone();
    let mut pressure: EnumMap<Good, Option<f64>> = EnumMap::default();
    let mut ln_sum = 0.0;
    let mut n = 0usize;
    for (g, s) in &supply {
        if let Some(sv) = s {
            let p = (demand_weight(g, luxury) * mix(g) + 0.02) / (sv + 0.02);
            pressure[g] = Some(p);
            ln_sum += p.ln();
            n += 1;
        }
    }
    if n == 0 {
        market.prices = EnumMap::default();
        return;
    }
    let gm = (ln_sum / n as f64).exp();
    let mut next: EnumMap<Good, Option<f64>> = EnumMap::default();
    for (g, p) in &pressure {
        if let Some(p) = p {
            let base = base_value(g);
            // M14.5 tuning — the scarcity ratio saturates BELOW the hard
            // clamp (0.148^0.55 ≈ 0.35×, 16^0.55 ≈ 4.6×): a single-supplier
            // good settles dear but off the pin, so the 0.3×/5× clamp is
            // reachable only by shock(), and shocks decay. A pinned price
            // is a dead signal; the economy gate counts them as mis-tuning.
            let r = (p / gm).clamp(0.148, 16.0);
            let target = (base * r.powf(0.55)).clamp(0.3 * base, 5.0 * base);
            let old = market.price(g);
            next[g] = Some(round2(0.75 * old + 0.25 * target));
        }
    }
    market.prices = next;
}

/// Recompute WORLD prices from relative scarcity (the blended ledger the
/// UI's market tab and explain.rs read).
pub fn update_prices(market: &mut Market, settlements: &[Settlement], people_style: &[usize]) {
    compute_prices(market, settlements.iter(), None, people_style);
}

// ---------------------------------------------------------- market areas

/// M5.2 — the route web carved into market areas: each area is the towns
/// that trade through one hub, and it keeps its own price list.
#[derive(Default)]
pub struct MarketAreas {
    /// Hub settlement id per area, in area order.
    pub hubs: Vec<SettlementId>,
    /// Settlement index -> area index. Rebuilt with the towns.
    pub area: Vec<usize>,
    /// One market per area, prices carried across rebuilds by hub id.
    pub markets: Vec<Market>,
}

impl MarketAreas {
    pub fn area_of(&self, si: usize) -> usize {
        self.area.get(si).copied().unwrap_or(0)
    }
    pub fn market_of(&self, si: usize) -> Option<&Market> {
        self.markets.get(self.area_of(si))
    }
}

/// Unweighted hop distance from `from`, cut off at `max_hops` (hub spacing).
fn within_hops(adj: &[Vec<(usize, f64)>], from: usize, to: usize, max_hops: usize) -> bool {
    if from == to {
        return true;
    }
    let mut seen = vec![false; adj.len()];
    let mut frontier = vec![from];
    seen[from] = true;
    for _ in 0..max_hops {
        let mut next = Vec::new();
        for &u in &frontier {
            for &(v, _) in &adj[u] {
                if v == to {
                    return true;
                }
                if !seen[v] {
                    seen[v] = true;
                    next.push(v);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    false
}

/// Carve the route web into market areas around the great towns. Hubs are
/// the biggest towns at least 3 route-legs apart; every town joins the hub
/// cheapest to reach along the actual roads and lanes.
/// The settlement id→index map every monthly pass shares (E5.2): built
/// once per month in the tick loop, passed down — no pass rebuilds it.
pub fn sidx(settlements: &[Settlement]) -> HashMap<SettlementId, usize> {
    settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect()
}

pub fn build_areas(
    settlements: &[Settlement],
    routes: &[Route],
    prev: Option<&MarketAreas>,
    by_id: &HashMap<SettlementId, usize>,
) -> MarketAreas {
    let n = settlements.len();
    if n == 0 {
        return MarketAreas::default();
    }
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for r in routes {
        let (Some(&a), Some(&b)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let c = r.cost.max(1.0);
        adj[a].push((b, c));
        adj[b].push((a, c));
        edges.push((a, b, c));
    }

    // hubs: largest towns first, at least 3 legs from every earlier hub
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        settlements[b]
            .pop
            .cmp(&settlements[a].pop)
            .then(settlements[a].id.cmp(&settlements[b].id))
    });
    let cap = (n / 14).clamp(1, 12);
    let mut hubs: Vec<usize> = Vec::new();
    for &cand in &order {
        if hubs.len() >= cap {
            break;
        }
        if !hubs.is_empty() && settlements[cand].pop < 800 {
            break;
        }
        if hubs.iter().all(|&h| !within_hops(&adj, cand, h, 2)) {
            hubs.push(cand);
        }
    }
    if hubs.is_empty() {
        hubs.push(order[0]);
    }

    // cheapest-hub assignment: Bellman–Ford relaxation over the fixed edge
    // list — no float-keyed heap, fully deterministic
    let mut dist = vec![f64::INFINITY; n];
    let mut area = vec![usize::MAX; n];
    for (k, &h) in hubs.iter().enumerate() {
        dist[h] = 0.0;
        area[h] = k;
    }
    for _ in 0..n {
        let mut changed = false;
        for &(a, b, c) in &edges {
            if dist[a] + c < dist[b] - 1e-9 {
                dist[b] = dist[a] + c;
                area[b] = area[a];
                changed = true;
            }
            if dist[b] + c < dist[a] - 1e-9 {
                dist[a] = dist[b] + c;
                area[a] = area[b];
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // marooned towns fall to the nearest hub as the crow flies
    for i in 0..n {
        if area[i] == usize::MAX {
            let mut best = 0usize;
            let mut bd = f64::INFINITY;
            for (k, &h) in hubs.iter().enumerate() {
                let dx = (settlements[i].x - settlements[h].x) as f64;
                let dy = (settlements[i].y - settlements[h].y) as f64;
                let d = dx * dx + dy * dy;
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            area[i] = best;
        }
    }

    // carry each hub's price book across the rebuild
    let hub_ids: Vec<SettlementId> = hubs.iter().map(|&h| settlements[h].id).collect();
    let markets: Vec<Market> = hub_ids
        .iter()
        .map(|id| {
            prev.and_then(|p| {
                p.hubs
                    .iter()
                    .position(|h| h == id)
                    .map(|k| p.markets[k].clone())
            })
            .unwrap_or_default()
        })
        .collect();

    MarketAreas {
        hubs: hub_ids,
        area,
        markets,
    }
}

/// Recompute every area's price list against the full world catalogue —
/// a good nobody nearby makes is dear there, and the caravans know it.
/// Every book is then anchored to the world blend: even a shut-in valley
/// hears rumours of prices down the road, and itinerant peddlers ferry
/// the basics long before the great caravans bother (M5.2).
pub fn update_area_prices(
    areas: &mut MarketAreas,
    settlements: &[Settlement],
    world: &Market,
    people_style: &[usize],
) {
    // M14.7 — the world anchor IS long-distance arbitrage in disguise, so
    // how hard it pulls on a local book is a transport-class fact, not a
    // constant: a "world price" only exists for goods that actually cross
    // the world. Precious anchors hard (gold is gold everywhere), the
    // everyday middle moderately, bulk barely (grain is priced by the
    // valley that grew it), and fresh fruit — no salt cures it — hardly
    // at all. This, not a painted ring, is where von Thünen lives.
    let openness = |g: Good| -> f64 {
        use crate::resources::Transport;
        if g.perishable() && !crate::resources::salt_cured(g) {
            return 0.05;
        }
        match g.transport() {
            Transport::Bulk => 0.12,
            Transport::Ordinary => 0.35,
            Transport::Precious => 0.60,
        }
    };
    let catalogue: GoodSet = settlements
        .iter()
        .flat_map(|s| s.goods.iter().copied())
        .collect();
    for k in 0..areas.markets.len() {
        let members: Vec<&Settlement> = settlements
            .iter()
            .enumerate()
            .filter(|(i, _)| areas.area.get(*i) == Some(&k))
            .map(|(_, s)| s)
            .collect();
        compute_prices(&mut areas.markets[k], members.into_iter(), Some(catalogue), people_style);
        let mk = &mut areas.markets[k];
        for g in catalogue.iter() {
            if let Some(local) = mk.prices[g] {
                let anchor = world.price(g);
                mk.prices[g] = Some(round2(local + (anchor - local) * openness(g)));
            }
        }
    }
}

/// M14.2 PRESERVES — the areas whose yards can salt a catch: any member
/// town listing salt. Computed once per pass and shared by the border
/// equalizer, the gravity lane and the merchants.
pub fn areas_with_salt(areas: &MarketAreas, settlements: &[Settlement]) -> Vec<bool> {
    let mut has = vec![false; areas.markets.len()];
    for (i, s) in settlements.iter().enumerate() {
        if s.goods.contains(&Good::Salt) {
            if let Some(&k) = areas.area.get(i) {
                if k < has.len() {
                    has[k] = true;
                }
            }
        }
    }
    has
}

/// M14.7 CARRIAGE — what fraction of a price gap a route can profitably
/// carry, set by value density. Reach is measured in units of the median
/// leg cost `c0` (scale-free, like the M5.4 attenuation): bulk pays
/// freight per ton it cannot afford overland, precious shrugs at
/// distance. Sea legs are ~9× cheaper per km in `Route::cost` (ADR-0010),
/// so "bulk moves by water" falls out of the same number — a sea route
/// of the same length simply costs less, and bulk's short reach covers
/// it. Fresh fruit rots on the axle: no salt cures it (M14.2 gates the
/// curable), so it trades next door or not at all.
fn carriage(g: Good, cost: f64, c0: f64) -> f64 {
    use crate::resources::Transport;
    let reach = match g.transport() {
        Transport::Bulk => 0.75 * c0,
        Transport::Ordinary => 3.0 * c0,
        Transport::Precious => 24.0 * c0,
    };
    let mut f = reach / (reach + cost.max(0.0));
    if g.perishable() && !crate::resources::salt_cured(g) {
        f *= 0.15;
    }
    f
}

/// The median leg cost — the distance unit every carriage reach is
/// quoted in. Median, not mean: one heroic sea crossing must not
/// re-price every cart on the map.
fn median_cost(routes: &[Route]) -> f64 {
    let mut cs: Vec<f64> = routes.iter().map(|r| r.cost.max(1.0)).collect();
    if cs.is_empty() {
        return 50.0;
    }
    cs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cs[cs.len() / 2]
}

/// Every route that crosses an area border drags the two price books
/// toward each other — trade equalizes what it touches (M5.3), at the
/// rate the cargo class can actually pay for (M14.7).
pub fn equalize_along_routes(
    areas: &mut MarketAreas,
    settlements: &[Settlement],
    routes: &[Route],
    by_id: &HashMap<SettlementId, usize>,
    salted: &[bool],
) {
    let c0 = median_cost(routes);
    for r in routes {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        if ka == kb || ka >= areas.markets.len() || kb >= areas.markets.len() {
            continue;
        }
        let rate0 = 0.05 * r.w.min(1.0);
        let goods: GoodSet = settlements[ia]
            .goods
            .iter()
            .chain(settlements[ib].goods.iter())
            .copied()
            .collect();
        for g in goods.iter() {
            // M14.2 PRESERVES — fresh catch spoils at the area border;
            // with a salting yard on either side it travels like any ware
            if crate::resources::salt_cured(g)
                && !(salted.get(ka).copied().unwrap_or(false)
                    || salted.get(kb).copied().unwrap_or(false))
            {
                continue;
            }
            // M14.7 — equalization runs at the rate the cargo class pays
            // for: grain converges along cheap water, gold across the map
            let rate = rate0 * carriage(g, r.cost, c0);
            let pa = areas.markets[ka].price(g);
            let pb = areas.markets[kb].price(g);
            let mid = 0.5 * (pa + pb);
            areas.markets[ka].set(g, round2(pa + (mid - pa) * rate));
            areas.markets[kb].set(g, round2(pb + (mid - pb) * rate));
        }
    }
}

/// Client rows: hubs, per-town area index, and the widest price spreads.
pub fn areas_json(areas: &MarketAreas, settlements: &[Settlement]) -> Value {
    let mut counts = vec![0usize; areas.hubs.len()];
    for &a in &areas.area {
        if a < counts.len() {
            counts[a] += 1;
        }
    }
    let by_id: HashMap<SettlementId, &Settlement> =
        settlements.iter().map(|s| (s.id, s)).collect();
    let hubs: Vec<Value> = areas
        .hubs
        .iter()
        .zip(counts.iter())
        .enumerate()
        .map(|(k, (id, n))| {
            // local price list, ordered for determinism (alphabetical keys)
            let prices: BTreeMap<&'static str, f64> = areas
                .markets
                .get(k)
                .map(|m| {
                    m.iter_some()
                        .map(|(g, p)| (g.name(), round2(p)))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": id,
                "name": by_id.get(id).map(|s| s.name.clone()).unwrap_or_default(),
                "n": n,
                "p": prices,
            })
        })
        .collect();
    // widest spreads: for every good priced anywhere, max/min across areas
    let mut spreads: Vec<Value> = Vec::new();
    if areas.markets.len() > 1 {
        let goods: GoodSet = areas
            .markets
            .iter()
            .flat_map(|m| m.keys())
            .collect();
        let mut rows: Vec<(f64, Value)> = Vec::new();
        for g in goods.iter() {
            let mut lo = (f64::INFINITY, 0usize);
            let mut hi = (f64::NEG_INFINITY, 0usize);
            for (k, m) in areas.markets.iter().enumerate() {
                let p = m.price(g);
                if p < lo.0 {
                    lo = (p, k);
                }
                if p > hi.0 {
                    hi = (p, k);
                }
            }
            if lo.0 <= 0.0 || !lo.0.is_finite() || !hi.0.is_finite() {
                continue;
            }
            let ratio = hi.0 / lo.0;
            if ratio < 1.15 {
                continue;
            }
            let hub_name = |k: usize| -> String {
                areas
                    .hubs
                    .get(k)
                    .and_then(|id| by_id.get(id))
                    .map(|s| s.name.clone())
                    .unwrap_or_default()
            };
            rows.push((
                ratio,
                json!({
                    "g": g,
                    "ratio": round2(ratio),
                    "hi": { "hub": hub_name(hi.1), "p": round2(hi.0) },
                    "lo": { "hub": hub_name(lo.1), "p": round2(lo.0) },
                }),
            ));
        }
        rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        spreads = rows.into_iter().take(3).map(|(_, v)| v).collect();
    }
    json!({ "hubs": hubs, "of": areas.area, "spread": spreads })
}

// ---------------------------------------------------------------- recipes

/// M5.1 — what the workshops turn ore into, and what it takes.
pub struct Recipe {
    pub out: Good,
    /// any one of these arts unlocks the craft
    pub tech_any: &'static [TechId],
    /// workforce gate: no finished goods from a hamlet
    pub min_pop: i64,
    /// any one of these among the town's goods feeds the forge
    pub ore_any: &'static [Good],
    /// the forge burns coal or charcoal (timber)
    pub needs_fuel: bool,
}

pub const RECIPES: [Recipe; 8] = [
    Recipe { out: Good::Tools, tech_any: &[TechId::Bronze, TechId::Iron], min_pop: 1200, ore_any: &[Good::Copper, Good::Iron], needs_fuel: true },
    Recipe { out: Good::Weapons, tech_any: &[TechId::Steel], min_pop: 2500, ore_any: &[Good::Iron], needs_fuel: true },
    // M14.5 — gems join gold and silver at the jeweler's bench
    Recipe { out: Good::Jewelry, tech_any: &[TechId::Coin], min_pop: 2000, ore_any: &[Good::Gold, Good::Silver, Good::Gems], needs_fuel: false },
    // M14.5 — the earth crafts: riverbank clay through the kilns
    Recipe { out: Good::Pottery, tech_any: &[TechId::Pottery], min_pop: 800, ore_any: &[Good::Clay], needs_fuel: true },
    Recipe { out: Good::Brick, tech_any: &[TechId::Masonry], min_pop: 1500, ore_any: &[Good::Clay], needs_fuel: true },
    // M14.6 — the soft trades: fleece to the loom, hides to the tan-pits
    // (bark lore, not fire), grapes to the press and into amphorae.
    Recipe { out: Good::Cloth, tech_any: &[TechId::Loom], min_pop: 1000, ore_any: &[Good::Wool], needs_fuel: false },
    Recipe { out: Good::Leather, tech_any: &[TechId::HerbLore], min_pop: 1000, ore_any: &[Good::Hides], needs_fuel: false },
    Recipe { out: Good::Wine, tech_any: &[TechId::Pottery], min_pop: 1200, ore_any: &[Good::Grapes], needs_fuel: false },
];

const FORGE_LIT: [&str; 3] = [
    "The smiths of {S} ring day and night — {G} of {S} make are sold in every port.",
    "{S} raises a guildhall of hammers: its {G} carry the town's mark now.",
    "Ore goes into {S} and {G} come out; the caravans pay in silver.",
];

/// M14.5 — the clay crafts speak of kilns, not anvils.
const KILN_LIT: [&str; 3] = [
    "The kilns of {S} glow through the night — {G} of {S} make travel the river roads.",
    "{S} digs the riverbank and fires it true: its {G} carry the town's stamp now.",
    "Clay goes into {S} and {G} come out; every market stall stacks them high.",
];

/// M14.6 — and the soft trades speak of looms, tan-pits and presses.
const WORKS_LIT: [&str; 3] = [
    "The workshops of {S} hum from bell to bell — {G} of {S} make are asked for by name.",
    "{S} turns the country's harvest into {G}: the raw carts come in, the finished carts go out.",
    "What the hinterland grows, {S} refines — its {G} fetch twice the raw at any fair.",
];

/// The voice and the vocabulary of each craft: (works, feedstock, lines).
/// One place to look up how a workshop speaks when it lights or goes cold.
fn craft_voice(out: Good) -> (&'static str, &'static str, &'static [&'static str]) {
    match out {
        Good::Pottery | Good::Brick => ("kilns", "clay", &KILN_LIT),
        Good::Cloth => ("looms", "wool", &WORKS_LIT),
        Good::Leather => ("tan-pits", "hides", &WORKS_LIT),
        Good::Wine => ("presses", "grapes", &WORKS_LIT),
        _ => ("forges", "ore", &FORGE_LIT),
    }
}

/// Towns with ore, fuel, hands and the art work it into finished goods.
/// Returns chronicle lines for forges newly lit or newly gone cold.
pub fn craft_pass(
    peoples: &mut Peoples,
    areas: &MarketAreas,
    month_abs: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let Peoples { settlements, societies: socs, .. } = peoples;
    let mut events = Vec::new();
    // Inputs are sourced from the whole MARKET AREA, not just the town's
    // own pits — that is what the market carve is for (M5.2). A forge town
    // buys ore off the carts; it only goes cold when the whole area's
    // seams are spent.
    let mut area_goods: Vec<GoodSet> = vec![GoodSet::EMPTY; areas.markets.len()];
    // ...and each area supports only a couple of workshops per finished
    // good: the first forge takes the custom, the rest buy its wares.
    let mut area_craft: BTreeMap<(usize, Good), usize> = BTreeMap::new();
    for (i, s) in settlements.iter().enumerate() {
        if let Some(&k) = areas.area.get(i) {
            if let Some(set) = area_goods.get_mut(k) {
                set.extend(s.goods.iter().copied());
            }
            for rc in RECIPES.iter() {
                if s.goods.contains(&rc.out) {
                    *area_craft.entry((k, rc.out)).or_insert(0) += 1;
                }
            }
        }
    }
    for (si, s) in settlements.iter_mut().enumerate() {
        let Some(soc) = socs.get(s.people.idx()) else { continue };
        let k_area = areas.area.get(si).copied();
        let nearby: GoodSet = k_area
            .and_then(|k| area_goods.get(k))
            .copied()
            .unwrap_or_default();
        let own: GoodSet = s.goods.iter().copied().collect();
        let has_good = |g: Good| -> bool { own.contains(g) || nearby.contains(g) };
        let fuel = has_good(Good::Coal) || has_good(Good::Timber);
        for rc in RECIPES.iter() {
            let has = s.goods.contains(&rc.out);
            let eligible = s.pop >= rc.min_pop
                && rc.tech_any.iter().any(|&t| soc.knows(t))
                && rc.ore_any.iter().any(|&o| has_good(o))
                && (!rc.needs_fuel || fuel);
            // the market niche: at most 2 workshops per good per area, and
            // a forge takes months of guild wrangling to light, not a tick
            let niche = k_area
                .map(|k| *area_craft.get(&(k, rc.out)).unwrap_or(&0) < 2)
                .unwrap_or(false);
            if eligible && !has && niche && rng.gen::<f64>() < 0.04 {
                if let Some(k) = k_area {
                    *area_craft.entry((k, rc.out)).or_insert(0) += 1;
                }
                s.goods.push(rc.out);
                s.goods.truncate(8);
                // the finished good becomes the export when it out-earns
                // whatever the town shipped before
                let local = areas.market_of(si);
                let p_out = local.map(|m| m.price(rc.out)).unwrap_or_else(|| base_value(rc.out));
                let p_cur = s
                    .exports
                    .map(|e| local.map(|m| m.price(e)).unwrap_or_else(|| base_value(e)))
                    .unwrap_or(0.0);
                if p_out > p_cur {
                    s.exports = Some(rc.out);
                }
                let (_, _, lines) = craft_voice(rc.out);
                let t = lines[rng.gen_range(0..lines.len())];
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Trade,
                    text: t.replace("{S}", &s.name).replace("{G}", rc.out.name()),
                    // anchor the ground (M9.2)
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
            } else if !eligible && has {
                s.goods.retain(|g| *g != rc.out);
                if s.exports == Some(rc.out) {
                    s.exports = s.goods.first().copied();
                }
                let (works, feed, _) = craft_voice(rc.out);
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Trade,
                    text: format!(
                        "The {} of {} go cold — no {} comes, and the {} trade dies with them.",
                        works, s.name, feed, rc.out
                    ),
                    // anchor the ground (M9.2)
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
            }
        }
    }
    events
}


// ------------------------------------------------------- M58 claim pressure

/// M58 — how much unmet craft it takes before a crown treats a metal as
/// something it must *have* rather than merely buy. One idle workshop's
/// worth of demand (a town exactly at a recipe's `min_pop`) counts 1.0;
/// the pressure is that weight, capped, so a realm with a single dark
/// forge presses gently and an industrial heartland starved of iron
/// presses hard.
pub const CLAIM_SATIATE: f64 = 1.0;

/// M58 — how much louder a deprived crown hears a seam of the ore it
/// lacks, per unit of claim pressure. At 1.0 a realm with one idle forge
/// values that seam twice over, and a realm at the cap four times. The
/// gain multiplies the *seam's* worth (and its local ceiling), never the
/// suitability of the ground it lies under.
pub const CLAIM_GAIN: f64 = 1.0;

/// M58 — how much further a claim-driven crown will victual a lane, per
/// unit of pressure, as a fraction of `trade::CARAVAN_BUDGET`. At 0.25 a
/// realm at the cap pays for a lane ~75% longer than the ordinary trade
/// would carry: the state subsidises the water and fodder that private
/// caravans would not. Reach is bought, never granted — the Dijkstra
/// budget still has to reach the ground across real terrain cost.
pub const CLAIM_REACH_GAIN: f64 = 0.25;

/// M58 — the ceiling on claim pressure per good and realm. A crown can
/// want a metal very badly; it cannot want it infinitely, and without a
/// cap one enormous smelting realm would drown every other signal.
pub const CLAIM_PRESSURE_CAP: f64 = 3.0;

/// M58 — the deprived crafts of a realm: for every (realm, feedstock)
/// pair, the weight of workshops that realm's towns could run — they
/// have the art, the hands and the fuel — but cannot, because no seam of
/// that ore lies inside their market area.
///
/// This is *demand*, not price. A market price says a good is dear; a
/// dark forge says a crown is structurally without it, which is the
/// thing that historically sent states to claim distant, unlivable
/// ground (Potosí, the Rio Tinto concessions, the Kalgoorlie fields).
/// It touches no site score: it re-weights how loudly a *known* seam
/// calls, and how far a state will pay to victual the lane to it.
///
/// Deterministic by construction: `BTreeMap` keys, `Copy` enums, no
/// wall-clock, no hash iteration into output (ADR-0003).
pub fn claim_pressure(
    settlements: &[Settlement],
    societies: &[society::Society],
    areas: &MarketAreas,
) -> BTreeMap<(RealmId, Good), f64> {
    // what each market area can put on a forge floor
    let mut area_goods: Vec<GoodSet> = vec![GoodSet::EMPTY; areas.markets.len()];
    for (i, s) in settlements.iter().enumerate() {
        if let Some(&k) = areas.area.get(i) {
            if let Some(set) = area_goods.get_mut(k) {
                set.extend(s.goods.iter().copied());
            }
        }
    }
    let mut out: BTreeMap<(RealmId, Good), f64> = BTreeMap::new();
    for (si, s) in settlements.iter().enumerate() {
        let Some(soc) = societies.get(s.people.idx()) else { continue };
        let nearby: GoodSet = areas
            .area
            .get(si)
            .and_then(|&k| area_goods.get(k))
            .copied()
            .unwrap_or_default();
        let own: GoodSet = s.goods.iter().copied().collect();
        let has_good = |g: Good| -> bool { own.contains(g) || nearby.contains(g) };
        let fuel = has_good(Good::Coal) || has_good(Good::Timber);
        for rc in RECIPES.iter() {
            // only mineral feedstocks: a realm short of wool shears more
            // sheep, it does not colonize a desert for them.
            if !rc.ore_any.iter().any(|g| g.is_mineral()) {
                continue;
            }
            let capable = s.pop >= rc.min_pop
                && rc.tech_any.iter().any(|&t| soc.knows(t))
                && (!rc.needs_fuel || fuel);
            if !capable {
                continue;
            }
            if rc.ore_any.iter().any(|&o| has_good(o)) {
                continue; // the forge is fed; no claim to press
            }
            // the weight of the idle craft: a town at the recipe's bar
            // counts one workshop, a city several — but no more than
            // the cap, since a crown's reach is not linear in its size.
            let w = ((s.pop as f64) / (rc.min_pop as f64)).min(CLAIM_PRESSURE_CAP);
            for &o in rc.ore_any.iter().filter(|g| g.is_mineral()) {
                let e = out.entry((s.realm, o)).or_insert(0.0);
                *e = (*e + w).min(CLAIM_PRESSURE_CAP * CLAIM_SATIATE);
            }
        }
    }
    for v in out.values_mut() {
        *v /= CLAIM_SATIATE;
    }
    out
}

// -------------------------------------------------------------- merchants

/// M5.5 — a named trader who rides the widest price gap out of their home
/// market, month after month, until the road or old age takes them.
#[derive(Serialize, Clone)]
pub struct Merchant {
    pub name: String,
    /// Entity id in the registry (M6.2).
    pub ent: EntityId,
    /// Home settlement id.
    pub home: SettlementId,
    /// Month they took to the roads.
    pub born: i64,
    /// Whole coin on the wire (E4.2) — displayed rounded.
    #[serde(serialize_with = "crate::util::ser_round_i64")]
    pub wealth: f64,
    pub alive: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fate: String,
}

const MERCHANT_CAP: usize = 10;

/// Spawn, trade, retire. Merchants buy where a good is cheap, sell where
/// it is dear, take their cut and leave both prices a little closer.
#[allow(clippy::too_many_arguments)]
pub fn merchant_pass(
    eco: &mut Economy,
    peoples: &mut Peoples,
    routes: &[Route],
    taken: &mut HashSet<String>,
    month_abs: i64,
    rng: &mut Pcg64Mcg,
    reg: &mut Registry,
    by_id: &HashMap<SettlementId, usize>,
) -> Vec<Event> {
    let Economy { merchants, areas, .. } = eco;
    let Peoples { settlements, peoples: cultures, societies: socs, .. } = peoples;
    let mut events = Vec::new();
    if areas.markets.len() < 2 {
        return events;
    }
    let salted = areas_with_salt(areas, settlements);

    // which areas the roads actually join (merchants follow routes) —
    // M14.7: remember the CHEAPEST leg joining each pair, because that is
    // the leg a merchant would take and the freight their cargo pays
    let mut linked: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for r in routes {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        if ka != kb {
            let e = linked.entry((ka.min(kb), ka.max(kb))).or_insert(f64::MAX);
            *e = e.min(r.cost.max(1.0));
        }
    }
    let c0 = median_cost(routes);

    // --- new blood: once a year, coin-wise cultures send out a trader
    if month_abs.rem_euclid(12) == 5 {
        let alive = merchants.iter().filter(|m| m.alive).count();
        if alive < MERCHANT_CAP {
            for (ci, cu) in cultures.iter().enumerate() {
            let cid = PeopleId(ci);
                if !socs.get(ci).map_or(false, |so| so.knows(TechId::Coin)) {
                    continue;
                }
                let of_culture = merchants
                    .iter()
                    .filter(|m| {
                        m.alive
                            && by_id
                                .get(&m.home)
                                .map_or(false, |&i| settlements[i].people == cid)
                    })
                    .count();
                if of_culture >= 2 || rng.gen::<f64>() > 0.30 {
                    continue;
                }
                // the richest sizeable town of the people sends its trader
                let Some(home) = settlements
                    .iter()
                    .filter(|s| s.people == cid && s.pop >= 2500)
                    .max_by(|a, b| a.wealth.partial_cmp(&b.wealth).unwrap())
                else {
                    continue;
                };
                let name = naming::coin(rng, &cu.style, taken).word;
                let ent = reg.add_person(&name, "merchant", month_abs, Some(cid));
                events.push(Event {
                    m: month_abs,
                    s: home.name.clone(),
                    k: EventKind::Trade,
                    text: format!(
                        "{} of {} takes to the roads with a mule-train and a ledger.",
                        name, home.name
                    ),
                    ids: smallvec![ent],
                    x: home.x,
                    y: home.y,
                    ..Default::default()
                });
                merchants.push(Merchant {
                    name,
                    ent,
                    home: home.id,
                    born: month_abs,
                    wealth: 10.0,
                    alive: true,
                    fate: String::new(),
                });
            }
        }
    }

    // --- the season's runs
    let hub_names: Vec<String> = areas
        .hubs
        .iter()
        .map(|id| {
            by_id
                .get(id)
                .map(|&i| settlements[i].name.clone())
                .unwrap_or_default()
        })
        .collect();
    let hub_name = |k: usize| -> String { hub_names.get(k).cloned().unwrap_or_default() };
    for mi in 0..merchants.len() {
        if !merchants[mi].alive {
            continue;
        }
        let Some(&hi) = by_id.get(&merchants[mi].home) else {
            merchants[mi].alive = false;
            merchants[mi].fate = "their town lost to the map".to_string();
            continue;
        };
        let ka = areas.area_of(hi);
        // widest gap from home market to any route-linked area
        let mut best: Option<(f64, Good, usize)> = None; // (gap, good, other)
        let goods: Vec<Good> = areas.markets[ka].keys().collect();
        for (&(x, y), &leg) in &linked {
            let kb = if x == ka {
                y
            } else if y == ka {
                x
            } else {
                continue;
            };
            for &g in &goods {
                // M14.2 — merchants carry salt-fish, never fresh: no yard
                // in either market, no perishable run
                if crate::resources::salt_cured(g)
                    && !(salted.get(ka).copied().unwrap_or(false)
                        || salted.get(kb).copied().unwrap_or(false))
                {
                    continue;
                }
                // M14.7 — the margin is what survives the freight: a
                // grain gap across the world loses to a gem gap next door
                let gap = (areas.markets[kb].price(g) - areas.markets[ka].price(g)).abs()
                    * carriage(g, leg, c0);
                if best.as_ref().map_or(true, |(bg, _, _)| gap > *bg) {
                    best = Some((gap, g, kb));
                }
            }
        }
        if let Some((gap, good, kb)) = best {
            if gap > 0.15 {
                let skill = (1.0 + merchants[mi].wealth / 400.0).min(2.0);
                let profit = 0.5 * gap * skill;
                merchants[mi].wealth = round2(merchants[mi].wealth + 0.6 * profit);
                settlements[hi].wealth = round2(settlements[hi].wealth + 0.4 * profit);
                // their loads close the gap faster than the ambient trade
                let pa = areas.markets[ka].price(good);
                let pb = areas.markets[kb].price(good);
                let mid = 0.5 * (pa + pb);
                areas.markets[ka].set(good, round2(pa + (mid - pa) * 0.04));
                areas.markets[kb].set(good, round2(pb + (mid - pb) * 0.04));
                if gap > 2.0 && rng.gen::<f64>() < 0.10 {
                    let (cheap, dear) = if pa < pb { (ka, kb) } else { (kb, ka) };
                    let prior = reg.mention(merchants[mi].ent);
                    let told = if prior >= 2 {
                        format!("{} — the ledger known on every quay —", merchants[mi].name)
                    } else {
                        merchants[mi].name.clone()
                    };
                    events.push(Event {
                        m: month_abs,
                        s: merchants[mi].name.clone(),
                        k: EventKind::Trade,
                        text: format!(
                            "{} brings {} out of the {} market to {}; the price breaks by the quay.",
                            told,
                            good,
                            hub_name(cheap),
                            hub_name(dear)
                        ),
                        ids: smallvec![merchants[mi].ent],
                        x: settlements[hi].x,
                        y: settlements[hi].y,
                        ..Default::default()
                    });
                }
            }
        }
        // --- the road is long
        let age = month_abs - merchants[mi].born;
        if age > 360 {
            merchants[mi].alive = false;
            merchants[mi].fate = "retired rich".to_string();
            let half = merchants[mi].wealth * 0.5;
            settlements[hi].wealth = round2(settlements[hi].wealth + half);
            reg.close(
                merchants[mi].ent,
                month_abs,
                &format!("retired rich; {} inherited the fortune", settlements[hi].name),
            );
            events.push(Event {
                m: month_abs,
                s: merchants[mi].name.clone(),
                k: EventKind::Trade,
                text: format!(
                    "{} hangs up the ledger after thirty years on the roads; {} inherits the fortune.",
                    merchants[mi].name, settlements[hi].name
                ),
                ids: smallvec![merchants[mi].ent],
                x: settlements[hi].x,
                y: settlements[hi].y,
                ..Default::default()
            });
        } else if rng.gen::<f64>() < 0.0015 {
            merchants[mi].alive = false;
            merchants[mi].fate = "lost on the road".to_string();
            reg.close(merchants[mi].ent, month_abs, "lost on the road");
            events.push(Event {
                m: month_abs,
                s: merchants[mi].name.clone(),
                k: EventKind::Trade,
                text: format!(
                    "Word comes that {}'s caravan never reached the pass. The road keeps what it takes.",
                    merchants[mi].name
                ),
                ids: smallvec![merchants[mi].ent],
                ..Default::default()
            });
        }
    }
    merchants.retain(|m| m.alive || !m.fate.is_empty());
    events
}

// ---------------------------------------------------------------- monthly

const GUILD_WORKS: [&str; 3] = [
    "The guilds of {S} gild the harbour gate and pave the quays in white stone.",
    "The guilds of {S} throw a stone bridge over the river, and the toll-box sings.",
    "The guilds of {S} raise a covered market whose roof gleams like a dragon's back.",
];

/// Incomes, taxes, guild works and market talk for one month.
/// `route_flow` is filled with this month's realized flow per route —
/// the harness checks it against the gravity model (M5.4).
pub fn monthly(
    eco: &mut Economy,
    peoples: &mut Peoples,
    routes: &[Route],
    month_abs: i64,
    rng: &mut Pcg64Mcg,
    by_id: &HashMap<SettlementId, usize>,
) -> Vec<Event> {
    // M14.9 — the tastes of the peoples enter the books
    let people_style: Vec<usize> = peoples
        .peoples
        .iter()
        .map(|p| crate::culture::style_index(&p.style))
        .collect();
    let Economy { market, areas, route_flow, .. } = eco;
    let Peoples { settlements, realms, societies: socs, .. } = peoples;
    let mut events = Vec::new();
    update_prices(market, settlements, &people_style);
    update_area_prices(areas, settlements, market, &people_style);
    let salted = areas_with_salt(areas, settlements);
    equalize_along_routes(areas, settlements, routes, by_id, &salted);

    let mods: Vec<society::Mods> = socs.iter().map(society::mods_for).collect();


    // --- trade first (read phase): every route pays both ends.
    // Carriage pays a little; ARBITRAGE pays well — the spread between the
    // two ends' market areas is where the trade income now lives (M5.3).
    let mut trade_income = vec![0.0f64; settlements.len()];
    route_flow.clear();
    route_flow.resize(routes.len(), 0.0);
    // the median leg cost sets the distance scale for attenuation (M5.4):
    // scale-free, so re-tuning route costing never re-tunes the economy
    let c0 = median_cost(routes);
    for (ri, r) in routes.iter().enumerate() {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        // M37 — the ice shuts the lane: an icebound month moves no
        // cargo and pays nobody; route_flow keeps the zero on record.
        if r.closed & (1u16 << month_abs.rem_euclid(12)) != 0 {
            continue;
        }
        let (sa, sb) = (&settlements[ia], &settlements[ib]);
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        let pa = market.price(sa.exports.unwrap_or(Good::Grain));
        let pb = market.price(sb.exports.unwrap_or(Good::Grain));
        // gravity carriage (M5.4): cargo scales with BOTH ends' masses and
        // thins with distance — big close pairs carry the trade
        let mass = ((sa.pop as f64 * sb.pop as f64).sqrt() / 1400.0).clamp(0.05, 5.0);
        let att = c0 / (c0 + r.cost.max(1.0));
        // the goods either end actually ships, priced in each end's market
        let mut gap = 0.0;
        if ka != kb && ka < areas.markets.len() && kb < areas.markets.len() {
            let goods: GoodSet = sa
                .goods
                .iter()
                .chain(sb.goods.iter())
                .copied()
                .collect();
            let mut gaps: Vec<f64> = goods
                .iter()
                // M14.2 — un-salted perishables earn nothing across the
                // border: no yard, no salt-fish trade
                .filter(|&g| {
                    !crate::resources::salt_cured(g)
                        || salted.get(ka).copied().unwrap_or(false)
                        || salted.get(kb).copied().unwrap_or(false)
                })
                // M14.7 — a gap only earns what the cargo class can haul:
                // bulk gaps die on long overland legs, precious gaps pay
                .map(|g| {
                    (areas.markets[ka].price(g) - areas.markets[kb].price(g)).abs()
                        * carriage(g, r.cost, c0)
                })
                .collect();
            gaps.sort_by(|a, b| b.partial_cmp(a).unwrap());
            gap = gaps.into_iter().take(4).sum();
        }
        let mut flow = (0.12 * 0.5 * (pa + pb) + 0.55 * gap) * r.w * mass * att;
        // barge legs ride the seasons: high water carries more cargo
        if r.ramp != 0.0 {
            let phase = (2.0 * std::f64::consts::PI
                * (month_abs.rem_euclid(12)) as f64 / 12.0)
                .cos();
            flow *= (1.0 + 0.5 * r.ramp * phase).max(0.4);
        }
        // M48 — the monsoon trades sail a double-frequency year: both
        // monsoon heights carry cargo, the turns of the wind becalm it.
        // (Burst months never reach here — they sit in `closed` above.)
        if r.season != 0.0 {
            flow *= crate::trade::season_mult(r.season, month_abs);
        }
        let ta = mods.get(sa.people.idx()).map(|m| m.trade).unwrap_or(1.0);
        let tb = mods.get(sb.people.idx()).map(|m| m.trade).unwrap_or(1.0);
        // a harbour works the cranes: more cargo through, more dues taken
        let ha = if sa.port { 1.25 } else { 1.0 };
        let hb = if sb.port { 1.25 } else { 1.0 };
        trade_income[ia] += flow * ta * ha;
        trade_income[ib] += flow * tb * hb;
        route_flow[ri] = flow * 0.5 * (ta * ha + tb * hb);
    }

    // --- production, upkeep, tithe
    let total_pop: i64 = settlements.iter().map(|s| s.pop).sum();
    let total_wealth: f64 = settlements.iter().map(|s| s.wealth).sum();
    // same luxury formula as update_prices — one taste, two ledgers
    let luxury = (total_wealth / (total_pop.max(1) as f64) / 4.0).min(0.5);
    for (i, s) in settlements.iter_mut().enumerate() {
        let m = mods.get(s.people.idx()).cloned().unwrap_or_default();
        // M2.4 Bettencourt: socio-economic output scales superlinearly with
        // town size (∝ pop^1.15) — denser streets, faster deals — while
        // infrastructure upkeep scales sublinearly (∝ pop^0.85): shared
        // walls, shared wells. Cities out-earn their size; villages don't.
        let workforce = (s.pop as f64 / 800.0).powf(1.15);
        // even the goodless town tills, fishes and hauls: a subsistence
        // floor so big farm towns never book zero wealth forever
        let mut production = (0.50 + 0.50 * s.food.min(1.5)) * market.price(Good::Grain);
        for (gi, &g) in s.goods.iter().enumerate() {
            // a town sells at ITS OWN market's price — the local ledger,
            // not the world blend (M5.2)
            let p = areas
                .market_of(i)
                .map(|mk| mk.price(g))
                .unwrap_or_else(|| market.price(g));
            production += p * 0.85f64.powi(gi as i32);
        }
        // M2.4: agglomeration — smiths, weavers, scribes, inns. City crafts
        // and services don't depend on what the hinterland exports; a fish
        // glut must not make a metropolis poor. Scales with the world's
        // taste for luxury, rides the pop^1.15 workforce like everything.
        production += 0.55 * luxury;
        production *= workforce * m.production;
        let upkeep = 1.05 * (s.pop as f64 / 1500.0).powf(0.85);
        let income = production + trade_income[i] - upkeep;
        s.wealth = round2((s.wealth + income).max(0.0));
        if income > 0.0 {
            // ADR-0018 — the tithe follows the banner: coin flows to the
            // crown that rules the town, not to the tongue spoken in it.
            if let Some(realm) = realms.get_mut(s.realm.0) {
                realm.treasury = round2(realm.treasury + 0.08 * income);
            }
        }

        // --- guild works: a rich town spends its coin on stone and show.
        // The spending threshold scales like output (∝ pop^1.15), not like
        // headcount — a linear cap would pin wealth ∝ pop and erase the
        // Bettencourt signal the production side just built (M2.4).
        let show_off = 1.6 * 800.0 * (s.pop as f64 / 800.0).powf(1.15);
        if s.wealth > show_off && s.wealth > 500.0 && rng.gen::<f64>() < 0.008 {
            s.wealth = round2(s.wealth * 0.65);
            s.pop += (s.pop / 80).max(5);
            let which = if s.coastal {
                GUILD_WORKS[0]
            } else if s.river {
                GUILD_WORKS[1]
            } else {
                GUILD_WORKS[2]
            };
            events.push(Event {
                m: month_abs,
                s: s.name.clone(),
                k: EventKind::Wonder,
                text: which.replace("{S}", &s.name),
                // anchor the ground (M9.2)
                x: s.x,
                y: s.y,
                ..Default::default()
            });
        }
    }

    // --- market talk: shortages and gluts worth a line in the chronicle
    if rng.gen::<f64>() < 0.010 {
        let mut dearest: Option<(Good, f64)> = None;
        let mut cheapest: Option<(Good, f64)> = None;
        for (g, p) in market.iter_some() {
            let ratio = p / base_value(g);
            if dearest.map_or(true, |(_, r)| ratio > r) {
                dearest = Some((g, ratio));
            }
            if cheapest.map_or(true, |(_, r)| ratio < r) {
                cheapest = Some((g, ratio));
            }
        }
        // anchor market events to the loudest producer's ground so the
        // second reading (resolve_events) can always find them a subject —
        // a good's name is vocabulary, not an entity, and resolves to nothing.
        let producer_of = |g: crate::resources::Good| {
            settlements
                .iter()
                .filter(|s| s.goods.contains(&g))
                .max_by_key(|s| s.pop)
                .map(|s| (s.name.clone(), s.x, s.y))
        };
        if let Some((g, r)) = dearest {
            if r > 2.2 {
                let (producer, px, py) = producer_of(g)
                    .unwrap_or_else(|| ("distant ports".to_string(), -1, -1));
                events.push(Event {
                    m: month_abs,
                    s: g.to_string(),
                    k: EventKind::Trade,
                    text: format!(
                        "{} fetches many times its old price; caravans race for {}.",
                        capitalize(g.name()), producer
                    ),
                    x: px,
                    y: py,
                    ..Default::default()
                });
            }
        }
        if events.is_empty() {
            if let Some((g, r)) = cheapest {
                if r < 0.5 {
                    let (px, py) = producer_of(g).map(|(_, x, y)| (x, y)).unwrap_or((-1, -1));
                    events.push(Event {
                        m: month_abs,
                        s: g.to_string(),
                        k: EventKind::Trade,
                        text: format!(
                            "The bottom falls out of the {} trade; warehouses overflow and merchants weep.",
                            g
                        ),
                        x: px,
                        y: py,
                        ..Default::default()
                    });
                }
            }
        }
    }

    events
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;
use crate::state::{Economy, Peoples};

/// Diagnostics bands (E2.7): prices and the spread of wealth.
pub const BANDS: &[Band] = &[
    Band { name: "max pinned price share", sweet: (0.0, 0.25), hard: (0.0, 0.55), target: "sweet ≤25% · hard ≤55%" },
    Band { name: "wealth gini", sweet: (0.20, 0.80), hard: (0.05, 0.95), target: "sweet 0.20–0.80 — some inequality, no monopoly" },
    // M14.7 re-based: with class-split openness the mean rides on bulk's
    // deliberate dispersion (grain is priced by the valley that grew it),
    // so the sweet ceiling moves 3.0→4.5; the hard wall stays a wall.
    Band { name: "wild collapse share", sweet: (0.0, 0.25), hard: (0.0, 0.5), target: "M14.8: overharvest bites somewhere, never everywhere" },
    Band { name: "inter-area price divergence", sweet: (1.03, 4.5), hard: (1.0, 6.0), target: "M5.2 gate: local markets disagree, but not madly" },
    Band { name: "wealth~pop scaling β", sweet: (0.90, 1.60), hard: (0.50, 2.10), target: "M2.4: superlinear output, target ≈1.15" },
    Band { name: "iron/grain price ratio", sweet: (1.5, 14.0), hard: (0.8, 40.0), target: "M2.7: metal dear, bread cheap" },
    Band { name: "gold/grain price ratio", sweet: (2.5, 80.0), hard: (1.2, 300.0), target: "M2.7: the precious envelope" },
    Band { name: "salt/grain price ratio", sweet: (0.8, 10.0), hard: (0.4, 40.0), target: "M14.2: dear inland, never gold — the Hodges envelope" },
    Band { name: "wool/grain price ratio", sweet: (1.0, 12.0), hard: (0.5, 40.0), target: "M14.3: the staple export — dearer than bread, cheaper than iron" },
];
