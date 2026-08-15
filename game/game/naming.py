"""Toponymy: detect geographic features and name them.

The map should explain itself: the ocean, seas, continents, isles,
mountain ranges, deserts, forests, rivers and lakes all get names drawn
from small syllable banks. Geography speaks the "old tongue" (one bank
for the whole world) while each culture names its own settlements — see
culture.py.
"""

import numpy as np
from scipy import ndimage

from game import constants as gc

SYLLABLES = {
    "old": {
        "pre": ["Aur", "Bel", "Cal", "Dor", "El", "Far", "Gal", "Hal", "Ith",
                "Kar", "Lor", "Mal", "Nor", "Or", "Pel", "Quel", "Ser", "Tal",
                "Um", "Vor", "Yl", "Zar"],
        "mid": ["a", "e", "i", "o", "u", "ae", "ia", "or", "an", "el", "ar"],
        "end": ["ath", "dor", "eth", "ia", "ion", "mar", "nor", "os", "rin",
                "thas", "um", "wyn", "ys"],
    },
    "hellenic": {
        "pre": ["Kal", "Thes", "Ery", "Del", "Ar", "Kor", "Pel", "Nax", "Ida",
                "Olyn", "Thra", "Mel", "Or", "Phi", "Xan", "Hel", "Leu", "Myr"],
        "mid": ["li", "ra", "do", "ka", "the", "mo", "sy", "le", "ei", "an"],
        "end": ["opia", "ossa", "ene", "ikos", "antheia", "polis", "ion",
                "aia", "yra", "anthe", "eia", "os"],
    },
    "nordic": {
        "pre": ["Skjal", "Thor", "Ulf", "Bryn", "Eir", "Frost", "Hav", "Jor",
                "Kald", "Nor", "Sten", "Varg", "Hrim", "Grim", "Odd", "Sol"],
        "mid": ["a", "e", "en", "ar", "ur"],
        "end": ["vik", "heim", "gard", "stad", "berg", "dal", "mark", "nes",
                "holm", "fell", "strand"],
    },
    "arid": {
        "pre": ["Al", "Zar", "Qas", "Mir", "Sah", "Kha", "Dun", "Azh", "Bak",
                "Tam", "Ras", "Jal", "Nef", "Ash"],
        "mid": ["a", "i", "u", "ara", "im"],
        "end": ["bar", "dun", "mesh", "ra", "sur", "zad", "kar", "esh", "ah",
                "iyya", "met"],
    },
    "sylvan": {
        "pre": ["Ael", "Briar", "Fen", "Glen", "Haw", "Lin", "Moss", "Roe",
                "Syl", "Thal", "Wil", "Yew", "El", "Ash"],
        "mid": ["en", "or", "a", "wy"],
        "end": ["dell", "mere", "shade", "thorn", "wick", "wood", "hollow",
                "glade", "brook", "leaf", "run"],
    },
    "steppe": {
        "pre": ["Bor", "Dzun", "Kesh", "Khar", "Orda", "Sar", "Tem", "Ulan",
                "Yur", "Qar", "Bay", "Alta", "Ker"],
        "mid": ["a", "u", "ge", "ta"],
        "end": ["gan", "tau", "gol", "bek", "sarai", "chi", "dag", "kum",
                "kent", "su"],
    },
}


def make_word(rng, style, taken):
    """One deterministic, unique name word in the given style."""
    bank = SYLLABLES.get(style, SYLLABLES["old"])
    for _ in range(96):
        parts = [bank["pre"][int(rng.integers(len(bank["pre"])))]]
        if rng.random() < 0.42:
            parts.append(bank["mid"][int(rng.integers(len(bank["mid"])))])
        parts.append(bank["end"][int(rng.integers(len(bank["end"])))])
        w = "".join(parts)
        if w not in taken:
            taken.add(w)
            return w
    w = f"{bank['pre'][0]}{len(taken)}"
    taken.add(w)
    return w


_TEMPLATES = {
    "ocean": ["The {w} Ocean", "The {w} Deep"],
    "sea": ["Sea of {w}", "The {w} Sea", "Gulf of {w}", "The {w} Expanse"],
    "continent": ["{w}"],
    "island": ["Isle of {w}", "{w} Isle"],
    "range": ["The {w} Mountains", "The {w} Range", "The Peaks of {w}",
              "The {w} Reach"],
    "desert": ["The {w} Desert", "The {w} Wastes", "The Sands of {w}"],
    "forest": ["The {w}wood", "{w} Forest", "The Woods of {w}"],
    "river": ["River {w}", "The {w}"],
    "lake": ["Lake {w}", "The {w} Mere"],
}


def _phrase(rng, kind, word):
    t = _TEMPLATES[kind]
    return t[int(rng.integers(len(t)))].format(w=word)


def _interior_anchor(lab, idx, slices):
    """Point deepest inside the component — labels land inside their shape.

    The mask is zero-padded so map edges count as boundaries; otherwise the
    ocean's anchor lands on the border of the map and the label gets cut off.
    """
    sl = slices[idx - 1]
    m = np.pad(lab[sl] == idx, 1)
    d = ndimage.distance_transform_edt(m)[1:-1, 1:-1]
    y, x = np.unravel_index(int(np.argmax(d)), d.shape)
    return int(y + sl[0].start), int(x + sl[1].start)


def _components(mask, min_area, cap, structure=None):
    lab, n = ndimage.label(mask, structure=structure)
    if n == 0:
        return lab, []
    areas = ndimage.sum_labels(np.ones_like(lab), lab, index=np.arange(1, n + 1))
    order = np.argsort(areas)[::-1]
    keep = [(int(i + 1), float(areas[i])) for i in order if areas[i] >= min_area]
    return lab, keep[:cap]


_S8 = np.ones((3, 3), dtype=bool)


def name_features(world, seed):
    """Returns (features, world_name). Feature: {t, name, x, y, size}."""
    rng = np.random.default_rng(seed + 12000)
    taken = set()
    h = world["height"]
    biomes = world["biomes"]
    rivers = world["rivers"]
    lakes = world["lakes"]
    discharge = world["discharge"]
    size = h.shape[0]
    sc = (size / 512.0) ** 2
    features = []

    def add(kind, lab, comps, anchor=None):
        slices = ndimage.find_objects(lab)
        for idx, area in comps:
            if anchor is None:  # deepest interior point
                y, x = _interior_anchor(lab, idx, slices)
            else:  # peak of a field
                sl = slices[idx - 1]
                masked = np.where(lab[sl] == idx, anchor[sl], -np.inf)
                y, x = np.unravel_index(int(np.argmax(masked)), masked.shape)
                y, x = int(y + sl[0].start), int(x + sl[1].start)
            features.append({
                "t": kind,
                "name": _phrase(rng, kind, make_word(rng, "old", taken)),
                "x": int(x), "y": int(y), "size": int(area),
            })

    # ocean & seas
    sea = h < 0
    lab, comps = _components(sea, 900 * sc, 7, structure=_S8)
    if comps:
        biggest, rest = comps[0], comps[1:]
        add("ocean" if biggest[1] >= 15000 * sc else "sea", lab, [biggest])
        add("sea", lab, rest)

    # continents & islands
    lab, comps = _components(~sea, 60 * sc, 12, structure=_S8)
    add("continent", lab, [c for c in comps if c[1] >= 9000 * sc][:3])
    add("island", lab, [c for c in comps if c[1] < 9000 * sc][:8])

    # mountain ranges (dilated so nearby ridges merge into one range)
    peaks = h > 0.52
    merged = ndimage.binary_dilation(peaks, iterations=2)
    lab, comps = _components(merged, 45 * sc, 8, structure=_S8)
    add("range", lab, comps, anchor=h)

    # deserts & forests
    lab, comps = _components(biomes == gc.DESERT, 260 * sc, 5, structure=_S8)
    add("desert", lab, comps)
    forest = np.isin(biomes, [gc.WOODLAND, gc.SEASONAL_RAIN_FOREST,
                              gc.TEMPERATE_RAIN_FOREST, gc.BOREAL_FOREST,
                              gc.TROPICAL_RAIN_FOREST])
    lab, comps = _components(forest, 450 * sc, 7, structure=_S8)
    add("forest", lab, comps)

    # rivers (anchored near their strongest reach) & lakes
    lab, comps = _components(rivers, 45 * sc, 10, structure=_S8)
    add("river", lab, comps, anchor=discharge)
    lab, comps = _components(lakes, 18 * sc, 6, structure=_S8)
    add("lake", lab, comps)

    world_name = make_word(rng, "old", taken)
    return features, world_name
