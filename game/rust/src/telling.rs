//! M6 — The Telling. The chronicle's second reading: weights that say
//! which events matter, a mythologizing layer that renders great deeds
//! the way the fireside remembers them (M6.9), and the story sifter —
//! Felt-style patterns run over the structured log to lift microstories
//! out of the noise (M6.5), ranked by eventfulness and reversal (M6.7).
//!
//! Deterministic throughout: no rng — every choice hashes off the event
//! itself, so the same world tells the same tales, always.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ids::EntityId;
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::world::EventKind;
use crate::world::Event;

// ---------------------------------------------------------------- weight

/// How loudly an event kind rings down the years — one row per kind in
/// the event table beside `EventKind` itself (E2.3).
pub fn weight(k: EventKind) -> i32 {
    k.weight()
}

/// Which way fortune leans for the subject: +1 rising, −1 falling, 0 flat.
/// The reversal detector (M6.7) counts the sign changes.
pub fn fortune(k: EventKind) -> i32 {
    k.fortune()
}

// ---------------------------------------------------------------- hashing

/// FNV-1a over a seed and a string — the telling's only die.
pub fn det_hash(seed: u64, s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325 ^ seed.wrapping_mul(0x100000001b3);
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------- legends

const OPENERS: [&str; 6] = [
    "So the songs tell it —",
    "As the grey-beards tell it,",
    "In the telling of later days,",
    "The fireside chronicle says:",
    "Long after, they sang of it —",
    "The keepers of the telling swear it so:",
];

const CLOSERS: [&str; 4] = [
    " So it is told.",
    " The song does not lie, they say.",
    " No two tellings agree on more than that.",
    " The rest the years have eaten.",
];

/// The mythologized rendering (M6.9): numbers blur into "untold", an
/// opener sets the event at a fireside remove, and sometimes a closing
/// formula admits how little the song really knows.
pub fn legendize(e: &Event) -> String {
    let h = det_hash(e.m as u64, &e.s);
    let opener = OPENERS[(h % OPENERS.len() as u64) as usize];
    // counts of three digits or more become the stuff of song
    let mut vague = String::with_capacity(e.text.len());
    let mut digits = String::new();
    for c in e.text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                if digits.len() >= 3 {
                    vague.push_str("untold");
                } else {
                    vague.push_str(&digits);
                }
                digits.clear();
            }
            vague.push(c);
        }
    }
    if !digits.is_empty() {
        if digits.len() >= 3 {
            vague.push_str("untold");
        } else {
            vague.push_str(&digits);
        }
    }
    let closer = if h.rotate_left(17) % 5 < 2 {
        CLOSERS[(h.rotate_left(31) % CLOSERS.len() as u64) as usize]
    } else {
        ""
    };
    format!("{} {}{}", opener, vague, closer)
}

// ---------------------------------------------------------------- stories

#[derive(Serialize, Clone)]
pub struct Story {
    /// Pattern id: rise-fall · trials · rivalry · mine-curse · tide-turned ·
    /// founders-dream · roads-of · restless-crowns · relic-road.
    pub pattern: String,
    pub title: String,
    /// Entities the story is about; the first is the protagonist.
    pub ids: Vec<EntityId>,
    /// Years spanned (inclusive).
    pub y0: i64,
    pub y1: i64,
    /// Eventfulness score (M6.7): weights + reversals + span.
    pub score: f64,
    /// The beats, chronological, capped — full events so the client can
    /// render either layer of the telling.
    pub beats: Vec<Event>,
}

/// Sum of weights + reversal bonus + span bonus, the ranking die (M6.7).
fn score_beats(beats: &[&Event]) -> f64 {
    let mut sc = 0.0;
    let mut last = 0i32;
    let mut reversals = 0;
    for b in beats {
        sc += weight(b.k) as f64;
        let f = fortune(b.k);
        if f != 0 {
            if last != 0 && f != last {
                reversals += 1;
            }
            last = f;
        }
    }
    let span_y = if beats.len() >= 2 {
        (beats[beats.len() - 1].m - beats[0].m) / 12
    } else {
        0
    };
    sc + 2.5 * reversals as f64 + (span_y as f64 / 50.0).min(2.0)
}

fn reversal_count(beats: &[&Event]) -> usize {
    let mut last = 0i32;
    let mut n = 0;
    for b in beats {
        let f = fortune(b.k);
        if f != 0 {
            if last != 0 && f != last {
                n += 1;
            }
            last = f;
        }
    }
    n
}

/// Trim a beat list to at most `cap`, keeping the opening, the loudest
/// middles and the close — the shape survives, the filler goes.
fn trim_beats(mut beats: Vec<&Event>, cap: usize) -> Vec<Event> {
    if beats.len() > cap {
        let head: Vec<&Event> = beats.drain(..2).collect();
        let tail: Vec<&Event> = beats.split_off(beats.len() - 2);
        let mut mid = beats;
        mid.sort_by(|a, b| {
            weight(b.k)
                .cmp(&weight(a.k))
                .then(a.m.cmp(&b.m))
        });
        mid.truncate(cap - 4);
        let mut all: Vec<&Event> = head;
        all.extend(mid);
        all.extend(tail);
        all.sort_by_key(|e| e.m);
        all.into_iter().cloned().collect()
    } else {
        beats.into_iter().cloned().collect()
    }
}

/// Run every pattern over the structured log (M6.5). Deterministic:
/// BTree ordering everywhere, ties broken by year then title.
pub fn sift(events: &[Event], reg: &Registry) -> Vec<Story> {
    // per-entity event index, in id order
    let mut index: BTreeMap<EntityId, Vec<usize>> = BTreeMap::new();
    for (i, e) in events.iter().enumerate() {
        for &id in &e.ids {
            index.entry(id).or_default().push(i);
        }
    }
    let beats_of = |id: EntityId| -> Vec<&Event> {
        index
            .get(&id)
            .map(|v| v.iter().map(|&i| &events[i]).collect())
            .unwrap_or_default()
    };

    let mut out: BTreeMap<(String, EntityId), Story> = BTreeMap::new();
    let mut push = |pattern: &str, key: EntityId, title: String, ids: Vec<EntityId>, beats: Vec<&Event>| {
        if beats.is_empty() {
            return;
        }
        let score = score_beats(&beats);
        let y0 = beats[0].m / 12;
        let y1 = beats[beats.len() - 1].m / 12;
        let story = Story {
            pattern: pattern.to_string(),
            title,
            ids,
            y0,
            y1,
            score,
            beats: trim_beats(beats, 12),
        };
        let k = (pattern.to_string(), key);
        match out.get(&k) {
            Some(prev) if prev.score >= story.score => {}
            _ => {
                out.insert(k, story);
            }
        }
    };

    for ent in &reg.items {
        let beats = beats_of(ent.id);
        if beats.len() < 2 {
            continue;
        }
        match ent.kind {
            // --- rise-fall / trials: a settlement raised high, then struck
            EntityKind::Settlement => {
                let rises = beats.iter().filter(|b| fortune(b.k) > 0).count();
                let blows = beats
                    .iter()
                    .filter(|b| fortune(b.k) < 0 && weight(b.k) >= 3)
                    .count();
                if rises >= 1 && ent.until.is_some() {
                    push(
                        "rise-fall",
                        ent.id,
                        format!("The Rise and Fall of {}", ent.name),
                        vec![ent.id],
                        beats.clone(),
                    );
                } else if rises >= 1 && blows >= 3 {
                    push(
                        "trials",
                        ent.id,
                        format!("The Trials of {}", ent.name),
                        vec![ent.id],
                        beats.clone(),
                    );
                }
                // --- mine-curse: the strike, then the silence
                let strike = beats.iter().position(|b| b.k == EventKind::Discovery);
                let spent = beats.iter().rposition(|b| b.k == EventKind::Depletion);
                if let (Some(a), Some(b)) = (strike, spent) {
                    if b > a && beats[b].m - beats[a].m <= 80 * 12 {
                        let arc: Vec<&Event> = beats[a..=b]
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e.k,
                                    EventKind::Discovery
                                        | EventKind::Depletion
                                        | EventKind::Found
                                        | EventKind::Growth
                                        | EventKind::Trade
                                )
                            })
                            .copied()
                            .collect();
                        if arc.len() >= 2 {
                            push(
                                "mine-curse",
                                ent.id,
                                format!("The Curse of the Mines of {}", ent.name),
                                vec![ent.id],
                                arc,
                            );
                        }
                    }
                }
            }
            // --- tide-turned wars, marked by the epithet politics awards
            EntityKind::War => {
                if ent.epithets.iter().any(|e| e == "the Tide-Turned")
                    || reversal_count(&beats) >= 2
                {
                    push(
                        "tide-turned",
                        ent.id,
                        format!("How {} Turned", ent.name),
                        vec![ent.id],
                        beats.clone(),
                    );
                }
            }
            EntityKind::Person => match ent.role.as_str() {
                // --- founder's dream: their camp grown into wonders
                "founder" => {
                    let mut sett: Option<EntityId> = None;
                    for b in &beats {
                        if b.k == EventKind::Found {
                            sett = b
                                .ids
                                .iter()
                                .copied()
                                .find(|&i| reg.get(i).map(|x| x.kind == EntityKind::Settlement).unwrap_or(false));
                        }
                    }
                    if let Some(sid) = sett {
                        let sb = beats_of(sid);
                        if sb.iter().any(|b| b.k == EventKind::Wonder) {
                            let mut arc = beats.clone();
                            arc.extend(sb.iter().filter(|b| {
                                matches!(b.k, EventKind::Wonder | EventKind::Growth | EventKind::Found)
                            }));
                            arc.sort_by_key(|e| e.m);
                            arc.dedup_by(|a, b| a.m == b.m && a.text == b.text);
                            push(
                                "founders-dream",
                                ent.id,
                                format!("{}'s Dream", ent.name),
                                vec![ent.id, sid],
                                arc,
                            );
                        }
                    }
                }
                // --- a merchant's whole road, told at the end of it
                "merchant" => {
                    if beats.len() >= 3 {
                        push(
                            "roads-of",
                            ent.id,
                            format!("The Roads of {}", ent.name),
                            vec![ent.id],
                            beats.clone(),
                        );
                    }
                }
                _ => {}
            },
            // --- restless crowns: three circlets in a hundred years
            EntityKind::Culture => {
                let crowns: Vec<&&Event> =
                    beats.iter().filter(|b| b.k == EventKind::Ruler).collect();
                for w in crowns.windows(3) {
                    if w[2].m - w[0].m <= 100 * 12 {
                        push(
                            "restless-crowns",
                            ent.id,
                            format!("The Restless Crowns of the {}", ent.name),
                            vec![ent.id],
                            beats
                                .iter()
                                .filter(|b| matches!(b.k, EventKind::Ruler | EventKind::Realm | EventKind::War))
                                .copied()
                                .collect(),
                        );
                        break;
                    }
                }
            }
            // --- the relic's road: an artifact with a provenance (M6.3)
            EntityKind::Artifact => {
                if beats.len() >= 3 {
                    push(
                        "relic-road",
                        ent.id,
                        format!("The Wanderings of {}", ent.name),
                        vec![ent.id],
                        beats.clone(),
                    );
                }
            }
            _ => {}
        }
    }

    // --- the long rivalry: the same two peoples, war upon war
    // (kindle events carry [war, culture, culture] ids)
    let mut pair_wars: BTreeMap<(EntityId, EntityId), Vec<EntityId>> = BTreeMap::new();
    for e in events {
        if e.k == EventKind::War && e.text.starts_with("War kindles") && e.ids.len() >= 3 {
            let (a, b) = (e.ids[1].min(e.ids[2]), e.ids[1].max(e.ids[2]));
            pair_wars.entry((a, b)).or_default().push(e.ids[0]);
        }
    }
    for ((a, b), wars) in &pair_wars {
        if wars.len() < 2 {
            continue;
        }
        let (Some(ca), Some(cb)) = (reg.get(*a), reg.get(*b)) else {
            continue;
        };
        let mut arc: Vec<&Event> = Vec::new();
        for &w in wars {
            let wb = beats_of(w);
            // the kindling, the loudest middle, the peace
            if let Some(first) = wb.first() {
                arc.push(first);
            }
            if wb.len() > 2 {
                if let Some(mid) = wb[1..wb.len() - 1]
                    .iter()
                    .max_by_key(|e| (weight(e.k), -e.m))
                {
                    arc.push(mid);
                }
            }
            if wb.len() > 1 {
                arc.push(wb[wb.len() - 1]);
            }
        }
        arc.sort_by_key(|e| e.m);
        arc.dedup_by(|x, y| x.m == y.m && x.text == y.text);
        push(
            "rivalry",
            EntityId(a.0 * 100_000 + b.0),
            format!("The Long Rivalry of the {} and the {}", ca.name, cb.name),
            vec![*a, *b],
            arc,
        );
    }

    let mut stories: Vec<Story> = out.into_values().collect();
    stories.sort_by(|x, y| {
        y.score
            .partial_cmp(&x.score)
            .unwrap()
            .then(x.y0.cmp(&y.y0))
            .then(x.title.cmp(&y.title))
    });
    // Dedup bounds: no one pattern may drown the rest — the trials of a
    // hundred towns all score loud, but the telling wants variety (M6.5).
    let mut per: BTreeMap<String, usize> = BTreeMap::new();
    stories.retain(|s| {
        let n = per.entry(s.pattern.clone()).or_insert(0);
        *n += 1;
        *n <= 10
    });
    stories.truncate(48);
    stories
}
