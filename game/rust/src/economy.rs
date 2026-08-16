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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use enum_map::EnumMap;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ids::{CultureId, EntityId, SettlementId};
use crate::culture::Culture;
use crate::entity::Registry;
use crate::naming;
use crate::resources::{Abundance, Good, GoodSet};
use crate::settlements::Settlement;
use crate::society::{self, Society, TechId};
use crate::trade::Route;
use crate::util::round2;
use crate::world::EventKind;
use crate::world::Event;

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
fn compute_prices<'a, I>(market: &mut Market, members: I, catalogue: Option<GoodSet>)
where
    I: Iterator<Item = &'a Settlement>,
{
    let mut supply: EnumMap<Good, Option<f64>> = EnumMap::default();
    let mut total_pop: i64 = 0;
    let mut total_wealth = 0.0;
    for s in members {
        total_pop += s.pop;
        total_wealth += s.wealth;
        for (i, &g) in s.goods.iter().enumerate() {
            let w = (s.pop as f64 / 1000.0) * 0.7f64.powi(i as i32);
            *supply[g].get_or_insert(0.0) += w;
        }
    }
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
            let p = (demand_weight(g, luxury) + 0.02) / (sv + 0.02);
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
            let target = (base * (p / gm).powf(0.55)).clamp(0.3 * base, 5.0 * base);
            let old = market.price(g);
            next[g] = Some(round2(0.75 * old + 0.25 * target));
        }
    }
    market.prices = next;
}

/// Recompute WORLD prices from relative scarcity (the blended ledger the
/// UI's market tab and explain.rs read).
pub fn update_prices(market: &mut Market, settlements: &[Settlement]) {
    compute_prices(market, settlements.iter(), None);
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
pub fn update_area_prices(areas: &mut MarketAreas, settlements: &[Settlement], world: &Market) {
    const OPENNESS: f64 = 0.5; // how hard the world blend pulls on a local book
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
        compute_prices(&mut areas.markets[k], members.into_iter(), Some(catalogue));
        let mk = &mut areas.markets[k];
        for g in catalogue.iter() {
            if let Some(local) = mk.prices[g] {
                let anchor = world.price(g);
                mk.prices[g] = Some(round2(local + (anchor - local) * OPENNESS));
            }
        }
    }
}

/// Every route that crosses an area border drags the two price books
/// toward each other — trade equalizes what it touches (M5.3).
fn equalize_along_routes(
    areas: &mut MarketAreas,
    settlements: &[Settlement],
    routes: &[Route],
    by_id: &HashMap<SettlementId, usize>,
) {
    for r in routes {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        if ka == kb || ka >= areas.markets.len() || kb >= areas.markets.len() {
            continue;
        }
        let rate = 0.05 * r.w.min(1.0);
        let goods: GoodSet = settlements[ia]
            .goods
            .iter()
            .chain(settlements[ib].goods.iter())
            .copied()
            .collect();
        for g in goods.iter() {
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
struct Recipe {
    out: Good,
    /// any one of these arts unlocks the craft
    tech_any: &'static [TechId],
    /// workforce gate: no finished goods from a hamlet
    min_pop: i64,
    /// any one of these among the town's goods feeds the forge
    ore_any: &'static [Good],
    /// the forge burns coal or charcoal (timber)
    needs_fuel: bool,
}

const RECIPES: [Recipe; 3] = [
    Recipe { out: Good::Tools, tech_any: &[TechId::Bronze, TechId::Iron], min_pop: 1200, ore_any: &[Good::Copper, Good::Iron], needs_fuel: true },
    Recipe { out: Good::Weapons, tech_any: &[TechId::Steel], min_pop: 2500, ore_any: &[Good::Iron], needs_fuel: true },
    Recipe { out: Good::Jewelry, tech_any: &[TechId::Coin], min_pop: 2000, ore_any: &[Good::Gold, Good::Silver], needs_fuel: false },
];

const FORGE_LIT: [&str; 3] = [
    "The smiths of {S} ring day and night — {G} of {S} make are sold in every port.",
    "{S} raises a guildhall of hammers: its {G} carry the town's mark now.",
    "Ore goes into {S} and {G} come out; the caravans pay in silver.",
];

/// Towns with ore, fuel, hands and the art work it into finished goods.
/// Returns chronicle lines for forges newly lit or newly gone cold.
pub fn craft_pass(
    settlements: &mut [Settlement],
    socs: &[Society],
    areas: &MarketAreas,
    month_abs: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
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
        let Some(soc) = socs.get(s.culture.idx()) else { continue };
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
                let t = FORGE_LIT[rng.gen_range(0..FORGE_LIT.len())];
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Trade,
                    text: t.replace("{S}", &s.name).replace("{G}", rc.out.name()),
                    ..Default::default()
                });
            } else if !eligible && has {
                s.goods.retain(|g| *g != rc.out);
                if s.exports == Some(rc.out) {
                    s.exports = s.goods.first().copied();
                }
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Trade,
                    text: format!(
                        "The forges of {} go cold — no ore comes, and the {} trade dies with them.",
                        s.name, rc.out
                    ),
                    ..Default::default()
                });
            }
        }
    }
    events
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
    merchants: &mut Vec<Merchant>,
    settlements: &mut [Settlement],
    areas: &mut MarketAreas,
    routes: &[Route],
    socs: &[Society],
    cultures: &[Culture],
    taken: &mut HashSet<String>,
    month_abs: i64,
    rng: &mut Pcg64Mcg,
    reg: &mut Registry,
    by_id: &HashMap<SettlementId, usize>,
) -> Vec<Event> {
    let mut events = Vec::new();
    if areas.markets.len() < 2 {
        return events;
    }

    // which areas the roads actually join (merchants follow routes)
    let mut linked: BTreeSet<(usize, usize)> = BTreeSet::new();
    for r in routes {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        if ka != kb {
            linked.insert((ka.min(kb), ka.max(kb)));
        }
    }

    // --- new blood: once a year, coin-wise cultures send out a trader
    if month_abs.rem_euclid(12) == 5 {
        let alive = merchants.iter().filter(|m| m.alive).count();
        if alive < MERCHANT_CAP {
            for (ci, cu) in cultures.iter().enumerate() {
            let cid = CultureId(ci);
                if !socs.get(ci).map_or(false, |so| so.knows(TechId::Coin)) {
                    continue;
                }
                let of_culture = merchants
                    .iter()
                    .filter(|m| {
                        m.alive
                            && by_id
                                .get(&m.home)
                                .map_or(false, |&i| settlements[i].culture == cid)
                    })
                    .count();
                if of_culture >= 2 || rng.gen::<f64>() > 0.30 {
                    continue;
                }
                // the richest sizeable town of the people sends its trader
                let Some(home) = settlements
                    .iter()
                    .filter(|s| s.culture == cid && s.pop >= 2500)
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
        for &(x, y) in &linked {
            let kb = if x == ka {
                y
            } else if y == ka {
                x
            } else {
                continue;
            };
            for &g in &goods {
                let gap = (areas.markets[kb].price(g) - areas.markets[ka].price(g)).abs();
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
    settlements: &mut [Settlement],
    routes: &[Route],
    market: &mut Market,
    areas: &mut MarketAreas,
    route_flow: &mut Vec<f64>,
    socs: &mut [Society],
    month_abs: i64,
    rng: &mut Pcg64Mcg,
    by_id: &HashMap<SettlementId, usize>,
) -> Vec<Event> {
    let mut events = Vec::new();
    update_prices(market, settlements);
    update_area_prices(areas, settlements, market);
    equalize_along_routes(areas, settlements, routes, by_id);

    let mods: Vec<society::Mods> = socs.iter().map(society::mods_for).collect();


    // --- trade first (read phase): every route pays both ends.
    // Carriage pays a little; ARBITRAGE pays well — the spread between the
    // two ends' market areas is where the trade income now lives (M5.3).
    let mut trade_income = vec![0.0f64; settlements.len()];
    route_flow.clear();
    route_flow.resize(routes.len(), 0.0);
    // the median leg cost sets the distance scale for attenuation (M5.4):
    // scale-free, so re-tuning route costing never re-tunes the economy
    let c0 = {
        let mut cs: Vec<f64> = routes.iter().map(|r| r.cost.max(1.0)).collect();
        if cs.is_empty() {
            50.0
        } else {
            cs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            cs[cs.len() / 2]
        }
    };
    for (ri, r) in routes.iter().enumerate() {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
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
                .map(|g| (areas.markets[ka].price(g) - areas.markets[kb].price(g)).abs())
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
        let ta = mods.get(sa.culture.idx()).map(|m| m.trade).unwrap_or(1.0);
        let tb = mods.get(sb.culture.idx()).map(|m| m.trade).unwrap_or(1.0);
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
        let m = mods.get(s.culture.idx()).cloned().unwrap_or_default();
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
            if let Some(soc) = socs.get_mut(s.culture.idx()) {
                soc.treasury = round2(soc.treasury + 0.08 * income);
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
        if let Some((g, r)) = dearest {
            if r > 2.2 {
                let producer = settlements
                    .iter()
                    .filter(|s| s.goods.contains(&g))
                    .max_by_key(|s| s.pop)
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "distant ports".to_string());
                events.push(Event {
                    m: month_abs,
                    s: g.to_string(),
                    k: EventKind::Trade,
                    text: format!(
                        "{} fetches many times its old price; caravans race for {}.",
                        capitalize(g.name()), producer
                    ),
                    ..Default::default()
                });
            }
        }
        if events.is_empty() {
            if let Some((g, r)) = cheapest {
                if r < 0.5 {
                    events.push(Event {
                        m: month_abs,
                        s: g.to_string(),
                        k: EventKind::Trade,
                        text: format!(
                            "The bottom falls out of the {} trade; warehouses overflow and merchants weep.",
                            g
                        ),
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

/// Diagnostics bands (E2.7): prices and the spread of wealth.
pub const BANDS: &[Band] = &[
    Band { name: "max pinned price share", sweet: (0.0, 0.25), hard: (0.0, 0.55), target: "sweet ≤25% · hard ≤55%" },
    Band { name: "wealth gini", sweet: (0.20, 0.80), hard: (0.05, 0.95), target: "sweet 0.20–0.80 — some inequality, no monopoly" },
];
