//! Toponymy — port of naming.py: detect geographic features and name them.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::constants as gc;
use crate::ndimage;

pub struct Bank {
    pub pre: &'static [&'static str],
    pub mid: &'static [&'static str],
    pub end: &'static [&'static str],
}

pub fn bank(style: &str) -> &'static Bank {
    match style {
        "hellenic" => &HELLENIC,
        "nordic" => &NORDIC,
        "arid" => &ARID,
        "sylvan" => &SYLVAN,
        "steppe" => &STEPPE,
        _ => &OLD,
    }
}

pub static OLD: Bank = Bank {
    pre: &[
        "Aur", "Bel", "Cal", "Dor", "El", "Far", "Gal", "Hal", "Ith", "Kar", "Lor", "Mal",
        "Nor", "Or", "Pel", "Quel", "Ser", "Tal", "Um", "Vor", "Yl", "Zar",
    ],
    mid: &["a", "e", "i", "o", "u", "ae", "ia", "or", "an", "el", "ar"],
    end: &[
        "ath", "dor", "eth", "ia", "ion", "mar", "nor", "os", "rin", "thas", "um", "wyn", "ys",
    ],
};

pub static HELLENIC: Bank = Bank {
    pre: &[
        "Kal", "Thes", "Ery", "Del", "Ar", "Kor", "Pel", "Nax", "Ida", "Olyn", "Thra", "Mel",
        "Or", "Phi", "Xan", "Hel", "Leu", "Myr",
    ],
    mid: &["li", "ra", "do", "ka", "the", "mo", "sy", "le", "ei", "an"],
    end: &[
        "opia", "ossa", "ene", "ikos", "antheia", "polis", "ion", "aia", "yra", "anthe",
        "eia", "os",
    ],
};

pub static NORDIC: Bank = Bank {
    pre: &[
        "Skjal", "Thor", "Ulf", "Bryn", "Eir", "Frost", "Hav", "Jor", "Kald", "Nor", "Sten",
        "Varg", "Hrim", "Grim", "Odd", "Sol",
    ],
    mid: &["a", "e", "en", "ar", "ur"],
    end: &[
        "vik", "heim", "gard", "stad", "berg", "dal", "mark", "nes", "holm", "fell", "strand",
    ],
};

pub static ARID: Bank = Bank {
    pre: &[
        "Al", "Zar", "Qas", "Mir", "Sah", "Kha", "Dun", "Azh", "Bak", "Tam", "Ras", "Jal",
        "Nef", "Ash",
    ],
    mid: &["a", "i", "u", "ara", "im"],
    end: &[
        "bar", "dun", "mesh", "ra", "sur", "zad", "kar", "esh", "ah", "iyya", "met",
    ],
};

pub static SYLVAN: Bank = Bank {
    pre: &[
        "Ael", "Briar", "Fen", "Glen", "Haw", "Lin", "Moss", "Roe", "Syl", "Thal", "Wil",
        "Yew", "El", "Ash",
    ],
    mid: &["en", "or", "a", "wy"],
    end: &[
        "dell", "mere", "shade", "thorn", "wick", "wood", "hollow", "glade", "brook", "leaf",
        "run",
    ],
};

pub static STEPPE: Bank = Bank {
    pre: &[
        "Bor", "Dzun", "Kesh", "Khar", "Orda", "Sar", "Tem", "Ulan", "Yur", "Qar", "Bay",
        "Alta", "Ker",
    ],
    mid: &["a", "u", "ge", "ta"],
    end: &[
        "gan", "tau", "gol", "bek", "sarai", "chi", "dag", "kum", "kent", "su",
    ],
};

/// One deterministic, unique name word in the given style.
pub fn make_word(rng: &mut Pcg64Mcg, style: &str, taken: &mut HashSet<String>) -> String {
    let b = bank(style);
    for _ in 0..96 {
        let mut w = String::from(b.pre[rng.gen_range(0..b.pre.len())]);
        if rng.gen::<f64>() < 0.42 {
            w.push_str(b.mid[rng.gen_range(0..b.mid.len())]);
        }
        w.push_str(b.end[rng.gen_range(0..b.end.len())]);
        if !taken.contains(&w) {
            taken.insert(w.clone());
            return w;
        }
    }
    let w = format!("{}{}", b.pre[0], taken.len());
    taken.insert(w.clone());
    w
}

fn templates(kind: &str) -> &'static [&'static str] {
    match kind {
        "ocean" => &["The {w} Ocean", "The {w} Deep"],
        "sea" => &["Sea of {w}", "The {w} Sea", "Gulf of {w}", "The {w} Expanse"],
        "continent" => &["{w}"],
        "island" => &["Isle of {w}", "{w} Isle"],
        "range" => &[
            "The {w} Mountains",
            "The {w} Range",
            "The Peaks of {w}",
            "The {w} Reach",
        ],
        "desert" => &["The {w} Desert", "The {w} Wastes", "The Sands of {w}"],
        "forest" => &["The {w}wood", "{w} Forest", "The Woods of {w}"],
        "river" => &["River {w}", "The {w}"],
        "lake" => &["Lake {w}", "The {w} Mere"],
        _ => &["{w}"],
    }
}

fn phrase(rng: &mut Pcg64Mcg, kind: &str, word: &str) -> String {
    let t = templates(kind);
    t[rng.gen_range(0..t.len())].replace("{w}", word)
}

#[derive(Serialize, Clone)]
pub struct Feature {
    pub t: String,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub size: i64,
}

enum Anchor<'a> {
    Interior,
    Peak(&'a Array2<f64>),
}

fn add_features(
    features: &mut Vec<Feature>,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    kind: &str,
    labeled: &ndimage::Labeled,
    comps: &[(usize, f64)],
    anchor: Anchor,
) {
    for &(idx, area) in comps {
        let (y, x) = match anchor {
            Anchor::Interior => ndimage::interior_anchor(labeled, idx),
            Anchor::Peak(field) => ndimage::peak_anchor(labeled, idx, field),
        };
        let word = make_word(rng, "old", taken);
        let name = phrase(rng, kind, &word);
        features.push(Feature {
            t: kind.to_string(),
            name,
            x: x as i64,
            y: y as i64,
            size: area as i64,
        });
    }
}

/// Returns (features, world_name).
pub fn name_features(
    height: &Array2<f64>,
    biomes: &Array2<u8>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f64>,
    seed: i64,
) -> (Vec<Feature>, String) {
    let mut rng = crate::util::rng(seed + 12000);
    let mut taken: HashSet<String> = HashSet::new();
    let size = height.dim().0;
    let sc = (size as f64 / 512.0).powi(2);
    let mut features: Vec<Feature> = Vec::new();

    // ocean & seas
    let sea = height.mapv(|h| h < 0.0);
    let lab = ndimage::label(&sea, true);
    let comps = ndimage::top_components(&lab, 900.0 * sc, 7);
    if !comps.is_empty() {
        let biggest = comps[0];
        let kind = if biggest.1 >= 15000.0 * sc { "ocean" } else { "sea" };
        add_features(&mut features, &mut rng, &mut taken, kind, &lab, &[biggest], Anchor::Interior);
        add_features(&mut features, &mut rng, &mut taken, "sea", &lab, &comps[1..], Anchor::Interior);
    }

    // continents & islands
    let land = sea.mapv(|s| !s);
    let lab = ndimage::label(&land, true);
    let comps = ndimage::top_components(&lab, 60.0 * sc, 12);
    let continents: Vec<(usize, f64)> = comps
        .iter()
        .filter(|c| c.1 >= 9000.0 * sc)
        .take(3)
        .cloned()
        .collect();
    let islands: Vec<(usize, f64)> = comps
        .iter()
        .filter(|c| c.1 < 9000.0 * sc)
        .take(8)
        .cloned()
        .collect();
    add_features(&mut features, &mut rng, &mut taken, "continent", &lab, &continents, Anchor::Interior);
    add_features(&mut features, &mut rng, &mut taken, "island", &lab, &islands, Anchor::Interior);

    // mountain ranges (dilated so nearby ridges merge into one range)
    let peaks = height.mapv(|h| h > 0.52);
    let merged = ndimage::binary_dilation(&peaks, 2);
    let lab = ndimage::label(&merged, true);
    let comps = ndimage::top_components(&lab, 45.0 * sc, 8);
    add_features(&mut features, &mut rng, &mut taken, "range", &lab, &comps, Anchor::Peak(height));

    // deserts & forests
    let desert = biomes.mapv(|b| b == gc::DESERT);
    let lab = ndimage::label(&desert, true);
    let comps = ndimage::top_components(&lab, 260.0 * sc, 5);
    add_features(&mut features, &mut rng, &mut taken, "desert", &lab, &comps, Anchor::Interior);

    let forest = biomes.mapv(|b| {
        b == gc::WOODLAND
            || b == gc::SEASONAL_RAIN_FOREST
            || b == gc::TEMPERATE_RAIN_FOREST
            || b == gc::BOREAL_FOREST
            || b == gc::TROPICAL_RAIN_FOREST
    });
    let lab = ndimage::label(&forest, true);
    let comps = ndimage::top_components(&lab, 450.0 * sc, 7);
    add_features(&mut features, &mut rng, &mut taken, "forest", &lab, &comps, Anchor::Interior);

    // rivers (anchored near their strongest reach) & lakes
    let lab = ndimage::label(rivers, true);
    let comps = ndimage::top_components(&lab, 45.0 * sc, 10);
    add_features(&mut features, &mut rng, &mut taken, "river", &lab, &comps, Anchor::Peak(discharge));

    let lab = ndimage::label(lakes, true);
    let comps = ndimage::top_components(&lab, 18.0 * sc, 6);
    add_features(&mut features, &mut rng, &mut taken, "lake", &lab, &comps, Anchor::Interior);

    let world_name = make_word(&mut rng, "old", &mut taken);
    (features, world_name)
}
