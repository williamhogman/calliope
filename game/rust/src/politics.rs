//! Politics with consequences (M4): influence-map territory, wars that
//! move borders, an opinion web with aggressive-expansion dread and
//! coalitions, sieges behind walls, and the slow tides of asabiyyah and
//! legitimacy that raise realms at the frontiers and break them in the
//! soft centuries after.
//!
//! Everything here is a pure function of the seed: one shared rng stream,
//! fixed iteration order, no wall-clock. The chronicle narrates; this
//! module decides.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::ids::{CultureId, EntityId, SettlementId};
use crate::chronicle::{self, ChronicleState};
use crate::culture::{self, Culture};
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::naming;
use crate::settlements::Settlement;
use crate::society::{self, Society};
use crate::util::round2;
use crate::world::EventKind;
use crate::world::Event;

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

// ---------------------------------------------------------------- state

#[derive(Serialize, Clone)]
pub struct War {
    /// Leading belligerents: `a` declared on `b`.
    pub a: CultureId,
    pub b: CultureId,
    /// Realms that joined each banner after the kindling.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allies_a: Vec<CultureId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allies_b: Vec<CultureId>,
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
    pub fn involves(&self, c: CultureId) -> bool {
        self.a == c
            || self.b == c
            || self.allies_a.contains(&c)
            || self.allies_b.contains(&c)
    }
    fn side_a(&self) -> Vec<CultureId> {
        let mut v = vec![self.a];
        v.extend(&self.allies_a);
        v
    }
    fn side_b(&self) -> Vec<CultureId> {
        let mut v = vec![self.b];
        v.extend(&self.allies_b);
        v
    }
}

#[derive(Clone)]
pub struct Siege {
    /// Settlement id under siege (ids are stable for a settlement's life).
    pub target: SettlementId,
    /// Culture doing the besieging.
    pub attacker: CultureId,
    /// 0..100; the wall falls at 100.
    pub progress: f64,
}

#[derive(Clone)]
pub struct Tribute {
    pub from: CultureId,
    pub to: CultureId,
    pub per_month: f64,
    pub months_left: i64,
}

#[derive(Default)]
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
    pub vassal_of: Vec<Option<CultureId>>,
    pub tributes: Vec<Tribute>,
    /// Battle marks awaiting the map (M9.4): (x, y, month, loser town,
    /// winner culture). The world drains these into named battlefields.
    pub marks: Vec<(i64, i64, i64, String, CultureId)>,
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
            vassal_of: vec![None; n],
            tributes: Vec::new(),
            marks: Vec::new(),
            transfers: Vec::new(),
            n,
        }
    }

    pub fn op(&self, a: CultureId, b: CultureId) -> f64 {
        self.opinion[a.0 * self.n + b.0]
    }
    pub fn op_add(&mut self, a: CultureId, b: CultureId, d: f64) {
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

fn towns_of<'a>(setts: &'a [Settlement], c: CultureId) -> Vec<usize> {
    setts
        .iter()
        .enumerate()
        .filter(|(_, s)| s.culture == c)
        .map(|(i, _)| i)
        .collect()
}

pub fn alive(setts: &[Settlement], c: CultureId) -> bool {
    setts.iter().any(|s| s.culture == c)
}

fn pop_of(setts: &[Settlement], c: CultureId) -> i64 {
    setts.iter().filter(|s| s.culture == c).map(|s| s.pop).sum()
}

/// Squared distance between the closest settlements of two realms.
fn closest2(setts: &[Settlement], a: CultureId, b: CultureId) -> f64 {
    let mut best = f64::INFINITY;
    for sa in setts.iter().filter(|s| s.culture == a) {
        for sb in setts.iter().filter(|s| s.culture == b) {
            let dy = (sa.y - sb.y) as f64;
            let dx = (sa.x - sb.x) as f64;
            best = best.min(dy * dy + dx * dx);
        }
    }
    best
}

fn neighbours(setts: &[Settlement], a: CultureId, b: CultureId) -> bool {
    closest2(setts, a, b) <= NEIGHBOUR_RANGE * NEIGHBOUR_RANGE
}

/// Fielded strength of one banner: pooled souls with diminishing returns,
/// sharpened by the arts of war, solidarity and a believed-in crown.
fn strength(
    setts: &[Settlement],
    socs: &[Society],
    pol: &Politics,
    leader: CultureId,
    allies: &[CultureId],
) -> f64 {
    let mut total = 0.0;
    for (ci, share) in std::iter::once((leader, 1.0))
        .chain(allies.iter().map(|&c| (c, 0.5)))
    {
        let p = pop_of(setts, ci) as f64;
        if p <= 0.0 {
            continue;
        }
        let war = socs
            .get(ci.0)
            .map(|s| society::mods_for(s).war)
            .unwrap_or(1.0);
        let asab = pol.asab.get(ci.0).copied().unwrap_or(0.5);
        let legit = pol.legit.get(ci.0).copied().unwrap_or(0.7);
        total += p.powf(0.6) * war * (0.55 + 0.9 * asab) * (0.8 + 0.4 * legit) * share;
    }
    total
}

/// Hand a settlement to a new banner and say so.
fn transfer(
    setts: &mut [Settlement],
    idx: usize,
    to: CultureId,
    cultures: &[Culture],
    month: i64,
    why: &str,
    events: &mut Vec<Event>,
    transfers: &mut Vec<SettlementId>,
) {
    let from = setts[idx].culture;
    setts[idx].culture = to;
    // the world may let the conqueror lay a new name over the old (M9.2)
    transfers.push(setts[idx].id);
    events.push(Event {
        m: month,
        s: setts[idx].name.clone(),
        k: EventKind::War,
        text: format!(
            "{} passes from the {} to the banners of the {} — {}.",
            setts[idx].name, cultures[from.0].people, cultures[to.0].people, why
        ),
        // anchor the ground, not the name: conquest may rename the town
        // this very tick (M9.2) and the resolver must still find it
        x: setts[idx].x,
        y: setts[idx].y,
        ..Default::default()
    });
}

// ---------------------------------------------------------------- territory

/// M4.1 — influence-map territory. Every settlement projects weight
/// pop^0.85, raised by its realm's era and solidarity, out to a radius
/// that grows with that weight; each land cell belongs to the realm with
/// the strongest summed pull. Wilderness stays unowned. Owner = culture
/// id, −1 = none.
pub fn influence_map(
    height: &Array2<f32>,
    settlements: &[Settlement],
    socs: &[Society],
    asab: &[f64],
    n_cultures: usize,
) -> Array2<i16> {
    let (h, w) = height.dim();
    let hw = h * w;
    let mut acc = vec![0f32; hw];
    let mut stamp = vec![u16::MAX; hw];
    let mut bestv = vec![0f32; hw];
    let mut owner = vec![-1i16; hw];

    // group settlement indices by culture, in culture order (deterministic)
    for c in 0..n_cultures {
        let towns: Vec<&Settlement> =
            settlements.iter().filter(|s| s.culture.0 == c).collect();
        if towns.is_empty() {
            continue;
        }
        let era = socs.get(c).map(|s| s.era as f64).unwrap_or(0.0);
        let coh = asab.get(c).copied().unwrap_or(0.5);
        let mut boxes: Vec<(usize, usize, usize, usize, f64, f64, f64)> = Vec::new();
        for s in &towns {
            let weight = (s.pop as f64).powf(0.85) * (1.0 + 0.20 * era) * (0.75 + 0.5 * coh);
            let r = (2.2 * weight.powf(0.30)).clamp(5.0, 42.0);
            let reach = (1.45 * r).ceil() as i64;
            let y0 = (s.y - reach).max(0) as usize;
            let y1 = ((s.y + reach) as usize).min(h - 1);
            let x0 = (s.x - reach).max(0) as usize;
            let x1 = ((s.x + reach) as usize).min(w - 1);
            boxes.push((y0, y1, x0, x1, s.y as f64, s.x as f64, r));
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
        // claim: compare this realm's summed pull against the best so far
        for (y0, y1, x0, x1, _, _, _) in &boxes {
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

// ---------------------------------------------------------------- monthly

/// One month of statecraft. Returns (events, borders_changed).
#[allow(clippy::too_many_arguments)]
pub fn monthly(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month: i64,
    settlements: &mut Vec<Settlement>,
    cultures: &mut Vec<Culture>,
    socs: &mut Vec<Society>,
    territory: &Array2<i16>,
    reg: &mut Registry,
) -> (Vec<Event>, bool) {
    let mut events = Vec::new();
    let mut borders_changed = false;
    let n = cultures.len();
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
    for a in (0..n).map(CultureId) {
        for b in (0..n).map(CultureId) {
            if a != b && alive(settlements, a) && alive(settlements, b)
                && neighbours(settlements, a, b)
            {
                pol.op_add(a, b, -FRICTION);
            }
        }
    }

    // --- asabiyyah: solidarity surges at hard frontiers, gutters in
    // safe hearts; legitimacy drifts home to a workable middle
    for c in (0..n).map(CultureId) {
        if !alive(settlements, c) {
            continue;
        }
        let at_war = pol.wars.iter().any(|w| w.involves(c));
        let frontier = frontier_exposure(settlements, territory, cultures, c);
        let up = ASAB_SURGE * (frontier + if at_war { 0.6 } else { 0.0 });
        pol.asab[c.0] = (pol.asab[c.0] + up - ASAB_DECAY).clamp(0.05, 1.0);
        let target = 0.72;
        pol.legit[c.0] += (target - pol.legit[c.0]) * 0.0025;
    }

    // --- tribute caravans set out
    let mut spent: Vec<Event> = Vec::new();
    pol.tributes.retain_mut(|t| {
        t.months_left -= 1;
        if let Some(s) = socs.get_mut(t.from.0) {
            let pay = t.per_month.min(s.treasury);
            s.treasury = round2(s.treasury - pay);
            if let Some(r) = socs.get_mut(t.to.0) {
                r.treasury = round2(r.treasury + pay);
            }
        }
        if t.months_left <= 0 {
            spent.push(Event {
                m: month,
                s: cultures[t.from.0].people.clone(),
                k: EventKind::Realm,
                text: format!(
                    "The last tribute caravan of the {} reaches the {}; the debt of the old war is paid.",
                    cultures[t.from.0].people, cultures[t.to.0].people
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
    for v in (0..n).map(CultureId) {
        let Some(suz) = pol.vassal_of[v.0] else { continue };
        if !alive(settlements, v) {
            pol.vassal_of[v.0] = None;
            continue;
        }
        if !alive(settlements, suz) {
            pol.vassal_of[v.0] = None;
            events.push(Event {
                m: month,
                s: cultures[v.0].people.clone(),
                k: EventKind::Realm,
                text: format!(
                    "With the fall of their masters, the {} are answerable to no one again.",
                    cultures[v.0].people
                ),
                ..Default::default()
            });
            continue;
        }
        if let Some(s) = socs.get_mut(v.0) {
            let due = round2((s.treasury * 0.006).max(0.3).min(s.treasury));
            s.treasury = round2(s.treasury - due);
            if let Some(r) = socs.get_mut(suz.0) {
                r.treasury = round2(r.treasury + due);
            }
        }
        // independence: high solidarity, a distracted or weakened master
        let suz_at_war = pol.wars.iter().any(|w| w.involves(suz));
        let sv = strength(settlements, socs, pol, v, &[]);
        let ss = strength(settlements, socs, pol, suz, &[]);
        let opening = if suz_at_war || sv > ss { 1.0 } else { 0.2 };
        if rng.gen::<f64>() < 0.0022 * pol.asab[v.0] * opening {
            pol.vassal_of[v.0] = None;
            pol.op_add(v, suz, -50.0);
            pol.op_add(suz, v, -50.0);
            events.push(Event {
                m: month,
                s: cultures[v.0].people.clone(),
                k: EventKind::Realm,
                text: format!(
                    "The {} cast off the yoke of the {} and stand as their own realm once more.",
                    cultures[v.0].people, cultures[suz.0].people
                ),
                ..Default::default()
            });
            if pol.wars.len() < 3 && rng.gen::<f64>() < 0.5 {
                kindle_war(pol, rng, month, suz, v, cultures, "the War of the Broken Leash", &mut events, taken, reg);
            }
        }
    }

    // --- fortification: exposed border towns raise walls (treasury sink)
    if month.rem_euclid(12) == 3 {
        for c in (0..n).map(CultureId) {
            if !alive(settlements, c) {
                continue;
            }
            let treasury = socs.get(c.0).map(|s| s.treasury).unwrap_or(0.0);
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
                for o in settlements.iter().filter(|o| o.culture != c) {
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
                if let Some(s) = socs.get_mut(c.0) {
                    if s.treasury >= cost + 30.0 {
                        s.treasury = round2(s.treasury - cost);
                        settlements[i].fort += 1;
                        let what = match settlements[i].fort {
                            1 => "raises a palisade of sharpened oak",
                            2 => "rings itself in stone walls",
                            _ => "crowns its walls with towers and an iron gate",
                        };
                        events.push(Event {
                            m: month,
                            s: settlements[i].name.clone(),
                            k: EventKind::Society,
                            text: format!("{} {} — the border is watched.", settlements[i].name, what),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    // --- wars: battles, raids, sieges, and peace
    let (war_events, changed) = conduct_wars(pol, rng, month, settlements, cultures, socs, reg);
    events.extend(war_events);
    borders_changed |= changed;

    // --- new wars kindle out of grievance and dread
    if pol.wars.len() < 3 {
        'outer: for a in (0..n).map(CultureId) {
            for b in (0..n).map(CultureId) {
                if a == b {
                    continue;
                }
                if !alive(settlements, a) || !alive(settlements, b) {
                    continue;
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
                    // one war in three is sworn before a god (M3.5)
                    let war_god = cultures[a.0]
                        .pantheon
                        .iter()
                        .find(|g| g.domain == "war")
                        .or_else(|| cultures[a.0].pantheon.first());
                    let name = if rng.gen::<f64>() < 0.33 && war_god.is_some() {
                        format!("the War of {}'s Altar", war_god.unwrap().name)
                    } else {
                        WAR_NAMES[rng.gen_range(0..WAR_NAMES.len())].to_string()
                    };
                    kindle_war(pol, rng, month, a, b, cultures, &name, &mut events, taken, reg);
                    // coalitions: realms that dread the aggressor rally to
                    // the defender's banner (M4.3)
                    let wi = pol.wars.len() - 1;
                    let mut joined = Vec::new();
                    for j in (0..n).map(CultureId) {
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
                            s: cultures[j.0].people.clone(),
                            k: EventKind::War,
                            text: format!(
                                "Dreading the appetite of the {}, the {} swear common cause with the {}.",
                                cultures[a.0].people, cultures[j.0].people, cultures[b.0].people
                            ),
                            ..Default::default()
                        });
                    }
                    // loyal vassals march with their suzerain
                    for j in (0..n).map(CultureId) {
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

    // --- rebellion: big, hollow realms shed their edges (M4.5)
    let reb = rebellion_pass(pol, chron, rng, taken, month, settlements, cultures, socs, reg);
    if !reb.is_empty() {
        borders_changed = true;
        events.extend(reb);
    }

    (events, borders_changed)
}

/// Share of a realm's towns that sit on a hard frontier: foreign-owned
/// territory (of a different *style*, the meta-ethnic edge) within reach.
fn frontier_exposure(
    setts: &[Settlement],
    territory: &Array2<i16>,
    cultures: &[Culture],
    c: CultureId,
) -> f64 {
    let (h, w) = territory.dim();
    let towns = towns_of(setts, c);
    if towns.is_empty() {
        return 0.0;
    }
    let my_style = &cultures[c.0].style;
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
                if o >= 0 && o as usize != c.0 {
                    let os = &cultures[o as usize].style;
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
    a: CultureId,
    b: CultureId,
    cultures: &[Culture],
    name: &str,
    events: &mut Vec<Event>,
    taken: &mut HashSet<String>,
    reg: &mut Registry,
) {
    let until = month + rng.gen_range(24..72);
    // the war and the generals who will carry it enter the telling (M6.2)
    let war_ent = reg.add(EntityKind::War, name, month, None, -1, -1);
    let gen_a_name = naming::make_word(rng, &cultures[a.0].style, taken);
    let gen_b_name = naming::make_word(rng, &cultures[b.0].style, taken);
    let gen_a = reg.add_person(&gen_a_name, "general", month, Some(a));
    let gen_b = reg.add_person(&gen_b_name, "general", month, Some(b));
    let ca_ent = reg.find_kind(EntityKind::Culture, &cultures[a.0].people).unwrap_or(EntityId(-1));
    let cb_ent = reg.find_kind(EntityKind::Culture, &cultures[b.0].people).unwrap_or(EntityId(-1));
    events.push(Event {
        m: month,
        s: name.to_string(),
        k: EventKind::War,
        text: format!(
            "War kindles between the {} and the {} — men will call it {}. The banners of the {} follow {}; the {} look to {}.",
            cultures[a.0].people, cultures[b.0].people, name,
            cultures[a.0].people, gen_a_name, cultures[b.0].people, gen_b_name
        ),
        ids: vec![war_ent, ca_ent, cb_ent, gen_a, gen_b],
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
    cultures: &[Culture],
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
        let sa = strength(settlements, socs, pol, a, &pol.wars[wi].allies_a);
        let sb = strength(settlements, socs, pol, b, &pol.wars[wi].allies_b);
        let total = (sa + sb).max(1e-9);

        // war chests drain while the banners fly
        for &side in side_a.iter().chain(side_b.iter()) {
            if let Some(s) = socs.get_mut(side.0) {
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
                        .filter(|o| o.culture == winner)
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
                            " {} of the {} held the day.",
                            gen_name, cultures[winner.0].people
                        );
                    }
                }
                events.push(Event {
                    m: month,
                    s: pol.wars[wi].name.clone(),
                    k: EventKind::War,
                    text: format!(
                        "The hosts meet under the walls of {} — the {} carry the day, and the {} leave their dead on the field.{}",
                        settlements[fi].name, cultures[winner.0].people, cultures[loser.0].people, coda
                    ),
                    ids: vec![pol.wars[wi].ent, gen],
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
            let att_war = socs
                .get(attacker.0)
                .map(|s| society::mods_for(s).war)
                .unwrap_or(1.0);
            let walls = socs
                .get(victim_c.0)
                .map(|s| society::mods_for(s).defense)
                .unwrap_or(1.0);
            let victims: Vec<usize> = settlements
                .iter()
                .enumerate()
                .filter(|(_, s)| s.culture == victim_c && s.pop > 90)
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
                if let Some(sa_) = socs.get_mut(attacker.0) {
                    sa_.treasury = round2(sa_.treasury + 0.6 * plunder);
                }
                pol.wars[wi].score += if a_raids { 1.0 } else { -1.0 } * (0.6 + plunder / 60.0).min(2.0);
                let text = if plunder > 25.0 {
                    format!(
                        "Raiders of the {} burn the fields of {} — {} souls lost, {} in coin carried off.",
                        cultures[attacker.0].people,
                        settlements[vi].name,
                        loss,
                        plunder.round() as i64
                    )
                } else {
                    format!(
                        "Raiders of the {} burn the fields of {} — {} souls lost.",
                        cultures[attacker.0].people, settlements[vi].name, loss
                    )
                };
                events.push(Event {
                    m: month,
                    s: settlements[vi].name.clone(),
                    k: EventKind::War,
                    text,
                    ids: vec![pol.wars[wi].ent],
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
                        .filter(|o| o.culture == att)
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
                        "The host of the {} sits down before {} — the siege begins.",
                        cultures[att.0].people, settlements[ti].name
                    ),
                    ids: vec![pol.wars[wi].ent],
                    x: settlements[ti].x,
                    y: settlements[ti].y,
                    ..Default::default()
                });
            }
        }
        if let Some(siege) = pol.wars[wi].siege.clone() {
            let ti = settlements.iter().position(|s| s.id == siege.target);
            match ti {
                Some(ti) if settlements[ti].culture != siege.attacker => {
                    let att = siege.attacker;
                    let def = settlements[ti].culture;
                    let (satt, sdef) = if att == a { (sa, sb) } else { (sb, sa) };
                    // besiegers eat coin
                    if let Some(s) = socs.get_mut(att.0) {
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
                                "A relief host of the {} scatters the besiegers — {} breathes again.",
                                cultures[def.0].people, settlements[ti].name
                            ),
                            ids: vec![pol.wars[wi].ent],
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
                            if let Some(s) = socs.get_mut(att.0) {
                                s.treasury = round2(s.treasury + 0.7 * plunder);
                            }
                            settlements[ti].fort = settlements[ti].fort.saturating_sub(1);
                            transfer(
                                settlements, ti, att, cultures, month,
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
        let (evs, changed) = make_peace(pol, rng, month, &war, settlements, cultures, socs, reg);
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
    cultures: &[Culture],
    socs: &mut [Society],
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
        let people = cultures.get(side.0).map(|c| c.people.clone()).unwrap_or_default();
        reg.close(
            gen,
            month,
            &format!("led the hosts of the {} in {}", people, war.name),
        );
    }
    let verdict = if margin < SCORE_TRIBUTE {
        "ended with neither side the better for it".to_string()
    } else {
        format!(
            "ended in victory for the {}",
            cultures.get(winner.0).map(|c| c.people.as_str()).unwrap_or("victors")
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
                "{} gutters out — of the {} nothing remains to make peace with.",
                war.name, cultures[loser.0].people
            ),
            ids: vec![war.ent],
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
                "Peace is sworn between the {} and the {}; {} is over, and neither side gained more than graves.",
                cultures[war.a.0].people, cultures[war.b.0].people, war.name
            ),
            ids: vec![war.ent],
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
        let lump = round2(socs.get(loser.0).map(|s| s.treasury * 0.35).unwrap_or(0.0));
        if let Some(s) = socs.get_mut(loser.0) {
            s.treasury = round2(s.treasury - lump);
        }
        if let Some(s) = socs.get_mut(winner.0) {
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
                "{} ends: the {} buy their peace — {} in coin, and tribute caravans to the {} for ten years.",
                war.name, cultures[loser.0].people, lump.round() as i64, cultures[winner.0].people
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
            .filter(|o| o.culture == winner)
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
        transfer(settlements, i, winner, cultures, month, "ceded at the peace table", &mut events, &mut pol.transfers);
        borders_changed = true;
    }
    pol.ae[winner.0] = (pol.ae[winner.0] + 8.0 * take as f64).min(100.0);

    if annex {
        events.push(Event {
            m: month,
            s: cultures[loser.0].people.clone(),
            k: EventKind::Realm,
            text: format!(
                "{} ends in ruin for the {}: their last towns pass to the {}, and their realm is struck from the rolls.",
                war.name, cultures[loser.0].people, cultures[winner.0].people
            ),
            ids: vec![war.ent],
            ..Default::default()
        });
        // the people leave the rolls of the living realms (M6.1)
        if let Some(ce) = reg.find_kind(EntityKind::Culture, &cultures[loser.0].people) {
            reg.close(
                ce,
                month,
                &format!("their realm struck from the rolls at the end of {}", war.name),
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
                "{} ends with the {} on their knees: they kneel as vassals of the {}, their tribute set, their wars no longer their own.",
                war.name, cultures[loser.0].people, cultures[winner.0].people
            ),
            ..Default::default()
        });
    } else {
        events.push(Event {
            m: month,
            s: war.name.clone(),
            k: EventKind::War,
            text: format!(
                "{} is over. The {} dictate the peace, and the border stones are moved.",
                war.name, cultures[winner.0].people
            ),
            ..Default::default()
        });
    }
    (events, borders_changed)
}

/// M4.5 — hollow realms crack. A big polity with guttering asabiyyah and
/// a doubted crown sheds its farthest towns as a new realm of the same
/// tongue, young and burning.
#[allow(clippy::too_many_arguments)]
fn rebellion_pass(
    pol: &mut Politics,
    chron: &mut ChronicleState,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    month: i64,
    settlements: &mut [Settlement],
    cultures: &mut Vec<Culture>,
    socs: &mut Vec<Society>,
    reg: &mut Registry,
) -> Vec<Event> {
    let mut events = Vec::new();
    let n = cultures.len();
    for c in (0..n).map(CultureId) {
        let towns = towns_of(settlements, c);
        if towns.len() < 4 {
            continue;
        }
        let size_factor = ((towns.len() as f64 - 3.0) / 6.0).min(1.0);
        let p = 0.0030 * (1.0 - pol.asab[c.0]) * (1.1 - pol.legit[c.0]).max(0.0) * size_factor;
        if rng.gen::<f64>() >= p {
            continue;
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
            continue;
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
            continue;
        }
        // a new realm of the old tongue
        let new_id = CultureId(cultures.len());
        let nc = culture::secede(&cultures[c.0], new_id, rng, taken);
        let people = nc.people.clone();
        let parent_people = cultures[c.0].people.clone();
        cultures.push(nc);
        // the society splits: arts travel, coin is divided by heads
        let share = rebels.len() as f64 / towns.len() as f64;
        let parent_soc = socs[c.0].clone();
        let mut new_soc = Society {
            culture: new_id.0,
            era: parent_soc.era,
            polity: parent_soc.polity.saturating_sub(1).max(1),
            techs: parent_soc.techs.clone(),
            known: parent_soc.known,
            knowledge: round2(parent_soc.knowledge * 0.7),
            treasury: round2(parent_soc.treasury * share),
        };
        if let Some(ps) = socs.get_mut(c.0) {
            ps.treasury = round2(ps.treasury * (1.0 - share));
        }
        new_soc.polity = new_soc.polity.min(society::POLITIES.len() - 1);
        socs.push(new_soc);
        for &i in &rebels {
            settlements[i].culture = new_id;
        }
        // a young crown, and a court that remembers why it left
        pol.grow(cultures.len());
        pol.asab[new_id.0] = 0.85;
        pol.legit[new_id.0] = 0.55;
        pol.op_add(new_id, c, -65.0);
        pol.op_add(c, new_id, -65.0);
        pol.legit[c.0] = (pol.legit[c.0] - 0.10).max(0.0);
        // the new people and their first crown enter the telling (M6.1)
        let culture_ent = reg.add(EntityKind::Culture, &people, month, Some(new_id), -1, -1);
        let ruler = chronicle::new_ruler(rng, &cultures[new_id.0], taken, month, reg);
        let ruler_name = ruler.title();
        let ruler_ent = ruler.ent;
        chron.rulers.push(ruler);
        events.push(Event {
            m: month,
            s: people.clone(),
            k: EventKind::Realm,
            text: format!(
                "The far towns rise against the {}: {} settlements follow {} out of the old realm, and men begin to speak of the {}.",
                parent_people, rebels.len(), ruler_name, people
            ),
            ids: vec![culture_ent, ruler_ent],
            x: settlements[seed_town].x,
            y: settlements[seed_town].y,
            ..Default::default()
        });
        // half the time the old realm marches to take back its own
        if pol.wars.len() < 3 && rng.gen::<f64>() < 0.5 {
            let name = format!("the {} Rising", cultures[new_id.0].name);
            kindle_war(pol, rng, month, c, new_id, cultures, &name, &mut events, taken, reg);
        }
        break; // at most one rising a month, world-wide
    }
    events
}

// ---------------------------------------------------------------- naming

/// Kept for symmetry with the old chronicle API; politics owns war names.
pub fn war_name_bank() -> &'static [&'static str] {
    &WAR_NAMES
}

// naming is pulled in transitively via culture::secede
#[allow(unused_imports)]
use naming as _naming;
