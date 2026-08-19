//! M15 — The Assay: property-proofs for the resource economy.
//!
//! Where the diagnose harness measures one world at a time, the assay
//! states laws and lets proptest hunt for the world that breaks them:
//! the ontology is a rooted forest (M15.2), placement respects the land
//! (M15.3), prices stay clamped and drift-free under any shock sequence
//! (M15.4), more supply never raises a price and a new route never
//! widens a spread (M15.5), extraction books balance double-entry
//! (M15.6), and the pack reader survives hostile bytes (M15.7).
//!
//! Native-only (dev-dependency lane); the wasm never carries any of it.

use std::collections::HashMap;
use std::sync::OnceLock;

use proptest::prelude::*;
use strum::IntoEnumIterator;

use calliope::constants as gc;
use calliope::culture;
use calliope::economy::{self, Market, RECIPES};
use calliope::ids::SettlementId;
use calliope::pack::validate_pack;
use calliope::resources::{Good, Place, GOODS, STRATEGIC_MINIMA};
use calliope::settlements::Settlement;
use calliope::society;
use calliope::world::World;

/// Shelf labels a chain may terminate in — the roots of the forest.
const ROOTS: [&str; 5] = ["food", "material", "fuel", "craft", "luxury"];

/// One 256-world, generated once and shared read-only across tests.
fn base_world() -> &'static World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| World::generate(12345, 256))
}

/// The per-people style indices exactly as the tick derives them (M14.9).
fn styles(w: &World) -> Vec<usize> {
    w.peoples
        .peoples
        .iter()
        .map(|p| culture::style_index(&p.style))
        .collect()
}

fn goods_in_minima(g: Good) -> bool {
    STRATEGIC_MINIMA.iter().any(|&(m, _, _)| m == g)
}

// ======================================================================
// M15.2 — the ontology is a rooted forest
// ======================================================================

#[test]
fn ontology_is_a_rooted_forest() {
    let lint = calliope::resources::ontology_lint();
    assert!(lint.is_empty(), "ontology_lint found drift: {lint:?}");

    // Single-parent law: every label has exactly one parent across the
    // whole table — a forest, not a tangle.
    let mut parent: HashMap<&str, &str> = HashMap::new();
    for spec in GOODS.iter() {
        assert!(!spec.isa.is_empty(), "{}: empty ISA chain", spec.good);
        assert_eq!(
            *spec.isa.last().unwrap(),
            spec.category,
            "{}: chain must terminate in its shelf category",
            spec.good
        );
        assert!(
            ROOTS.contains(&spec.category),
            "{}: category {:?} is not a root shelf",
            spec.good,
            spec.category
        );
        // roots only at the end; no label twice in one chain
        for (i, &l) in spec.isa.iter().enumerate() {
            if i + 1 < spec.isa.len() {
                assert!(
                    !ROOTS.contains(&l),
                    "{}: root {:?} appears mid-chain",
                    spec.good,
                    l
                );
            }
            assert_eq!(
                spec.isa.iter().filter(|&&m| m == l).count(),
                1,
                "{}: label {:?} repeats in its chain",
                spec.good,
                l
            );
        }
        for w in spec.isa.windows(2) {
            let prev = parent.insert(w[0], w[1]);
            assert!(
                prev.is_none() || prev == Some(w[1]),
                "label {:?} has two parents: {:?} and {:?}",
                w[0],
                prev.unwrap(),
                w[1]
            );
        }
        assert!(
            economy::base_value(spec.good).is_finite() && economy::base_value(spec.good) > 0.0,
            "{}: non-positive base value",
            spec.good
        );
    }
}

#[test]
fn recipes_close_over_the_ontology() {
    for r in RECIPES.iter() {
        let spec = r.out.spec();
        assert!(
            matches!(spec.place, Place::None),
            "{}: a crafted good must never also be placed",
            r.out
        );
        assert!(!r.ore_any.is_empty(), "{}: recipe with no inputs", r.out);
        assert!(!r.tech_any.is_empty(), "{}: recipe with no art", r.out);
        assert!(r.min_pop > 0, "{}: recipe with no workforce gate", r.out);
        for &o in r.ore_any.iter() {
            assert_ne!(o, r.out, "{}: recipe feeds on its own output", r.out);
            // an input must be obtainable: placed in the ground, farmed
            // (Grain), or an animal secondary derived on the hoof (M14.3)
            let derived = matches!(o, Good::Grain | Good::Wool | Good::Hides);
            assert!(
                !matches!(o.spec().place, Place::None) || derived,
                "{}: input {} is neither placed, farmed nor derived",
                r.out,
                o
            );
        }
    }
}

#[test]
fn requires_names_real_arts() {
    for spec in GOODS.iter() {
        if let Some(label) = spec.requires {
            assert!(
                society::requires_resolves(label),
                "{}: REQUIRES {:?} names no tech, folkway or family",
                spec.good,
                label
            );
        }
    }
    // and the tech table itself is sound: unique names, no self-requires
    let mut seen = std::collections::HashSet::new();
    for t in society::TECHS.iter() {
        assert!(seen.insert(t.name), "duplicate tech name {:?}", t.name);
        for &r in t.requires.iter() {
            assert_ne!(r as usize, t.id as usize, "{:?} requires itself", t.name);
            assert!(
                society::tech(r).era <= t.era,
                "{:?} requires a later-era art",
                t.name
            );
        }
    }
}

// ======================================================================
// M15.3 — placement respects the land
// ======================================================================

#[test]
fn placement_respects_the_land() {
    for &seed in &[12345i64, 777, 90210] {
        let w = World::generate(seed, 256);
        let (rows, cols) = w.fields.height.dim();
        assert!(!w.deposits.is_empty(), "seed {seed}: a world with no deposits");

        for d in &w.deposits {
            let spec = d.r.spec();
            assert!(
                d.x >= 0 && (d.x as usize) < cols && d.y >= 0 && (d.y as usize) < rows,
                "seed {seed}: {} at ({},{}) off the map",
                d.r, d.x, d.y
            );
            assert!(
                (0.35 - 1e-9..=1.0 + 1e-9).contains(&d.rich),
                "seed {seed}: {} richness {} outside [0.35, 1.0]",
                d.r, d.rich
            );
            // reserve law: the table decides whether the land renews
            match (spec.reserve, d.r) {
                (None, _) => assert!(
                    d.left < 0.0,
                    "seed {seed}: renewable {} carries a finite reserve",
                    d.r
                ),
                (Some(_), Good::Salt) => { /* pans renew, rock seams do not */ }
                (Some(_), _) => assert!(
                    d.left > 0.0,
                    "seed {seed}: mineral {} born exhausted",
                    d.r
                ),
            }
            if spec.known_p >= 1.0 && !goods_in_minima(d.r) {
                assert!(d.known, "seed {seed}: plain-sight {} born hidden", d.r);
            }
            assert!(
                (0.0..=1.0).contains(&d.stock) && d.phase == 0,
                "seed {seed}: {} born mid-collapse",
                d.r
            );

            // mask conformity — floor-injected minima goods and salt
            // (pan-or-seam) answer to their own laws, checked elsewhere.
            if goods_in_minima(d.r) || d.r == Good::Salt {
                continue;
            }
            let (y, x) = (d.y as usize, d.x as usize);
            let b = w.fields.biomes[[y, x]];
            let h = w.fields.height[[y, x]];
            let wet = b == gc::WATER
                || w.fields.strahler[[y, x]] > 0
                || w.fields.flags[[y, x]] & 0b11 != 0;
            let ok = match spec.place {
                Place::None => false, // produced goods never lie in the ground
                Place::Biomes(list) => list.contains(&b),
                Place::Above(t) => h > t,
                Place::Band(lo, hi) => h > lo && h <= hi,
                Place::BiomesOrBand(list, lo, hi) => {
                    list.contains(&b) || (h > lo && h <= hi)
                }
                Place::BiomesAndBand(list, lo, hi) => {
                    list.contains(&b) && h > lo && h <= hi
                }
                Place::Waters => wet,
                Place::AboveOrPlacer(t, p) => h > t || (wet && h > 0.0 && h <= p) || h > p,
                Place::Coast(list) => list.contains(&b),
                Place::RiverBanks(t) => h <= t + 1e-6,
                Place::CoastOrBand(..) => true,
            };
            assert!(
                ok,
                "seed {seed}: {} at ({},{}) violates its placement mask (biome {}, h {:.3})",
                d.r, d.x, d.y, b, h
            );
        }

        // spacing: the 5×5 maxima race keeps same-good seams ≥3 apart
        for (i, a) in w.deposits.iter().enumerate() {
            if goods_in_minima(a.r) || a.r == Good::Salt {
                continue;
            }
            for b in w.deposits.iter().skip(i + 1) {
                if a.r != b.r {
                    continue;
                }
                let cheb = (a.x - b.x).abs().max((a.y - b.y).abs());
                assert!(
                    cheb >= 3,
                    "seed {seed}: two {} seams {} apart — thinning failed",
                    a.r, cheb
                );
            }
        }

        // the floor of fate: every strategic mineral meets its minimum
        for &(g, min_n, _) in STRATEGIC_MINIMA.iter() {
            let n = w.deposits.iter().filter(|d| d.r == g).count();
            assert!(
                n >= min_n,
                "seed {seed}: only {n} {g} seams, floor is {min_n}"
            );
        }
        // M19 — deposits re-seated: pooled across the homed minerals,
        // at least 80% of seams sit in a home province (the floor pass
        // is province-blind, so per-good shares may dip; the pool holds).
        {
            let rows =
                calliope::resources::province_consistency(&w.deposits, &w.fields.rock);
            let (in_home, total) = rows
                .iter()
                .fold((0usize, 0usize), |(a, b), &(_, ih, t)| (a + ih, b + t));
            assert!(
                total == 0 || (in_home as f64) / (total as f64) >= 0.80,
                "seed {seed}: only {in_home} of {total} homed seams sit in their province"
            );
        }
        // the salting shore: at least one renewing pan, two sources total
        let pans = w
            .deposits
            .iter()
            .filter(|d| d.r == Good::Salt && d.left < 0.0)
            .count();
        let salts = w.deposits.iter().filter(|d| d.r == Good::Salt).count();
        assert!(pans >= 1, "seed {seed}: no coastal salt pan");
        assert!(salts >= 2, "seed {seed}: fewer than two salt sources");
    }
}

// ======================================================================
// M15.4 — prices stay clamped, finite and drift-free
// ======================================================================

#[test]
fn prices_stay_clamped_and_finite() {
    let w = base_world();
    let st = styles(w);
    let setts = &w.peoples.settlements;
    let mut m = Market::default();
    for _ in 0..600 {
        economy::update_prices(&mut m, setts, &st);
    }
    for (g, p) in m.iter_some() {
        let b = economy::base_value(g);
        assert!(p.is_finite(), "{g}: price went non-finite");
        assert!(
            p >= 0.3 * b - 0.01 && p <= 5.0 * b + 0.01,
            "{g}: price {p} outside the 0.3×/5× clamp of base {b}"
        );
    }
}

#[test]
fn renormalization_is_drift_free() {
    let w = base_world();
    let st = styles(w);
    let setts = &w.peoples.settlements;
    let mut m = Market::default();
    for _ in 0..3000 {
        economy::update_prices(&mut m, setts, &st);
    }
    let snap = |m: &Market| -> Vec<(Good, f64)> { m.iter_some().collect() };
    let a = snap(&m);
    for _ in 0..3000 {
        economy::update_prices(&mut m, setts, &st);
    }
    let b = snap(&m);
    assert_eq!(a.len(), b.len(), "goods appeared or vanished at equilibrium");
    let mut mean_log = 0.0;
    for ((ga, pa), (gb, pb)) in a.iter().zip(b.iter()) {
        assert_eq!(ga, gb);
        assert!(
            (pa - pb).abs() <= 0.01 + 1e-9,
            "{ga}: price still moving at equilibrium ({pa} → {pb})"
        );
        mean_log += (pb / economy::base_value(*gb)).ln();
    }
    mean_log /= b.len() as f64;
    assert!(
        mean_log.abs() < 0.6,
        "book-wide drift: mean log price ratio {mean_log}"
    );
}

proptest! {
    /// Any shock, any magnitude: the clamp holds and the direction is
    /// honoured — dearer news never cheapens a good, glut never raises it.
    #[test]
    fn shocks_respect_clamp_and_direction(
        gi in 0usize..Good::iter().count(),
        factor in 0.01f64..40.0,
    ) {
        let g = Good::iter().nth(gi).unwrap();
        let b = economy::base_value(g);
        let mut m = Market::default();
        let before = m.price(g);
        m.shock(g, factor);
        let after = m.price(g);
        prop_assert!(after.is_finite());
        prop_assert!(after >= 0.3 * b - 1e-9 && after <= 5.0 * b + 1e-9);
        if factor >= 1.0 {
            prop_assert!(after >= before.min(5.0 * b) - 1e-9);
        } else {
            prop_assert!(after <= before.max(0.3 * b) + 1e-9);
        }
    }
}

// ======================================================================
// M15.5 — metamorphic market laws
// ======================================================================

/// Converge a fresh book over the given towns and read one good's price.
fn settled_price(setts: &[Settlement], st: &[usize], g: Good) -> f64 {
    let mut m = Market::default();
    for _ in 0..400 {
        economy::update_prices(&mut m, setts, st);
    }
    m.price(g)
}

#[test]
fn more_supply_never_raises_a_price() {
    let w = base_world();
    let st = styles(w);
    for g in [Good::Timber, Good::Iron, Good::Fish] {
        let setts = w.peoples.settlements.clone();
        // A good nobody lists has no market price to defend — the ledger
        // answers with base_value(), a resting default, not a signal the
        // law may compare against. (Found by M16: a reshaped 256² world
        // where no town works iron.)
        if !setts.iter().any(|s| s.goods.contains(&g)) {
            continue;
        }
        let p0 = settled_price(&setts, &st, g);

        // three more towns list the good — supply widens
        let mut more = setts.clone();
        let mut added = 0;
        for s in more.iter_mut() {
            if !s.goods.contains(&g) && added < 3 {
                s.goods.push(g);
                added += 1;
            }
        }
        prop_assert_supply(g, added, settled_price(&more, &st, g), p0, true);

        // all but one lister drops it — supply narrows
        let mut fewer = setts.clone();
        let mut kept = false;
        for s in fewer.iter_mut() {
            if s.goods.contains(&g) {
                if kept {
                    s.goods.retain(|x| *x != g);
                } else {
                    kept = true;
                }
            }
        }
        if kept {
            prop_assert_supply(g, 1, settled_price(&fewer, &st, g), p0, false);
        }
    }
}

fn prop_assert_supply(g: Good, n: usize, p_new: f64, p_old: f64, widened: bool) {
    if n == 0 {
        return;
    }
    if widened {
        assert!(
            p_new <= p_old + 0.02,
            "{g}: widening supply raised the price {p_old} → {p_new}"
        );
    } else {
        assert!(
            p_new >= p_old - 0.02,
            "{g}: narrowing supply lowered the price {p_old} → {p_new}"
        );
    }
}

#[test]
fn a_new_route_never_widens_a_spread() {
    let w = base_world();
    let st = styles(w);
    let setts = &w.peoples.settlements;
    let by_id: HashMap<SettlementId, usize> =
        setts.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    if w.routes.len() < 2 {
        return; // a world too small to carve — nothing to prove
    }
    // the dearest route is the likeliest bridge between areas
    let (ri, bridge) = w
        .routes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.cost.partial_cmp(&b.1.cost).unwrap())
        .unwrap();
    let (Some(&ia), Some(&ib)) = (by_id.get(&bridge.a), by_id.get(&bridge.b)) else {
        return;
    };

    let spread = |routes: &[calliope::trade::Route]| -> f64 {
        let mut areas = economy::build_areas(setts, routes, None, &by_id);
        let anchor = &w.economy.market;
        for _ in 0..120 {
            economy::update_area_prices(&mut areas, setts, anchor, &st);
            let salted = economy::areas_with_salt(&areas, setts);
            economy::equalize_along_routes(&mut areas, setts, routes, &by_id, &salted);
        }
        let (ka, kb) = (areas.area_of(ia), areas.area_of(ib));
        if ka == kb {
            return 0.0;
        }
        let (ma, mb) = (&areas.markets[ka], &areas.markets[kb]);
        let mut sum = 0.0;
        let mut n = 0;
        for (g, pa) in ma.iter_some() {
            let pb = mb.price(g);
            if pb > 0.0 {
                sum += (pa / pb).ln().abs();
                n += 1;
            }
        }
        if n == 0 { 0.0 } else { sum / n as f64 }
    };

    let with_route = spread(&w.routes);
    let without: Vec<calliope::trade::Route> = w
        .routes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != ri)
        .map(|(_, r)| r.clone())
        .collect();
    let without_route = spread(&without);
    assert!(
        with_route <= without_route + 0.10,
        "the bridge route widened the spread it should close: {with_route} with vs {without_route} without"
    );
}

// ======================================================================
// M15.6 — the books balance: double-entry conservation
// ======================================================================

#[test]
fn extraction_books_balance() {
    let mut w = World::generate(777, 256);
    let left0: Vec<f64> = w.deposits.iter().map(|d| d.left).collect();
    let stock0: Vec<f64> = w.deposits.iter().map(|d| d.stock).collect();
    w.tick_json(240);
    assert_eq!(
        w.flows.extracted.len(),
        w.deposits.len(),
        "flow meters lost sync with the deposit ledger"
    );
    let mut mined = 0usize;
    let mut breathed = 0usize;
    for (di, d) in w.deposits.iter().enumerate() {
        if d.r.spec().reserve.is_some() && left0[di] >= 0.0 {
            // reserve drawn == meter, to the penny
            let drawn = left0[di] - d.left;
            assert!(
                (drawn - w.flows.extracted[di]).abs() < 1e-6,
                "{}: ledger says {} drawn, meter says {}",
                d.r, drawn, w.flows.extracted[di]
            );
            assert!(drawn >= -1e-9, "{}: reserve grew back", d.r);
            if drawn > 0.0 {
                mined += 1;
            }
        } else {
            // renewables never touch the reserve meter …
            assert_eq!(
                w.flows.extracted[di], 0.0,
                "{}: a renewable drew reserve",
                d.r
            );
            // … and the stock meter carries exactly the stock's journey
            let moved = d.stock - stock0[di];
            assert!(
                (moved - w.flows.dstock[di]).abs() < 1e-9,
                "{}: stock moved {} but the meter reads {}",
                d.r, moved, w.flows.dstock[di]
            );
            if w.flows.dstock[di] != 0.0 {
                breathed += 1;
            }
        }
    }
    assert!(mined > 0, "240 months and nothing mined — meters never exercised");
    assert!(breathed > 0, "240 months and no wild ground breathed");
}

// ======================================================================
// M15.7 — the pack survives hostile bytes
// ======================================================================

fn pristine() -> &'static (Vec<u8>, (usize, usize)) {
    static P: OnceLock<(Vec<u8>, (usize, usize))> = OnceLock::new();
    P.get_or_init(|| {
        let bytes = base_world().pack();
        let ok = validate_pack(&bytes).expect("pristine pack must validate");
        (bytes, ok)
    })
}

#[test]
fn pristine_pack_validates() {
    let (bytes, (arrays, blob)) = pristine();
    assert!(*arrays >= 8, "suspiciously few arrays in the pack");
    assert!(*blob > 0);
    assert!(bytes.len() > *blob);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every strict prefix is rejected — truncation can never pass.
    #[test]
    fn truncation_never_validates(frac in 0.0f64..1.0) {
        let (bytes, _) = pristine();
        let len = ((bytes.len() - 1) as f64 * frac) as usize;
        prop_assert!(validate_pack(&bytes[..len]).is_err());
    }

    /// A flipped bit anywhere either fails cleanly or leaves the summary
    /// untouched (header padding, label bytes) — it never panics and
    /// never changes what the client would allocate.
    #[test]
    fn bit_flips_never_panic(pos_f in 0.0f64..1.0, bit in 0u8..8) {
        let (bytes, ok) = pristine();
        let mut m = bytes.clone();
        let pos = ((m.len() - 1) as f64 * pos_f) as usize;
        m[pos] ^= 1 << bit;
        match validate_pack(&m) {
            Err(_) => {}
            Ok(summary) => prop_assert_eq!(summary, *ok),
        }
    }

    /// Random garbage is never mistaken for a world.
    #[test]
    fn garbage_never_validates(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        prop_assert!(validate_pack(&bytes).is_err());
    }
}
