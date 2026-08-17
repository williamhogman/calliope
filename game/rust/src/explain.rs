//! Explain — term ledgers for derived quantities, Victoria-style "why?".
//!
//! Every ledger is an additive decomposition: the rows sum exactly to the
//! total, so the player can see *which* force moves a number and by how
//! much. The math here mirrors the live simulation term for term:
//!
//!   * settlement growth  → `World::tick_month` in world.rs
//!   * good price         → `economy::update_prices`
//!
//! If a constant changes there, it must change here — both sites carry a
//! cross-reference comment. Read-only: an explain call never mutates state.

use serde_json::{json, Value};

use crate::ids::SettlementId;
use crate::climate;
use crate::economy::{base_value, demand_weight};
use crate::resources::Good;
use crate::society;
use crate::world::World;

/// Entry point: JSON ledger for (`kind`, `key`), or `{}` when unknown.
pub fn explain(world: &World, kind: &str, key: &str) -> String {
    let v = match kind {
        "settlement" => key
            .parse::<i64>()
            .ok()
            .and_then(|id| explain_settlement(world, SettlementId(id))),
        "good" => explain_good(world, key),
        _ => None,
    };
    v.unwrap_or_else(|| json!({})).to_string()
}

// ------------------------------------------------------------- settlement

/// Monthly growth ledger: each row is the marginal souls/month added or
/// removed by one force, applied in the same order as `tick_month`.
/// Random shocks (plague, storms, fire) are events, not terms — the ledger
/// shows the deterministic expectation for the current month.
fn explain_settlement(world: &World, id: SettlementId) -> Option<Value> {
    let s = world.peoples.settlements.iter().find(|s| s.id == id)?;
    let md = world
        .societies
        .get(s.culture.idx())
        .map(society::mods_for)
        .unwrap_or_default();
    let (y, x) = (s.y as usize, s.x as usize);
    let month = world.month.rem_euclid(12);
    let t_now = climate::month_temperature(world.fields.tmean[[y, x]] as f64, world.fields.tamp[[y, x]] as f64, month);
    let pop = s.pop as f64;

    // -- multiplier chain, identical to tick_month (world.rs)
    let r0 = 0.005;
    let cold = if t_now < -8.0 {
        0.25
    } else if t_now < 0.0 {
        0.6
    } else {
        1.0
    };
    let trade = 1.0 + 0.04 * (s.connections.min(4) as f64);
    let coin = 1.0 + 0.04 * (s.wealth / (pop + 1.0)).min(1.0);
    // M2.2: K lives in settlements::capacity_at, stored on s.k each month —
    // the ledger reads the stored value so there is no second formula copy.
    let k = s.k.max(180.0);
    let crowd = 1.0 - pop / k;

    let g0 = pop * r0;
    let g1 = g0 * cold;
    let g2 = g1 * trade;
    let g3 = g2 * md.growth;
    let g4 = g3 * coin;
    let g5 = g4 * crowd;
    let port = if s.port { pop * 0.0012 } else { 0.0 };
    let total = g5 + port;

    let mut terms = vec![json!({ "l": "Hearth & kin", "v": g0 })];
    if cold < 1.0 {
        let label = if t_now < -8.0 { "Deep cold" } else { "Cold season" };
        terms.push(json!({ "l": label, "v": g1 - g0 }));
    }
    if s.connections > 0 {
        terms.push(json!({
            "l": format!("Trade routes ({})", s.connections.min(4)),
            "v": g2 - g1,
        }));
    }
    if (md.growth - 1.0).abs() > 1e-9 {
        terms.push(json!({ "l": "Arts of peace", "v": g3 - g2 }));
    }
    if coin > 1.0 + 1e-9 {
        terms.push(json!({ "l": "Coin draws folk", "v": g4 - g3 }));
    }
    terms.push(json!({
        "l": format!("Crowding ({} / {} souls)", s.pop, k.round() as i64),
        "v": g5 - g4,
    }));
    if s.port {
        terms.push(json!({ "l": "Harbour", "v": port }));
    }

    Some(json!({
        "title": "Growth this month",
        "dp": 1,
        "unit": " souls",
        "terms": terms,
        "total": total,
        "total_label": "Souls / month",
    }))
}

// ------------------------------------------------------------------- good

/// Price ledger: base worth from rarity, the scarcity premium supply and
/// demand would set today, and the inertia/shock residue between that
/// target and the smoothed market price. Rows sum to the current price.
fn explain_good(world: &World, key: &str) -> Option<Value> {
    let good: Good = key.parse().ok()?;
    // supply and demand exactly as update_prices computes them (economy.rs)
    let mut supply = 0.0f64;
    let mut worked_by = 0usize;
    let mut total_pop: i64 = 0;
    let mut total_wealth = 0.0f64;
    for s in &world.peoples.settlements {
        total_pop += s.pop;
        total_wealth += s.wealth;
        for (i, &g) in s.goods.iter().enumerate() {
            if g == good {
                supply += (s.pop as f64 / 1000.0) * 0.7f64.powi(i as i32);
                worked_by += 1;
            }
        }
    }
    if !world.economy.market.contains(good) && worked_by == 0 {
        return None; // not a good this world knows
    }
    let luxury = (total_wealth / (total_pop.max(1) as f64) / 4.0).min(0.5);

    // relative scarcity: pressure measured against the market's geometric
    // mean, exactly as `economy::update_prices` does it.
    let mut supplies: enum_map::EnumMap<Good, Option<f64>> = enum_map::EnumMap::default();
    for s in &world.peoples.settlements {
        for (i, &g) in s.goods.iter().enumerate() {
            *supplies[g].get_or_insert(0.0) +=
                (s.pop as f64 / 1000.0) * 0.7f64.powi(i as i32);
        }
    }
    let mut pressures: Vec<f64> = Vec::new();
    let mut own_pressure = None;
    for (g, sup) in &supplies {
        if let Some(sup) = sup {
            let p = (demand_weight(g, luxury) + 0.02) / (sup + 0.02);
            if g == good {
                own_pressure = Some(p);
            }
            pressures.push(p);
        }
    }
    if pressures.is_empty() {
        return None;
    }
    let gm = (pressures.iter().map(|p| p.ln()).sum::<f64>() / pressures.len() as f64).exp();
    let pressure = own_pressure
        .unwrap_or_else(|| (demand_weight(good, luxury) + 0.02) / (supply + 0.02));

    let base = base_value(good);
    let target = (base * (pressure / gm).powf(0.55)).clamp(0.3 * base, 5.0 * base);
    let price = world.economy.market.price(good);

    let scarcity_label = if target >= base {
        "Scarcity premium"
    } else {
        "Glut discount"
    };
    let residue = price - target;
    let residue_label = if residue.abs() < 0.005 {
        "Market at rest"
    } else if residue > 0.0 {
        "Old shocks fading"
    } else {
        "Price still settling"
    };

    Some(json!({
        "title": "Why this price",
        "dp": 2,
        "unit": " coin",
        "terms": [
            { "l": "Base worth (rarity)", "v": base },
            { "l": scarcity_label, "v": target - base },
            { "l": residue_label, "v": residue },
        ],
        "total": price,
        "total_label": "Price today",
    }))
}
