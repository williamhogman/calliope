"""Resources — the triples ontology from resources.hy, ported and completed.

Relations are stored exactly as before: universe[(relation, ':right', a)] = b
and universe[(relation, ':left', b)] = a, built from (subjects, REL, objects)
triples, with ISA closure walked via game.utils.bf_traverse (the original
apply-kleene*). The original data had typos (ADBUNDANCE, montain, a dangling
"common" row); those are fixed and FOUNDIN is now fully specified so
resources actually appear in the world.
"""

from itertools import product

import numpy as np

from game import constants as gc
from game import utils
from game.noisegen import Perlin3


def _ensure_col(x):
    return x if isinstance(x, (list, tuple)) else [x]


def to_triple(universe, entry):
    a_s, rel, b_s = entry
    for a, b in product(_ensure_col(a_s), _ensure_col(b_s)):
        universe[(rel, ":right", a)] = b
        universe[(rel, ":left", b)] = a
    return universe


def triples(*entries):
    universe = {}
    for entry in entries:
        to_triple(universe, entry)
    return universe


RESOURCES = triples(
    ("bananas", "ISA", "fruit"),

    (["blueberries", "strawberries", "blackberries"], "ISA", "berry"),
    ("berry", "ISA", "fruit"),
    ("berry", "REQUIRES", "gathering"),
    ("fruit", "ISA", "food"),

    (["cattle", "sheep", "horse", "pig"], "ISA", "livestock"),
    (["deer", "elk"], "ISA", "game"),
    (["livestock", "game"], "ISA", "animal"),
    ("animal", "ISA", "food"),

    ("fish", "ISA", "food"),
    ("fish", "REQUIRES", "fishing"),

    ("timber", "ISA", "material"),
    ("stone", "ISA", "material"),
    ("coal", "ISA", "fuel"),

    (["copper", "silver", "gold", "iron", "mithril"], "ISA", "metal"),
    ("metal", "ISA", "material"),
    ("metal", "REQUIRES", "metal-working"),
    ("iron", "REQUIRES", "iron-working"),
    ("mithril", "REQUIRES", "mithril-smithing"),

    (["coal", "copper", "iron", "stone", "timber"], "ABUNDANCE", "common"),
    (["silver", "cattle", "horse"], "ABUNDANCE", "uncommon"),
    ("gold", "ABUNDANCE", "rare"),
    ("mithril", "ABUNDANCE", "legendary"),

    ("metal", "FOUNDIN", "mountain"),
)


def apply_kleene_star(universe, relation, origin):
    """Transitive closure along a relation — the original apply-kleene*."""
    return utils.bf_traverse(
        lambda cur: universe.get((relation, ":right", cur)), origin)


def isa_chain(name):
    return list(apply_kleene_star(RESOURCES, "ISA", name))


def requires(name):
    """First REQUIRES found walking up the ISA chain."""
    for step in isa_chain(name):
        req = RESOURCES.get(("REQUIRES", ":right", step))
        if req:
            return req
    return None


def abundance(name):
    for step in isa_chain(name):
        ab = RESOURCES.get(("ABUNDANCE", ":right", step))
        if ab:
            return ab
    return "common"


def category(name):
    chain = isa_chain(name)
    for top in ("food", "material", "fuel"):
        if top in chain:
            return top
    return chain[-1] if len(chain) > 1 else "misc"


# --- placement ---------------------------------------------------------

_ABUNDANCE_QUANTILE = {"common": 0.945, "uncommon": 0.975, "rare": 0.988, "legendary": 0.9965}

_DISPLAY_COLORS = {
    "bananas": "#f5d442", "blueberries": "#5b6ee1", "strawberries": "#e4485b",
    "blackberries": "#6b3fa0", "cattle": "#c98d5a", "sheep": "#e8e2d0",
    "horse": "#a9754f", "pig": "#e0a3a3", "deer": "#b08968", "elk": "#8a6f52",
    "fish": "#7fd4e8", "timber": "#4f8f3a", "stone": "#9aa2ad", "coal": "#3a3f46",
    "copper": "#d97742", "silver": "#c8d0da", "gold": "#f2c14e", "iron": "#8f4f38",
    "mithril": "#8ef0e2",
}


def _biome_mask(biomes, ids):
    m = np.zeros(biomes.shape, dtype=bool)
    for b in ids:
        m |= biomes == b
    return m


def _suitability(name, world):
    """Boolean mask of cells where a resource can occur."""
    b = world["biomes"]
    h = world["height"]
    rivers = world["rivers"]
    lakes = world["lakes"]
    land = h >= 0

    forests = _biome_mask(b, [gc.WOODLAND, gc.SEASONAL_RAIN_FOREST,
                              gc.TEMPERATE_RAIN_FOREST, gc.BOREAL_FOREST,
                              gc.TROPICAL_RAIN_FOREST])
    mountains = land & (h > 0.5)
    hills = land & (h > 0.3) & (h <= 0.6)

    table = {
        "bananas": _biome_mask(b, [gc.TROPICAL_RAIN_FOREST]),
        "blueberries": _biome_mask(b, [gc.BOREAL_FOREST, gc.TUNDRA]),
        "strawberries": _biome_mask(b, [gc.GRASSLAND, gc.WOODLAND]),
        "blackberries": _biome_mask(b, [gc.WOODLAND, gc.SEASONAL_RAIN_FOREST]),
        "cattle": _biome_mask(b, [gc.GRASSLAND]),
        "sheep": _biome_mask(b, [gc.GRASSLAND, gc.TUNDRA]) | hills,
        "horse": _biome_mask(b, [gc.GRASSLAND, gc.SAVANNA]),
        "pig": _biome_mask(b, [gc.WOODLAND, gc.SEASONAL_RAIN_FOREST]),
        "deer": _biome_mask(b, [gc.WOODLAND, gc.SEASONAL_RAIN_FOREST, gc.TEMPERATE_RAIN_FOREST]),
        "elk": _biome_mask(b, [gc.BOREAL_FOREST, gc.TUNDRA]),
        "fish": _coastal(world) | rivers | lakes,
        "timber": forests,
        "stone": mountains,
        "coal": hills,
        "copper": land & (h > 0.45),
        "iron": land & (h > 0.45),
        "silver": land & (h > 0.55),
        "gold": (land & (h > 0.6)) | (rivers & (h > 0.35)),  # veins + placer
        "mithril": land & (h > 0.8),
    }
    return table[name]


def _coastal(world):
    h = world["height"]
    sea = h < 0
    land = ~sea
    coast = np.zeros_like(sea)
    coast[:-1, :] |= sea[:-1, :] & land[1:, :]
    coast[1:, :] |= sea[1:, :] & land[:-1, :]
    coast[:, :-1] |= sea[:, :-1] & land[:, 1:]
    coast[:, 1:] |= sea[:, 1:] & land[:, :-1]
    return coast


ALL_PLACEABLE = ["bananas", "blueberries", "strawberries", "blackberries",
                 "cattle", "sheep", "horse", "pig", "deer", "elk", "fish",
                 "timber", "stone", "coal", "copper", "iron", "silver",
                 "gold", "mithril"]


def place_resources(world, seed):
    """Returns a list of deposits: {r, x, y, rich} thinned to local maxima."""
    size = world["height"].shape[0]
    half = size // 2
    yy, xx = np.mgrid[0:half, 0:half].astype(np.float64)
    deposits = []
    from scipy import ndimage

    noise = Perlin3(seed + 5000)
    for i, name in enumerate(ALL_PLACEABLE):
        mask = _suitability(name, world)
        if not mask.any():
            continue
        # noise evaluated at half resolution, upsampled — 4x faster
        small = noise.fbm(xx / half * 11.0, yy / half * 11.0,
                          np.full_like(xx, 1.7 + i * 0.61), octaves=3)
        field = np.repeat(np.repeat(small, 2, axis=0), 2, axis=1)[:size, :size]
        vals = field[mask]
        q = _ABUNDANCE_QUANTILE[abundance(name)]
        thresh = np.quantile(vals, q)
        hot = mask & (field >= thresh)
        # thin to local maxima so deposits are point-like; deterministic jitter
        # breaks ties on the 2x2 plateaus the upsampling creates, so each
        # 5x5 window yields exactly one deposit instead of a duplicate cluster
        rng = np.random.default_rng(seed * 31 + i)
        fj = field + rng.random(field.shape) * 1e-6
        maxima = fj == ndimage.maximum_filter(fj, size=5)
        spots = hot & maxima
        ys, xs = np.where(spots)
        lo, hi = float(field.min()), float(field.max())
        for y, x in zip(ys.tolist(), xs.tolist()):
            rich = (float(field[y, x]) - lo) / max(hi - lo, 1e-9)
            deposits.append({"r": name, "x": int(x), "y": int(y),
                             "rich": round(0.35 + 0.65 * rich, 2)})
    return deposits


def resource_meta():
    return {
        name: {
            "category": category(name),
            "abundance": abundance(name),
            "requires": requires(name),
            "isa": isa_chain(name)[1:],
            "color": _DISPLAY_COLORS.get(name, "#cccccc"),
        }
        for name in ALL_PLACEABLE
    }
