//! Toponymy — port of naming.py: detect geographic features and name them.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

use crate::constants as gc;
use crate::ndimage;

/// A morpheme bank: every fragment carries its gloss (M3.3) — no name is
/// emitted whose parts cannot be read back. Fragments are ordered by
/// frequency: draws are power-law weighted (M3.2), so each tongue leans on
/// its favourite sounds and names within a culture rhyme with each other.
pub struct Bank {
    pub pre: &'static [(&'static str, &'static str)],
    pub mid: &'static [(&'static str, &'static str)],
    pub end: &'static [(&'static str, &'static str)],
}

pub const STYLES: [&str; 6] = ["old", "hellenic", "nordic", "arid", "sylvan", "steppe"];

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
        ("Aur", "golden"), ("Bel", "bright"), ("Cal", "white"), ("Dor", "gate"),
        ("El", "star"), ("Far", "far"), ("Gal", "singing"), ("Hal", "holy"),
        ("Ith", "silver"), ("Kar", "stone"), ("Lor", "old"), ("Mal", "dark"),
        ("Nor", "north"), ("Or", "dawn"), ("Pel", "grey"), ("Quel", "quiet"),
        ("Ser", "serpent"), ("Tal", "tall"), ("Um", "shadowed"), ("Vor", "cold"),
        ("Yl", "wind"), ("Zar", "fire"),
    ],
    mid: &[
        ("a", "high"), ("e", "pale"), ("i", "little"), ("o", "great"),
        ("u", "deep"), ("ae", "elder"), ("ia", "fair"), ("or", "golden"),
        ("an", "long"), ("el", "starlit"), ("ar", "proud"),
    ],
    end: &[
        ("ath", "height"), ("dor", "gate"), ("eth", "hearth"), ("ia", "land"),
        ("ion", "tower"), ("mar", "sea"), ("nor", "watch"), ("os", "sanctum"),
        ("rin", "crown"), ("thas", "throne"), ("um", "tomb"), ("wyn", "meadow"),
        ("ys", "isle"),
    ],
};

pub static HELLENIC: Bank = Bank {
    pre: &[
        ("Kal", "fair"), ("Thes", "sacred"), ("Ery", "red"), ("Del", "clear"),
        ("Ar", "noble"), ("Kor", "maiden"), ("Pel", "clay"), ("Nax", "blessed"),
        ("Ida", "wooded"), ("Olyn", "old"), ("Thra", "bold"), ("Mel", "honeyed"),
        ("Or", "mountain"), ("Phi", "beloved"), ("Xan", "golden"), ("Hel", "sun"),
        ("Leu", "white"), ("Myr", "fragrant"),
    ],
    mid: &[
        ("li", "graceful"), ("ra", "high"), ("do", "twin"), ("ka", "good"),
        ("the", "divine"), ("mo", "lone"), ("sy", "joined"), ("le", "smooth"),
        ("ei", "narrow"), ("an", "upper"),
    ],
    end: &[
        ("opia", "outlook"), ("ossa", "height"), ("ene", "dwelling"),
        ("ikos", "district"), ("antheia", "blossoming"), ("polis", "city"),
        ("ion", "sanctuary"), ("aia", "land"), ("yra", "shore"),
        ("anthe", "flower"), ("eia", "haven"), ("os", "place"),
    ],
};

pub static NORDIC: Bank = Bank {
    pre: &[
        ("Skjal", "shield"), ("Thor", "thunder"), ("Ulf", "wolf"), ("Bryn", "mailed"),
        ("Eir", "merciful"), ("Frost", "frost"), ("Hav", "sea"), ("Jor", "earth"),
        ("Kald", "cold"), ("Nor", "north"), ("Sten", "stone"), ("Varg", "warg"),
        ("Hrim", "rime"), ("Grim", "grim"), ("Odd", "spear-point"), ("Sol", "sun"),
    ],
    mid: &[
        ("a", "high"), ("e", "old"), ("en", "lone"), ("ar", "great"), ("ur", "ancient"),
    ],
    end: &[
        ("vik", "bay"), ("heim", "home"), ("gard", "stead"), ("stad", "town"),
        ("berg", "rock"), ("dal", "valley"), ("mark", "borderland"), ("nes", "headland"),
        ("holm", "islet"), ("fell", "mountain"), ("strand", "shore"),
    ],
};

pub static ARID: Bank = Bank {
    pre: &[
        ("Al", "high"), ("Zar", "golden"), ("Qas", "fortress"), ("Mir", "princely"),
        ("Sah", "desert"), ("Kha", "lordly"), ("Dun", "dune"), ("Azh", "burning"),
        ("Bak", "garden"), ("Tam", "palm"), ("Ras", "headland"), ("Jal", "mighty"),
        ("Nef", "soul"), ("Ash", "ashen"),
    ],
    mid: &[
        ("a", "white"), ("i", "little"), ("u", "old"), ("ara", "wandering"), ("im", "twin"),
    ],
    end: &[
        ("bar", "land"), ("dun", "hill"), ("mesh", "market"), ("ra", "sun"),
        ("sur", "wall"), ("zad", "child"), ("kar", "rock"), ("esh", "fire"),
        ("ah", "oasis"), ("iyya", "place"), ("met", "monument"),
    ],
};

pub static SYLVAN: Bank = Bank {
    pre: &[
        ("Ael", "gentle"), ("Briar", "briar"), ("Fen", "marsh"), ("Glen", "valley"),
        ("Haw", "hawthorn"), ("Lin", "linden"), ("Moss", "moss"), ("Roe", "deer"),
        ("Syl", "forest"), ("Thal", "quiet"), ("Wil", "willow"), ("Yew", "yew"),
        ("El", "elm"), ("Ash", "ash"),
    ],
    mid: &[
        ("en", "hidden"), ("or", "old"), ("a", "fair"), ("wy", "winding"),
    ],
    end: &[
        ("dell", "dale"), ("mere", "pool"), ("shade", "shade"), ("thorn", "thorn"),
        ("wick", "hamlet"), ("wood", "wood"), ("hollow", "hollow"), ("glade", "clearing"),
        ("brook", "brook"), ("leaf", "leaf"), ("run", "rill"),
    ],
};

pub static STEPPE: Bank = Bank {
    pre: &[
        ("Bor", "grey"), ("Dzun", "eastern"), ("Kesh", "swift"), ("Khar", "black"),
        ("Orda", "horde"), ("Sar", "yellow"), ("Tem", "iron"), ("Ulan", "red"),
        ("Yur", "tent"), ("Qar", "dark"), ("Bay", "rich"), ("Alta", "golden"),
        ("Ker", "wide"),
    ],
    mid: &[
        ("a", "vast"), ("u", "old"), ("ge", "little"), ("ta", "high"),
    ],
    end: &[
        ("gan", "plain"), ("tau", "mountain"), ("gol", "river"), ("bek", "lord"),
        ("sarai", "hall"), ("chi", "keeper"), ("dag", "peak"), ("kum", "sand"),
        ("kent", "city"), ("su", "water"),
    ],
};

/// A coined name and the reading of its parts: "Frostvik — 'the frost bay'".
pub struct Coined {
    pub word: String,
    pub ety: String,
}

/// Power-law weighted index (M3.2): w_i ∝ (i+1)^-0.8. The head of each
/// bank does most of the work, the tail stays rare — like real morpheme
/// frequency — while unique-name pressure still reaches the whole bank.
fn zipf_idx(rng: &mut Pcg64Mcg, n: usize) -> usize {
    let mut total = 0.0;
    for i in 0..n {
        total += 1.0 / ((i + 1) as f64).powf(0.8);
    }
    let mut u = rng.gen::<f64>() * total;
    for i in 0..n {
        u -= 1.0 / ((i + 1) as f64).powf(0.8);
        if u <= 0.0 {
            return i;
        }
    }
    n - 1
}

/// One deterministic, unique name word in the given style, with etymology.
pub fn coin(rng: &mut Pcg64Mcg, style: &str, taken: &mut HashSet<String>) -> Coined {
    let b = bank(style);
    for _ in 0..96 {
        let (p, pg) = b.pre[zipf_idx(rng, b.pre.len())];
        let mut w = String::from(p);
        let mut ety = format!("the {}", pg);
        if rng.gen::<f64>() < 0.42 {
            let (m, mg) = b.mid[zipf_idx(rng, b.mid.len())];
            w.push_str(m);
            ety = format!("the {} {}", pg, mg);
        }
        let (e, eg) = b.end[zipf_idx(rng, b.end.len())];
        w.push_str(e);
        ety.push(' ');
        ety.push_str(eg);
        if !taken.contains(&w) {
            taken.insert(w.clone());
            return Coined { word: w, ety };
        }
    }
    let (p, pg) = b.pre[0];
    let (e, eg) = b.end[0];
    let w = format!("{}{}{}", p, e, taken.len());
    taken.insert(w.clone());
    Coined { word: w, ety: format!("the {} {}", pg, eg) }
}

/// One deterministic, unique name word in the given style.
pub fn make_word(rng: &mut Pcg64Mcg, style: &str, taken: &mut HashSet<String>) -> String {
    coin(rng, style, taken).word
}

fn templates(kind: &str) -> &'static [&'static str] {
    match kind {
        "ocean" => &["The {w} Ocean", "The {w} Deep"],
        "sea" => &["Sea of {w}", "The {w} Sea", "Gulf of {w}", "The {w} Expanse"],
        "continent" => &["{w}"],
        "island" => &["Isle of {w}", "{w} Isle"],
        "archipelago" => &["The {w} Isles", "The {w} Archipelago", "The Scatter of {w}"],
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
        "bay" => &["The Bay of {w}", "{w} Bay", "The {w} Cove"],
        "strait" => &["The Straits of {w}", "The {w} Narrows", "The {w} Passage"],
        "cape" => &["Cape {w}", "The {w} Headland", "{w} Point"],
        "peak" => &["Mount {w}", "The {w} Horn", "The Spire of {w}"],
        "highland" => &["The {w} Highlands", "The {w} Plateau", "The {w} Tablelands"],
        "marsh" => &["The {w} Fen", "The {w} Marshes", "The {w} Mire"],
        "delta" => &["The {w} Delta", "The Mouths of the {w}"],
        "pass" => &["The {w} Pass", "{w} Gap", "The Gates of {w}"],
        "ford" => &["{w} Ford", "The {w} Crossing", "The Fords of {w}"],
        _ => &["{w}"],
    }
}

fn phrase(rng: &mut Pcg64Mcg, kind: &str, word: &str) -> String {
    let t = templates(kind);
    t[rng.gen_range(0..t.len())].replace("{w}", word)
}

#[derive(Serialize, Clone, Default)]
pub struct Feature {
    pub t: String,
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub size: i64,
    /// Reading of the name's parts (M3.3), e.g. "the frost bay".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ety: String,
    /// People whose tongue named it; empty = the Old Tongue.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub people: String,
    /// Exonym (M3.4): what the *other* folk across the border call it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alt: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alt_people: String,
    /// The name this feature carried before war or wear renamed it
    /// (M9.3/M9.4). Rivers never take one: hydronyms are conserved (M9.2).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub formerly: String,
}

enum Anchor<'a> {
    Interior,
    Peak(&'a Array2<f32>),
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
        let c = coin(rng, "old", taken);
        let name = phrase(rng, kind, &c.word);
        features.push(Feature {
            t: kind.to_string(),
            name,
            x: x as i64,
            y: y as i64,
            size: area as i64,
            ety: c.ety,
            ..Default::default()
        });
    }
}

/// Returns (features, world_name).
pub fn name_features(
    height: &Array2<f32>,
    biomes: &Array2<u8>,
    rivers: &Array2<bool>,
    lakes: &Array2<bool>,
    discharge: &Array2<f32>,
    tmean: &Array2<f32>,
    precip: &Array2<f32>,
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
    let comps = ndimage::top_components(&lab, 18.0 * sc, 26);
    let continents: Vec<(usize, f64)> = comps
        .iter()
        .filter(|c| c.1 >= 9000.0 * sc)
        .take(3)
        .cloned()
        .collect();
    let isles: Vec<(usize, f64)> = comps
        .iter()
        .filter(|c| c.1 < 9000.0 * sc)
        .cloned()
        .collect();

    // Archipelagos: constellations of neighbouring isles share one sweeping
    // name, and their members go unlabelled so the chart stays clean.
    fn uf_root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let anchors: Vec<(f64, f64)> = isles
        .iter()
        .map(|&(idx, _)| {
            let (y, x) = ndimage::interior_anchor(&lab, idx);
            (y as f64, x as f64)
        })
        .collect();
    let link = size as f64 * 0.085;
    let mut parent: Vec<usize> = (0..isles.len()).collect();
    for i in 0..isles.len() {
        for j in (i + 1)..isles.len() {
            let dy = anchors[i].0 - anchors[j].0;
            let dx = anchors[i].1 - anchors[j].1;
            if dy * dy + dx * dx < link * link {
                let (ri, rj) = (uf_root(&mut parent, i), uf_root(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..isles.len() {
        let r = uf_root(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut in_arch: HashSet<usize> = HashSet::new();
    for members in groups.values() {
        if members.len() < 3 {
            continue;
        }
        let total: f64 = members.iter().map(|&i| isles[i].1).sum();
        if total < 80.0 * sc {
            continue;
        }
        let cy = members.iter().map(|&i| anchors[i].0).sum::<f64>() / members.len() as f64;
        let cx = members.iter().map(|&i| anchors[i].1).sum::<f64>() / members.len() as f64;
        let c = coin(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "archipelago", &c.word);
        features.push(Feature {
            t: "archipelago".into(),
            name,
            x: cx as i64,
            y: cy as i64,
            size: total as i64,
            ety: c.ety,
            ..Default::default()
        });
        for &i in members {
            in_arch.insert(i);
        }
    }
    let islands: Vec<(usize, f64)> = isles
        .iter()
        .enumerate()
        .filter(|(i, c)| !in_arch.contains(i) && c.1 >= 60.0 * sc)
        .map(|(_, c)| *c)
        .take(8)
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

    // rivers (anchored near their strongest reach), keeping each river's word
    // so its delta can carry the same name
    let (hgt, wid) = height.dim();
    let riv_lab = ndimage::label(rivers, true);
    let riv_comps = ndimage::top_components(&riv_lab, 45.0 * sc, 10);
    let mut river_words: Vec<(usize, String, String)> = Vec::new();
    for &(idx, area) in &riv_comps {
        let (y, x) = ndimage::peak_anchor(&riv_lab, idx, discharge);
        let c = coin(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "river", &c.word);
        features.push(Feature {
            t: "river".into(), name, x: x as i64, y: y as i64,
            size: area as i64, ety: c.ety.clone(), ..Default::default()
        });
        river_words.push((idx, c.word, c.ety));
    }

    // deltas: the mightiest river mouths, named for their rivers
    let mut max_dis = 0.0f32;
    for &d in discharge.iter() {
        if d > max_dis { max_dis = d; }
    }
    let mut mouths: Vec<(f64, usize, usize, String, String)> = Vec::new();
    for (idx, word, ety) in &river_words {
        let (y0, y1, x0, x1) = riv_lab.bbox[idx - 1];
        let mut best: Option<(f64, usize, usize)> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                if riv_lab.lab[[y, x]] != *idx as i32 {
                    continue;
                }
                let mut coastal = false;
                for dy in -1isize..=1 {
                    for dx in -1isize..=1 {
                        let (ny, nx) = (y as isize + dy, x as isize + dx);
                        if ny < 0 || nx < 0 || ny >= hgt as isize || nx >= wid as isize {
                            continue;
                        }
                        if height[[ny as usize, nx as usize]] < 0.0 {
                            coastal = true;
                        }
                    }
                }
                if coastal && best.map_or(true, |(d, _, _)| discharge[[y, x]] as f64 > d) {
                    best = Some((discharge[[y, x]] as f64, y, x));
                }
            }
        }
        if let Some((d, y, x)) = best {
            mouths.push((d, y, x, word.clone(), ety.clone()));
        }
    }
    mouths.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (d, y, x, word, ety) in mouths.into_iter().take(3) {
        if d < 0.15 * max_dis as f64 {
            break;
        }
        let name = phrase(&mut rng, "delta", &word);
        features.push(Feature {
            t: "delta".into(), name, x: x as i64, y: y as i64,
            size: 14, ety, ..Default::default()
        });
    }

    let lab = ndimage::label(lakes, true);
    let comps = ndimage::top_components(&lab, 18.0 * sc, 6);
    add_features(&mut features, &mut rng, &mut taken, "lake", &lab, &comps, Anchor::Interior);

    // ---- coastal geometry: how much land surrounds each cell ----
    let land_f = land.mapv(|b| if b { 1.0 } else { 0.0 });
    let landness = ndimage::gaussian_filter(&land_f, 3.0);
    let wdist = ndimage::distance_transform_edt(&sea);

    // straits: narrow channels pinched between shores, open at both ends
    let strait_mask = Array2::from_shape_fn(height.dim(), |(y, x)| {
        sea[[y, x]] && wdist[[y, x]] <= 3.0 && landness[[y, x]] > 0.42
    });
    let slab = ndimage::label(&strait_mask, true);
    let scomps = ndimage::top_components(&slab, 10.0 * sc, 10);
    let mut strait_cells: HashSet<(usize, usize)> = HashSet::new();
    let mut straits_named = 0usize;
    for &(idx, area) in &scomps {
        if straits_named >= 4 {
            break;
        }
        let (y0, y1, x0, x1) = slab.bbox[idx - 1];
        let mut cells: Vec<(usize, usize)> = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if slab.lab[[y, x]] == idx as i32 {
                    cells.push((y, x));
                }
            }
        }
        // farthest pair of channel cells (two greedy sweeps)
        let far = |from: (usize, usize)| {
            let mut best = from;
            let mut bd = -1.0f64;
            for &c in &cells {
                let d = (c.0 as f64 - from.0 as f64).hypot(c.1 as f64 - from.1 as f64);
                if d > bd {
                    bd = d;
                    best = c;
                }
            }
            (best, bd)
        };
        let (a, _) = far(cells[0]);
        let (b, span) = far(a);
        if span < 7.0 {
            continue; // a pinprick, not a passage
        }
        // both ends must open onto free water
        let open = |p: (usize, usize)| {
            for dy in -5isize..=5 {
                for dx in -5isize..=5 {
                    let (ny, nx) = (p.0 as isize + dy, p.1 as isize + dx);
                    if ny < 0 || nx < 0 || ny >= hgt as isize || nx >= wid as isize {
                        continue;
                    }
                    let (ny, nx) = (ny as usize, nx as usize);
                    if sea[[ny, nx]] && landness[[ny, nx]] < 0.40 {
                        return true;
                    }
                }
            }
            false
        };
        if !(open(a) && open(b)) {
            continue;
        }
        for &c in &cells {
            strait_cells.insert(c);
        }
        // anchor mid-channel, snapped to the nearest water cell of the strait
        let (my, mx) = ((a.0 + b.0) as f64 / 2.0, (a.1 + b.1) as f64 / 2.0);
        let mut anchor = cells[0];
        let mut bd = f64::INFINITY;
        for &c in &cells {
            let d = (c.0 as f64 - my).hypot(c.1 as f64 - mx);
            if d < bd {
                bd = d;
                anchor = c;
            }
        }
        let c = coin(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "strait", &c.word);
        features.push(Feature {
            t: "strait".into(), name, x: anchor.1 as i64, y: anchor.0 as i64,
            size: area as i64, ety: c.ety, ..Default::default()
        });
        straits_named += 1;
    }

    // bays: sea reaching deep into the land
    let bay_mask = Array2::from_shape_fn(height.dim(), |(y, x)| {
        sea[[y, x]] && landness[[y, x]] > 0.58 && !strait_cells.contains(&(y, x))
    });
    let blab = ndimage::label(&bay_mask, true);
    let bcomps = ndimage::top_components(&blab, 14.0 * sc, 6);
    add_features(&mut features, &mut rng, &mut taken, "bay", &blab, &bcomps, Anchor::Interior);

    // capes: land thrust far out into the water, attached to a big landmass
    // (small free-standing islets are already named as islands)
    let cape_mask = Array2::from_shape_fn(height.dim(), |(y, x)| {
        land[[y, x]] && landness[[y, x]] < 0.40
    });
    let clab = ndimage::label(&cape_mask, true);
    let ccomps = ndimage::top_components(&clab, 6.0 * sc, 14);
    let llab = ndimage::label(&land, true);
    let mut capes = 0usize;
    for &(idx, area) in &ccomps {
        if capes >= 5 {
            break;
        }
        let (y, x) = ndimage::interior_anchor(&clab, idx);
        let owner = llab.lab[[y, x]];
        if owner <= 0 || llab.areas[(owner - 1) as usize] < 2500.0 * sc {
            continue;
        }
        let c = coin(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "cape", &c.word);
        features.push(Feature {
            t: "cape".into(), name, x: x as i64, y: y as i64,
            size: area as i64, ety: c.ety, ..Default::default()
        });
        capes += 1;
    }

    // lone peaks: the tallest summits, held well apart
    let maxf = ndimage::maximum_filter(height, 7);
    let mut summits: Vec<(f64, usize, usize)> = Vec::new();
    for y in 0..hgt {
        for x in 0..wid {
            if height[[y, x]] > 0.60 && (height[[y, x]] - maxf[[y, x]]).abs() < 1e-12 {
                summits.push((height[[y, x]] as f64, y, x));
            }
        }
    }
    summits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    for (_, y, x) in summits {
        if chosen.len() >= 5 {
            break;
        }
        if chosen
            .iter()
            .any(|&(cy, cx)| (cy as f64 - y as f64).hypot(cx as f64 - x as f64) < 24.0)
        {
            continue;
        }
        chosen.push((y, x));
        let c = coin(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "peak", &c.word);
        features.push(Feature {
            t: "peak".into(), name, x: x as i64, y: y as i64,
            size: 9, ety: c.ety, ..Default::default()
        });
    }

    // highlands: broad elevated country with little local relief
    let minf = ndimage::maximum_filter(&height.mapv(|h| -h), 7).mapv(|v| -v);
    let hl_mask = Array2::from_shape_fn(height.dim(), |(y, x)| {
        let h = height[[y, x]];
        h > 0.28 && h < 0.52 && (maxf[[y, x]] - minf[[y, x]]) < 0.14
    });
    let hlab = ndimage::label(&hl_mask, true);
    let hcomps = ndimage::top_components(&hlab, 260.0 * sc, 4);
    add_features(&mut features, &mut rng, &mut taken, "highland", &hlab, &hcomps, Anchor::Interior);

    // fens: low, rain-soaked ground beside fresh water
    let fresh_src = Array2::from_shape_fn(height.dim(), |(y, x)| rivers[[y, x]] || lakes[[y, x]]);
    let fresh = ndimage::binary_dilation(&fresh_src, 2);
    let marsh_mask = Array2::from_shape_fn(height.dim(), |(y, x)| {
        land[[y, x]]
            && height[[y, x]] < 0.09
            && precip[[y, x]] > 850.0
            && tmean[[y, x]] > -1.0
            && fresh[[y, x]]
    });
    let mlab = ndimage::label(&marsh_mask, true);
    let mcomps = ndimage::top_components(&mlab, 26.0 * sc, 4);
    add_features(&mut features, &mut rng, &mut taken, "marsh", &mlab, &mcomps, Anchor::Interior);

    let world_name = make_word(&mut rng, "old", &mut taken);
    (features, world_name)
}

// --- named by the roads themselves -----------------------------------------

/// Greedy placement of route-born landmarks, mightiest first.
fn place_route_marks(
    features: &mut Vec<Feature>,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    mut cands: Vec<(f64, i64, i64)>,
    kind: &str,
    cap: usize,
    min_d: f64,
) {
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let mut placed: Vec<(i64, i64)> = features
        .iter()
        .filter(|f| f.t == kind)
        .map(|f| (f.x, f.y))
        .collect();
    for (_, x, y) in cands {
        if placed.len() >= cap {
            break;
        }
        if placed
            .iter()
            .any(|&(px, py)| (((px - x) as f64).hypot((py - y) as f64)) < min_d)
        {
            continue;
        }
        let c = coin(rng, "old", taken);
        let name = phrase(rng, kind, &c.word);
        features.push(Feature {
            t: kind.to_string(),
            name,
            x,
            y,
            size: 10,
            ety: c.ety,
            ..Default::default()
        });
        placed.push((x, y));
    }
}

/// The trade roads name the land they must conquer: the high saddle a
/// caravan climbs becomes a Pass, the place it wades a great river
/// becomes a Ford. Landmarks are born from use, not decree.
pub fn name_route_features(
    features: &mut Vec<Feature>,
    rng: &mut Pcg64Mcg,
    taken: &mut HashSet<String>,
    routes: &[crate::trade::Route],
    height: &Array2<f32>,
    rivers: &Array2<bool>,
    discharge: &Array2<f32>,
) {
    let (hh, ww) = height.dim();
    let mut pass_cands: Vec<(f64, i64, i64)> = Vec::new();
    let mut ford_cands: Vec<(f64, i64, i64)> = Vec::new();
    for r in routes {
        let n = r.path.len();
        if n < 5 {
            continue;
        }
        let mut best_pass: Option<(f64, i64, i64)> = None;
        let mut best_ford: Option<(f64, i64, i64)> = None;
        for (i, pt) in r.path.iter().enumerate() {
            if i < 2 || i + 2 >= n {
                continue; // never at the town gates themselves
            }
            if r.m.get(i).copied().unwrap_or(0) != crate::trade::MODE_LAND {
                continue;
            }
            let (x, y) = (pt[0], pt[1]);
            if x < 0 || y < 0 || x >= ww as i64 || y >= hh as i64 {
                continue;
            }
            let h = height[[y as usize, x as usize]] as f64;
            if h > 0.45 && best_pass.map_or(true, |(bh, _, _)| h > bh) {
                best_pass = Some((h, x, y));
            }
            let mut dmax = 0.0f64;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= ww as i64 || ny >= hh as i64 {
                        continue;
                    }
                    let (nxu, nyu) = (nx as usize, ny as usize);
                    if rivers[[nyu, nxu]] {
                        dmax = dmax.max(discharge[[nyu, nxu]] as f64);
                    }
                }
            }
            if dmax > 60.0 && best_ford.map_or(true, |(bd, _, _)| dmax > bd) {
                best_ford = Some((dmax, x, y));
            }
        }
        if let Some(c) = best_pass {
            pass_cands.push(c);
        }
        if let Some(c) = best_ford {
            ford_cands.push(c);
        }
    }
    place_route_marks(features, rng, taken, pass_cands, "pass", 5, 18.0);
    place_route_marks(features, rng, taken, ford_cands, "ford", 6, 14.0);
}

// ---------------------------------------------------------------------------
// M3.1 + M3.4 — culture-styled toponyms & border exonyms
// ---------------------------------------------------------------------------

/// Per-culture formation strategies: how each tongue builds a feature name
/// from a coined word. Falls back to the Old-Tongue generics when a style
/// has no habit for that kind of place.
fn styled_templates(style: &str, kind: &str) -> Option<&'static [&'static str]> {
    Some(match (style, kind) {
        // nordic: hard compounds, the word swallows the generic
        ("nordic", "range") => &["The {w}fell", "The {w} Fells"],
        ("nordic", "peak") => &["{w}tind", "The Horn of {w}"],
        ("nordic", "bay") => &["{w}vik", "The {w}fjord"],
        ("nordic", "island") => &["{w}holm", "{w}oy"],
        ("nordic", "archipelago") => &["The {w} Skerries"],
        ("nordic", "forest") => &["The {w}skog"],
        ("nordic", "lake") => &["{w}vatn"],
        ("nordic", "marsh") => &["The {w}myr"],
        ("nordic", "highland") => &["The {w}vidda"],
        ("nordic", "cape") => &["{w}nes"],
        ("nordic", "strait") => &["The {w}sund"],
        ("nordic", "pass") => &["The {w}skard"],
        ("nordic", "ford") => &["{w}vad"],
        // hellenic: classical constructions, generic leads
        ("hellenic", "range") => &["The Mountains of {w}", "The {w} Oros"],
        ("hellenic", "peak") => &["Mount {w}", "The Throne of {w}"],
        ("hellenic", "bay") => &["The Gulf of {w}"],
        ("hellenic", "island") => &["The Isle of {w}"],
        ("hellenic", "archipelago") => &["The {w}ades"],
        ("hellenic", "forest") => &["The Sacred Wood of {w}"],
        ("hellenic", "lake") => &["Lake {w}"],
        ("hellenic", "marsh") => &["The {w} Marsh"],
        ("hellenic", "highland") => &["The {w} Plateau"],
        ("hellenic", "cape") => &["Cape {w}"],
        ("hellenic", "strait") => &["The Straits of {w}"],
        ("hellenic", "pass") => &["The Gates of {w}"],
        ("hellenic", "ford") => &["The Crossing of {w}"],
        // arid: construct-state possessives, wells and walls
        ("arid", "range") => &["The Wall of {w}", "The {w} Jabals"],
        ("arid", "peak") => &["The Spire of {w}"],
        ("arid", "bay") => &["The Anchorage of {w}"],
        ("arid", "island") => &["The Isle of {w}"],
        ("arid", "archipelago") => &["The Scatter of {w}"],
        ("arid", "desert") => &["The {w} Erg", "The Anvil of {w}"],
        ("arid", "forest") => &["The Groves of {w}"],
        ("arid", "lake") => &["The Mirror of {w}"],
        ("arid", "marsh") => &["The Reeds of {w}"],
        ("arid", "highland") => &["The {w} Tableland"],
        ("arid", "cape") => &["The Horn of {w}"],
        ("arid", "strait") => &["The Gate of {w}"],
        ("arid", "pass") => &["The Wells of {w}"],
        ("arid", "ford") => &["The Wading of {w}"],
        // sylvan: soft, lowercase-hearted places
        ("sylvan", "range") => &["The {w} Downs"],
        ("sylvan", "peak") => &["The {w} Tor"],
        ("sylvan", "bay") => &["The {w} Cove"],
        ("sylvan", "island") => &["The {w} Holt"],
        ("sylvan", "archipelago") => &["The {w} Eyots"],
        ("sylvan", "forest") => &["The {w}wood", "The Deep of {w}"],
        ("sylvan", "lake") => &["The {w} Mere"],
        ("sylvan", "marsh") => &["The {w} Carr"],
        ("sylvan", "highland") => &["The {w} Wolds"],
        ("sylvan", "cape") => &["The {w} Hook"],
        ("sylvan", "strait") => &["The {w} Race"],
        ("sylvan", "pass") => &["The {w} Gap"],
        ("sylvan", "ford") => &["The {w} Stepping"],
        // steppe: sky-wide compounds
        ("steppe", "range") => &["The {w} Tau"],
        ("steppe", "peak") => &["{w} Dag"],
        ("steppe", "bay") => &["The {w} Reach"],
        ("steppe", "island") => &["{w} Aral"],
        ("steppe", "archipelago") => &["The {w} Scatter"],
        ("steppe", "desert") => &["The {w} Kum"],
        ("steppe", "forest") => &["The {w} Thicket"],
        ("steppe", "lake") => &["{w} Nor"],
        ("steppe", "marsh") => &["The {w} Sink"],
        ("steppe", "highland") => &["The {w} Steppe"],
        ("steppe", "cape") => &["The {w} Point"],
        ("steppe", "strait") => &["The {w} Throat"],
        ("steppe", "pass") => &["The {w} Saddle"],
        ("steppe", "ford") => &["The {w} Wade"],
        _ => return None,
    })
}

/// Build a feature name in a culture's style, falling back to generics.
pub fn styled_phrase(rng: &mut Pcg64Mcg, style: &str, kind: &str, word: &str) -> String {
    if let Some(t) = styled_templates(style, kind) {
        t[rng.gen_range(0..t.len())].replace("{w}", word)
    } else {
        phrase(rng, kind, word)
    }
}

/// How far a people's tongue carries from its towns, in cells (~4 km each).
pub const TONGUE_REACH: f64 = 30.0;

/// M3.1/M3.4 — the peoples lay their own names over the land they live in.
/// Features near a culture's towns are re-named in that culture's style
/// (with etymology kept); features that two peoples both live beside keep
/// an exonym from the second tongue. Oceans, seas, continents, rivers and
/// deltas keep their Old-Tongue names — water and the primordial world are
/// named once and conservatively (anticipating M9.2).
pub fn culture_toponyms(
    features: &mut [Feature],
    settlements: &[crate::settlements::Settlement],
    cultures: &[crate::culture::Culture],
    taken: &mut HashSet<String>,
    seed: i64,
) {
    if cultures.is_empty() || settlements.is_empty() {
        return;
    }
    let mut rng = crate::util::rng(seed + 13000);
    for f in features.iter_mut() {
        match f.t.as_str() {
            "ocean" | "sea" | "continent" | "river" | "delta" => continue,
            _ => {}
        }
        // nearest town of each culture
        let mut best: Vec<(f64, usize)> = vec![(f64::INFINITY, 0); cultures.len()];
        for s in settlements {
            let d = (s.x - f.x) as f64;
            let e = (s.y - f.y) as f64;
            let d2 = d * d + e * e;
            if d2 < best[s.culture.idx()].0 {
                best[s.culture.idx()] = (d2, s.culture.idx());
            }
        }
        let mut near: Vec<(f64, usize)> = best
            .into_iter()
            .enumerate()
            .filter(|(_, (d2, _))| d2.is_finite())
            .map(|(ci, (d2, _))| (d2, ci))
            .collect();
        near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // the namer must live close; far wilds keep the Old Tongue
        if near.is_empty() || near[0].0.sqrt() > TONGUE_REACH {
            continue;
        }
        // endonym: the closest people re-name it in their own tongue
        let cu = &cultures[near[0].1];
        let c = coin(&mut rng, &cu.style, taken);
        f.name = styled_phrase(&mut rng, &cu.style, &f.t, &c.word);
        f.ety = c.ety;
        f.people = cu.people.clone();
        // exonym: a second people keeps its own word for it. A mountain is
        // seen and spoken of from further off than it is farmed, so the
        // border partner's reach runs wider than the namer's.
        if near.len() > 1 && near[1].0.sqrt() <= TONGUE_REACH * 1.8 {
            let other = &cultures[near[1].1];
            let oc = coin(&mut rng, &other.style, taken);
            f.alt = styled_phrase(&mut rng, &other.style, &f.t, &oc.word);
            f.alt_people = other.people.clone();
        }
    }
}

/// M3.4, the slow half — tongues catch up with the map. As peoples spread,
/// a feature named at the dawn comes within reach of a second people, who
/// keep their own word for it. This pass only ADDS exonyms: the map's
/// first names are conservative and stand (renaming under conquest is
/// M9.2's affair). Returns (feature name, other people, alt name) per
/// new doubling, for the chronicle to speak of.
pub fn exonym_pass(
    features: &mut [Feature],
    settlements: &[crate::settlements::Settlement],
    cultures: &[crate::culture::Culture],
    taken: &mut HashSet<String>,
    rng: &mut Pcg64Mcg,
) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if cultures.len() < 2 || settlements.is_empty() {
        return out;
    }
    for f in features.iter_mut() {
        if !f.alt.is_empty() {
            continue;
        }
        match f.t.as_str() {
            "ocean" | "sea" | "continent" | "river" | "delta" => continue,
            _ => {}
        }
        let mut best: Vec<f64> = vec![f64::INFINITY; cultures.len()];
        for s in settlements {
            let dx = (s.x - f.x) as f64;
            let dy = (s.y - f.y) as f64;
            let d2 = dx * dx + dy * dy;
            if d2 < best[s.culture.idx()] {
                best[s.culture.idx()] = d2;
            }
        }
        let mut near: Vec<(f64, usize)> = best
            .into_iter()
            .enumerate()
            .filter(|(_, d2)| d2.is_finite())
            .map(|(ci, d2)| (d2, ci))
            .collect();
        near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // the feature must sit in somebody's spoken-of country — wild
        // Old-Tongue land far from every hearth keeps its single name
        if near.is_empty() || near[0].0.sqrt() > TONGUE_REACH {
            continue;
        }
        // and a second people must live near enough to speak of it
        let other = near.iter().find(|(d2, ci)| {
            cultures[*ci].people != f.people && d2.sqrt() <= TONGUE_REACH * 1.8
        });
        let Some(&(_, oi)) = other else { continue };
        let cu = &cultures[oi];
        let oc = coin(rng, &cu.style, taken);
        f.alt = styled_phrase(rng, &cu.style, &f.t, &oc.word);
        f.alt_people = cu.people.clone();
        out.push((f.name.clone(), cu.people.clone(), f.alt.clone()));
    }
    out
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E11.6): the tongue must own its toponyms.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band { name: "toponyms classify to culture", sweet: (0.9, 1.0), hard: (0.8, 1.0), target: "M3 gate: sampled toponyms ≥ 90%" },
];
