"""Settlements: founding, monthly growth, colonisation, territory, events."""

import numpy as np
from scipy import ndimage

from game import constants as gc
from game import climate, naming

TIERS = [(0, "Camp"), (250, "Village"), (1000, "Town"), (5000, "City")]


def _tier(pop):
    name = TIERS[0][1]
    for threshold, t in TIERS:
        if pop >= threshold:
            name = t
    return name


def _adjacency(mask):
    return ndimage.binary_dilation(mask, iterations=2)


def found_settlements(world, seed):
    """Score cells and greedily found settlements with min spacing.

    Also stashes the pristine score/food grids on the world dict so later
    colonisation can reuse them.
    """
    h = world["height"]
    biomes = world["biomes"]
    tmean = world["tmean"]
    rivers = world["rivers"]
    lakes = world["lakes"]
    deposits = world["deposits"]
    fert = world["fertility"]
    size = h.shape[0]
    land = h >= 0

    sea = h < 0
    coast = land & _adjacency(sea)
    near_fresh = land & (_adjacency(rivers) | _adjacency(lakes))

    # food kernel from deposits whose ISA chain reaches "food"
    from game.resources import isa_chain
    food = np.zeros_like(h, dtype=np.float64)
    for d in deposits:
        if "food" in isa_chain(d["r"]):
            food[d["y"], d["x"]] += d["rich"]
    food = ndimage.gaussian_filter(food, sigma=5.0) * 60.0
    food = np.clip(food, 0.0, 3.0)

    comfort = np.exp(-((tmean - 12.0) / 14.0) ** 2)

    score = (
        2.2 * near_fresh.astype(float)
        + 1.6 * coast.astype(float)
        + food
        + 2.0 * comfort
        + 2.6 * fert
        - 2.5 * (biomes == gc.DESERT)
        - 3.5 * (biomes == gc.ICE)
        - 1.5 * (biomes == gc.TUNDRA)
        - 2.0 * np.clip(h - 0.5, 0, 1) * 4.0
    )
    score[~land] = -1e9

    rng = np.random.default_rng(seed + 9000)
    taken_names = world.setdefault("taken_names", set())
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
            "name": naming.make_word(rng, "hellenic", taken_names),
            "x": int(x), "y": int(y),
            "pop": pop,
            "tier": _tier(pop),
            "food": _site_food(food, fert, near_fresh, coast, y, x),
            "coastal": bool(coast[y, x]),
            "river": bool(near_fresh[y, x]),
            "culture": 0,
        })
        working[(yy - y) ** 2 + (xx - x) ** 2 < min_dist ** 2] = -1e9

    # persist grids for colonisation during ticks
    world["site_score"] = score
    world["food_grid"] = food
    world["near_fresh"] = near_fresh
    world["coast"] = coast
    world["max_settlements"] = int(n_target * 2.5)
    return settlements


def _site_food(food_grid, fert, near_fresh, coast, y, x):
    return round(float(max(
        0.35,
        food_grid[y, x] + 1.6 * fert[y, x]
        + 1.4 * near_fresh[y, x] + coast[y, x])), 2)


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
        # plague finds the crowded streets
        if pop > 2200 and rng.random() < 0.004:
            loss = int(pop * rng.uniform(0.06, 0.16))
            pop -= loss
            events.append({"m": month_abs, "s": s["name"],
                           "text": f"Plague stalks the streets of {s['name']} — {loss} souls perish."})
        # a golden harvest, in high summer, on good soil
        if month == 6 and s["food"] > 2.2 and rng.random() < 0.05:
            events.append({"m": month_abs, "s": s["name"],
                           "text": f"The harvest overflows in {s['name']}; granaries groan."})
            growth *= 2.0
        pop = max(20, int(round(pop + growth)))
        old_tier = s["tier"]
        s["pop"] = pop
        s["tier"] = _tier(pop)
        if s["tier"] != old_tier:
            events.append({"m": month_abs, "s": s["name"],
                           "text": f"{s['name']} has grown into a {s['tier'].lower()}."})
    return events


def try_colonize(world, month_abs, rng, cultures):
    """Crowded settlements send out settlers to found colonies."""
    from game import trade  # local import: trade also imports settlements

    settlements = world["settlements"]
    limit = world.get("max_settlements", 40)
    events = []
    founded = False
    for parent in list(settlements):
        if len(settlements) >= limit:
            break
        if parent["pop"] < 380 or parent["pop"] < 0.72 * _capacity(parent):
            continue
        if rng.random() > 0.02:
            continue
        site = _colony_site(world, parent)
        if site is None:
            continue
        y, x = site
        migrants = max(40, int(parent["pop"] * rng.uniform(0.08, 0.14)))
        parent["pop"] = max(60, parent["pop"] - migrants)
        cid = parent.get("culture", 0)
        style = cultures[cid]["style"] if cultures else "hellenic"
        name = naming.make_word(rng, style, world.setdefault("taken_names", set()))
        fert = world["fertility"]
        s = {
            "id": max(o["id"] for o in settlements) + 1,
            "name": name, "x": int(x), "y": int(y),
            "pop": migrants, "tier": _tier(migrants),
            "food": _site_food(world["food_grid"], fert,
                               world["near_fresh"], world["coast"], y, x),
            "coastal": bool(world["coast"][y, x]),
            "river": bool(world["near_fresh"][y, x]),
            "culture": cid, "connections": 0,
        }
        settlements.append(s)
        trade.goods_for(s, world)
        trade.connect_settlement(world, s)
        founded = True
        where = (" by the sea." if s["coastal"]
                 else " on fresh water." if s["river"] else " in the wilds.")
        events.append({"m": month_abs, "s": name,
                       "text": f"Settlers out of {parent['name']} raise {name}{where}"})
    return events, founded


def _colony_site(world, parent):
    score = world["site_score"]
    size = score.shape[0]
    yy, xx = np.mgrid[0:size, 0:size]
    d2p = (yy - parent["y"]) ** 2 + (xx - parent["x"]) ** 2
    mask = (d2p >= 16 ** 2) & (d2p <= 60 ** 2) & (score > 2.2)
    min_d2 = (size / 22.0) ** 2
    for o in world["settlements"]:
        mask &= ((yy - o["y"]) ** 2 + (xx - o["x"]) ** 2) >= min_d2
    if not mask.any():
        return None
    sc = np.where(mask, score, -1e9)
    idx = int(np.argmax(sc))
    return divmod(idx, size)


def territory_radius(pop):
    return 2.0 + 2.4 * np.log10(max(pop, 10))
