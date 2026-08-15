"""Settlements: founding, monthly growth, territory, events."""

import numpy as np
from scipy import ndimage

from game import constants as gc
from game import climate

TIERS = [(0, "Camp"), (250, "Village"), (1000, "Town"), (5000, "City")]

_PREFIX = ["Kal", "Thes", "Ery", "Del", "Ar", "Kor", "Pel", "Nax", "Ida",
           "Olyn", "Thra", "Mel", "Or", "Phi", "Xan", "Hel", "Leu", "Myr"]
_MID = ["li", "ra", "do", "ka", "the", "mo", "sy", "le", "ei", "an"]
_SUFFIX = ["opia", "ossa", "ene", "ikos", "antheia", "polis", "ion", "aia",
           "yra", "anthe", "eia", "os"]


def _tier(pop):
    name = TIERS[0][1]
    for threshold, t in TIERS:
        if pop >= threshold:
            name = t
    return name


def _make_name(rng, taken):
    for _ in range(64):
        parts = [rng.choice(_PREFIX)]
        if rng.random() < 0.55:
            parts.append(rng.choice(_MID))
        parts.append(rng.choice(_SUFFIX))
        name = "".join(parts)
        if name not in taken:
            taken.add(name)
            return name
    return f"Kalliope{len(taken)}"


def _adjacency(mask):
    return ndimage.binary_dilation(mask, iterations=2)


def found_settlements(world, seed):
    """Score cells and greedily found settlements with min spacing."""
    h = world["height"]
    biomes = world["biomes"]
    tmean = world["tmean"]
    rivers = world["rivers"]
    lakes = world["lakes"]
    deposits = world["deposits"]
    size = h.shape[0]
    land = h >= 0

    sea = h < 0
    coast = land & _adjacency(sea)
    near_fresh = land & (_adjacency(rivers) | _adjacency(lakes))

    # food kernel from deposits whose ISA chain reaches "food"
    from game.resources import isa_chain
    food = np.zeros_like(h)
    for d in deposits:
        if "food" in isa_chain(d["r"]):
            food[d["y"], d["x"]] += d["rich"]
    food = ndimage.gaussian_filter(food, sigma=5.0) * 60.0

    comfort = np.exp(-((tmean - 12.0) / 14.0) ** 2)

    score = (
        2.2 * near_fresh.astype(float)
        + 1.6 * coast.astype(float)
        + np.clip(food, 0, 3.0)
        + 2.0 * comfort
        - 2.5 * (biomes == gc.DESERT)
        - 3.5 * (biomes == gc.ICE)
        - 1.5 * (biomes == gc.TUNDRA)
        - 2.0 * np.clip(h - 0.5, 0, 1) * 4.0
    )
    score[~land] = -1e9

    rng = np.random.default_rng(seed + 9000)
    taken_names = set()
    settlements = []
    working = score.copy()
    n_target = max(6, size // 32)
    min_dist = size / 18.0
    yy, xx = np.mgrid[0:size, 0:size]

    for i in range(n_target * 3):
        if len(settlements) >= n_target:
            break
        idx = int(np.argmax(working))
        y, x = divmod(idx, size)
        if working[y, x] < 2.0:
            break
        pop = int(rng.integers(40, 140))
        settlements.append({
            "id": len(settlements),
            "name": _make_name(rng, taken_names),
            "x": int(x), "y": int(y),
            "pop": pop,
            "tier": _tier(pop),
            "food": round(float(np.clip(food[y, x], 0.2, 3.0) + 1.4 * near_fresh[y, x] + coast[y, x]), 2),
            "coastal": bool(coast[y, x]),
            "river": bool(near_fresh[y, x]),
        })
        working[(yy - y) ** 2 + (xx - x) ** 2 < min_dist ** 2] = -1e9

    return settlements


def _capacity(s):
    return 900.0 * max(s["food"], 0.3)


def tick_settlements(world, month_abs, rng):
    """One month of growth; returns events."""
    events = []
    tmean = world["tmean"]
    tamp = world["tamp"]
    month = month_abs % 12
    for s in world["settlements"]:
        t_now = float(climate.month_temperature(
            tmean[s["y"], s["x"]], tamp[s["y"], s["x"]], month))
        r = 0.014
        if t_now < -8.0:
            r *= 0.25
        elif t_now < 0.0:
            r *= 0.6
        r *= 1.0 + 0.04 * min(s.get("connections", 0), 4)  # trade bonus
        k = _capacity(s)
        pop = s["pop"]
        growth = pop * r * (1.0 - pop / k)
        # harsh winter shock
        if t_now < -14.0 and rng.random() < 0.10 and pop > 60:
            loss = int(pop * rng.uniform(0.02, 0.06))
            pop -= loss
            events.append({"m": month_abs, "s": s["name"],
                           "text": f"A brutal winter grips {s['name']} — {loss} lost."})
        pop = max(20, int(round(pop + growth)))
        old_tier = s["tier"]
        s["pop"] = pop
        s["tier"] = _tier(pop)
        if s["tier"] != old_tier:
            events.append({"m": month_abs, "s": s["name"],
                           "text": f"{s['name']} has grown into a {s['tier'].lower()}."})
    return events


def territory_radius(pop):
    return 2.0 + 2.4 * np.log10(max(pop, 10))
