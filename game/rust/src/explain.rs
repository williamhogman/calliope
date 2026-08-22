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

use crate::agriculture::SoilOrder;
use crate::climate;
use crate::constants::METRES_PER_UNIT;
use crate::economy::{base_value, demand_weight};
use crate::ids::SettlementId;
use crate::landform;
use crate::resources::Good;
use crate::society;
use crate::state::CellFlags;
use crate::world::World;

/// Entry point: JSON ledger for (`kind`, `key`), or `{}` when unknown.
pub fn explain(world: &World, kind: &str, key: &str) -> String {
    let v = match kind {
        "settlement" => key
            .parse::<i64>()
            .ok()
            .and_then(|id| explain_settlement(world, SettlementId(id))),
        "good" => explain_good(world, key),
        "cell" => explain_cell(world, key),
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
        .peoples.societies
        .get(s.people.idx())
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
    // M24 — the rebuild arc: kin return while the arc is open and the
    // town still stands below its pre-disaster strength.
    let rebuild = if s.rebuild_until > 0 && s.pop < s.rebuild_peak {
        pop * 0.012
    } else {
        0.0
    };
    let total = g5 + port + rebuild;

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
    // M45 — the anchorage on the ledger: the shelter score the founding
    // and the harbour dues both priced, read from the same field.
    let shelter = world
        .shelter
        .get([y, x])
        .copied()
        .unwrap_or(0.0) as f64;
    if s.port {
        terms.push(json!({
            "l": format!("Harbour (shelter {:.2})", shelter),
            "v": port,
        }));
    }
    if rebuild > 0.0 {
        terms.push(json!({ "l": "Rebuilding", "v": rebuild }));
    }

    // M47 — the nutrient shore on the ledger: the strongest upwelling
    // within reach of the harbour, read from the same packed field the
    // diagnostics band. Era IV's fisheries will price this line.
    let mut upwell = 0.0f32;
    for dy in -2i64..=2 {
        for dx in -2i64..=2 {
            let yy = y as i64 + dy;
            let xx = x as i64 + dx;
            if yy < 0 || xx < 0 {
                continue;
            }
            if let Some(&u) = world.fields.upwelling.get([yy as usize, xx as usize]) {
                upwell = upwell.max(u);
            }
        }
    }

    Some(json!({
        "title": "Growth this month",
        "dp": 1,
        "unit": " souls",
        "terms": terms,
        "total": total,
        "total_label": "Souls / month",
        // M45 — site provenance: why the town stands where it stands.
        "site": {
            "shelter": crate::util::round2(shelter),
            "upwelling": crate::util::round2(upwell as f64),
            "coastal": s.coastal,
            "river": s.river,
        },
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

    // M14.9 — the taste mix, exactly as compute_prices folds it in
    let mut pop_by_style = [0.0f64; crate::culture::N_STYLES];
    for s in &world.peoples.settlements {
        let sty = world
            .peoples
            .peoples
            .get(s.people.0)
            .map(|p| crate::culture::style_index(&p.style))
            .unwrap_or(0);
        pop_by_style[sty] += s.pop as f64;
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
            let p = (demand_weight(g, luxury) * mix(g) + 0.02) / (sup + 0.02);
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
        .unwrap_or_else(|| (demand_weight(good, luxury) * mix(good) + 0.02) / (supply + 0.02));

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

// ------------------------------------------------------------------- cell

/// M61 — "why is this here": the causal chain that built one cell, deep
/// time forward. Every row reads *recorded* generation state — the rock
/// province the plates dealt (M18), the ice ledger's own grids (M29–M35),
/// the river/sediment/aquifer ledgers (M54/M59), the soil order (M51) —
/// never a re-derivation. Stages are ordered 0 stone · 1 ice · 2 water ·
/// 3 soil · 4 landform, and the final row is always the stored landform
/// word (M60): the chain must end on the map's own vocabulary, exactly.
fn explain_cell(world: &World, key: &str) -> Option<Value> {
    let (ys, xs) = key.split_once(',')?;
    let y = ys.trim().parse::<usize>().ok()?;
    let x = xs.trim().parse::<usize>().ok()?;
    let (h, w) = world.fields.height.dim();
    if y >= h || x >= w {
        return None;
    }

    let row = |s: u8, k: &str, l: &str, d: String| json!({ "s": s, "k": k, "l": l, "d": d });
    let mut chain: Vec<Value> = Vec::new();

    // -- stage 0 · stone: the province the plate history dealt (M16–M19)
    let (rl, rd) = match world.fields.rock[[y, x]] {
        0 => ("Shield", "the old craton floor — basement rock worn low over deep time"),
        1 => ("Basin", "layered sediments — old sea-floors and river loads stacked and lifted dry"),
        2 => ("Fold belt", "collision country — the crust buckled into ranges where plates met"),
        _ => ("Volcanic province", "fire country — rock born of eruptions along the old plate seams"),
    };
    chain.push(row(0, "stone", rl, rd.to_string()));

    // -- stage 1 · ice: the long winter's ledger, entry by entry (M29–M35)
    let ice = &world.ice;
    let thick = ice.thickness[[y, x]] as f64;
    if thick > 0.0 {
        chain.push(row(1, "ice", "Under the sheet",
            format!("ice stood {:.0} m thick here at the glacial maximum", thick)));
    }
    let carved_m = ice.carved[[y, x]] as f64 * METRES_PER_UNIT;
    if carved_m >= 10.0 {
        chain.push(row(1, "ice", "Ice-carved",
            format!("the ice ground this floor about {:.0} m deeper", carved_m)));
    }
    if ice.till[[y, x]] > 0.05 {
        chain.push(row(1, "ice", "Till country",
            "the melting sheet dropped its ground-up rock here as till".to_string()));
    }
    let ow = ice.outwash[[y, x]];
    if ow >= 0.45 {
        chain.push(row(1, "ice", "Outwash",
            if ow >= 0.9 {
                "meltwater braided over this ground and planed it into gravel".to_string()
            } else {
                "a meltwater apron — sand and gravel spread flat below the old margin".to_string()
            }));
    }
    if ice.loess[[y, x]] > 0.05 {
        chain.push(row(1, "ice", "Loess mantle",
            "wind lifted glacial dust off the outwash plains and laid it here in drifts".to_string()));
    }
    if ice.modern[[y, x]] > 0.0 {
        chain.push(row(1, "ice", "Glacier today",
            "snowfall still outruns the melt on these heights".to_string()));
    }

    // -- stage 2 · water: rivers, lakes, silt, the sea, the table (M54/M59)
    let height = world.fields.height[[y, x]] as f64;
    let flags = CellFlags::from_bits_truncate(world.fields.flags[[y, x]]);
    if height < 0.0 {
        chain.push(row(2, "water", "The sea",
            format!("{:.0} m of water stand over this ground", -height * METRES_PER_UNIT)));
    } else {
        if flags.contains(CellFlags::RIVER) {
            let order = world.fields.strahler[[y, x]];
            let flow = world.fields.discharge[[y, x]];
            let (wl, wd) = if flags.contains(CellFlags::SEASONAL) {
                ("Wadi", format!("a seasonal river — roaring in the rains, dry the rest (order {})", order))
            } else if flags.contains(CellFlags::BRAIDED) {
                ("Braided river", format!("the river splits and rejoins over its own gravel (order {} · flow {:.0})", order, flow))
            } else {
                ("River", format!("running water crosses this cell (order {} · flow {:.0})", order, flow))
            };
            chain.push(row(2, "water", wl, wd));
        }
        if flags.contains(CellFlags::SALT) {
            chain.push(row(2, "water", "Salt basin",
                "rivers die here and leave their salt — no way out to the sea".to_string()));
        } else if flags.contains(CellFlags::LAKE) {
            chain.push(row(2, "water", "Lake",
                "standing fresh water fills this hollow".to_string()));
        }
        let silt_m = world.sediment.depth[[y, x]] as f64 * METRES_PER_UNIT;
        if world.sediment.delta[[y, x]] {
            chain.push(row(2, "water", "Fan-built",
                "this ground is new — the river raised it grain by grain at its mouth".to_string()));
        } else if silt_m >= 1.0 {
            chain.push(row(2, "water", "Silt-laid",
                format!("the river left about {:.0} m of fill here in flood", silt_m)));
        }
        let table = world.fields.aquifer[[y, x]] as f64;
        if !flags.contains(CellFlags::LAKE) {
            if table <= 0.5 {
                chain.push(row(2, "water", "Water at the surface",
                    "the water table daylights here".to_string()));
            } else {
                chain.push(row(2, "water", "The water table",
                    format!("fresh water stands about {:.0} m down", table)));
            }
        }
    }

    // -- stage 3 · soil: what all of the above weathered into (M51/M52)
    if height >= 0.0 {
        let so = SoilOrder::from_code(world.fields.soil[[y, x]]);
        if so != SoilOrder::None {
            let fert = so.fertility();
            let label = {
                let n = so.name();
                let mut c = n.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            };
            chain.push(row(3, "soil", &label,
                format!("bears {:.1}× the yield of plain brown earth", fert)));
        }
    }

    // -- stage 4 · landform: the terminal word, verbatim from the lane (M60)
    let lf = world.fields.landform[[y, x]] as usize;
    let word = landform::NAMES.get(lf).copied().unwrap_or("open sea");
    chain.push(row(4, "landform", word,
        "the one word the map keeps for everything above".to_string()));

    Some(json!({
        "title": "Why is this here",
        "chain": chain,
    }))
}
