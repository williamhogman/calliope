//! M13 — The Arc of Empires (ADR-0019). A **civilization** is derived
//! state: the kinship-closure of living peoples (M12.1 metric) plus the
//! realms that carry them, named, registry-tracked, recomputed yearly and
//! matched to the standing roster. Nothing authoritative points at a civ;
//! deleting the roster loses names and stage, never simulation truth.
//!
//! The arc is a four-stage machine — Rising → Golden → Waning →
//! Interregnum — driven only by quantities the engine already keeps:
//! legitimacy, asabiyyah, treasury (the golden gate), and an overstretch
//! index `Σ (1 + d/D_SPAN) / capacity` — span of control, towns weighted
//! by remoteness from the anchor seat (M13.3, ADR-0020). Collapse
//! opens an interregnum that reuses the M11 ladder: the pass raises
//! unrest and guts asabiyyah on member realms and lets the existing
//! secession/coup rungs mint the successor realms — realms, never
//! peoples, by construction (ADR-0018).
//!
//! Deterministic: index-ordered iteration everywhere, one rng stream,
//! fixed draw order.

use std::collections::HashSet;

use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use smallvec::smallvec;

use crate::artifact::Artifact;
use crate::culture::kinship;
use crate::entity::{EntityKind, Registry};
use crate::event::{Event, EventIds, EventKind};
use crate::ids::{CivId, EntityId, PeopleId, RealmId, SettlementId};
use crate::naming;
use crate::politics::Politics;
use crate::settlements::Settlement;
use crate::state::{Chronicle, Peoples};

// ---------------------------------------------------------------- tuning

/// Kinship edge for the closure: two peoples this kindred stand in one
/// civilization (assimilation flips towns at ≈0.55; unions ask more).
const KIN_EDGE: f64 = 0.45;
/// Hysteresis: a standing member is retained until it falls below this —
/// borders breathe on decade clocks, they do not flap yearly.
const KIN_KEEP: f64 = 0.32;
/// Scale gates for minting: a civilization is an *empire-tier* name.
const MIN_TOWNS: usize = 8;
const MIN_REALMS: usize = 2;
/// The golden gate (M13.2), all sustained `GOLDEN_YEARS` in a row:
/// high legitimacy, hot asabiyyah, a full treasury, no overstretch.
const GOLDEN_LEGIT: f64 = 0.58;
const GOLDEN_ASAB: f64 = 0.52;
const GOLDEN_WEALTH: f64 = 700.0;
const GOLDEN_YEARS: u8 = 4;
/// M13.2 — golden-age research pace, applied through `Society.boon`.
pub const TECH_BOON: f64 = 1.30;
/// Overstretch (M13.3, ADR-0020): strain accrues while `admin / capacity`
/// sits above this; `WANE_YEARS` strained years rot the court.
const STRETCH_TRIGGER: f64 = 1.0;
const WANE_YEARS: u8 = 8;
/// Span-of-control capacity (ADR-0020): courts a member realm can staff,
/// scaled by era institutions and cohesion. Load is towns weighted by
/// remoteness, never raw population — mass feeds treasuries, not writs.
/// Calibrated on seed 12345 @512: a compact 2-crown civ (avg remoteness
/// ~55 cells) sits just under the golden gate; a 1-crown sprawler over
/// 44 towns at ~150 cells breaks past the trigger.
const CAP_TOWNS: f64 = 12.0;
/// Remoteness half-scale in cells (≈384 km): a town this far from the
/// anchor seat costs two courts' worth of riders and seals.
const D_SPAN: f64 = 96.0;
/// Collapse gate (M13.4): solidarity guttered *and* the crowns doubted.
const COLLAPSE_ASAB: f64 = 0.34;
const COLLAPSE_LEGIT: f64 = 0.45;
/// The Tainter clause (ADR-0020): this many net strained years break the
/// empire even while the hymns still sound. Relief pays the clock down
/// double (`STRAIN_RELIEF`/yr), so collapse only lands on families whose
/// fragmentation never actually shortens the writ — the tail outcome,
/// not the default: most lineages oscillate rising↔waning for centuries.
const COLLAPSE_STRAIN: u8 = 34;
const STRAIN_RELIEF: u8 = 2;
/// The interregnum runs at most this long before the record closes it.
const INTERREGNUM_YEARS: i64 = 12;
/// A fallen family of peoples stays uncounted this long (ADR-0020):
/// the dark age between an empire and its successor civilization.
const DARK_AGE_YEARS: i64 = 55;
/// Monuments per golden age (M13.2), each a registry-tracked artifact.
const MONUMENTS_CAP: u8 = 3;

// ---------------------------------------------------------------- state

/// Where a civilization stands on its arc.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Rising,
    Golden,
    Waning,
    Interregnum,
}

/// One named civilization (M13.1). Serialized rows ride the `civs` wire
/// block; `#[serde(skip)]` fields are pass bookkeeping only.
#[derive(Serialize, Clone)]
pub struct Civ {
    pub id: CivId,
    /// Registry handle — the chronicle's key for the arc (M13.6).
    pub ent: EntityId,
    pub name: String,
    /// Member tongues, sorted — the kinship-closure as of the last pass.
    pub peoples: Vec<PeopleId>,
    pub stage: Stage,
    /// Month of naming (the dawn of the tier, not of the peoples).
    pub founded: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub golden_since: Option<i64>,
    /// M13.5 — the paramount realm, when a hegemony stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paramount: Option<RealmId>,
    /// The paramount tier's name ("the Vess Hegemony"), when standing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hegemony: Option<String>,
    pub alive: bool,
    /// Driver snapshot from the last pass — the arc made explainable:
    /// the same numbers the stage machine read (M13.3, "why is this so?").
    pub crowns: usize,
    pub towns: usize,
    pub legit: f64,
    pub asab: f64,
    pub wealth: f64,
    pub stretch: f64,
    /// Golden-age tally and standing monuments — the legend UI's chips.
    pub monuments: u8,
    pub golden_ages: u8,
    /// The two sides of the stretch ratio, kept for the ledger.
    #[serde(skip)]
    pub admin: f64,
    #[serde(skip)]
    pub capacity: f64,
    // ---- bookkeeping (never on the wire) ----
    #[serde(skip)]
    pub streak: u8,
    #[serde(skip)]
    pub strain: u8,
    /// Month the interregnum opened; -1 while standing.
    #[serde(skip)]
    pub fall_began: i64,
    #[serde(skip)]
    pub realms_at_fall: usize,
    /// Month the record closed this civ; -1 while alive (dark-age clock).
    #[serde(skip)]
    pub ended: i64,
}

// ---------------------------------------------------------------- helpers

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn join(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra != rb {
        parent[ra.max(rb)] = ra.min(rb);
    }
}

/// Living realms whose crown people belongs to the set and that still
/// hold at least one town, in index order.
fn member_realms(
    comp: &[PeopleId],
    realms: &[crate::politics::Realm],
    settlements: &[Settlement],
) -> Vec<RealmId> {
    realms
        .iter()
        .filter(|r| r.alive && comp.contains(&r.people))
        .filter(|r| settlements.iter().any(|s| s.realm == r.id))
        .map(|r| r.id)
        .collect()
}

/// Seat coordinates and name of a realm, falling back to its biggest town.
fn seat_of(
    realms: &[crate::politics::Realm],
    settlements: &[Settlement],
    r: RealmId,
) -> Option<(SettlementId, String, i64, i64)> {
    let realm = realms.get(r.idx())?;
    settlements
        .iter()
        .find(|s| s.id == realm.seat)
        .or_else(|| {
            settlements
                .iter()
                .filter(|s| s.realm == r)
                .max_by_key(|s| (s.pop, -s.id.0))
        })
        .map(|s| (s.id, s.name.clone(), s.x, s.y))
}

/// The member realm with the most towns — the arc's map anchor.
fn anchor_realm(members: &[RealmId], settlements: &[Settlement]) -> Option<RealmId> {
    members
        .iter()
        .copied()
        .max_by_key(|&r| {
            (
                settlements.iter().filter(|s| s.realm == r).count(),
                std::cmp::Reverse(r.idx()),
            )
        })
}

/// Push the civ's entity plus (when found) one realm's entity.
fn arc_ids(reg: &Registry, civ_ent: EntityId, realm_name: Option<&str>) -> EventIds {
    let mut ids: EventIds = smallvec![civ_ent];
    if let Some(nm) = realm_name {
        if let Some(e) = reg.find_kind(EntityKind::Realm, nm) {
            ids.push(e);
        }
    }
    ids
}

// ---------------------------------------------------------------- pass

/// One civilization year (M13): closure, matching, minting, the arc,
/// hegemony, and the golden boon. Runs yearly after the kindred and
/// union passes, before the patina.
pub fn civ_pass(
    peoples: &mut Peoples,
    pol: &mut Politics,
    chron: &mut Chronicle,
    month: i64,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
) -> Vec<Event> {
    let mut events = Vec::new();
    let Peoples {
        settlements,
        peoples: folk,
        realms,
        societies,
        civs,
        coresidence,
    } = peoples;
    let Chronicle {
        artifacts,
        registry: reg,
        ..
    } = chron;

    // The boon is owned here (M13.2): reset every pass, re-granted below.
    for so in societies.iter_mut() {
        so.boon = 1.0;
    }

    // ---- 1 · the kinship-closure over living, settled peoples ----------
    let n = folk.len();
    let mut towns_of_people = vec![0usize; n];
    for s in settlements.iter() {
        if let Some(t) = towns_of_people.get_mut(s.people.idx()) {
            *t += 1;
        }
    }
    let living: Vec<usize> = (0..n)
        .filter(|&i| folk[i].alive && towns_of_people[i] > 0)
        .collect();
    let mut parent: Vec<usize> = (0..n).collect();
    for (ai, &a) in living.iter().enumerate() {
        for &b in living.iter().skip(ai + 1) {
            if kinship(PeopleId(a), PeopleId(b), folk, coresidence) >= KIN_EDGE {
                join(&mut parent, a, b);
            }
        }
    }
    // components in root order — deterministic
    let mut comps: Vec<Vec<PeopleId>> = Vec::new();
    {
        let mut by_root: Vec<(usize, Vec<PeopleId>)> = Vec::new();
        for &i in &living {
            let r = root(&mut parent, i);
            match by_root.iter_mut().find(|(rr, _)| *rr == r) {
                Some((_, v)) => v.push(PeopleId(i)),
                None => by_root.push((r, vec![PeopleId(i)])),
            }
        }
        by_root.sort_by_key(|(r, _)| *r);
        comps.extend(by_root.into_iter().map(|(_, v)| v));
    }

    // ---- 2 · match components to the standing roster --------------------
    // Best overlap wins; ties fall to the older civ. A standing member
    // not in the component is retained while any kinship tie ≥ KIN_KEEP
    // holds (hysteresis).
    let mut comp_of_civ: Vec<Option<usize>> = vec![None; civs.len()];
    let mut civ_of_comp: Vec<Option<usize>> = vec![None; comps.len()];
    for (ci, civ) in civs.iter().enumerate() {
        if !civ.alive {
            continue;
        }
        let mut best: Option<(usize, usize)> = None; // (overlap, comp idx)
        for (ki, comp) in comps.iter().enumerate() {
            if civ_of_comp[ki].is_some() {
                continue;
            }
            let ov = comp.iter().filter(|p| civ.peoples.contains(p)).count();
            if ov > 0 && best.map_or(true, |(bo, _)| ov > bo) {
                best = Some((ov, ki));
            }
        }
        if let Some((_, ki)) = best {
            comp_of_civ[ci] = Some(ki);
            civ_of_comp[ki] = Some(ci);
        }
    }

    // membership update on matched civs, with accession events
    for ci in 0..civs.len() {
        let Some(ki) = comp_of_civ[ci] else { continue };
        let mut next: Vec<PeopleId> = comps[ki].clone();
        // hysteresis: retain old members that still hold a thread
        for &old in &civs[ci].peoples.clone() {
            if next.contains(&old) {
                continue;
            }
            if !folk.get(old.idx()).map_or(false, |p| p.alive)
                || towns_of_people[old.idx()] == 0
            {
                continue;
            }
            let keeps = next
                .iter()
                .any(|&m| kinship(old, m, folk, coresidence) >= KIN_KEEP);
            if keeps {
                next.push(old);
            }
        }
        next.sort();
        // accession: a people newly counted among the civilization
        for &p in &next {
            if !civs[ci].peoples.contains(&p) && civs[ci].alive {
                let pn = folk[p.idx()].people.clone();
                let mut ids: EventIds = smallvec![civs[ci].ent];
                if let Some(e) = reg.find_kind(EntityKind::Culture, &folk[p.idx()].people) {
                    ids.push(e);
                }
                events.push(Event {
                    m: month,
                    s: civs[ci].name.clone(),
                    k: EventKind::Kindred,
                    text: format!(
                        "The {} are counted among {} now — one family of peoples under many crowns.",
                        pn, civs[ci].name
                    ),
                    ids,
                    ..Default::default()
                });
            }
        }
        civs[ci].peoples = next;
    }

    // a living civ left with no component: its peoples scattered or were
    // absorbed — the record closes it quietly (no interregnum: there is
    // nothing left standing to fragment).
    for ci in 0..civs.len() {
        if !civs[ci].alive || comp_of_civ[ci].is_some() {
            continue;
        }
        civs[ci].alive = false;
        civs[ci].ended = month;
        reg.close(civs[ci].ent, month, "its peoples scattered among other tongues");
        events.push(Event {
            m: month,
            s: civs[ci].name.clone(),
            k: EventKind::Era,
            text: format!(
                "No court now answers for {} — its peoples have passed into other families, and the name into the old songs.",
                civs[ci].name
            ),
            ids: smallvec![civs[ci].ent],
            ..Default::default()
        });
    }

    // ---- 3 · mint new civilizations over unmatched components -----------
    for (ki, comp) in comps.iter().enumerate() {
        if civ_of_comp[ki].is_some() {
            continue;
        }
        let members = member_realms(comp, realms, settlements);
        let towns: usize = comp.iter().map(|p| towns_of_people[p.idx()]).sum();
        if members.len() < MIN_REALMS || towns < MIN_TOWNS {
            continue;
        }
        // dark age (ADR-0020): a family lately fallen is not re-counted
        // the next year — the scribes wait a generation before they dare
        // name a successor civilization.
        let dark = civs.iter().any(|c| {
            !c.alive
                && c.ended >= 0
                && month - c.ended < DARK_AGE_YEARS * 12
                && comp.iter().any(|p| c.peoples.contains(p))
        });
        if dark {
            continue;
        }
        // named in the dominant people's tongue
        let dom = comp
            .iter()
            .copied()
            .max_by_key(|p| (towns_of_people[p.idx()], std::cmp::Reverse(p.idx())))
            .unwrap();
        let style = folk[dom.idx()].style.clone();
        let w = naming::make_word(rng, &style, taken);
        let name = match rng.gen_range(0..4) {
            0 => format!("the {} Ecumene", w),
            1 => format!("the Weal of {}", w),
            2 => format!("the {} Concord", w),
            _ => format!("the Circle of {}", w),
        };
        taken.insert(name.clone());
        let anchor = anchor_realm(&members, settlements);
        let (ax, ay, aname) = anchor
            .and_then(|r| seat_of(realms, settlements, r))
            .map(|(_, nm, x, y)| (x, y, nm))
            .unwrap_or((-1, -1, String::new()));
        let ent = reg.add(EntityKind::Civilization, &name, month, Some(dom), ax, ay);
        let mut sorted = comp.clone();
        sorted.sort();
        let folk_names: Vec<String> = sorted
            .iter()
            .take(3)
            .map(|p| folk[p.idx()].people.clone())
            .collect();
        events.push(Event {
            m: month,
            s: name.clone(),
            k: EventKind::Era,
            text: format!(
                "Scribes in {} first write of {} — the {} and their kindred counted as one civilization, {} crowns of one family.",
                if aname.is_empty() { "the courts".to_string() } else { aname },
                name,
                folk_names.join(", the "),
                members.len()
            ),
            ids: smallvec![ent],
            x: ax,
            y: ay,
            legend: format!(
                "It is said the peoples of {} were one hearth-fire scattered by the wind, and know each other by the warmth.",
                name
            ),
            ..Default::default()
        });
        civs.push(Civ {
            id: CivId(civs.len()),
            ent,
            name,
            peoples: sorted,
            stage: Stage::Rising,
            founded: month,
            golden_since: None,
            paramount: None,
            hegemony: None,
            alive: true,
            crowns: members.len(),
            towns,
            legit: 0.0,
            asab: 0.0,
            wealth: 0.0,
            stretch: 0.0,
            admin: 0.0,
            capacity: 0.0,
            streak: 0,
            strain: 0,
            fall_began: -1,
            realms_at_fall: 0,
            monuments: 0,
            golden_ages: 0,
            ended: -1,
        });
    }

    // ---- 4 · the arc, per living civilization ---------------------------
    for ci in 0..civs.len() {
        if !civs[ci].alive {
            continue;
        }
        let members = member_realms(&civs[ci].peoples, realms, settlements);
        if members.is_empty() {
            // every carrying crown has fallen: straight to the closing
            if civs[ci].stage != Stage::Interregnum {
                civs[ci].stage = Stage::Interregnum;
                civs[ci].fall_began = month;
                civs[ci].realms_at_fall = 0;
            }
        }

        let anchor = anchor_realm(&members, settlements);
        let seat = anchor.and_then(|r| seat_of(realms, settlements, r));
        let anchor_name = anchor.and_then(|r| realms.get(r.idx())).map(|r| r.name.clone());
        let (sx, sy) = seat.as_ref().map(|&(_, _, x, y)| (x, y)).unwrap_or((-1, -1));
        let seat_name = seat.as_ref().map(|(_, nm, _, _)| nm.clone()).unwrap_or_default();

        // metrics over the member realms, town-weighted. Administrative
        // load is span of control (ADR-0020): every town costs one court
        // plus a remoteness surcharge against the anchor seat — the writ
        // thins with distance, not with headcount.
        let mut tw = 0.0;
        let mut lw = 0.0;
        let mut aw = 0.0;
        let mut wealth = 0.0;
        let mut admin = 0.0;
        let mut era_max = 0usize;
        for &r in &members {
            let towns: Vec<&Settlement> =
                settlements.iter().filter(|s| s.realm == r).collect();
            let t = towns.len() as f64;
            tw += t;
            lw += pol.legit.get(r.idx()).copied().unwrap_or(0.5) * t;
            aw += pol.asab.get(r.idx()).copied().unwrap_or(0.5) * t;
            wealth += realms.get(r.idx()).map(|x| x.treasury).unwrap_or(0.0);
            for s in &towns {
                let dist = if sx >= 0 {
                    (((s.x - sx).pow(2) + (s.y - sy).pow(2)) as f64).sqrt()
                } else {
                    0.0
                };
                admin += 1.0 + dist / D_SPAN;
            }
            if let Some(so) = societies.get(realms[r.idx()].people.idx()) {
                era_max = era_max.max(so.era);
            }
        }
        let legit = if tw > 0.0 { lw / tw } else { 0.0 };
        let asab = if tw > 0.0 { aw / tw } else { 0.0 };
        let capacity = CAP_TOWNS
            * members.len() as f64
            * (1.0 + 0.20 * era_max as f64)
            * (0.70 + 0.60 * asab);
        let stretch = if capacity > 0.0 { admin / capacity } else { 0.0 };
        let qual = legit >= GOLDEN_LEGIT
            && asab >= GOLDEN_ASAB
            && wealth >= GOLDEN_WEALTH
            && stretch < 0.95;

        civs[ci].crowns = members.len();
        civs[ci].towns = tw as usize;
        civs[ci].legit = legit;
        civs[ci].asab = asab;
        civs[ci].wealth = wealth;
        civs[ci].stretch = stretch;
        civs[ci].admin = admin;
        civs[ci].capacity = capacity;

        match civs[ci].stage {
            Stage::Rising => {
                if qual {
                    civs[ci].streak += 1;
                } else {
                    civs[ci].streak = 0;
                }
                if civs[ci].streak >= GOLDEN_YEARS {
                    civs[ci].stage = Stage::Golden;
                    civs[ci].golden_since = Some(month);
                    civs[ci].golden_ages += 1;
                    civs[ci].monuments = 0;
                    civs[ci].streak = 0;
                    civs[ci].strain = 0;
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text: format!(
                            "A golden age dawns over {} — treasuries full, courts trusted, the roads safe from {} to the far marches. Masons and scholars are summoned by every crown.",
                            civs[ci].name,
                            if seat_name.is_empty() { "the seats".to_string() } else { seat_name.clone() }
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        legend: format!(
                            "Grandmothers date things from it still: 'in the high noon of {}', they say, and mean better days.",
                            civs[ci].name
                        ),
                        ..Default::default()
                    });
                }
                // overreach before the flowering (M13.3): a family can
                // sprawl past its writ without ever seeing a golden age —
                // the same strain clock runs here as under Golden.
                if stretch > STRETCH_TRIGGER {
                    civs[ci].strain = civs[ci].strain.saturating_add(1);
                } else {
                    civs[ci].strain = civs[ci].strain.saturating_sub(STRAIN_RELIEF);
                }
                if civs[ci].strain >= WANE_YEARS && civs[ci].stage == Stage::Rising {
                    civs[ci].stage = Stage::Waning;
                    civs[ci].streak = 0;
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Realm,
                        text: format!(
                            "{} has grown faster than its writ — the clerks count more roads than riders past {}, and the far provinces learn to wait. The family begins to wane before it ever flowered.",
                            civs[ci].name,
                            if seat_name.is_empty() { "the seats".to_string() } else { seat_name.clone() }
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        ..Default::default()
                    });
                }
            }
            Stage::Golden => {
                // monuments (M13.2): registry-tracked artifacts that never wander
                if civs[ci].monuments < MONUMENTS_CAP && rng.gen::<f64>() < 0.35 {
                    if let (Some(r), Some((sid, town_name, x, y))) = (anchor, seat.clone()) {
                        let dom = realms[r.idx()].people;
                        let style = folk[dom.idx()].style.clone();
                        let w2 = naming::make_word(rng, &style, taken);
                        let kind = ["Colossus", "Gate", "Needle", "Dome", "Stair"]
                            [rng.gen_range(0..5)];
                        let mname = format!("the {} of {}", kind, w2);
                        if !taken.contains(&mname) {
                            taken.insert(mname.clone());
                            let ment =
                                reg.add(EntityKind::Artifact, &mname, month, Some(dom), x, y);
                            artifacts.push(Artifact {
                                ent: ment,
                                name: mname.clone(),
                                kind: "monument".into(),
                                holder: sid,
                                maker: dom,
                                keeper: r,
                                made: month,
                                lost: false,
                            });
                            civs[ci].monuments += 1;
                            events.push(Event {
                                m: month,
                                s: mname.clone(),
                                k: EventKind::Wonder,
                                text: format!(
                                    "In {} the masons of {} raise {} — golden-age stone, meant to outlast the crowns that paid for it.",
                                    town_name, civs[ci].name, mname
                                ),
                                ids: smallvec![ment, civs[ci].ent],
                                x,
                                y,
                                legend: format!(
                                    "Travellers say you see {} a day before you see the walls.",
                                    mname
                                ),
                                ..Default::default()
                            });
                        }
                    }
                }
                // overstretch and decadence (M13.3)
                if stretch > STRETCH_TRIGGER || asab < 0.45 {
                    civs[ci].strain += 1;
                    if civs[ci].strain == 1 {
                        events.push(Event {
                            m: month,
                            s: civs[ci].name.clone(),
                            k: EventKind::Realm,
                            text: format!(
                                "The clerks of {} count more roads than riders — the writ runs slow past {}, and the provinces learn to wait.",
                                civs[ci].name,
                                if seat_name.is_empty() { "the seats".to_string() } else { seat_name.clone() }
                            ),
                            ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                            x: sx,
                            y: sy,
                            ..Default::default()
                        });
                    }
                } else {
                    civs[ci].strain = civs[ci].strain.saturating_sub(STRAIN_RELIEF);
                }
                if civs[ci].strain >= WANE_YEARS {
                    civs[ci].stage = Stage::Waning;
                    civs[ci].streak = 0;
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text: format!(
                            "The court of {} gilds its halls while the granaries thin — offices are sold, the marches go unpaid, and old soldiers drink to better emperors. The golden age is over.",
                            civs[ci].name
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        legend: format!(
                            "The chroniclers of {} write smaller in these years, as if ashamed of the ink.",
                            civs[ci].name
                        ),
                        ..Default::default()
                    });
                }
            }
            Stage::Waning => {
                // the Khaldun decay, surfaced (M13.3): court-rot erodes the
                // member realms, scaled by how far the writ is stretched —
                // it must outpace the monthly tide to mean anything.
                let rot = stretch.min(5.0);
                for &r in &members {
                    if let Some(a) = pol.asab.get_mut(r.idx()) {
                        *a = (*a - 0.010 * rot).max(0.05);
                    }
                    if let Some(l) = pol.legit.get_mut(r.idx()) {
                        *l = (*l - 0.012 * rot).max(0.05);
                    }
                    if let Some(u) = pol.unrest.get_mut(r.idx()) {
                        *u = (*u + 0.03 * rot).min(1.0);
                    }
                }
                // strain keeps the clock while the stretch holds (Tainter)
                if stretch >= STRETCH_TRIGGER {
                    civs[ci].strain = civs[ci].strain.saturating_add(1);
                } else {
                    civs[ci].strain = civs[ci].strain.saturating_sub(STRAIN_RELIEF);
                }
                if (month / 12) % 2 == 0 {
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Realm,
                        text: format!(
                            "Court-rot in {}: two stewards keep three ledgers, tribute arrives light, and every crown blames another. The old solidarity gutters.",
                            civs[ci].name
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        ..Default::default()
                    });
                }
                // renaissance: the arc can turn back up (polities oscillate)
                if qual {
                    civs[ci].streak += 1;
                } else {
                    civs[ci].streak = 0;
                }
                if civs[ci].streak >= GOLDEN_YEARS {
                    civs[ci].stage = Stage::Golden;
                    civs[ci].golden_since = Some(month);
                    civs[ci].golden_ages += 1;
                    civs[ci].monuments = 0;
                    civs[ci].streak = 0;
                    civs[ci].strain = 0;
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text: format!(
                            "Against the run of the years, {} rights itself — debts paid, marches manned, the courts speaking with one voice again. A second flowering begins.",
                            civs[ci].name
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        ..Default::default()
                    });
                } else if (asab < COLLAPSE_ASAB && legit < COLLAPSE_LEGIT)
                    || civs[ci].strain >= COLLAPSE_STRAIN
                {
                    // the break (M13.4)
                    civs[ci].stage = Stage::Interregnum;
                    civs[ci].fall_began = month;
                    civs[ci].realms_at_fall = members.len();
                    if let Some(h) = civs[ci].hegemony.take() {
                        civs[ci].paramount = None;
                        events.push(Event {
                            m: month,
                            s: civs[ci].name.clone(),
                            k: EventKind::Realm,
                            text: format!(
                                "{} dissolves — the tribute carts stop on the roads, and no one sends riders after them.",
                                h
                            ),
                            ids: smallvec![civs[ci].ent],
                            x: sx,
                            y: sy,
                            ..Default::default()
                        });
                    }
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text: format!(
                            "{} breaks. The crowns that answered one family answer none; garrisons melt from the marches, and every province keeps its own grain. An interregnum begins.",
                            civs[ci].name
                        ),
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        legend: format!(
                            "They say the last high court of {} argued precedence while the couriers' horses were eaten in the yard.",
                            civs[ci].name
                        ),
                        ..Default::default()
                    });
                }
            }
            Stage::Interregnum => {
                // the fall runs through the M11 ladder (ADR-0019): pressure
                // on, cooldowns cut short — the existing rungs mint the
                // successor realms. Realms, never peoples. Cut, not off:
                // capping the remaining calm at a year keeps the churn
                // yearly (the fall runs 4–12y, room enough to fragment)
                // while honouring M11.6 — no realm convulses monthly, not
                // even a dying one. Fully disarmed, a rich lettered crown
                // re-chartered every few months instead of falling.
                for &r in &members {
                    if let Some(u) = pol.unrest.get_mut(r.idx()) {
                        *u = u.max(0.88);
                    }
                    if let Some(a) = pol.asab.get_mut(r.idx()) {
                        *a = (a.min(0.46)) * 0.94;
                    }
                    if let Some(c) = pol.calm_until.get_mut(r.idx()) {
                        *c = (*c).min(month + 12);
                    }
                }
                // the roads go unsafe: member towns thin a little each year
                for s in settlements.iter_mut() {
                    if members.contains(&s.realm) && s.pop > 150 {
                        s.pop = ((s.pop as f64) * 0.985) as i64;
                    }
                }
                let fragments = members.len() > civs[ci].realms_at_fall;
                let years_down = (month - civs[ci].fall_began) / 12;
                if (fragments && years_down >= 4) || years_down >= INTERREGNUM_YEARS {
                    // succession (M13.4): close the arc, name the heirs
                    civs[ci].alive = false;
                    civs[ci].ended = month;
                    let mut heirs: Vec<String> = members
                        .iter()
                        .filter_map(|&r| realms.get(r.idx()))
                        .map(|r| r.name.clone())
                        .collect();
                    heirs.truncate(3);
                    reg.close(
                        civs[ci].ent,
                        month,
                        "fell; its crowns scattered among successor realms",
                    );
                    let text = if heirs.is_empty() {
                        format!(
                            "The interregnum ends with no heir at all — where {} ruled, only ruins and road-stones carry the old names.",
                            civs[ci].name
                        )
                    } else {
                        format!(
                            "The interregnum ends. {} is gone; {} and the other successor crowns divide its roads, its debts and its gods — the same peoples, under smaller banners.",
                            civs[ci].name,
                            heirs.join(", ")
                        )
                    };
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text,
                        ids: arc_ids(reg, civs[ci].ent, anchor_name.as_deref()),
                        x: sx,
                        y: sy,
                        legend: format!(
                            "Men ploughing by the old capitals of {} still turn up milestones with distances to cities that kept other names.",
                            civs[ci].name
                        ),
                        ..Default::default()
                    });
                }
            }
        }

        // ---- 5 · hegemony (M13.5): the paramount tier over the members --
        if civs[ci].alive && civs[ci].stage != Stage::Interregnum && members.len() >= 2 {
            let mut best: Option<(usize, RealmId)> = None;
            for &r in &members {
                let mut subs: Vec<RealmId> = Vec::new();
                for &v in &members {
                    if v == r {
                        continue;
                    }
                    let vassal = pol
                        .vassal_of
                        .get(v.idx())
                        .copied()
                        .flatten()
                        .map_or(false, |s| s == r);
                    let tribute = pol
                        .tributes
                        .iter()
                        .any(|t| t.from == v && t.to == r && t.months_left > 0);
                    if vassal || tribute {
                        subs.push(v);
                    }
                }
                if subs.len() >= 2
                    && best.map_or(true, |(bn, br)| {
                        subs.len() > bn || (subs.len() == bn && r.idx() < br.idx())
                    })
                {
                    best = Some((subs.len(), r));
                }
            }
            match (best, civs[ci].paramount) {
                (Some((_, r)), old) if old != Some(r) => {
                    let rn = realms[r.idx()].name.clone();
                    let hname = format!("the {} Hegemony", rn);
                    civs[ci].paramount = Some(r);
                    civs[ci].hegemony = Some(hname.clone());
                    let hseat = seat_of(realms, settlements, r);
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Era,
                        text: format!(
                            "Within {}, the crowns now look to one seat: tribute and homage flow to {}, and the scribes begin writing of {}.",
                            civs[ci].name, rn, hname
                        ),
                        ids: arc_ids(reg, civs[ci].ent, Some(rn.as_str())),
                        x: hseat.as_ref().map(|&(_, _, x, _)| x).unwrap_or(-1),
                        y: hseat.as_ref().map(|&(_, _, _, y)| y).unwrap_or(-1),
                        ..Default::default()
                    });
                }
                (None, Some(_)) => {
                    let h = civs[ci].hegemony.take().unwrap_or_default();
                    civs[ci].paramount = None;
                    events.push(Event {
                        m: month,
                        s: civs[ci].name.clone(),
                        k: EventKind::Realm,
                        text: format!(
                            "{} lapses — the homage goes unrenewed, and no one marches to collect it.",
                            h
                        ),
                        ids: smallvec![civs[ci].ent],
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }

        // ---- 6 · the golden boon (M13.2), through the one hook ----------
        if civs[ci].alive && civs[ci].stage == Stage::Golden {
            for &p in &civs[ci].peoples {
                if let Some(so) = societies.get_mut(p.idx()) {
                    so.boon = TECH_BOON;
                }
            }
        }
    }

    events
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7) — the M13 acceptance gates in numbers.
pub const BANDS: &[Band] = &[
    Band { name: "living civilizations", sweet: (1.0, 5.0), hard: (0.0, 8.0), target: "M13.1: the tier exists without swallowing the world" },
    Band { name: "civ arcs completed per 300 y", sweet: (1.0, 4.0), hard: (0.5, 8.0), target: "M13.4 gate: ≥1 full rise-and-fall per 300 y on most seeds" },
    Band { name: "successor realms per collapse", sweet: (1.0, 6.0), hard: (0.0, 12.0), target: "M13.4: fragmentation through the M11 ladder, not deletion" },
];
