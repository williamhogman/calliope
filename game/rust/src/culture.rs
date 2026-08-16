//! Cultures — port of culture.py: k-means clustering, styles, renaming.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::constants as gc;
use crate::naming;
use crate::settlements::Settlement;

pub const CULTURE_COLORS: [&str; 12] = [
    "#d4a94a", "#6f9ceb", "#c86b6b", "#7fb069", "#a06fd4", "#5bc0be",
    "#e08d5a", "#8fa3d4", "#c95f8e", "#9db55c", "#7d6fd4", "#4fb08d",
];

fn style_by_biome(b: u8) -> &'static str {
    match b {
        x if x == gc::TUNDRA || x == gc::BOREAL_FOREST || x == gc::ICE => "nordic",
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

fn demonym(style: &str) -> &'static str {
    match style {
        "hellenic" => "ians",
        "nordic" => "folk",
        "arid" => "im",
        "sylvan" => "kin",
        "steppe" => "aks",
        _ => "ites",
    }
}

const ALL_STYLES: [&str; 5] = ["hellenic", "steppe", "nordic", "sylvan", "arid"];

/// M3.5 — a named god with a domain; cited in omens, festivals and wars.
#[derive(Serialize, Clone)]
pub struct God {
    pub name: String,
    pub domain: String,
}

/// Domains a pantheon may hold; each culture draws four, distinct.
pub const DOMAINS: [&str; 10] = [
    "the sea", "the harvest", "war", "storms", "the dead",
    "the hearth", "fate", "the wild", "craft", "the moon",
];

#[derive(Serialize, Clone)]
pub struct Culture {
    pub id: usize,
    pub name: String,
    pub people: String,
    pub style: String,
    pub color: String,
    /// The culture's gods (M3.5); index 0 is the chief god.
    pub pantheon: Vec<God>,
}

/// Draw a four-god pantheon in the culture's tongue. Deterministic:
/// domain picks and names both come off the shared rng stream.
fn make_pantheon(
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

/// Cluster settlements into cultures, rename them in-style.
pub fn assign_cultures(
    biomes: &Array2<u8>,
    settlements: &mut [Settlement],
    taken: &mut HashSet<String>,
    seed: i64,
) -> Vec<Culture> {
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
    let mut cultures: Vec<Culture> = Vec::new();
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
        cultures.push(Culture {
            id: cid,
            people: format!("{}{}", root, demonym(style)),
            name: root,
            style: style.to_string(),
            color: CULTURE_COLORS[cid % CULTURE_COLORS.len()].to_string(),
            pantheon: make_pantheon(&mut rng, style, taken),
        });
    }

    // rename settlements in their culture's tongue, keeping the reading
    // of each name's parts (M3.3)
    for (s, &l) in settlements.iter_mut().zip(lab.iter()) {
        s.culture = l;
        s.namer = l;
        let c = naming::coin(&mut rng, &cultures[l].style, taken);
        s.name = c.word;
        s.ety = c.ety;
    }

    cultures
}

/// M4.5 — a rising carves a new realm out of an old one. The rebels keep
/// their parent's tongue and gods (it is a political break, not a new
/// people) but take a new name and a new colour on the map.
pub fn secede(
    parent: &Culture,
    new_id: usize,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
) -> Culture {
    let root = naming::make_word(rng, &parent.style, taken);
    Culture {
        id: new_id,
        people: format!("{}{}", root, demonym(&parent.style)),
        name: root,
        style: parent.style.clone(),
        color: CULTURE_COLORS[new_id % CULTURE_COLORS.len()].to_string(),
        pantheon: parent.pantheon.clone(),
    }
}
