//! Peoples — the generational axis (ADR-0018): tongue, gods, demonym,
//! name bank, lineage. Founded by k-means clustering of the dawn towns;
//! after the dawn the roster changes only on the slow clocks — divergence
//! mints a daughter people, merging (M12) folds one into another. The
//! political axis (crowns, treasuries, wars) lives in `politics::Realm`.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::PeopleId;
use crate::constants as gc;
use crate::naming;
use crate::settlements::Settlement;

pub const CULTURE_COLORS: [&str; 12] = [
    "#d4a94a", "#6f9ceb", "#c86b6b", "#7fb069", "#a06fd4", "#5bc0be",
    "#e08d5a", "#8fa3d4", "#c95f8e", "#9db55c", "#7d6fd4", "#4fb08d",
];

/// Color for a realm or people minted after the dawn — keeps cycling the
/// shared palette so both axes stay in one hue family.
pub fn next_realm_color(i: usize) -> String {
    CULTURE_COLORS[i % CULTURE_COLORS.len()].to_string()
}

fn style_by_biome(b: u8) -> &'static str {
    match b {
        x if x == gc::TUNDRA || x == gc::WET_TUNDRA || x == gc::BOREAL_FOREST || x == gc::ICE => {
            "nordic"
        }
        x if x == gc::DESERT || x == gc::SAVANNA => "arid",
        x if x == gc::TROPICAL_RAIN_FOREST
            || x == gc::TEMPERATE_RAIN_FOREST
            || x == gc::SEASONAL_RAIN_FOREST
            || x == gc::WOODLAND =>
        {
            "sylvan"
        }
        x if x == gc::GRASSLAND => "steppe",
        _ => "hellenic",
    }
}

pub fn demonym(style: &str) -> &'static str {
    match style {
        "hellenic" => "ians",
        "nordic" => "folk",
        "arid" => "im",
        "sylvan" => "kin",
        "steppe" => "aks",
        _ => "ites",
    }
}

pub const ALL_STYLES: [&str; 5] = ["hellenic", "steppe", "nordic", "sylvan", "arid"];
pub const N_STYLES: usize = ALL_STYLES.len();

/// Index of a people's style in `ALL_STYLES`; unknown styles read as
/// hellenic (the temperate default).
pub fn style_index(style: &str) -> usize {
    ALL_STYLES.iter().position(|s| *s == style).unwrap_or(0)
}

/// M14.9 — per-culture tastes: small demand multipliers keyed on the
/// people's style (itself the homeland biome — `style_by_biome`). The
/// steppe prizes horses; the wine-dark coasts prize wine and marble; the
/// north prizes furs and a cup of southern sun; the desert prizes spice,
/// salt and the timber it does not have; forest folk take timber for
/// granted. 1.0 = indifferent. Declared once here beside the styles
/// (ADR-0015); `economy::compute_prices` folds it in as a pop-weighted
/// mix per market, so a market's book leans toward the people around it.
pub fn taste(style_ix: usize, g: crate::resources::Good) -> f64 {
    use crate::resources::Good;
    match (ALL_STYLES.get(style_ix).copied().unwrap_or("hellenic"), g) {
        ("hellenic", Good::Wine) => 1.30,
        ("hellenic", Good::Marble) => 1.30,
        ("hellenic", Good::Jewelry) => 1.20,
        ("hellenic", Good::Gems) => 1.15,
        ("hellenic", Good::Pottery) => 1.10,
        ("steppe", Good::Horse) => 1.60,
        ("steppe", Good::Wool) => 1.25,
        ("steppe", Good::Hides) => 1.20,
        ("steppe", Good::Leather) => 1.20,
        ("steppe", Good::Fish) => 0.80,
        ("steppe", Good::Timber) => 0.90,
        ("nordic", Good::Furs) => 1.40,
        ("nordic", Good::Wine) => 1.25,
        ("nordic", Good::Timber) => 1.20,
        ("nordic", Good::Fish) => 1.20,
        ("nordic", Good::Wool) => 1.15,
        ("sylvan", Good::Timber) => 0.85,
        ("sylvan", Good::Dyes) => 1.25,
        ("sylvan", Good::Furs) => 1.15,
        ("sylvan", Good::Hides) => 1.10,
        ("sylvan", Good::Marble) => 0.85,
        ("arid", Good::Timber) => 1.40,
        ("arid", Good::Spices) => 1.30,
        ("arid", Good::Dyes) => 1.20,
        ("arid", Good::Horse) => 1.20,
        ("arid", Good::Salt) => 1.15,
        ("arid", Good::Furs) => 0.60,
        _ => 1.0,
    }
}

/// M3.5 — a named god with a domain; cited in omens, festivals and wars.
#[derive(Serialize, Clone)]
pub struct God {
    pub name: String,
    pub domain: String,
}

/// Domains a pantheon may hold; each people draws four, distinct.
pub const DOMAINS: [&str; 10] = [
    "the sea", "the harvest", "war", "storms", "the dead",
    "the hearth", "fate", "the wild", "craft", "the moon",
];

/// A people (ADR-0018): everything that travels with the tongue and the
/// blood, nothing that travels with the crown.
#[derive(Serialize, Clone)]
pub struct People {
    pub id: PeopleId,
    pub name: String,
    /// Demonym in display form ("Norrfolk") — the wire field the client
    /// has always read.
    pub people: String,
    pub style: String,
    pub color: String,
    /// The people's gods (M3.5); index 0 is the chief god.
    pub pantheon: Vec<God>,
    /// M12.1 lineage — the people this one diverged from, if any; kinship
    /// remembers the parent for generations.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent: Option<PeopleId>,
    /// False once merged into another people (M12.4). A dead people's
    /// names stay in the strata; its row stays so ids never shift.
    pub alive: bool,
}

/// Draw a four-god pantheon in the people's tongue. Deterministic:
/// domain picks and names both come off the shared rng stream.
pub fn make_pantheon(
    rng: &mut Pcg64Mcg,
    style: &str,
    taken: &mut HashSet<String>,
) -> Vec<God> {
    let mut picked: Vec<usize> = Vec::new();
    while picked.len() < 4 {
        let d = rng.gen_range(0..DOMAINS.len());
        if !picked.contains(&d) {
            picked.push(d);
        }
    }
    picked
        .into_iter()
        .map(|d| God {
            name: naming::make_word(rng, style, taken),
            domain: DOMAINS[d].to_string(),
        })
        .collect()
}

/// Deterministic k-means with greedy max-min init.
fn kmeans(pts: &[(f64, f64)], k: usize, rng: &mut Pcg64Mcg) -> Vec<usize> {
    let n = pts.len();
    let mut centers: Vec<(f64, f64)> = vec![pts[rng.gen_range(0..n)]];
    while centers.len() < k {
        let mut best = f64::NEG_INFINITY;
        let mut bi = 0usize;
        for (i, p) in pts.iter().enumerate() {
            let d = centers
                .iter()
                .map(|c| (p.0 - c.0).hypot(p.1 - c.1))
                .fold(f64::INFINITY, f64::min);
            if d > best {
                best = d;
                bi = i;
            }
        }
        centers.push(pts[bi]);
    }
    let mut lab = vec![0usize; n];
    for _ in 0..16 {
        for (i, p) in pts.iter().enumerate() {
            let mut best = f64::INFINITY;
            let mut bl = 0usize;
            for (ci, c) in centers.iter().enumerate() {
                let d = (p.0 - c.0).hypot(p.1 - c.1);
                if d < best {
                    best = d;
                    bl = ci;
                }
            }
            lab[i] = bl;
        }
        for (ci, c) in centers.iter_mut().enumerate() {
            let members: Vec<&(f64, f64)> =
                pts.iter().zip(lab.iter()).filter(|(_, &l)| l == ci).map(|(p, _)| p).collect();
            if !members.is_empty() {
                let sy: f64 = members.iter().map(|p| p.0).sum();
                let sx: f64 = members.iter().map(|p| p.1).sum();
                *c = (sy / members.len() as f64, sx / members.len() as f64);
            }
        }
    }
    lab
}

/// Cluster settlements into peoples, rename them in-style.
pub fn assign_cultures(
    biomes: &Array2<u8>,
    settlements: &mut [Settlement],
    taken: &mut HashSet<String>,
    seed: i64,
) -> Vec<People> {
    if settlements.is_empty() {
        return Vec::new();
    }
    let mut rng = crate::util::rng(seed + 4242);
    let n = settlements.len();
    let k = (n / 4).clamp(2, n.min(6));
    let pts: Vec<(f64, f64)> = settlements
        .iter()
        .map(|s| (s.y as f64, s.x as f64))
        .collect();
    let lab = kmeans(&pts, k, &mut rng);

    let mut used_styles: HashSet<&'static str> = HashSet::new();
    let mut peoples: Vec<People> = Vec::new();
    for cid in 0..k {
        let members: Vec<&Settlement> = settlements
            .iter()
            .zip(lab.iter())
            .filter(|(_, &l)| l == cid)
            .map(|(s, _)| s)
            .collect();
        let members: Vec<&Settlement> = if members.is_empty() {
            vec![&settlements[0]]
        } else {
            members
        };
        // dominant biome of the homeland decides the tongue (first-seen wins ties)
        let mut counts: Vec<(u8, usize)> = Vec::new();
        for s in &members {
            let b = biomes[[s.y as usize, s.x as usize]];
            if let Some(e) = counts.iter_mut().find(|e| e.0 == b) {
                e.1 += 1;
            } else {
                counts.push((b, 1));
            }
        }
        // first-seen wins ties, like Python's max() over dict insertion order
        let mut dom = counts[0];
        for e in &counts {
            if e.1 > dom.1 {
                dom = *e;
            }
        }
        let mut style = style_by_biome(dom.0);
        if used_styles.contains(style) {
            style = ALL_STYLES
                .iter()
                .find(|st| !used_styles.contains(*st))
                .copied()
                .unwrap_or(style);
        }
        used_styles.insert(style);
        let root = naming::make_word(&mut rng, style, taken);
        peoples.push(People {
            id: PeopleId(cid),
            people: format!("{}{}", root, demonym(style)),
            name: root,
            style: style.to_string(),
            color: CULTURE_COLORS[cid % CULTURE_COLORS.len()].to_string(),
            pantheon: make_pantheon(&mut rng, style, taken),
            parent: None,
            alive: true,
        });
    }

    // rename settlements in their people's tongue, keeping the reading
    // of each name's parts (M3.3)
    for (s, &l) in settlements.iter_mut().zip(lab.iter()) {
        s.people = PeopleId(l);
        s.namer = PeopleId(l);
        let c = naming::coin(&mut rng, &peoples[l].style, taken);
        s.name = c.word;
        s.ety = c.ety;
    }

    peoples
}

/// M12 — a long-detached branch diverges into a daughter people: a new
/// tongue of the same family, the parent's gods carried along, and the
/// lineage remembered for the kinship metric. This — never secession —
/// is how the people roster grows after the dawn (ADR-0018).
pub fn diverge(
    parent: &People,
    new_id: PeopleId,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
) -> People {
    let root = naming::make_word(rng, &parent.style, taken);
    People {
        id: new_id,
        people: format!("{}{}", root, demonym(&parent.style)),
        name: root,
        style: parent.style.clone(),
        color: CULTURE_COLORS[new_id.idx() % CULTURE_COLORS.len()].to_string(),
        pantheon: parent.pantheon.clone(),
        parent: Some(parent.id),
        alive: true,
    }
}

// ================================================================ M12
// Kindred and Crown — kinship, assimilation, divergence, fusion. All of
// it runs on the generational clock: a yearly pass, changes a few times
// a century, every move a chronicle event.

use crate::economy::MarketAreas;
use crate::entity::{EntityKind, Registry};
use crate::event::{Event, EventKind};
use crate::politics::ADMIN_REACH;
use crate::state::Peoples;
use crate::util::Band;

/// M12.6 — diagnostics bands, declared beside the system (E2.7).
pub static BANDS: &[Band] = &[
    Band {
        name: "assimilation cadence",
        sweet: (1.0, 14.0),
        hard: (0.0, 40.0),
        target: "M12.2: towns turn kindred a few times a century, not monthly",
    },
    Band {
        name: "kindred moves per century",
        sweet: (1.0, 24.0),
        hard: (0.0, 60.0),
        target: "M12: the kindred clock ticks, but slowly",
    },
];

/// M12.1 — kinship between two peoples, 0..1. Four terms, per the
/// roadmap: shared style family, secession/divergence lineage, pantheon
/// overlap, and years spent under one realm (co-residence, either way).
pub fn kinship(a: PeopleId, b: PeopleId, peoples: &[People], co: &[Vec<f64>]) -> f64 {
    if a == b {
        return 1.0;
    }
    let (Some(pa), Some(pb)) = (peoples.get(a.idx()), peoples.get(b.idx())) else {
        return 0.0;
    };
    let mut k = 0.0;
    if pa.style == pb.style {
        k += 0.30;
    }
    // lineage: parent/child binds hardest, siblings still remember
    if pa.parent == Some(b.into()) || pb.parent == Some(a.into()) {
        k += 0.25;
    } else if pa.parent.is_some() && pa.parent == pb.parent {
        k += 0.18;
    }
    // pantheon: shared god names (a daughter people carries them along)
    let shared = pa
        .pantheon
        .iter()
        .filter(|g| pb.pantheon.iter().any(|h| h.name == g.name))
        .count();
    k += 0.20 * (shared as f64 / 4.0).min(1.0);
    // co-residence: a century of shared crowns saturates the term
    let lived = co
        .get(a.idx())
        .and_then(|r| r.get(b.idx()))
        .copied()
        .unwrap_or(0.0)
        + co.get(b.idx()).and_then(|r| r.get(a.idx())).copied().unwrap_or(0.0);
    k += 0.25 * (lived / 1200.0).min(1.0);
    k.clamp(0.0, 1.0)
}

/// Squared distance in cells between a settlement and its realm's seat,
/// or None when the seat is gone.
fn seat_dist(setts: &[Settlement], realms: &[crate::politics::Realm], s: &Settlement) -> Option<f64> {
    let r = realms.get(s.realm.0)?;
    let seat = setts.iter().find(|t| t.id == r.seat)?;
    let (dy, dx) = ((s.y - seat.y) as f64, (s.x - seat.x) as f64);
    Some((dy * dy + dx * dx).sqrt())
}

/// The kindred year (M12) — one pass, four movements:
///   1. co-residence bookkeeping (the kinship metric's slow term)
///   2. assimilation drift and flips (M12.2)
///   3. divergence: a far-flung branch becomes a daughter people (M12.1)
///   4. fusion: kindred peoples long under one crown become one (M12.4)
/// plus the minority exonym doubling (M12.5). Union of crowns (M12.3)
/// lives in `politics::union_pass` — it moves the political axis.
pub fn kindred_pass(
    peoples: &mut Peoples,
    areas: &MarketAreas,
    month: i64,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    reg: &mut Registry,
) -> Vec<Event> {
    let mut events = Vec::new();
    let np = peoples.peoples.len();
    if np == 0 || peoples.settlements.is_empty() {
        return events;
    }

    // ---- 1. co-residence: a year of every town's exposure ------------
    // co[A][B] += 12 · (share of A's towns under B-crowned realms), so
    // "150 years under one realm" means the whole people lived it.
    {
        let mut towns_of = vec![0usize; np];
        let mut under = vec![vec![0usize; np]; np];
        for s in &peoples.settlements {
            let Some(r) = peoples.realms.get(s.realm.0) else { continue };
            if !r.alive {
                continue;
            }
            towns_of[s.people.idx()] += 1;
            if r.people != s.people {
                under[s.people.idx()][r.people.idx()] += 1;
            }
        }
        for a in 0..np {
            if towns_of[a] == 0 {
                continue;
            }
            for b in 0..np {
                if under[a][b] > 0 {
                    peoples.coresidence[a][b] += 12.0 * under[a][b] as f64 / towns_of[a] as f64;
                }
            }
        }
    }

    // ---- 2. assimilation (M12.2) --------------------------------------
    // Towns of people A under a kindred crown B drift toward B over ~3-4
    // generations: faster along roads and inside the seat's market area,
    // slower beyond the crown's administrative reach.
    let mut flips: Vec<(usize, PeopleId, PeopleId)> = Vec::new();
    for i in 0..peoples.settlements.len() {
        let (crown, seat_far, seat_area) = {
            let s = &peoples.settlements[i];
            let Some(r) = peoples.realms.get(s.realm.0) else { continue };
            if !r.alive {
                continue;
            }
            let polity = peoples
                .societies
                .get(r.people.idx())
                .map_or(0, |so| so.polity.min(3));
            let far = seat_dist(&peoples.settlements, &peoples.realms, s)
                .map_or(false, |d| d > ADMIN_REACH[polity]);
            let seat_idx = peoples.settlements.iter().position(|t| t.id == r.seat);
            let same_area = seat_idx.map_or(false, |si| areas.area_of(si) == areas.area_of(i));
            (r.people, far, same_area)
        };
        let s = &mut peoples.settlements[i];
        if crown == s.people {
            // home crown: old leanings fade
            s.drift = (s.drift - 0.02).max(0.0);
            continue;
        }
        let k = kinship(s.people, crown, &peoples.peoples, &peoples.coresidence);
        if k < 0.20 {
            // an alien crown: the minority stands and remembers (M12.5) —
            // this is where exonyms live. No drift across non-kindred
            // pairs (the M12 gate); but the co-residence ledger keeps
            // filling, kinship keeps rising, and once the pair crosses
            // the kindred line the drift begins — empires absorb
            // strangers too, just by first becoming kin.
            //
            // The leaning dies at once, not by fade: a half-run drift
            // toward a crown that is no kin (or no longer rules — the
            // usual case, a conquest mid-decay) is void state, and the
            // M12 gate reads plain drift and means it. The old 0.02/mo
            // fade left stale drift across non-kindred pairs for up to
            // 50 months after a crown change — a real breach under seed
            // butterflies, not a tuning artifact.
            s.drift = 0.0;
            s.drift_to = None;
            continue;
        }
        if s.drift_to != Some(crown) {
            s.drift = 0.0;
            s.drift_to = Some(crown);
        }
        // ~1.6 %/yr integrated under a close-kindred crown (≈2-3
        // generations, research/06 Axelrod timescale); barely-kindred
        // pairs take generations more. Below the 0.20 line drift is off
        // entirely (see above) — kinship gates, then scales.
        let mut rate = 0.0095 * (0.35 + k);
        if s.connections > 0 {
            rate *= 1.35; // the roads carry the tongue
        }
        if seat_area {
            rate *= 1.25; // one market, one speech
        }
        if seat_far {
            rate *= 0.55; // straits and ranges slow the drift
        }
        s.drift += rate;
        if s.drift >= 1.0 {
            flips.push((i, s.people, crown));
        }
    }
    for (i, old, new) in flips {
        let (name, x, y);
        {
            let s = &mut peoples.settlements[i];
            s.people = new;
            s.drift = 0.0;
            s.drift_to = None;
            s.exonym = None; // the crown's word is now the folk's word
            name = s.name.clone();
            x = s.x;
            y = s.y;
        }
        let old_name = peoples.peoples[old.idx()].people.clone();
        let new_name = peoples.peoples[new.idx()].people.clone();
        events.push(Event {
            m: month,
            s: name.clone(),
            k: EventKind::Kindred,
            text: format!(
                "After long generations under the crown, the folk of {} count themselves {} now, not {} — though the old names on their hills keep the old sounds.",
                name, new_name, old_name
            ),
            x,
            y,
            ..Default::default()
        });
    }

    // ---- 3. divergence (M12.1 lineage; roster grows) -------------------
    // A branch three towns strong, a century old and far over the water
    // from the old country becomes a people of its own.
    if np < CULTURE_COLORS.len() * 2 {
        let mut minted = false;
        for pi in 0..np {
            if minted || !peoples.peoples[pi].alive {
                continue;
            }
            let pid = PeopleId(pi);
            let mine: Vec<usize> = (0..peoples.settlements.len())
                .filter(|&i| peoples.settlements[i].people == pid)
                .collect();
            if mine.len() < 6 {
                continue;
            }
            // population-weighted heartland
            let (mut cy, mut cx, mut wsum) = (0.0, 0.0, 0.0);
            for &i in &mine {
                let s = &peoples.settlements[i];
                let w = (s.pop.max(1)) as f64;
                cy += s.y as f64 * w;
                cx += s.x as f64 * w;
                wsum += w;
            }
            cy /= wsum;
            cx /= wsum;
            let far: Vec<usize> = mine
                .iter()
                .copied()
                .filter(|&i| {
                    let s = &peoples.settlements[i];
                    let (dy, dx) = (s.y as f64 - cy, s.x as f64 - cx);
                    (dy * dy + dx * dx).sqrt() > 84.0 && month - s.born >= 840
                })
                .collect();
            let far_pop: i64 = far.iter().map(|&i| peoples.settlements[i].pop).sum();
            if far.len() < 3 || far_pop < 700 || rng.gen::<f64>() > 0.12 {
                continue;
            }
            let new_id = PeopleId(peoples.peoples.len());
            let daughter = diverge(&peoples.peoples[pi], new_id, rng, taken);
            let d_name = daughter.people.clone();
            let p_name = peoples.peoples[pi].people.clone();
            peoples.peoples.push(daughter);
            // the daughter people enters the rolls of the living (M6.1):
            // without this, every later omen naming them rides id-less
            reg.add(EntityKind::Culture, &d_name, month, Some(new_id), -1, -1);
            // arts travel with the tongue: the branch carries its lore out
            let mut soc = peoples.societies[pi].clone();
            soc.people = new_id.idx();
            peoples.societies.push(soc);
            // grow the co-residence ledger
            for row in peoples.coresidence.iter_mut() {
                row.push(0.0);
            }
            peoples.coresidence.push(vec![0.0; new_id.idx() + 1]);
            let mut biggest = far[0];
            for &i in &far {
                peoples.settlements[i].people = new_id;
                peoples.settlements[i].drift = 0.0;
                peoples.settlements[i].drift_to = None;
                if peoples.settlements[i].pop > peoples.settlements[biggest].pop {
                    biggest = i;
                }
            }
            let b = &peoples.settlements[biggest];
            events.push(Event {
                m: month,
                s: b.name.clone(),
                k: EventKind::Kindred,
                text: format!(
                    "Sundered from the old country by distance and years, the far hearths around {} are a people of their own now — the {}, though their tongue still remembers the {}.",
                    b.name, d_name, p_name
                ),
                x: b.x,
                y: b.y,
                ..Default::default()
            });
            minted = true;
        }
    }

    // ---- 4. fusion (M12.4; roster falls) --------------------------------
    // A kindred minority whose towns have nearly all lived generations
    // under one crown folds into the dominant people. The old tongue
    // stays in the toponym strata (namer never moves); its gods join the
    // shared pantheon.
    let np_now = peoples.peoples.len();
    let mut fused = false;
    for a in 0..np_now {
        if fused || !peoples.peoples[a].alive {
            continue;
        }
        let pa = PeopleId(a);
        let a_towns: Vec<usize> = (0..peoples.settlements.len())
            .filter(|&i| peoples.settlements[i].people == pa)
            .collect();
        if a_towns.is_empty() {
            continue;
        }
        // the crown people most of A's towns live under
        let mut under = vec![0usize; np_now];
        for &i in &a_towns {
            if let Some(r) = peoples.realms.get(peoples.settlements[i].realm.0) {
                if r.alive && r.people != pa {
                    under[r.people.idx()] += 1;
                }
            }
        }
        let Some((b, &n_under)) = under.iter().enumerate().max_by_key(|(_, &n)| n) else {
            continue;
        };
        if n_under == 0 || !peoples.peoples[b].alive {
            continue;
        }
        let pb = PeopleId(b);
        let b_towns = peoples.settlements.iter().filter(|s| s.people == pb).count();
        let share = n_under as f64 / a_towns.len() as f64;
        let lived = peoples.coresidence[a][b];
        let kin = kinship(pa, pb, &peoples.peoples, &peoples.coresidence);
        if share < 0.85 || lived < 900.0 || kin < 0.50 || b_towns <= a_towns.len() {
            continue;
        }
        if rng.gen::<f64>() > 0.15 {
            continue;
        }
        // the fusion: A's towns speak B now; A leaves its names behind
        let a_name = peoples.peoples[a].people.clone();
        let b_name = peoples.peoples[b].people.clone();
        let keepsakes: Vec<String> = a_towns
            .iter()
            .filter(|&&i| peoples.settlements[i].namer == pa)
            .take(3)
            .map(|&i| peoples.settlements[i].name.clone())
            .collect();
        let mut biggest = a_towns[0];
        for &i in &a_towns {
            if peoples.settlements[i].pop > peoples.settlements[biggest].pop {
                biggest = i;
            }
            peoples.settlements[i].people = pb;
            peoples.settlements[i].drift = 0.0;
            peoples.settlements[i].drift_to = None;
            peoples.settlements[i].exonym = None;
        }
        peoples.peoples[a].alive = false;
        // the tongue leaves the rolls of the living (M6.1)
        if let Some(ce) = reg.find_kind(EntityKind::Culture, &a_name) {
            reg.close(ce, month, &format!("one people with the {} now", b_name));
        }
        // any court still speaking A now speaks B
        for r in peoples.realms.iter_mut() {
            if r.people == pa {
                r.people = pb;
            }
        }
        // the gods enter the shared pantheon, domains not already held
        let a_gods: Vec<God> = peoples.peoples[a].pantheon.clone();
        let bp = &mut peoples.peoples[b].pantheon;
        for g in a_gods {
            if bp.len() < 6 && !bp.iter().any(|h| h.domain == g.domain) {
                bp.push(g);
            }
        }
        let loans = if keepsakes.is_empty() {
            String::new()
        } else {
            format!(" The old tongue keeps its seat in the names — {}.", keepsakes.join(", "))
        };
        let bt = &peoples.settlements[biggest];
        events.push(Event {
            m: month,
            s: a_name.clone(),
            k: EventKind::Kindred,
            text: format!(
                "The {} and the {} are one people now; the last hearths that kept the old ways count themselves {}.{}",
                a_name, b_name, b_name, loans
            ),
            x: bt.x,
            y: bt.y,
            ..Default::default()
        });
        fused = true;
    }

    // ---- 5. minority exonyms (M12.5) ------------------------------------
    // A town that stands apart gains the crown's word for it on the
    // rolls — the doubling marks the seam. At most one a year, worldwide.
    let candidates: Vec<usize> = (0..peoples.settlements.len())
        .filter(|&i| {
            let s = &peoples.settlements[i];
            if s.exonym.is_some() {
                return false;
            }
            let Some(r) = peoples.realms.get(s.realm.0) else { return false };
            r.alive
                && r.people != s.people
                && kinship(s.people, r.people, &peoples.peoples, &peoples.coresidence) < 0.40
        })
        .collect();
    if !candidates.is_empty() && rng.gen::<f64>() < 0.35 {
        let i = candidates[rng.gen_range(0..candidates.len())];
        let crown_style = {
            let r = &peoples.realms[peoples.settlements[i].realm.0];
            peoples.peoples[r.people.idx()].style.clone()
        };
        let word = naming::make_word(rng, &crown_style, taken);
        let s = &mut peoples.settlements[i];
        s.exonym = Some(word.clone());
        events.push(Event {
            m: month,
            s: s.name.clone(),
            k: EventKind::Society,
            text: format!(
                "On the crown's rolls {} is written {}; its own folk keep the old name.",
                s.name, word
            ),
            x: s.x,
            y: s.y,
            ..Default::default()
        });
    }

    events
}
