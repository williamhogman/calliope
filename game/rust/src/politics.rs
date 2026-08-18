//! Politics with consequences (M4): influence-map territory, wars that
//! move borders, an opinion web with aggressive-expansion dread and
//! coalitions, sieges behind walls, and the slow tides of asabiyyah and
//! legitimacy that raise realms at the frontiers and break them in the
//! soft centuries after.
//!
//! ADR-0018: this module owns the **realm axis** — crowns, treasuries,
//! wars, borders. The people axis (tongue, gods, arts) lives in
//! `culture.rs`; a settlement carries one id of each, and conquest moves
//! only the realm.
//!
//! Everything here is a pure function of the seed: one shared rng stream,
//! fixed iteration order, no wall-clock. The chronicle narrates; this
//! module decides.

use smallvec::smallvec;
use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::{EntityId, PeopleId, RealmId, SettlementId};
use crate::chronicle::{self, ChronicleState, Ruler};
use crate::culture::People;
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::naming;
use crate::settlements::Settlement;
use crate::society::{self, Society};
use crate::util::round2;
use crate::event::EventKind;
use crate::event::Event;
use crate::state::{Chronicle, Peoples};

// ---------------------------------------------------------------- tuning

/// Monthly decay on opinion (toward 0) — grudges fade in a generation.
const OPINION_DECAY: f64 = 0.9965;
/// Monthly decay on aggressive-expansion dread.
const AE_DECAY: f64 = 0.9955;
/// Border friction: adjacent realms grind on each other, per month.
const FRICTION: f64 = 0.05;
/// Asabiyyah: monthly surge at a meta-ethnic frontier, and the slow
/// decay of solidarity in a safe realm (~3-4 generations to gutter).
const ASAB_SURGE: f64 = 0.0055;
const ASAB_DECAY: f64 = 0.0009;
/// Peace-term thresholds on |war score|.
const SCORE_TRIBUTE: f64 = 6.0;
const SCORE_CEDE: f64 = 14.0;
const SCORE_VASSAL: f64 = 26.0;
/// A war ends at once when the score runs away.
const SCORE_DECISIVE: f64 = 34.0;
/// Settlements within this range (cells) make two realms neighbours.
const NEIGHBOUR_RANGE: f64 = 90.0;
/// M11.1 — unrest: the monthly calm bleed on the 0..1 gauge. The feeds
/// (weak crown, guttered asabiyyah, hunger, war weariness, holdings
/// beyond reach) are inline in `monthly`; each ladder rung vents a slice
/// and arms a cooldown (M11.6) so realms convulse on the scale of years.
const UNREST_CALM: f64 = 0.010;
/// The ladder's rungs, lowest to highest (M11.2/4/5).
const LADDER_RIOT: f64 = 0.55;
const LADDER_CHARTER: f64 = 0.60;
const LADDER_COUP: f64 = 0.72;
const LADDER_SECEDE: f64 = 0.85;
/// M11.4 — administrative reach (cells from the seat) by polity tier:
/// bands, chiefdoms, kingdoms, empires. Towns beyond it feed unrest and
/// open the secession gate for a detached shore. Public: the kindred
/// pass (M12.2) reads the same reach to slow drift past the frontier.
pub const ADMIN_REACH: [f64; 4] = [14.0, 22.0, 32.0, 46.0];

// ---------------------------------------------------------------- realms

/// A realm (ADR-0018): the political clock. Name, ruling house, seat,
/// treasury — everything that changes by coup, conquest and coin rather
/// than by generations.
#[derive(Serialize, Clone)]
pub struct Realm {
    pub id: RealmId,
    /// The realm's name in its crown people's tongue ("Vessmark").
    pub name: String,
    /// The ruling line ("House Kaldra") — succession stays inside it
    /// until a crisis replaces it.
    pub house: String,
    /// The crown people — whose tongue names the court, whose gods bless
    /// the wars. Towns of other peoples may still fly this banner.
    pub people: PeopleId,
    /// Seat of the crown; re-seated if the seat falls.
    pub seat: SettlementId,
    pub color: String,
    /// Month of founding (0 = the dawn).
    pub founded: i64,
    /// False once struck from the rolls; the row stays so ids never shift.
    pub alive: bool,
    /// The crown's coin (moved off `Society` — ADR-0018): war chests,
    /// walls, tribute all draw on this.
    pub treasury: f64,
}

/// The crown people of a realm.
pub fn crown<'a>(peoples_v: &'a [People], realms: &[Realm], r: RealmId) -> &'a People {
    &peoples_v[realms[r.0].people.idx()]
}

/// Coin a realm name and house in a people's tongue.
fn coin_realm_name(
    rng: &mut Pcg64Mcg,
    style: &str,
    taken: &mut HashSet<String>,
) -> (String, String) {
    let name = naming::make_word(rng, style, taken);
    let house = format!("House {}", naming::make_word(rng, style, taken));
    (name, house)
}

/// The dawn realms: one crown per people, seated in its largest town
/// (ADR-0018 — the axes start aligned and drift apart from here).
pub fn init_realms(
    peoples_v: &[People],
    settlements: &mut [Settlement],
    taken: &mut HashSet<String>,
    reg: &mut Registry,
    seed: i64,
) -> Vec<Realm> {
    let mut rng = crate::util::rng(seed + 7171);
    let mut realms: Vec<Realm> = Vec::new();
    for (pi, p) in peoples_v.iter().enumerate() {
        let (name, house) = coin_realm_name(&mut rng, &p.style, taken);
        let seat = settlements
            .iter()
            .filter(|s| s.people == p.id)
            .max_by_key(|s| s.pop)
            .map(|s| (s.id, s.x, s.y))
            .unwrap_or((SettlementId(-1), -1, -1));
        reg.add(EntityKind::Realm, &name, 0, Some(p.id), seat.1, seat.2);
        realms.push(Realm {
            id: RealmId(pi),
            name,
            house,
            people: p.id,
            seat: seat.0,
            color: p.color.clone(),
            founded: 0,
            alive: true,
            treasury: 25.0,
        });
    }
    // the dawn towns fly the banner of their own people's crown
    for s in settlements.iter_mut() {
        s.realm = RealmId(s.people.idx().min(realms.len().saturating_sub(1)));
    }
    realms
}

// ---------------------------------------------------------------- state

#[derive(Serialize, Clone)]
pub struct War {
    /// Leading belligerents: `a` declared on `b`.
    pub a: RealmId,
    pub b: RealmId,
    /// Realms that joined each banner after the kindling.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allies_a: Vec<RealmId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allies_b: Vec<RealmId>,
    pub name: String,
    pub start: i64,
    /// Weariness cap: peace comes by this month at the latest.
    pub until: i64,
    /// Running war score; positive favours `a`.
    pub score: f64,
    #[serde(skip)]
    pub siege: Option<Siege>,
    /// Registry id of the war itself (M6.1); the client links through it.
    pub ent: EntityId,
    /// The named generals each banner follows (M6.2).
    #[serde(skip)]
    pub gen_a: EntityId,
    #[serde(skip)]
    pub gen_b: EntityId,
    #[serde(skip)]
    pub wins_a: u32,
    #[serde(skip)]
    pub wins_b: u32,
    /// One battlefield per war earns a name on the map (M9.4).
    #[serde(skip)]
    pub marked: bool,
    /// Sign of the score after the last swing, and how often it flipped —
    /// the reversal detector reads wars off this (M6.7).
    #[serde(skip)]
    pub last_sign: i32,
    #[serde(skip)]
    pub flips: u32,
}

impl War {
    pub fn involves(&self, c: RealmId) -> bool {
        self.a == c
            || self.b == c
            || self.allies_a.contains(&c)
            || self.allies_b.contains(&c)
    }
    fn side_a(&self) -> Vec<RealmId> {
        let mut v = vec![self.a];
        v.extend(&self.allies_a);
        v
    }
    fn side_b(&self) -> Vec<RealmId> {
        let mut v = vec![self.b];
        v.extend(&self.allies_b);
        v
    }
}

#[derive(Clone)]
pub struct Siege {
    /// Settlement id under siege (ids are stable for a settlement's life).
    pub target: SettlementId,
    /// Realm doing the besieging.
    pub attacker: RealmId,
    /// 0..100; the wall falls at 100.
    pub progress: f64,
}

#[derive(Clone)]
pub struct Tribute {
    pub from: RealmId,
    pub to: RealmId,
    pub per_month: f64,
    pub months_left: i64,
}

/// One pretender in a war of the circlet (M11.3).
#[derive(Clone)]
pub struct Claimant {
    pub name: String,
    pub house: String,
    pub ent: EntityId,
}

/// A war of the circlet (M11.3): the throne contested from inside the
/// hall. No borders move; when the term runs out the winner's house
/// rules. `seated` names the claimant holding the throne meanwhile.
#[derive(Clone)]
pub struct CircletWar {
    pub claimants: Vec<Claimant>,
    pub seated: usize,
    pub ends: i64,
}

pub struct Politics {
    pub wars: Vec<War>,
    /// Flat n×n: opinion[a*n+b] = how a's court regards b, −100..100.
    pub opinion: Vec<f64>,
    /// Aggressive-expansion dread each realm has earned in others' eyes.
    pub ae: Vec<f64>,
    /// Group solidarity, 0..1 (Ibn Khaldun by way of Turchin).
    pub asab: Vec<f64>,
    /// The ruling line's legitimacy, 0..1.
    pub legit: Vec<f64>,
    /// M11.1 — unrest, 0..1: the pressure gauge the ladder reads.
    pub unrest: Vec<f64>,
    /// M11.6 — no ladder rung may fire for a realm before this month.
    pub calm_until: Vec<i64>,
    /// M11.3 — active wars of the circlet, at most one per throne.
    pub crisis: Vec<Option<CircletWar>>,
    /// M11.6 — the house last cast down by a coup, per realm: a crisis
    /// pretender may carry it back (the restoration arc).
    pub deposed: Vec<Option<String>>,
    pub vassal_of: Vec<Option<RealmId>>,
    pub tributes: Vec<Tribute>,
    /// Battle marks awaiting the map (M9.4): (x, y, month, loser town,
    /// winner realm). The world drains these into named battlefields.
    pub marks: Vec<(i64, i64, i64, String, RealmId)>,
    /// Settlements handed over this month (M9.2): the world lets the
    /// conqueror lay a name-layer over some of them.
    pub transfers: Vec<SettlementId>,
    n: usize,
}

impl Politics {
    pub fn init(n: usize) -> Politics {
        Politics {
            wars: Vec::new(),
            opinion: vec![0.0; n * n],
            ae: vec![0.0; n],
            asab: vec![0.55; n],
            legit: vec![0.7; n],
            unrest: vec![0.15; n],
            calm_until: vec![0; n],
            crisis: vec![None; n],
            deposed: vec![None; n],
            vassal_of: vec![None; n],
            tributes: Vec::new(),
            marks: Vec::new(),
            transfers: Vec::new(),
            n,
        }
    }

    pub fn op(&self, a: RealmId, b: RealmId) -> f64 {
        self.opinion[a.0 * self.n + b.0]
    }
    pub fn op_add(&mut self, a: RealmId, b: RealmId, d: f64) {
        let v = &mut self.opinion[a.0 * self.n + b.0];
        *v = (*v + d).clamp(-100.0, 100.0);
    }

    /// The realm roster grew (secession): widen every table, seeding the
    /// newcomer's rows with zeros.
    pub fn grow(&mut self, n_new: usize) {
        if n_new <= self.n {
            return;
        }
        let mut op = vec![0.0; n_new * n_new];
        for a in 0..self.n {
            for b in 0..self.n {
                op[a * n_new + b] = self.opinion[a * self.n + b];
            }
        }
        self.opinion = op;
        self.ae.resize(n_new, 0.0);
        self.asab.resize(n_new, 0.55);
        self.legit.resize(n_new, 0.7);
        self.unrest.resize(n_new, 0.25);
        self.calm_until.resize(n_new, 0);
        self.crisis.resize_with(n_new, || None);
        self.deposed.resize_with(n_new, || None);
        self.vassal_of.resize(n_new, None);
        self.n = n_new;
    }
}

// ---------------------------------------------------------------- banks

const WAR_NAMES: [&str; 12] = [
    "the Salt War", "the Amber War", "the Cattle War", "the Winter War",
    "the War of the Broken Oath", "the Border War", "the Bitter War",
    "the War of Low Tides", "the Nameless War", "the Widows' War",
    "the War of Two Rivers", "the Quarrel of Kings",
];

// ---------------------------------------------------------------- helpers

fn towns_of(setts: &[Settlement], c: RealmId) -> Vec<usize> {
    setts
        .iter()
        .enumerate()
        .filter(|(_, s)| s.realm == c)
        .map(|(i, _)| i)
        .collect()
}

pub fn alive(setts: &[Settlement], c: RealmId) -> bool {
    setts.iter().any(|s| s.realm == c)
}

fn pop_of(setts: &[Settlement], c: RealmId) -> i64 {
    setts.iter().filter(|s| s.realm == c).map(|s| s.pop).sum()
}

/// Squared distance between the closest settlements of two realms.
fn closest2(setts: &[Settlement], a: RealmId, b: RealmId) -> f64 {
    let mut best = f64::INFINITY;
    for sa in setts.iter().filter(|s| s.realm == a) {
        for sb in setts.iter().filter(|s| s.realm == b) {
            let dy = (sa.y - sb.y) as f64;
            let dx = (sa.x - sb.x) as f64;
            best = best.min(dy * dy + dx * dx);
        }
    }
    best
}

fn neighbours(setts: &[Settlement], a: RealmId, b: RealmId) -> bool {
    closest2(setts, a, b) <= NEIGHBOUR_RANGE * NEIGHBOUR_RANGE
}

/// Arts of one realm — read through its crown people (ADR-0018:
/// knowledge travels with the tongue, war chests with the crown).
fn realm_mods(realms: &[Realm], socs: &[Society], c: RealmId) -> society::Mods {
    realms
        .get(c.0)
        .and_then(|r| socs.get(r.people.idx()))
        .map(society::mods_for)
        .unwrap_or_default()
}

/// Fielded strength of one banner: pooled souls with diminishing returns,
/// sharpened by the arts of war, solidarity and a believed-in crown.
fn strength(
    setts: &[Settlement],
    realms: &[Realm],
    socs: &[Society],
    pol: &Politics,
    leader: RealmId,
    allies: &[RealmId],
) -> f64 {
    let mut total = 0.0;
    for (ci, share) in std::iter::once((leader, 1.0))
        .chain(allies.iter().map(|&c| (c, 0.5)))
    {
        let p = pop_of(setts, ci) as f64;
        if p <= 0.0 {
            continue;
        }
        let war = realm_mods(realms, socs, ci).war;
        let asab = pol.asab.get(ci.0).copied().unwrap_or(0.5);
        let legit = pol.legit.get(ci.0).copied().unwrap_or(0.7);
        total += p.powf(0.6) * war * (0.55 + 0.9 * asab) * (0.8 + 0.4 * legit) * share;
    }
    total
}

/// Hand a settlement to a new banner and say so. The realm moves; the
/// people stay (ADR-0018) — conquest plants a minority, not a migration.
fn transfer(
    setts: &mut [Settlement],
    idx: usize,
    to: RealmId,
    realms: &[Realm],
    month: i64,
    why: &str,
    events: &mut Vec<Event>,
    transfers: &mut Vec<SettlementId>,
) {
    let from = setts[idx].realm;
    setts[idx].realm = to;
    // the world may let the conqueror lay a new name over the old (M9.2)
    transfers.push(setts[idx].id);
    events.push(Event {
        m: month,
        s: setts[idx].name.clone(),
        k: EventKind::War,
        text: format!(
            "{} passes from {} to the banners of {} — {}.",
            setts[idx].name, realms[from.0].name, realms[to.0].name, why
        ),
        // anchor the ground, not the name: conquest may rename the town
        // this very tick (M9.2) and the resolver must still find it
        x: setts[idx].x,
        y: setts[idx].y,
        ..Default::default()
    });
}

// ---------------------------------------------------------------- territory

/// The shared influence kernel: every settlement projects `weight(s)`
/// out to a radius that grows with that weight; each land cell belongs
/// to the group with the strongest summed pull. Wilderness stays
/// unowned. Owner = group index, −1 = none.
fn influence_core(
    height: &Array2<f32>,
    settlements: &[Settlement],
    n_groups: usize,
    group: impl Fn(&Settlement) -> usize,
    weight: impl Fn(usize, &Settlement) -> f64,
) -> Array2<i16> {
    let (h, w) = height.dim();
    let hw = h * w;
    let mut acc = vec![0f32; hw];
    let mut stamp = vec![u16::MAX; hw];
    let mut bestv = vec![0f32; hw];
    let mut owner = vec![-1i16; hw];

    // group settlement indices in group order (deterministic)
    for c in 0..n_groups {
        let towns: Vec<&Settlement> =
            settlements.iter().filter(|s| group(s) == c).collect();
        if towns.is_empty() {
            continue;
        }
        let mut boxes: Vec<(usize, usize, usize, usize)> = Vec::new();
        for s in &towns {
            let weight = weight(c, s);
            let r = (2.2 * weight.powf(0.30)).clamp(5.0, 42.0);
            let reach = (1.45 * r).ceil() as i64;
            let y0 = (s.y - reach).max(0) as usize;
            let y1 = ((s.y + reach) as usize).min(h - 1);
            let x0 = (s.x - reach).max(0) as usize;
            let x1 = ((s.x + reach) as usize).min(w - 1);
            boxes.push((y0, y1, x0, x1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dy = y as f64 - s.y as f64;
                    let dx = x as f64 - s.x as f64;
                    let d = (dy * dy + dx * dx).sqrt();
                    if d > 1.45 * r {
                        continue;
                    }
                    let v = (weight / (1.0 + (d / r).powi(3))) as f32;
                    let i = y * w + x;
                    if stamp[i] != c as u16 {
                        stamp[i] = c as u16;
                        acc[i] = 0.0;
                    }
                    acc[i] += v;
                }
            }
        }
        // claim: compare this group's summed pull against the best so far
        for (y0, y1, x0, x1) in &boxes {
            for y in *y0..=*y1 {
                for x in *x0..=*x1 {
                    let i = y * w + x;
                    if stamp[i] == c as u16
                        && acc[i] > bestv[i]
                        && height[[y, x]] >= 0.0
                    {
                        bestv[i] = acc[i];
                        owner[i] = c as i16;
                    }
                }
            }
        }
    }
    Array2::from_shape_vec((h, w), owner).unwrap()
}

/// M4.1 — realm territory. Weight pop^0.85, raised by the crown's era
/// and solidarity. Owner = realm id, −1 = wilderness (ADR-0018).
pub fn influence_map(
    height: &Array2<f32>,
    settlements: &[Settlement],
    realms: &[Realm],
    socs: &[Society],
    asab: &[f64],
    n_realms: usize,
) -> Array2<i16> {
    influence_core(
        height,
        settlements,
        n_realms,
        |s| s.realm.0,
        |c, s| {
            let era = realms
                .get(c)
                .and_then(|r| socs.get(r.people.idx()))
                .map(|s| s.era as f64)
                .unwrap_or(0.0);
            let coh = asab.get(c).copied().unwrap_or(0.5);
            (s.pop as f64).powf(0.85) * (1.0 + 0.20 * era) * (0.75 + 0.5 * coh)
        },
    )
}

/// M10.6 — the people-axis influence map for the culture layer: where
/// each people's weight of settlement actually lies, crowns ignored.
pub fn peoples_influence_map(
    height: &Array2<f32>,
    settlements: &[Settlement],
    n_peoples: usize,
) -> Array2<i16> {
    influence_core(
        height,
        settlements,
        n_peoples,
        |s| s.people.0,
        |_, s| (s.pop as f64).powf(0.85),
    )
}

/// Run-length encode the owner grid as [run, value, run, value, …] —
/// a political map is long runs, so this ships small.
pub fn territory_rle(t: &Array2<i16>) -> Vec<i32> {
    let mut out = Vec::new();
    let mut run = 0i32;
    let mut cur = i16::MIN;
    for &v in t.iter() {
        if v == cur {
            run += 1;
        } else {
            if run > 0 {
                out.push(run);
                out.push(cur as i32);
            }
            cur = v;
            run = 1;
        }
    }
    if run > 0 {
        out.push(run);
        out.push(cur as i32);
    }
    out
}

/// E4.7 — dirty 32×32 tiles between the last-shipped and current grids.
/// `None` when the grids are identical; otherwise the JSON patch
/// `{"tw":N,"tiles":[[tx,ty,[run,val,…]],…]}` plus (changed, total) tile
/// counts so the caller can fall back to full RLE on upheavals. Each
/// tile's RLE is row-major inside the tile, same [run, value, …] code
/// the full grid uses.
pub fn territory_tile_patch(
    prev: &Array2<i16>,
    cur: &Array2<i16>,
    tile: usize,
) -> Option<(serde_json::Value, usize, usize)> {
    debug_assert_eq!(prev.dim(), cur.dim());
    let (h, w) = cur.dim();
    let tx_n = w.div_ceil(tile);
    let ty_n = h.div_ceil(tile);
    let mut tiles: Vec<serde_json::Value> = Vec::new();
    for ty in 0..ty_n {
        for tx in 0..tx_n {
            let (x0, y0) = (tx * tile, ty * tile);
            let (x1, y1) = ((x0 + tile).min(w), (y0 + tile).min(h));
            let mut differs = false;
            'scan: for y in y0..y1 {
                for x in x0..x1 {
                    if prev[[y, x]] != cur[[y, x]] {
                        differs = true;
                        break 'scan;
                    }
                }
            }
            if !differs {
                continue;
            }
            let mut rle: Vec<i32> = Vec::new();
            let mut run = 0i32;
            let mut cv = i16::MIN;
            for y in y0..y1 {
                for x in x0..x1 {
                    let v = cur[[y, x]];
                    if v == cv {
                        run += 1;
                    } else {
                        if run > 0 {
                            rle.push(run);
                            rle.push(cv as i32);
                        }
                        cv = v;
                        run = 1;
                    }
                }
            }
            if run > 0 {
                rle.push(run);
                rle.push(cv as i32);
            }
            tiles.push(serde_json::json!([tx, ty, rle]));
        }
    }
    if tiles.is_empty() {
        return None;
    }
    let changed = tiles.len();
    Some((
        serde_json::json!({ "tw": tile, "tiles": tiles }),
        changed,
        tx_n * ty_n,
    ))
}

// ---------------------------------------------------------------- monthly

/// One month of statecraft. Returns (events, borders_changed).
#[allow(clippy::too_many_arguments)]
pub fn monthly(
    pol: &mut Politics,
    record: &mut Chronicle,
    peoples: &mut Peoples,
    territory: &Array2<i16>,
    taken: &mut HashSet<String>,
    month: i64,
    rng: &mut Pcg64Mcg,
) -> (Vec<Event>, bool) {
    let Chronicle { state: chron, registry: reg, .. } = record;
    let Peoples { settlements, peoples: peoples_v, realms, societies: socs, coresidence, .. } = peoples;
    let mut events = Vec::new();
    let mut borders_changed = false;
    let n = realms.len();
    if n == 0 {
        return (events, false);
    }
    pol.grow(n);

    // --- the slow tides: opinion and dread fade, friction grinds
    for v in pol.opinion.iter_mut() {
        *v *= OPINION_DECAY;
    }
    for v in pol.ae.iter_mut() {
        *v *= AE_DECAY;
    }
    for a in (0..n).map(RealmId) {
        for b in (0..n).map(RealmId) {
            if a != b && alive(settlements, a) && alive(settlements, b)
                && neighbours(settlements, a, b)
            {
                pol.op_add(a, b, -FRICTION);
            }
        }
    }
    // M12.3 — the kinship pull: shared tongue and gods draw courts
    // together, gently, toward a modest warmth — never past a grudge
    // that a war is actively feeding. This is what makes the union of
    // crowns reachable at all: without it opinion only decays and
    // grinds, and no two courts ever stand warm enough to join.
    for a in 0..n {
        for b in 0..n {
            let (ra, rb) = (RealmId(a), RealmId(b));
            if a == b || !alive(settlements, ra) || !alive(settlements, rb) {
                continue;
            }
            if pol.wars.iter().any(|w| w.involves(ra) && w.involves(rb)) {
                continue;
            }
            let kin = culture::kinship(realms[a].people, realms[b].people, peoples_v, coresidence);
            if kin >= 0.55 && pol.op(ra, rb) < 30.0 {
                pol.op_add(ra, rb, 0.10 + 0.25 * kin);
            }
        }
    }

    // --- asabiyyah: solidarity surges at hard frontiers, gutters in
    // safe hearts; legitimacy drifts home to a workable middle
    for c in (0..n).map(RealmId) {
        if !alive(settlements, c) {
            continue;
        }
        let at_war = pol.wars.iter().any(|w| w.involves(c));
        let frontier = frontier_exposure(settlements, territory, peoples_v, realms, c);
        let up = ASAB_SURGE * (frontier + if at_war { 0.6 } else { 0.0 });
        pol.asab[c.0] = (pol.asab[c.0] + up - ASAB_DECAY).clamp(0.05, 1.0);
        let target = 0.72;
        pol.legit[c.0] += (target - pol.legit[c.0]) * 0.0025;
    }

    // --- unrest (M11.1): the gauge the ladder reads. A weak crown,
    // guttered solidarity, hunger in the towns, long wars, and holdings
    // beyond the seat's administrative reach all feed it; quiet months
    // bleed it off. A throne in dispute burns solidarity instead.
    for c in (0..n).map(RealmId) {
        if !alive(settlements, c) {
            continue;
        }
        if pol.crisis[c.0].is_some() {
            pol.asab[c.0] = (pol.asab[c.0] - 0.003).max(0.05);
            continue;
        }
        let towns = towns_of(settlements, c);
        if towns.is_empty() {
            continue;
        }
        let hungry = towns.iter().filter(|&&i| settlements[i].failing).count() as f64
            / towns.len() as f64;
        let weary = pol
            .wars
            .iter()
            .filter(|w| w.involves(c))
            .map(|w| (((month - w.start) as f64) / 120.0).min(1.0))
            .sum::<f64>()
            .min(1.0);
        let polity = socs.get(realms[c.0].people.idx()).map_or(0, |so| so.polity.min(3));
        let far = seat_of(settlements, realms, c).map_or(0.0, |(_, _, sx, sy)| {
            let out = towns
                .iter()
                .filter(|&&i| {
                    let dy = (settlements[i].y - sy) as f64;
                    let dx = (settlements[i].x - sx) as f64;
                    (dy * dy + dx * dx).sqrt() > ADMIN_REACH[polity]
                })
                .count();
            out as f64 / towns.len() as f64
        });
        let du = 0.024 * ((0.62 - pol.legit[c.0]).max(0.0) / 0.62)
            + 0.014 * ((0.30 - pol.asab[c.0]).max(0.0) / 0.30)
            + 0.020 * hungry
            + 0.012 * weary
            + 0.022 * far
            - UNREST_CALM;
        pol.unrest[c.0] = (pol.unrest[c.0] + du).clamp(0.0, 1.0);
    }

    // --- tribute caravans set out
    let mut spent: Vec<Event> = Vec::new();
    pol.tributes.retain_mut(|t| {
        t.months_left -= 1;
        if let Some(s) = realms.get_mut(t.from.0) {
            let pay = t.per_month.min(s.treasury);
            s.treasury = round2(s.treasury - pay);
            if let Some(r) = realms.get_mut(t.to.0) {
                r.treasury = round2(r.treasury + pay);
            }
        }
        if t.months_left <= 0 {
            spent.push(Event {
                m: month,
                s: realms[t.from.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "The last tribute caravan of {} reaches {}; the debt of the old war is paid.",
                    realms[t.from.0].name, realms[t.to.0].name
                ),
                ..Default::default()
            });
            false
        } else {
            true
        }
    });
    events.extend(spent);

    // --- vassals pay their dues, and sometimes slip the leash
    for v in (0..n).map(RealmId) {
        let Some(suz) = pol.vassal_of[v.0] else { continue };
        if !alive(settlements, v) {
            pol.vassal_of[v.0] = None;
            continue;
        }
        if !alive(settlements, suz) {
            pol.vassal_of[v.0] = None;
            events.push(Event {
                m: month,
                s: realms[v.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "With the fall of their masters, {} answers to no one again.",
                    realms[v.0].name
                ),
                ..Default::default()
            });
            continue;
        }
        if let Some(s) = realms.get_mut(v.0) {
            let due = round2((s.treasury * 0.006).max(0.3).min(s.treasury));
            s.treasury = round2(s.treasury - due);
            if let Some(r) = realms.get_mut(suz.0) {
                r.treasury = round2(r.treasury + due);
            }
        }
        // independence: high solidarity, a distracted or weakened master
        let suz_at_war = pol.wars.iter().any(|w| w.involves(suz));
        let sv = strength(settlements, realms, socs, pol, v, &[]);
        let ss = strength(settlements, realms, socs, pol, suz, &[]);
        let opening = if suz_at_war || sv > ss { 1.0 } else { 0.2 };
        if rng.gen::<f64>() < 0.0022 * pol.asab[v.0] * opening {
            pol.vassal_of[v.0] = None;
            pol.op_add(v, suz, -50.0);
            pol.op_add(suz, v, -50.0);
            events.push(Event {
                m: month,
                s: realms[v.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "{} casts off the yoke of {} and stands as its own realm once more.",
                    realms[v.0].name, realms[suz.0].name
                ),
                ..Default::default()
            });
            if pol.wars.len() < 3 && rng.gen::<f64>() < 0.5 {
                kindle_war(pol, rng, month, suz, v, peoples_v, realms, "the War of the Broken Leash", &mut events, taken, reg);
            }
        }
    }

    // --- fortification: exposed border towns raise walls (treasury sink)
    if month.rem_euclid(12) == 3 {
        for c in (0..n).map(RealmId) {
            if !alive(settlements, c) {
                continue;
            }
            let treasury = realms.get(c.0).map(|s| s.treasury).unwrap_or(0.0);
            let cost_next = |f: u8| 30.0 + 25.0 * f as f64;
            if treasury < cost_next(0) + 30.0 {
                continue;
            }
            // most exposed town: closest to any foreign settlement, walls not maxed
            let mut best: Option<(usize, f64)> = None;
            for &i in &towns_of(settlements, c) {
                if settlements[i].fort >= 3 || settlements[i].pop < 150 {
                    continue;
                }
                let mut d2 = f64::INFINITY;
                for o in settlements.iter().filter(|o| o.realm != c) {
                    let dy = (o.y - settlements[i].y) as f64;
                    let dx = (o.x - settlements[i].x) as f64;
                    d2 = d2.min(dy * dy + dx * dx);
                }
                if d2 < 60.0 * 60.0 && best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                    best = Some((i, d2));
                }
            }
            if let Some((i, _)) = best {
                let cost = cost_next(settlements[i].fort);
                if let Some(s) = realms.get_mut(c.0) {
                    if s.treasury >= cost + 30.0 {
                        s.treasury = round2(s.treasury - cost);
                        settlements[i].fort += 1;
                        let what = match settlements[i].fort {
                            1 => "raises a palisade of sharpened oak",
                            2 => "rings itself in stone walls",
                            _ => "crowns its walls with towers and an iron gate",
                        };
                        let mut ids: crate::event::EventIds = Default::default();
                        if let Some(e) =
                            reg.find_kind(EntityKind::Settlement, &settlements[i].name)
                        {
                            ids.push(e);
                        }
                        events.push(Event {
                            m: month,
                            s: settlements[i].name.clone(),
                            k: EventKind::Society,
                            text: format!("{} {} — the border is watched.", settlements[i].name, what),
                            ids,
                            x: settlements[i].x,
                            y: settlements[i].y,
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    // --- wars: battles, raids, sieges, and peace
    let (war_events, changed) = conduct_wars(pol, rng, month, settlements, peoples_v, realms, socs, reg);
    events.extend(war_events);
    borders_changed |= changed;

    // --- new wars kindle out of grievance and dread
    if pol.wars.len() < 3 {
        'outer: for a in (0..n).map(RealmId) {
            for b in (0..n).map(RealmId) {
                if a == b {
                    continue;
                }
                if !alive(settlements, a) || !alive(settlements, b) {
                    continue;
                }
                if pol.crisis[a.0].is_some() || pol.crisis[b.0].is_some() {
                    continue; // a throne in dispute wages no outward war (M11.3)
                }
                if pol.vassal_of[a.0].is_some() || pol.vassal_of[b.0] == Some(a) {
                    continue;
                }
                let already = pol.wars.iter().any(|w| w.involves(a) || w.involves(b));
                if already || !neighbours(settlements, a, b) {
                    continue;
                }
                let grudge = (-pol.op(a, b) / 70.0).clamp(0.0, 1.0);
                let dread = (pol.ae[b.0] / 45.0).clamp(0.0, 1.0);
                let appetite = 0.5 + pol.asab[a.0];
                let haz = 0.0010 * (1.0 + 2.2 * grudge + 1.4 * dread) * appetite;
                if rng.gen::<f64>() < haz {
                    // one war in three is sworn before a god (M3.5) — the
                    // crown people's war god carries the banner
                    let crown_a = crown(peoples_v, realms, a);
                    let war_god = crown_a
                        .pantheon
                        .iter()
                        .find(|g| g.domain == "war")
                        .or_else(|| crown_a.pantheon.first());
                    let name = if rng.gen::<f64>() < 0.33 && war_god.is_some() {
                        format!("the War of {}'s Altar", war_god.unwrap().name)
                    } else {
                        WAR_NAMES[rng.gen_range(0..WAR_NAMES.len())].to_string()
                    };
                    kindle_war(pol, rng, month, a, b, peoples_v, realms, &name, &mut events, taken, reg);
                    // coalitions: realms that dread the aggressor rally to
                    // the defender's banner (M4.3)
                    let wi = pol.wars.len() - 1;
                    let mut joined = Vec::new();
                    for j in (0..n).map(RealmId) {
                        if j == a || j == b || !alive(settlements, j) {
                            continue;
                        }
                        if pol.vassal_of[j.0].is_some() {
                            continue;
                        }
                        if pol.wars.iter().enumerate().any(|(k, w)| k != wi && w.involves(j)) {
                            continue;
                        }
                        let dreads = pol.ae[a.0] > 28.0 && pol.op(j, a) < -12.0;
                        let near = neighbours(settlements, j, a) || neighbours(settlements, j, b);
                        if dreads && near && joined.len() < 3 {
                            joined.push(j);
                        }
                    }
                    for &j in &joined {
                        pol.wars[wi].allies_b.push(j);
                        pol.op_add(j, b, 15.0);
                        pol.op_add(b, j, 15.0);
                        events.push(Event {
                            m: month,
                            s: realms[j.0].name.clone(),
                            k: EventKind::War,
                            text: format!(
                                "Dreading the appetite of {}, {} swears common cause with {}.",
                                realms[a.0].name, realms[j.0].name, realms[b.0].name
                            ),
                            ..Default::default()
                        });
                    }
                    // loyal vassals march with their suzerain
                    for j in (0..n).map(RealmId) {
                        if pol.vassal_of[j.0] == Some(b) && alive(settlements, j) {
                            pol.wars[wi].allies_b.push(j);
                        } else if pol.vassal_of[j.0] == Some(a) && alive(settlements, j) {
                            pol.wars[wi].allies_a.push(j);
                        }
                    }
                    break 'outer;
                }
            }
        }
    }

    // --- wars of the circlet resolve (M11.3): the throne settled by
    // blood inside the hall — the winner's house rules, no border moves
    events.extend(resolve_circlet_wars(pol, chron, rng, month, settlements, realms, reg));

    // --- the unrest ladder (M11): pressure vents on the lowest rung
    // that fits — riots, a charter, a palace coup — and only tears the
    // map when the secession gate opens (M11.4, formerly the M4.5 roll)
    let (lad, lad_borders) = unrest_ladder(
        pol, chron, rng, taken, month, settlements, peoples_v, realms, socs, reg,
    );
    borders_changed |= lad_borders;
    events.extend(lad);

    // --- the ledger of the living: a realm's alive flag follows its towns
    for r in realms.iter_mut() {
        let holds = settlements.iter().any(|s| s.realm == r.id);
        if r.alive && !holds {
            r.alive = false;
        }
    }

    // --- the seat endures, or it does not (M10.4): a crown whose seat
    // is lost — taken in war, ceded at the peace table, or fallen silent —
    // removes to its greatest remaining town and pays for the shame in
    // legitimacy. A seat merely outgrown translates the court quietly:
    // an event, no shock.
    for c in (0..n).map(RealmId) {
        if !realms[c.0].alive || !alive(settlements, c) {
            continue;
        }
        let seat_id = realms[c.0].seat;
        let held = settlements.iter().find(|s| s.id == seat_id);
        let ours = held.is_some_and(|s| s.realm == c);
        let Some(best) = towns_of(settlements, c)
            .into_iter()
            .max_by_key(|&i| settlements[i].pop)
        else {
            continue;
        };
        if !ours {
            // the seat has fallen — re-home the crown, and let it smart
            let shock = if held.is_some() { 0.16 } else { 0.08 };
            pol.legit[c.0] = (pol.legit[c.0] - shock).max(0.05);
            let text = match held {
                Some(fallen) => format!(
                    "The seat of {} is lost: {} flies the banners of {}, and the crown of {} removes to {} under a shadow.",
                    realms[c.0].name,
                    fallen.name,
                    realms[fallen.realm.0].name,
                    realms[c.0].house,
                    settlements[best].name
                ),
                None => format!(
                    "The old seat of {} lies silent; the crown of {} removes to {} under a shadow.",
                    realms[c.0].name, realms[c.0].house, settlements[best].name
                ),
            };
            realms[c.0].seat = settlements[best].id;
            let mut ids: crate::event::EventIds = Default::default();
            if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
                ids.push(e);
            }
            events.push(Event {
                m: month,
                s: realms[c.0].name.clone(),
                k: EventKind::Realm,
                text,
                ids,
                x: settlements[best].x,
                y: settlements[best].y,
                ..Default::default()
            });
        } else if settlements[best].id != seat_id
            && settlements[best].pop as f64
                > 1.6 * held.map_or(1.0, |s| s.pop.max(1) as f64)
        {
            // quiet translation: the halls of another town now outshine
            // the old seat, and the court follows the splendour
            let old_name = held.map(|s| s.name.clone()).unwrap_or_default();
            realms[c.0].seat = settlements[best].id;
            let mut ids: crate::event::EventIds = Default::default();
            if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
                ids.push(e);
            }
            events.push(Event {
                m: month,
                s: realms[c.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "The court of {} removes from {} to {}, whose halls now outshine the old seat.",
                    realms[c.0].name, old_name, settlements[best].name
                ),
                ids,
                x: settlements[best].x,
                y: settlements[best].y,
                ..Default::default()
            });
        }
    }

    (events, borders_changed)
}

/// Share of a realm's towns that sit on a hard frontier: foreign-owned
/// territory whose crown people is of a different *style* (the
/// meta-ethnic edge) within reach.
fn frontier_exposure(
    setts: &[Settlement],
    territory: &Array2<i16>,
    peoples_v: &[People],
    realms: &[Realm],
    c: RealmId,
) -> f64 {
    let (h, w) = territory.dim();
    let towns = towns_of(setts, c);
    if towns.is_empty() {
        return 0.0;
    }
    let my_style = &crown(peoples_v, realms, c).style;
    let mut exposed = 0usize;
    for &i in &towns {
        let (sy, sx) = (setts[i].y as isize, setts[i].x as isize);
        let mut hit = false;
        'scan: for dy in (-8isize..=8).step_by(4) {
            for dx in (-8isize..=8).step_by(4) {
                let y = sy + dy;
                let x = sx + dx;
                if y < 0 || x < 0 || y as usize >= h || x as usize >= w {
                    continue;
                }
                let o = territory[[y as usize, x as usize]];
                if o >= 0 && o as usize != c.0 && (o as usize) < realms.len() {
                    let os = &crown(peoples_v, realms, RealmId(o as usize)).style;
                    if os != my_style {
                        hit = true;
                        break 'scan;
                    }
                }
            }
        }
        if hit {
            exposed += 1;
        }
    }
    exposed as f64 / towns.len() as f64
}

#[allow(clippy::too_many_arguments)]
fn kindle_war(
    pol: &mut Politics,
    rng: &mut Pcg64Mcg,
    month: i64,
    a: RealmId,
    b: RealmId,
    peoples_v: &[People],
    realms: &[Realm],
    name: &str,
    events: &mut Vec<Event>,
    taken: &mut HashSet<String>,
    reg: &mut Registry,
) {
    let until = month + rng.gen_range(24..72);
    // the war and the generals who will carry it enter the telling (M6.2)
    let war_ent = reg.add(EntityKind::War, name, month, None, -1, -1);
    let gen_a_name = naming::make_word(rng, &crown(peoples_v, realms, a).style, taken);
    let gen_b_name = naming::make_word(rng, &crown(peoples_v, realms, b).style, taken);
    let gen_a = reg.add_person(&gen_a_name, "general", month, Some(realms[a.0].people));
    let gen_b = reg.add_person(&gen_b_name, "general", month, Some(realms[b.0].people));
    let ca_ent = reg.find_kind(EntityKind::Realm, &realms[a.0].name).unwrap_or(EntityId(-1));
    let cb_ent = reg.find_kind(EntityKind::Realm, &realms[b.0].name).unwrap_or(EntityId(-1));
    events.push(Event {
        m: month,
        s: name.to_string(),
        k: EventKind::War,
        text: format!(
            "War kindles between {} and {} — men will call it {}. The banners of {} follow {}; {} looks to {}.",
            realms[a.0].name, realms[b.0].name, name,
            realms[a.0].name, gen_a_name, realms[b.0].name, gen_b_name
        ),
        ids: smallvec![war_ent, ca_ent, cb_ent, gen_a, gen_b],
        ..Default::default()
    });
    pol.wars.push(War {
        a,
        b,
        allies_a: Vec::new(),
        allies_b: Vec::new(),
        name: name.to_string(),
        start: month,
        until,
        score: 0.0,
        siege: None,
        ent: war_ent,
        gen_a,
        gen_b,
        wins_a: 0,
        wins_b: 0,
        marked: false,
        last_sign: 0,
        flips: 0,
    });
    pol.op_add(a, b, -25.0);
    pol.op_add(b, a, -35.0);
}

/// Won a second field: the soldiers give their general a name (M6.8).
const GENERAL_EPITHETS: [&str; 6] = [
    "the Hammer", "the Unbroken", "the Wolf", "the Iron-Handed", "the Fox", "the Grim",
];

/// Deterministic index from the month and a name — no rng stream touched,
/// so flavor picks never perturb the simulation's dice (ADR-0003).
fn det_idx(month: i64, name: &str) -> usize {
    crate::telling::det_hash(month as u64, name) as usize
}

/// Battles, raids and sieges for every burning war; peaces where wars end.
#[allow(clippy::too_many_arguments)]
fn conduct_wars(
    pol: &mut Politics,
    rng: &mut Pcg64Mcg,
    month: i64,
    settlements: &mut Vec<Settlement>,
    _peoples_v: &[People],
    realms: &mut Vec<Realm>,
    socs: &mut [Society],
    reg: &mut Registry,
) -> (Vec<Event>, bool) {
    let mut events = Vec::new();
    let mut borders_changed = false;
    let mut ended: Vec<usize> = Vec::new();

    for wi in 0..pol.wars.len() {
        let (a, b) = (pol.wars[wi].a, pol.wars[wi].b);
        let side_a = pol.wars[wi].side_a();
        let side_b = pol.wars[wi].side_b();
        let sa = strength(settlements, realms, socs, pol, a, &pol.wars[wi].allies_a);
        let sb = strength(settlements, realms, socs, pol, b, &pol.wars[wi].allies_b);
        let total = (sa + sb).max(1e-9);

        // war chests drain while the banners fly
        for &side in side_a.iter().chain(side_b.iter()) {
            if let Some(s) = realms.get_mut(side.0) {
                s.treasury = round2((s.treasury - 3.0).max(0.0));
            }
        }

        // --- pitched battle
        if rng.gen::<f64>() < 0.09 {
            let roll = sa / total + rng.gen_range(-0.18..0.18);
            let a_wins = roll > 0.5;
            let (winner, loser) = if a_wins { (a, b) } else { (b, a) };
            let swing = rng.gen_range(2.5..5.5);
            pol.wars[wi].score += if a_wins { swing } else { -swing };
            // the reversal detector counts every turn of the tide (M6.7)
            let sign = if pol.wars[wi].score > 2.0 {
                1
            } else if pol.wars[wi].score < -2.0 {
                -1
            } else {
                0
            };
            if sign != 0 {
                if pol.wars[wi].last_sign != 0 && sign != pol.wars[wi].last_sign {
                    pol.wars[wi].flips += 1;
                }
                pol.wars[wi].last_sign = sign;
            }
            pol.legit[winner.0] = (pol.legit[winner.0] + 0.015).min(1.0);
            pol.legit[loser.0] = (pol.legit[loser.0] - 0.03).max(0.0);
            // the field is named for the loser's nearest town
            let field = towns_of(settlements, loser)
                .into_iter()
                .min_by_key(|&i| {
                    settlements
                        .iter()
                        .filter(|o| o.realm == winner)
                        .map(|o| {
                            let dy = o.y - settlements[i].y;
                            let dx = o.x - settlements[i].x;
                            dy * dy + dx * dx
                        })
                        .min()
                        .unwrap_or(i64::MAX)
                });
            if let Some(fi) = field {
                let loss = ((settlements[fi].pop as f64 * rng.gen_range(0.02..0.05)) as i64).max(3);
                settlements[fi].pop = (settlements[fi].pop - loss).max(40);
                // the winning general takes the credit (M6.2)
                let gen = if a_wins { pol.wars[wi].gen_a } else { pol.wars[wi].gen_b };
                let wins = if a_wins {
                    pol.wars[wi].wins_a += 1;
                    pol.wars[wi].wins_a
                } else {
                    pol.wars[wi].wins_b += 1;
                    pol.wars[wi].wins_b
                };
                let gen_name = reg.get(gen).map(|e| e.name.clone()).unwrap_or_default();
                let mut coda = String::new();
                if !gen_name.is_empty() {
                    let prior = reg.mention(gen);
                    if wins == 2 {
                        // second field won: the soldiers coin the name (M6.8)
                        let ep = GENERAL_EPITHETS
                            [det_idx(month, &gen_name) % GENERAL_EPITHETS.len()];
                        if reg.earn_epithet(gen, ep) {
                            coda = format!(" The soldiers begin to call {} \"{}\".", gen_name, ep);
                        }
                    } else if let Some(ep) =
                        reg.get(gen).and_then(|e| e.epithets.last().cloned())
                    {
                        // an earned name is used ever after (M6.8)
                        coda = format!(" {} {} held the line once more.", gen_name, ep);
                    } else if prior >= 2 {
                        coda = format!(
                            " {}, remembered from earlier fields, led the charge again.",
                            gen_name
                        );
                    } else {
                        coda = format!(
                            " {} of {} held the day.",
                            gen_name, realms[winner.0].name
                        );
                    }
                }
                events.push(Event {
                    m: month,
                    s: pol.wars[wi].name.clone(),
                    k: EventKind::War,
                    text: format!(
                        "The hosts meet under the walls of {} — {} carries the day, and {} leaves its dead on the field.{}",
                        settlements[fi].name, realms[winner.0].name, realms[loser.0].name, coda
                    ),
                    ids: smallvec![pol.wars[wi].ent, gen],
                    x: settlements[fi].x,
                    y: settlements[fi].y,
                    ..Default::default()
                });
                // a decisive field earns a name on the map — once per war (M9.4)
                if !pol.wars[wi].marked && swing >= 4.0 {
                    pol.wars[wi].marked = true;
                    pol.marks.push((
                        settlements[fi].x,
                        settlements[fi].y,
                        month,
                        settlements[fi].name.clone(),
                        winner,
                    ));
                }
            }
        }

        // --- raids burn the borderlands
        if rng.gen::<f64>() < 0.18 {
            let a_raids = rng.gen::<f64>() < sa / total;
            let (attacker, victim_c) = if a_raids { (a, b) } else { (b, a) };
            let att_war = realm_mods(realms, socs, attacker).war;
            let walls = realm_mods(realms, socs, victim_c).defense;
            let victims: Vec<usize> = settlements
                .iter()
                .enumerate()
                .filter(|(_, s)| s.realm == victim_c && s.pop > 90)
                .map(|(i, _)| i)
                .collect();
            if !victims.is_empty() {
                let vi = victims[rng.gen_range(0..victims.len())];
                let fortwall = 1.0 / (1.0 + 0.45 * settlements[vi].fort as f64);
                let frac = rng.gen_range(0.02..0.07) * att_war * walls * fortwall;
                let loss = ((settlements[vi].pop as f64 * frac) as i64).max(5);
                settlements[vi].pop = (settlements[vi].pop - loss).max(40);
                let plunder = round2(
                    settlements[vi].wealth * 0.25 * att_war.min(1.6) * walls * fortwall,
                );
                settlements[vi].wealth = round2((settlements[vi].wealth - plunder).max(0.0));
                if let Some(sa_) = realms.get_mut(attacker.0) {
                    sa_.treasury = round2(sa_.treasury + 0.6 * plunder);
                }
                pol.wars[wi].score += if a_raids { 1.0 } else { -1.0 } * (0.6 + plunder / 60.0).min(2.0);
                let text = if plunder > 25.0 {
                    format!(
                        "Raiders of {} burn the fields of {} — {} souls lost, {} in coin carried off.",
                        realms[attacker.0].name,
                        settlements[vi].name,
                        loss,
                        plunder.round() as i64
                    )
                } else {
                    format!(
                        "Raiders of {} burn the fields of {} — {} souls lost.",
                        realms[attacker.0].name, settlements[vi].name, loss
                    )
                };
                events.push(Event {
                    m: month,
                    s: settlements[vi].name.clone(),
                    k: EventKind::War,
                    text,
                    ids: smallvec![pol.wars[wi].ent],
                    x: settlements[vi].x,
                    y: settlements[vi].y,
                    ..Default::default()
                });
            }
        }

        // --- sieges: the slow arithmetic of walls (M4.4)
        if pol.wars[wi].siege.is_none() && rng.gen::<f64>() < 0.06 {
            // the stronger side moves first, but either may try
            let a_besieges = rng.gen::<f64>() < sa / total;
            let (att, def) = if a_besieges { (a, b) } else { (b, a) };
            // nearest worthwhile enemy town to any of the attacker's
            let target = towns_of(settlements, def)
                .into_iter()
                .filter(|&i| settlements[i].pop > 120)
                .min_by_key(|&i| {
                    settlements
                        .iter()
                        .filter(|o| o.realm == att)
                        .map(|o| {
                            let dy = o.y - settlements[i].y;
                            let dx = o.x - settlements[i].x;
                            dy * dy + dx * dx
                        })
                        .min()
                        .unwrap_or(i64::MAX)
                });
            if let Some(ti) = target {
                pol.wars[wi].siege = Some(Siege {
                    target: settlements[ti].id,
                    attacker: att,
                    progress: 0.0,
                });
                events.push(Event {
                    m: month,
                    s: settlements[ti].name.clone(),
                    k: EventKind::War,
                    text: format!(
                        "The host of {} sits down before {} — the siege begins.",
                        realms[att.0].name, settlements[ti].name
                    ),
                    ids: smallvec![pol.wars[wi].ent],
                    x: settlements[ti].x,
                    y: settlements[ti].y,
                    ..Default::default()
                });
            }
        }
        if let Some(siege) = pol.wars[wi].siege.clone() {
            let ti = settlements.iter().position(|s| s.id == siege.target);
            match ti {
                Some(ti) if settlements[ti].realm != siege.attacker => {
                    let att = siege.attacker;
                    let def = settlements[ti].realm;
                    let (satt, sdef) = if att == a { (sa, sb) } else { (sb, sa) };
                    // besiegers eat coin
                    if let Some(s) = realms.get_mut(att.0) {
                        s.treasury = round2((s.treasury - 2.0).max(0.0));
                    }
                    // relief: the defenders may break the siege
                    if rng.gen::<f64>() < 0.05 * (sdef / satt.max(1e-9)).min(2.0) {
                        pol.wars[wi].siege = None;
                        pol.wars[wi].score += if att == a { -2.0 } else { 2.0 };
                        events.push(Event {
                            m: month,
                            s: settlements[ti].name.clone(),
                            k: EventKind::War,
                            text: format!(
                                "A relief host of {} scatters the besiegers — {} breathes again.",
                                realms[def.0].name, settlements[ti].name
                            ),
                            ids: smallvec![pol.wars[wi].ent],
                            x: settlements[ti].x,
                            y: settlements[ti].y,
                            ..Default::default()
                        });
                    } else {
                        let fort = settlements[ti].fort as f64;
                        let pace = 16.0 * (satt / (satt + sdef).max(1e-9))
                            / (1.0 + 0.55 * fort)
                            * rng.gen_range(0.6..1.4);
                        let progress = siege.progress + pace;
                        if progress >= 100.0 {
                            // the wall falls
                            pol.wars[wi].siege = None;
                            let sack = ((settlements[ti].pop as f64 * 0.12) as i64).max(10);
                            settlements[ti].pop = (settlements[ti].pop - sack).max(60);
                            let plunder = round2(settlements[ti].wealth * 0.4);
                            settlements[ti].wealth = round2(settlements[ti].wealth - plunder);
                            if let Some(s) = realms.get_mut(att.0) {
                                s.treasury = round2(s.treasury + 0.7 * plunder);
                            }
                            settlements[ti].fort = settlements[ti].fort.saturating_sub(1);
                            transfer(
                                settlements, ti, att, realms, month,
                                "taken by storm after the long siege", &mut events,
                                &mut pol.transfers,
                            );
                            borders_changed = true;
                            pol.wars[wi].score += if att == a { 9.0 } else { -9.0 };
                            pol.ae[att.0] = (pol.ae[att.0] + 12.0).min(100.0);
                        } else {
                            pol.wars[wi].siege = Some(Siege { progress, ..siege });
                        }
                    }
                }
                _ => pol.wars[wi].siege = None, // town changed hands or vanished
            }
        }

        // --- is this war over?
        let score = pol.wars[wi].score;
        let a_dead = !alive(settlements, a);
        let b_dead = !alive(settlements, b);
        if month >= pol.wars[wi].until || score.abs() >= SCORE_DECISIVE || a_dead || b_dead {
            ended.push(wi);
        }
    }

    // --- peace terms, in the order the wars ended (M4.2)
    for &wi in ended.iter().rev() {
        let war = pol.wars.remove(wi);
        let (evs, changed) = make_peace(pol, rng, month, &war, settlements, realms, reg);
        events.extend(evs);
        borders_changed |= changed;
    }

    (events, borders_changed)
}

/// The score decides the terms: nothing, tribute, land, or the yoke.
#[allow(clippy::too_many_arguments)]
fn make_peace(
    pol: &mut Politics,
    _rng: &mut Pcg64Mcg,
    month: i64,
    war: &War,
    settlements: &mut [Settlement],
    realms: &mut [Realm],
    reg: &mut Registry,
) -> (Vec<Event>, bool) {
    let mut events = Vec::new();
    let mut borders_changed = false;
    let margin = war.score.abs();
    let (winner, loser) = if war.score >= 0.0 { (war.a, war.b) } else { (war.b, war.a) };

    // the war leaves the telling: generals go home, the war entity closes,
    // and a war whose tide turned twice earns its mark (M6.7)
    if war.flips >= 2 {
        reg.earn_epithet(war.ent, "the Tide-Turned");
    }
    for (gen, side) in [(war.gen_a, war.a), (war.gen_b, war.b)] {
        let banner = realms.get(side.0).map(|r| r.name.clone()).unwrap_or_default();
        reg.close(
            gen,
            month,
            &format!("led the hosts of {} in {}", banner, war.name),
        );
    }
    let verdict = if margin < SCORE_TRIBUTE {
        "ended with neither side the better for it".to_string()
    } else {
        format!(
            "ended in victory for {}",
            realms.get(winner.0).map(|r| r.name.as_str()).unwrap_or("the victors")
        )
    };
    reg.close(war.ent, month, &verdict);

    // co-belligerents part as friends
    for side in [war.side_a(), war.side_b()] {
        for i in 0..side.len() {
            for j in (i + 1)..side.len() {
                pol.op_add(side[i], side[j], 12.0);
                pol.op_add(side[j], side[i], 12.0);
            }
        }
    }

    if !alive(settlements, loser) {
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::War,
            text: format!(
                "{} gutters out — of {} nothing remains to make peace with.",
                war.name, realms[loser.0].name
            ),
            ids: smallvec![war.ent],
            ..Default::default()
        });
        return (events, false);
    }

    if margin < SCORE_TRIBUTE || !alive(settlements, winner) {
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::War,
            text: format!(
                "Peace is sworn between {} and {}; {} is over, and neither side gained more than graves.",
                realms[war.a.0].name, realms[war.b.0].name, war.name
            ),
            ids: smallvec![war.ent],
            ..Default::default()
        });
        return (events, false);
    }

    pol.legit[winner.0] = (pol.legit[winner.0] + 0.08).min(1.0);
    pol.legit[loser.0] = (pol.legit[loser.0] - 0.12).max(0.0);
    // defeat, felt at the frontier, hardens the losers' solidarity
    pol.asab[loser.0] = (pol.asab[loser.0] + 0.08).min(1.0);
    pol.op_add(loser, winner, -40.0);
    pol.op_add(winner, loser, -10.0);

    if margin < SCORE_CEDE {
        // tribute: a lump of the loser's treasury and ten years of caravans
        let lump = round2(realms.get(loser.0).map(|s| s.treasury * 0.35).unwrap_or(0.0));
        if let Some(s) = realms.get_mut(loser.0) {
            s.treasury = round2(s.treasury - lump);
        }
        if let Some(s) = realms.get_mut(winner.0) {
            s.treasury = round2(s.treasury + lump);
        }
        let per_month = round2((0.6 + lump * 0.01).min(4.0));
        pol.tributes.push(Tribute {
            from: loser,
            to: winner,
            per_month,
            months_left: 120,
        });
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::War,
            text: format!(
                "{} ends: {} buys its peace — {} in coin, and tribute caravans to {} for ten years.",
                war.name, realms[loser.0].name, lump.round() as i64, realms[winner.0].name
            ),
            ..Default::default()
        });
        return (events, false);
    }

    // land changes hands: the loser's towns nearest the winner
    let ceded = 1 + ((margin - SCORE_CEDE) / 9.0) as usize;
    let mut loser_towns = towns_of(settlements, loser);
    loser_towns.sort_by_key(|&i| {
        settlements
            .iter()
            .filter(|o| o.realm == winner)
            .map(|o| {
                let dy = o.y - settlements[i].y;
                let dx = o.x - settlements[i].x;
                dy * dy + dx * dx
            })
            .min()
            .unwrap_or(i64::MAX)
    });
    let n_towns = loser_towns.len();
    let vassalise = margin >= SCORE_VASSAL;
    let annex = vassalise && n_towns <= 2;
    let take = if annex {
        n_towns
    } else {
        ceded.min(n_towns.saturating_sub(1)).max(1)
    };
    for &i in loser_towns.iter().take(take) {
        transfer(settlements, i, winner, realms, month, "ceded at the peace table", &mut events, &mut pol.transfers);
        borders_changed = true;
    }
    pol.ae[winner.0] = (pol.ae[winner.0] + 8.0 * take as f64).min(100.0);

    if annex {
        events.push(Event {
            m: month,
            s: realms[loser.0].name.clone(),
            k: EventKind::Realm,
            text: format!(
                "{} ends in ruin for {}: its last towns pass to {}, and the realm is struck from the rolls.",
                war.name, realms[loser.0].name, realms[winner.0].name
            ),
            ids: smallvec![war.ent],
            ..Default::default()
        });
        // the realm leaves the rolls of the living (M6.1); its people
        // live on under a foreign crown (ADR-0018)
        realms[loser.0].alive = false;
        if let Some(ce) = reg.find_kind(EntityKind::Realm, &realms[loser.0].name) {
            reg.close(
                ce,
                month,
                &format!("struck from the rolls at the end of {}", war.name),
            );
        }
        pol.vassal_of[loser.0] = None;
    } else if vassalise {
        pol.vassal_of[loser.0] = Some(winner);
        pol.ae[winner.0] = (pol.ae[winner.0] + 14.0).min(100.0);
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::Realm,
            text: format!(
                "{} ends with {} on its knees: it kneels as vassal of {}, its tribute set, its wars no longer its own.",
                war.name, realms[loser.0].name, realms[winner.0].name
            ),
            ..Default::default()
        });
    } else {
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::War,
            text: format!(
                "{} is over. {} dictates the peace, and the border stones are moved.",
                war.name, realms[winner.0].name
            ),
            ..Default::default()
        });
    }
    (events, borders_changed)
}

/// The seat's index, name and position — falling back to the greatest
/// town under the banner when the named seat dangles mid-month (the
/// M10.4 fallback rule).
fn seat_of(
    settlements: &[Settlement],
    realms: &[Realm],
    c: RealmId,
) -> Option<(usize, String, i64, i64)> {
    let r = &realms[c.0];
    settlements
        .iter()
        .position(|s| s.id == r.seat && s.realm == c)
        .or_else(|| {
            let mut best: Option<usize> = None;
            for (i, s) in settlements.iter().enumerate() {
                if s.realm == c && best.map_or(true, |b| s.pop > settlements[b].pop) {
                    best = Some(i);
                }
            }
            best
        })
        .map(|i| (i, settlements[i].name.clone(), settlements[i].x, settlements[i].y))
}

/// M11 — the unrest ladder. Each realm's gauge is read once a month; the
/// highest rung whose threshold and conditions hold fires, vents part of
/// the pressure and arms a cooldown (M11.6). Secession sits at the top
/// and is the only rung that moves borders — and it is gated (M11.4):
/// only another people, or a shore beyond the crown's reach, may leave,
/// and only from a hollow realm. A rising that fails the gate falls
/// through to the coup rung instead of tearing the map.
#[allow(clippy::too_many_arguments)]
fn unrest_ladder(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month: i64,
    settlements: &mut [Settlement],
    peoples_v: &[People],
    realms: &mut Vec<Realm>,
    socs: &[Society],
    reg: &mut Registry,
) -> (Vec<Event>, bool) {
    let mut events = Vec::new();
    let mut borders = false;
    let mut seceded = false;
    let n0 = realms.len();
    for c in (0..n0).map(RealmId) {
        if !realms[c.0].alive || !alive(settlements, c) || pol.crisis[c.0].is_some() {
            continue;
        }
        if month < pol.calm_until[c.0] {
            continue;
        }
        let u = pol.unrest[c.0];
        if u < LADDER_RIOT {
            continue;
        }
        // --- top rung: secession (M11.4) — at most one a month, world-wide
        if u >= LADDER_SECEDE && !seceded && pol.asab[c.0] < 0.50 {
            if let Some(evs) = try_secession(
                pol, chron, rng, taken, month, settlements, peoples_v, realms, socs, reg, c,
            ) {
                events.extend(evs);
                borders = true;
                seceded = true;
                pol.unrest[c.0] = (pol.unrest[c.0] - 0.45).max(0.0);
                pol.calm_until[c.0] = month + 48;
                continue;
            }
        }
        // --- coup rung (M11.2): the palace settles what the street began
        if u >= LADDER_COUP && pol.legit[c.0] < 0.58 {
            events.extend(palace_coup(
                pol, chron, rng, taken, month, settlements, peoples_v, realms, reg, c,
            ));
            pol.unrest[c.0] = (pol.unrest[c.0] - 0.35).max(0.0);
            pol.calm_until[c.0] = month + 60;
            continue;
        }
        // --- charter rung (M11.5): a lettered crown buys the peace
        let towns = towns_of(settlements, c);
        let cost = 160.0 + 6.0 * towns.len() as f64;
        let lettered = socs
            .get(realms[c.0].people.idx())
            .map_or(false, |so| so.knows(society::TechId::Law));
        if u >= LADDER_CHARTER && lettered && realms[c.0].treasury >= cost {
            realms[c.0].treasury = round2(realms[c.0].treasury - cost);
            pol.legit[c.0] = (pol.legit[c.0] + 0.12).min(1.0);
            pol.unrest[c.0] = (pol.unrest[c.0] - 0.25).max(0.0);
            pol.calm_until[c.0] = month + 30;
            let seat = seat_of(settlements, realms, c);
            let ruler = chron.rulers.iter().find(|r| r.realm == c);
            let who = ruler
                .map(|r| r.title())
                .unwrap_or_else(|| realms[c.0].house.clone());
            let mut ids: crate::event::EventIds = smallvec![];
            if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
                ids.push(e);
            }
            if let Some(r) = ruler {
                ids.push(r.ent);
            }
            events.push(Event {
                m: month,
                s: realms[c.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "{} grants a charter of liberties{}: the crown's word set down in ink, and coin spent for quiet streets.",
                    who,
                    seat.as_ref()
                        .map(|(_, nm, _, _)| format!(" at {}", nm))
                        .unwrap_or_default()
                ),
                ids,
                x: seat.as_ref().map(|&(_, _, x, _)| x).unwrap_or(-1),
                y: seat.as_ref().map(|&(_, _, _, y)| y).unwrap_or(-1),
                ..Default::default()
            });
            continue;
        }
        // --- bottom rung: the street speaks and a little pressure vents
        if rng.gen::<f64>() < 0.35 {
            pol.unrest[c.0] = (pol.unrest[c.0] - 0.08).max(0.0);
            pol.calm_until[c.0] = month + 24;
            let seat = seat_of(settlements, realms, c);
            let mut ids: crate::event::EventIds = smallvec![];
            if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
                ids.push(e);
            }
            events.push(Event {
                m: month,
                s: realms[c.0].name.clone(),
                k: EventKind::Realm,
                text: format!(
                    "Bread riots shake {} — the peace of {} holds, but by a thread.",
                    seat.as_ref()
                        .map(|(_, nm, _, _)| nm.clone())
                        .unwrap_or_else(|| "the streets".into()),
                    realms[c.0].name
                ),
                ids,
                x: seat.as_ref().map(|&(_, _, x, _)| x).unwrap_or(-1),
                y: seat.as_ref().map(|&(_, _, _, y)| y).unwrap_or(-1),
                ..Default::default()
            });
        }
    }
    (events, borders)
}

/// M11.4 — the gated secession, formerly the M4.5 rebellion roll. Shape
/// first (enough towns on both sides of the split), then the gate: the
/// rebel bloc follows another people than the crown, or its seed town
/// lies beyond the crown's administrative reach. Risings mint realms,
/// never peoples (ADR-0018).
#[allow(clippy::too_many_arguments)]
fn try_secession(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month: i64,
    settlements: &mut [Settlement],
    peoples_v: &[People],
    realms: &mut Vec<Realm>,
    socs: &[Society],
    reg: &mut Registry,
    c: RealmId,
) -> Option<Vec<Event>> {
    let towns = towns_of(settlements, c);
    if towns.len() < 4 {
        return None;
    }
    // the capital is the largest town; the rising starts farthest away
    let capital = *towns.iter().max_by_key(|&&i| settlements[i].pop).unwrap();
    let seed_town = *towns
        .iter()
        .max_by_key(|&&i| {
            let dy = settlements[i].y - settlements[capital].y;
            let dx = settlements[i].x - settlements[capital].x;
            dy * dy + dx * dx
        })
        .unwrap();
    if seed_town == capital {
        return None;
    }
    let rebels: Vec<usize> = towns
        .iter()
        .copied()
        .filter(|&i| {
            let dys = settlements[i].y - settlements[seed_town].y;
            let dxs = settlements[i].x - settlements[seed_town].x;
            let dyc = settlements[i].y - settlements[capital].y;
            let dxc = settlements[i].x - settlements[capital].x;
            dys * dys + dxs * dxs < dyc * dyc + dxc * dxc
        })
        .collect();
    if rebels.len() < 2 || towns.len() - rebels.len() < 2 {
        return None;
    }
    // the would-be crown: the people most numerous among the rebel towns
    let crown_people = {
        let mut counts: Vec<(PeopleId, i64)> = Vec::new();
        for &i in &rebels {
            let p = settlements[i].people;
            if let Some(e) = counts.iter_mut().find(|e| e.0 == p) {
                e.1 += settlements[i].pop;
            } else {
                counts.push((p, settlements[i].pop));
            }
        }
        let mut best = counts[0];
        for e in &counts {
            if e.1 > best.1 {
                best = *e;
            }
        }
        best.0
    };
    // the gate (M11.4): another tongue under the banner, or a seed town
    // beyond the crown's administrative reach — otherwise no secession
    let old_crown = realms[c.0].people;
    let reach = ADMIN_REACH[socs.get(old_crown.idx()).map_or(0, |so| so.polity.min(3))];
    let far = seat_of(settlements, realms, c).map_or(false, |(_, _, sx, sy)| {
        let dy = (settlements[seed_town].y - sy) as f64;
        let dx = (settlements[seed_town].x - sx) as f64;
        (dy * dy + dx * dx).sqrt() > reach
    });
    if crown_people == old_crown && !far {
        return None;
    }
    let mut events = Vec::new();
    let style = peoples_v[crown_people.idx()].style.clone();
    let (name, house) = coin_realm_name(rng, &style, taken);
    let parent_name = realms[c.0].name.clone();
    let new_id = RealmId(realms.len());
    // coin is divided by heads; the arts stay with the people
    let share = rebels.len() as f64 / towns.len() as f64;
    let seed_pos = (settlements[seed_town].x, settlements[seed_town].y);
    let new_treasury = round2(realms[c.0].treasury * share);
    realms[c.0].treasury = round2(realms[c.0].treasury * (1.0 - share));
    realms.push(Realm {
        id: new_id,
        name: name.clone(),
        house,
        people: crown_people,
        seat: settlements[seed_town].id,
        color: culture::next_realm_color(new_id.idx()),
        founded: month,
        alive: true,
        treasury: new_treasury,
    });
    for &i in &rebels {
        settlements[i].realm = new_id;
    }
    // a young crown, and a court that remembers why it left
    pol.grow(realms.len());
    pol.asab[new_id.0] = 0.85;
    pol.legit[new_id.0] = 0.55;
    pol.unrest[new_id.0] = 0.30;
    pol.calm_until[new_id.0] = month + 36;
    pol.op_add(new_id, c, -65.0);
    pol.op_add(c, new_id, -65.0);
    pol.legit[c.0] = (pol.legit[c.0] - 0.10).max(0.0);
    // the new realm and its first crown enter the telling (M6.1)
    let realm_ent = reg.add(
        EntityKind::Realm, &name, month, Some(crown_people), seed_pos.0, seed_pos.1,
    );
    let ruler = chronicle::new_ruler(
        rng, &realms[new_id.0], &peoples_v[crown_people.idx()], taken, month, reg,
    );
    let ruler_name = ruler.title();
    let ruler_ent = ruler.ent;
    chron.rulers.push(ruler);
    events.push(Event {
        m: month,
        s: name.clone(),
        k: EventKind::Realm,
        text: format!(
            "The far towns rise against {}: {} settlements follow {} out of the old realm, and men begin to speak of {}.",
            parent_name, rebels.len(), ruler_name, name
        ),
        ids: smallvec![realm_ent, ruler_ent],
        x: seed_pos.0,
        y: seed_pos.1,
        ..Default::default()
    });
    // half the time the old realm marches to take back its own
    if pol.wars.len() < 3 && rng.gen::<f64>() < 0.5 {
        let war_name = format!("the {} Rising", name);
        kindle_war(pol, rng, month, c, new_id, peoples_v, realms, &war_name, &mut events, taken, reg);
    }
    Some(events)
}

/// M11.2 — a palace coup. The usurper is a living general from the
/// realm's wars when one stands, else a lord of the court; the old house
/// falls and a new one rules. Epithets are earned in the act: "the
/// Kingslayer" when the old ruler is slain, "the Usurper" otherwise.
#[allow(clippy::too_many_arguments)]
fn palace_coup(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month: i64,
    settlements: &[Settlement],
    peoples_v: &[People],
    realms: &mut [Realm],
    reg: &mut Registry,
    c: RealmId,
) -> Vec<Event> {
    let mut events = Vec::new();
    let Some(ri) = chron.rulers.iter().position(|r| r.realm == c) else {
        return events;
    };
    let people = &peoples_v[realms[c.0].people.idx()];
    // a marshal with an army at his back, when the realm has one afield
    let general = pol
        .wars
        .iter()
        .find_map(|w| {
            if w.a == c {
                Some(w.gen_a)
            } else if w.b == c {
                Some(w.gen_b)
            } else {
                None
            }
        })
        .and_then(|e| reg.get(e).map(|p| (p.name.clone(), e)));
    let was_marshal = general.is_some();
    let (name, ent) = general.unwrap_or_else(|| {
        let nm = naming::make_word(rng, &people.style, taken);
        let e = reg.add_person(&nm, "courtier", month, Some(realms[c.0].people));
        (nm, e)
    });
    let slain = rng.gen::<f64>() < 0.5;
    let epithet = if slain { "the Kingslayer" } else { "the Usurper" };
    reg.earn_epithet(ent, epithet);
    let old = chron.rulers[ri].clone();
    reg.close(
        old.ent,
        month,
        &if slain {
            format!("slain at his own table — the circlet of {} seized by {}", realms[c.0].name, name)
        } else {
            format!("cast down and driven into exile; {} took the circlet of {}", name, realms[c.0].name)
        },
    );
    let old_house = realms[c.0].house.clone();
    let new_house = format!("House {}", naming::make_word(rng, &people.style, taken));
    realms[c.0].house = new_house.clone();
    // the fallen house is remembered — a later crisis pretender may
    // carry it back (the restoration arc, M11.6)
    pol.deposed[c.0] = Some(old_house.clone());
    chron.rulers[ri] = Ruler {
        realm: c,
        name: name.clone(),
        epithet: epithet.to_string(),
        since: month,
        age_months: rng.gen_range(300..520),
        ent,
    };
    // a short honeymoon: doubted, but not so damned that the next coup
    // is already armed — chain-coups should be an arc, not the default
    pol.legit[c.0] = 0.50;
    let seat = seat_of(settlements, realms, c);
    let hand = if was_marshal { "the marshal of its armies" } else { "a lord of the court" };
    let deed = if slain {
        format!("{} of {} is slain at his own table", old.title(), realms[c.0].name)
    } else {
        format!("{} of {} is cast down and driven into exile", old.title(), realms[c.0].name)
    };
    let mut ids: crate::event::EventIds = smallvec![old.ent, ent];
    if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
        ids.insert(0, e);
    }
    events.push(Event {
        m: month,
        s: realms[c.0].name.clone(),
        k: EventKind::Ruler,
        text: format!(
            "{}. {}, {}, seizes the circlet — {} falls, and {} rules in its place.",
            deed, name, hand, old_house, new_house
        ),
        ids,
        x: seat.as_ref().map(|&(_, _, x, _)| x).unwrap_or(-1),
        y: seat.as_ref().map(|&(_, _, _, y)| y).unwrap_or(-1),
        ..Default::default()
    });
    events
}

/// M11.3 — wars of the circlet run their course. When the term is up a
/// claimant takes the throne, every rival claim closes, and the realm
/// keeps its borders — this was a war inside the hall. A claimant who
/// carries the deposed house home writes the restoration arc (M11.6).
fn resolve_circlet_wars(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    month: i64,
    settlements: &[Settlement],
    realms: &mut [Realm],
    reg: &mut Registry,
) -> Vec<Event> {
    let mut events = Vec::new();
    for c in (0..realms.len()).map(RealmId) {
        let due = matches!(
            pol.crisis.get(c.0),
            Some(Some(cw)) if month >= cw.ends || !realms[c.0].alive
        );
        if !due {
            continue;
        }
        let cw = pol.crisis[c.0].take().unwrap();
        if !realms[c.0].alive || !alive(settlements, c) {
            // the realm died mid-crisis: the seated claimant falls with
            // it (the fallen-crowns pass), the rivals' claims just end
            for (j, cl) in cw.claimants.iter().enumerate() {
                if j != cw.seated {
                    reg.close(cl.ent, month, "the claim died with the realm");
                }
            }
            continue;
        }
        let w = rng.gen_range(0..cw.claimants.len());
        for (j, cl) in cw.claimants.iter().enumerate() {
            if j == w {
                continue;
            }
            let fate = if rng.gen::<f64>() < 0.5 {
                "fell in the war of the circlet"
            } else {
                "yielded the claim and took the road into exile"
            };
            reg.close(cl.ent, month, &format!("{} of {}", fate, realms[c.0].name));
        }
        let winner = cw.claimants[w].clone();
        let restored = pol.deposed[c.0].as_deref() == Some(winner.house.as_str());
        let ri = chron.rulers.iter().position(|r| r.realm == c);
        if w != cw.seated {
            if let Some(ri) = ri {
                let epithet = if restored { "the Returned" } else { "the Hard-won" };
                reg.earn_epithet(winner.ent, epithet);
                chron.rulers[ri] = Ruler {
                    realm: c,
                    name: winner.name.clone(),
                    epithet: epithet.to_string(),
                    since: month,
                    age_months: rng.gen_range(280..520),
                    ent: winner.ent,
                };
            }
        } else if let Some(ri) = ri {
            chron.rulers[ri].epithet = "the Unbowed".to_string();
            reg.earn_epithet(winner.ent, "the Unbowed");
        }
        let old_house = realms[c.0].house.clone();
        realms[c.0].house = winner.house.clone();
        if restored {
            pol.deposed[c.0] = None;
        } else if old_house != winner.house {
            pol.deposed[c.0] = Some(old_house);
        }
        pol.legit[c.0] = 0.55;
        pol.unrest[c.0] = (pol.unrest[c.0] - 0.25).max(0.0);
        pol.calm_until[c.0] = month + 24;
        let seat = seat_of(settlements, realms, c);
        let text = if restored {
            format!(
                "The war of the circlet of {} is done: the old blood returns, and {} of {} is restored to the throne.",
                realms[c.0].name, winner.name, winner.house
            )
        } else {
            format!(
                "The war of the circlet of {} is done: {} of {} holds the throne, and the rival claims are ash.",
                realms[c.0].name, winner.name, winner.house
            )
        };
        let mut ids: crate::event::EventIds = smallvec![winner.ent];
        if let Some(e) = reg.find_kind(EntityKind::Realm, &realms[c.0].name) {
            ids.insert(0, e);
        }
        events.push(Event {
            m: month,
            s: realms[c.0].name.clone(),
            k: EventKind::Ruler,
            text,
            ids,
            x: seat.as_ref().map(|&(_, _, x, _)| x).unwrap_or(-1),
            y: seat.as_ref().map(|&(_, _, _, y)| y).unwrap_or(-1),
            ..Default::default()
        });
    }
    events
}

use crate::culture;

// ---------------------------------------------------------------- union

/// M12.3 — peaceful union: two realms of kindred peoples, warm courts
/// both ways and a shared threat at the door join under one crown, by
/// compact or by marriage. The greater crown rules everything; the
/// lesser house persists as a named sworn line at the united court. At
/// most one union a year, worldwide — crowns do not pool like raindrops.
pub fn union_pass(
    peoples: &mut Peoples,
    pol: &mut Politics,
    month: i64,
    rng: &mut Pcg64Mcg,
    reg: &mut Registry,
) -> Vec<Event> {
    let mut events = Vec::new();
    let Peoples { settlements, peoples: peoples_v, realms, coresidence, .. } = peoples;
    let n = realms.len();
    if n < 2 {
        return events;
    }
    // A secession earlier this same month may have outgrown the opinion
    // matrix — widen before indexing with today's roster (idempotent).
    pol.grow(n);
    let towns_of = |rid: usize| settlements.iter().filter(|s| s.realm.0 == rid).count();
    let pop_of = |rid: usize| -> i64 {
        settlements.iter().filter(|s| s.realm.0 == rid).map(|s| s.pop).sum()
    };
    /// Which door the union came through — the prose differs.
    enum Via {
        Compact,
        Oath,
    }
    for a in 0..n {
        for b in (a + 1)..n {
            if !realms[a].alive || !realms[b].alive {
                continue;
            }
            if pol.crisis[a].is_some() || pol.crisis[b].is_some() {
                continue;
            }
            let (ra, rb) = (RealmId(a), RealmId(b));
            if pol.wars.iter().any(|w| w.involves(ra) && w.involves(rb)) {
                continue;
            }
            let kin = culture::kinship(realms[a].people, realms[b].people, peoples_v, coresidence);
            if kin < 0.55 {
                continue;
            }
            // Two doors into one crown:
            //  - the compact of equals: two free kindred crowns, warm
            //    courts both ways, a shared threat at the door;
            //  - the oath fulfilled: a kindred sworn line folds into its
            //    suzerain after long warmth — no threat needed, the
            //    swearing was the threat.
            let sworn_to_a = pol.vassal_of[b] == Some(ra);
            let sworn_to_b = pol.vassal_of[a] == Some(rb);
            let via = if sworn_to_a || sworn_to_b {
                let (suz, vas) = if sworn_to_a { (a, b) } else { (b, a) };
                if pol.opinion[vas * n + suz] < 10.0 || towns_of(vas) < 1 {
                    continue;
                }
                if rng.gen::<f64>() > 0.15 {
                    continue;
                }
                Via::Oath
            } else {
                if pol.vassal_of[a].is_some() || pol.vassal_of[b].is_some() {
                    continue;
                }
                if towns_of(a) < 2 || towns_of(b) < 2 {
                    continue;
                }
                if pol.opinion[a * n + b] < 25.0 || pol.opinion[b * n + a] < 25.0 {
                    continue;
                }
                let threatened = pol
                    .wars
                    .iter()
                    .any(|w| w.involves(ra) != w.involves(rb) && (w.involves(ra) || w.involves(rb)))
                    || (0..n).any(|c| {
                        c != a
                            && c != b
                            && realms[c].alive
                            && pol.opinion[c * n + a] <= -25.0
                            && pol.opinion[c * n + b] <= -25.0
                    });
                if !threatened || rng.gen::<f64>() > 0.25 {
                    continue;
                }
                Via::Compact
            };

            // --- the joining: the suzerain keeps the crown by oath; by
            // compact, the greater. Either way one circlet remains.
            let (big, small) = match via {
                Via::Oath => {
                    if sworn_to_a {
                        (a, b)
                    } else {
                        (b, a)
                    }
                }
                Via::Compact => {
                    if pop_of(a) >= pop_of(b) {
                        (a, b)
                    } else {
                        (b, a)
                    }
                }
            };
            let (big_id, small_id) = (RealmId(big), RealmId(small));
            let small_name = realms[small].name.clone();
            let small_house = realms[small].house.clone();
            let small_seat_name = settlements
                .iter()
                .find(|s| s.id == realms[small].seat)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| small_name.clone());
            for s in settlements.iter_mut() {
                if s.realm == small_id {
                    s.realm = big_id;
                }
            }
            let dowry = realms[small].treasury;
            realms[big].treasury += dowry;
            realms[small].treasury = 0.0;
            realms[small].alive = false;
            // the folded crown leaves the rolls of the living (M6.1) —
            // same bookkeeping as a conquest death, gentler fate
            if let Some(ce) = reg.find_kind(EntityKind::Realm, &small_name) {
                reg.close(
                    ce,
                    month,
                    &format!("its crown joined with {} in union", realms[big].name),
                );
            }
            // sworn lines re-swear to the united crown; the folded
            // line's own oath is spent
            pol.vassal_of[small] = None;
            for v in pol.vassal_of.iter_mut() {
                if *v == Some(small_id) {
                    *v = Some(big_id);
                }
            }
            // the lesser crown's wars ride along under the united banner
            for w in pol.wars.iter_mut() {
                if w.a == small_id {
                    w.a = big_id;
                }
                if w.b == small_id {
                    w.b = big_id;
                }
                for v in w.allies_a.iter_mut().chain(w.allies_b.iter_mut()) {
                    if *v == small_id {
                        *v = big_id;
                    }
                }
            }
            pol.wars.retain(|w| w.a != w.b);
            // a union settles both courts for a season
            pol.legit[big] = (pol.legit[big] + 0.05).min(1.0);
            pol.unrest[big] = (pol.unrest[big] - 0.10).max(0.0);
            pol.unrest[small] = 0.0;
            pol.calm_until[big] = pol.calm_until[big].max(month + 36);
            let by_marriage = rng.gen::<f64>() < 0.5;
            let big_name = realms[big].name.clone();
            let (sx, sy) = settlements
                .iter()
                .find(|s| s.id == realms[big].seat)
                .map(|s| (s.x, s.y))
                .unwrap_or((-1, -1));
            let text = match via {
                Via::Oath => format!(
                    "The oath is fulfilled: long sworn to that crown, {} is a country of {} outright now, and the crowns of both are joined; House {} keeps its seat at {} as a sworn line.",
                    small_name, big_name, small_house, small_seat_name
                ),
                Via::Compact if by_marriage => format!(
                    "By a marriage long in the making, the crowns of {} and {} are joined; one circlet rules both countries now, and House {} keeps its honored seat at {} as a sworn line.",
                    big_name, small_name, small_house, small_seat_name
                ),
                Via::Compact => format!(
                    "With enemies at both doors, the crowns of {} and {} are joined by compact under one circlet; House {} keeps its seat at {} as a sworn line of the united realm.",
                    big_name, small_name, small_house, small_seat_name
                ),
            };
            events.push(Event {
                m: month,
                s: big_name,
                k: EventKind::Realm,
                text,
                x: sx,
                y: sy,
                ..Default::default()
            });
            return events; // at most one union a year
        }
    }
    events
}

// ---------------------------------------------------------------- naming

/// Kept for symmetry with the old chronicle API; politics owns war names.
pub fn war_name_bank() -> &'static [&'static str] {
    &WAR_NAMES
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E11.6): how much of the wild the banners may claim.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band { name: "land under banners", sweet: (0.05, 0.85), hard: (0.01, 0.98), target: "M4.1: realms claim some — never all — of the wild" },
    crate::util::Band { name: "largest realm pop share", sweet: (0.1, 0.75), hard: (0.02, 0.92), target: "M4 gate: no runaway single empire" },
];

#[cfg(test)]
mod tile_patch_tests {
    use super::*;

    fn apply(prev: &Array2<i16>, patch: &serde_json::Value) -> Array2<i16> {
        let mut out = prev.clone();
        let (h, w) = out.dim();
        let tw = patch["tw"].as_u64().unwrap() as usize;
        for t in patch["tiles"].as_array().unwrap() {
            let tx = t[0].as_u64().unwrap() as usize;
            let ty = t[1].as_u64().unwrap() as usize;
            let rle: Vec<i64> = t[2].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
            let (x0, y0) = (tx * tw, ty * tw);
            let t_w = tw.min(w - x0);
            let t_h = tw.min(h - y0);
            let mut j = 0usize;
            for k in (0..rle.len()).step_by(2) {
                for _ in 0..rle[k] {
                    assert!(j < t_w * t_h, "rle overruns the tile");
                    out[[y0 + j / t_w, x0 + j % t_w]] = rle[k + 1] as i16;
                    j += 1;
                }
            }
            assert_eq!(j, t_w * t_h, "rle must cover the whole tile");
        }
        out
    }

    #[test]
    fn roundtrip_borders_and_edges() {
        // 70×90: ragged edge tiles both ways at tile=32
        let mut prev = Array2::from_elem((70, 90), -1i16);
        for y in 10..40 {
            for x in 20..60 {
                prev[[y, x]] = 3;
            }
        }
        let mut cur = prev.clone();
        // a border shift, a new enclave in a ragged corner tile, a loss
        for y in 30..45 {
            for x in 55..70 {
                cur[[y, x]] = 5;
            }
        }
        cur[[69, 89]] = 7;
        for y in 10..14 {
            for x in 20..24 {
                cur[[y, x]] = -1;
            }
        }
        let (patch, changed, total) = territory_tile_patch(&prev, &cur, 32).unwrap();
        assert!(changed < total, "a local shift must not dirty every tile");
        assert_eq!(apply(&prev, &patch), cur, "patch over prev must equal cur");
    }

    #[test]
    fn identical_grids_ship_nothing() {
        let g = Array2::from_elem((64, 64), 2i16);
        assert!(territory_tile_patch(&g, &g.clone(), 32).is_none());
    }
}
