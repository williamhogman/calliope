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
    tmean: &Array2<f64>,
    precip: &Array2<f64>,
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
        let word = make_word(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "archipelago", &word);
        features.push(Feature {
            t: "archipelago".into(),
            name,
            x: cx as i64,
            y: cy as i64,
            size: total as i64,
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
    let mut river_words: Vec<(usize, String)> = Vec::new();
    for &(idx, area) in &riv_comps {
        let (y, x) = ndimage::peak_anchor(&riv_lab, idx, discharge);
        let word = make_word(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "river", &word);
        features.push(Feature { t: "river".into(), name, x: x as i64, y: y as i64, size: area as i64 });
        river_words.push((idx, word));
    }

    // deltas: the mightiest river mouths, named for their rivers
    let mut max_dis = 0.0f64;
    for &d in discharge.iter() {
        if d > max_dis { max_dis = d; }
    }
    let mut mouths: Vec<(f64, usize, usize, String)> = Vec::new();
    for (idx, word) in &river_words {
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
                if coastal && best.map_or(true, |(d, _, _)| discharge[[y, x]] > d) {
                    best = Some((discharge[[y, x]], y, x));
                }
            }
        }
        if let Some((d, y, x)) = best {
            mouths.push((d, y, x, word.clone()));
        }
    }
    mouths.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (d, y, x, word) in mouths.into_iter().take(3) {
        if d < 0.15 * max_dis {
            break;
        }
        let name = phrase(&mut rng, "delta", &word);
        features.push(Feature { t: "delta".into(), name, x: x as i64, y: y as i64, size: 14 });
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
        let word = make_word(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "strait", &word);
        features.push(Feature { t: "strait".into(), name, x: anchor.1 as i64, y: anchor.0 as i64, size: area as i64 });
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
        let word = make_word(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "cape", &word);
        features.push(Feature { t: "cape".into(), name, x: x as i64, y: y as i64, size: area as i64 });
        capes += 1;
    }

    // lone peaks: the tallest summits, held well apart
    let maxf = ndimage::maximum_filter(height, 7);
    let mut summits: Vec<(f64, usize, usize)> = Vec::new();
    for y in 0..hgt {
        for x in 0..wid {
            if height[[y, x]] > 0.60 && (height[[y, x]] - maxf[[y, x]]).abs() < 1e-12 {
                summits.push((height[[y, x]], y, x));
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
        let word = make_word(&mut rng, "old", &mut taken);
        let name = phrase(&mut rng, "peak", &word);
        features.push(Feature { t: "peak".into(), name, x: x as i64, y: y as i64, size: 9 });
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
        let word = make_word(rng, "old", taken);
        let name = phrase(rng, kind, &word);
        features.push(Feature {
            t: kind.to_string(),
            name,
            x,
            y,
            size: 10,
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
    height: &Array2<f64>,
    rivers: &Array2<bool>,
    discharge: &Array2<f64>,
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
            let h = height[[y as usize, x as usize]];
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
                        dmax = dmax.max(discharge[[nyu, nxu]]);
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
