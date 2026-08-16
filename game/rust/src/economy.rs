//! Economy — a real market over the goods the land yields. Every good has
//! a base worth by rarity; monthly world prices move with supply (who
//! produces it, weighted by workforce) against demand (mouths, categories,
//! and the taste for luxury that wealth brings). Settlements earn coin
//! from production and from the routes they sit on, pay their upkeep,
//! and tithe their people's treasury. Deterministic: BTreeMaps only.

use std::collections::{BTreeMap, HashMap};

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde_json::{json, Value};

use crate::resources::{abundance, isa_chain};
use crate::settlements::Settlement;
use crate::society::{self, Society};
use crate::trade::Route;
use crate::util::round2;
use crate::world::Event;

/// Base worth of one unit of a good, from its rarity in the world.
pub fn base_value(good: &str) -> f64 {
    let mut v = match abundance(good) {
        "uncommon" => 1.8,
        "rare" => 3.4,
        "legendary" => 7.0,
        _ => 1.0,
    };
    if isa_chain(good).iter().any(|s| s == "food") {
        v *= 0.85;
    }
    round2(v)
}

fn demand_weight(good: &str, luxury: f64) -> f64 {
    let chain = isa_chain(good);
    if chain.iter().any(|s| s == "food") {
        1.15
    } else if chain.iter().any(|s| s == "metal") {
        0.55 + luxury
    } else if chain.iter().any(|s| s == "material") {
        0.60 + 0.5 * luxury
    } else {
        0.45 + luxury
    }
}

// ---------------------------------------------------------------- market

#[derive(Default)]
pub struct Market {
    pub prices: BTreeMap<String, f64>,
    prev: BTreeMap<String, f64>,
}

impl Market {
    pub fn price(&self, good: &str) -> f64 {
        *self.prices.get(good).unwrap_or(&base_value(good))
    }

    /// Rows for the client: {g, p, b, t} sorted dearest first.
    pub fn snapshot(&self) -> Value {
        let mut rows: Vec<(&String, f64, f64, f64)> = self
            .prices
            .iter()
            .map(|(g, &p)| {
                let prev = *self.prev.get(g).unwrap_or(&p);
                (g, p, base_value(g), round2(p - prev))
            })
            .collect();
        rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Value::Array(
            rows.into_iter()
                .map(|(g, p, b, t)| json!({ "g": g, "p": p, "b": b, "t": t }))
                .collect(),
        )
    }

    /// A sudden strike or a dead mine jolts the price at once, before the
    /// slow-moving supply average has time to catch up.
    pub fn shock(&mut self, good: &str, factor: f64) {
        let p = self.price(good);
        let b = base_value(good);
        self.prices
            .insert(good.to_string(), round2((p * factor).clamp(0.3 * b, 5.0 * b)));
    }
}

/// Recompute world prices from *relative* scarcity. Demand pressure per
/// good is measured against the whole market's geometric mean, so a growing
/// world doesn't inflate every price toward the clamp at once: goods scarcer
/// than the market average rise above their base worth, plentiful staples
/// sink below it, and the clamps only catch the true extremes.
pub fn update_prices(market: &mut Market, settlements: &[Settlement]) {
    let mut supply: BTreeMap<String, f64> = BTreeMap::new();
    let total_pop: i64 = settlements.iter().map(|s| s.pop).sum();
    let total_wealth: f64 = settlements.iter().map(|s| s.wealth).sum();
    for s in settlements {
        for (i, g) in s.goods.iter().enumerate() {
            let w = (s.pop as f64 / 1000.0) * 0.7f64.powi(i as i32);
            *supply.entry(g.clone()).or_insert(0.0) += w;
        }
    }
    let luxury = (total_wealth / (total_pop.max(1) as f64) / 4.0).min(0.5);

    market.prev = market.prices.clone();
    let mut pressure: BTreeMap<String, f64> = BTreeMap::new();
    for (g, s) in &supply {
        pressure.insert(g.clone(), (demand_weight(g, luxury) + 0.02) / (s + 0.02));
    }
    if pressure.is_empty() {
        market.prices.clear();
        return;
    }
    let gm = (pressure.values().map(|p| p.ln()).sum::<f64>() / pressure.len() as f64).exp();
    let mut next: BTreeMap<String, f64> = BTreeMap::new();
    for (g, p) in &pressure {
        let base = base_value(g);
        let target = (base * (p / gm).powf(0.55)).clamp(0.3 * base, 5.0 * base);
        let old = market.price(g);
        next.insert(g.clone(), round2(0.75 * old + 0.25 * target));
    }
    market.prices = next;
}

// ---------------------------------------------------------------- monthly

const GUILD_WORKS: [&str; 3] = [
    "The guilds of {S} gild the harbour gate and pave the quays in white stone.",
    "The guilds of {S} throw a stone bridge over the river, and the toll-box sings.",
    "The guilds of {S} raise a covered market whose roof gleams like a dragon's back.",
];

/// Incomes, taxes, guild works and market talk for one month.
pub fn monthly(
    settlements: &mut [Settlement],
    routes: &[Route],
    market: &mut Market,
    socs: &mut [Society],
    month_abs: i64,
    rng: &mut Pcg64Mcg,
) -> Vec<Event> {
    let mut events = Vec::new();
    update_prices(market, settlements);

    let mods: Vec<society::Mods> = socs.iter().map(society::mods_for).collect();
    let by_id: HashMap<i64, usize> = settlements
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();

    // --- trade first (read phase): every route pays both ends
    let mut trade_income = vec![0.0f64; settlements.len()];
    for r in routes {
        let (Some(&ia), Some(&ib)) = (by_id.get(&r.a), by_id.get(&r.b)) else {
            continue;
        };
        let (sa, sb) = (&settlements[ia], &settlements[ib]);
        let pa = market.price(sa.exports.as_deref().unwrap_or("grain"));
        let pb = market.price(sb.exports.as_deref().unwrap_or("grain"));
        let flow = 0.30 * r.w * 0.5 * (pa + pb)
            * (sa.pop.min(sb.pop) as f64 / 600.0).min(1.0);
        let ta = mods.get(sa.culture).map(|m| m.trade).unwrap_or(1.0);
        let tb = mods.get(sb.culture).map(|m| m.trade).unwrap_or(1.0);
        // a harbour works the cranes: more cargo through, more dues taken
        let ha = if sa.port { 1.25 } else { 1.0 };
        let hb = if sb.port { 1.25 } else { 1.0 };
        trade_income[ia] += flow * ta * ha;
        trade_income[ib] += flow * tb * hb;
    }

    // --- production, upkeep, tithe
    for (i, s) in settlements.iter_mut().enumerate() {
        let m = mods.get(s.culture).cloned().unwrap_or_default();
        let workforce = (s.pop as f64 / 800.0).powf(0.75);
        let mut production = 0.0;
        for (gi, g) in s.goods.iter().enumerate() {
            production += market.price(g) * 0.85f64.powi(gi as i32);
        }
        production *= workforce * m.production;
        let upkeep = s.pop as f64 / 1500.0;
        let income = production + trade_income[i] - upkeep;
        s.wealth = round2((s.wealth + income).max(0.0));
        if income > 0.0 {
            if let Some(soc) = socs.get_mut(s.culture) {
                soc.treasury = round2(soc.treasury + 0.08 * income);
            }
        }

        // --- guild works: a rich town spends its coin on stone and show
        if s.wealth > 1.6 * s.pop as f64 && s.wealth > 500.0 && rng.gen::<f64>() < 0.008 {
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
                k: "wonder".to_string(),
                text: which.replace("{S}", &s.name),
            });
        }
    }

    // --- market talk: shortages and gluts worth a line in the chronicle
    if rng.gen::<f64>() < 0.010 {
        let mut dearest: Option<(&String, f64)> = None;
        let mut cheapest: Option<(&String, f64)> = None;
        for (g, &p) in &market.prices {
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
                    .filter(|s| s.goods.iter().any(|x| x == g))
                    .max_by_key(|s| s.pop)
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "distant ports".to_string());
                events.push(Event {
                    m: month_abs,
                    s: g.clone(),
                    k: "trade".to_string(),
                    text: format!(
                        "{} fetches many times its old price; caravans race for {}.",
                        capitalize(g), producer
                    ),
                });
            }
        }
        if events.is_empty() {
            if let Some((g, r)) = cheapest {
                if r < 0.5 {
                    events.push(Event {
                        m: month_abs,
                        s: g.clone(),
                        k: "trade".to_string(),
                        text: format!(
                            "The bottom falls out of the {} trade; warehouses overflow and merchants weep.",
                            g
                        ),
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
