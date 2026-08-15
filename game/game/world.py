"""World orchestration — the modern generate-layers (geo.hy) plus simulation."""

import time

import numpy as np

from game import agriculture
from game import biomes as biomes_mod
from game import climate, constants, culture, geo, hydrology, naming, resources, settlements, trade


class World:
    def __init__(self, seed: int, size: int = 512):
        self.seed = int(seed)
        self.size = int(size)
        self.month = 0  # absolute months since founding
        self.events = []
        self.rng = np.random.default_rng(seed + 777)
        self.timings = {}
        self._generate()

    def _generate(self):
        t0 = time.time()
        size = self.size
        seed = self.seed

        height = geo.heightmap(seed, size)
        water = height < 0.0
        self.timings["terrain"] = time.time() - t0

        t1 = time.time()
        lat = climate.latitude_deg(size)
        tmean = climate.temperature_mean(height, lat)
        tamp = climate.temperature_amplitude(lat, water)
        precip = climate.precipitation(height, water, tmean, lat)
        self.timings["climate"] = time.time() - t1

        t2 = time.time()
        hydro = hydrology.hydrology(height, water, precip)
        self.timings["hydrology"] = time.time() - t2

        t3 = time.time()
        biome_map = biomes_mod.classify(height, tmean, precip, hydro["lakes"])
        self.timings["biomes"] = time.time() - t3

        self.height = height.astype(np.float32)
        self.tmean = tmean.astype(np.float32)
        self.tamp = tamp.astype(np.float32)
        self.precip = precip.astype(np.float32)
        self.discharge = hydro["discharge"]
        self.biomes = biome_map
        self.rivers = hydro["rivers"]
        self.lakes = hydro["lakes"]

        t4 = time.time()
        self.fertility = agriculture.fertility(
            self.height, self.tmean, self.precip,
            self.rivers, self.lakes, self.discharge)
        self.timings["fertility"] = time.time() - t4

        # persistent world dict: simulation state lives here across ticks
        w = {
            "height": self.height, "biomes": self.biomes, "tmean": self.tmean,
            "tamp": self.tamp, "precip": self.precip, "rivers": self.rivers,
            "lakes": self.lakes, "discharge": self.discharge,
            "fertility": self.fertility,
        }
        self._wdict = w

        t5 = time.time()
        self.features, self.world_name = naming.name_features(w, seed)
        self.timings["naming"] = time.time() - t5

        t6 = time.time()
        self.deposits = resources.place_resources(w, seed)
        w["deposits"] = self.deposits
        self.timings["resources"] = time.time() - t6

        t7 = time.time()
        self.settlements = settlements.found_settlements(w, seed)
        w["settlements"] = self.settlements
        self.cultures = culture.assign_cultures(w, self.settlements, seed)
        trade.assign_goods(w, self.settlements)
        self.routes = trade.build_routes(w, self.settlements)
        w["routes"] = self.routes
        self.timings["settlements"] = time.time() - t7

        for s in self.settlements:
            people = self.cultures[s["culture"]]["people"] if self.cultures else "first peoples"
            self.events.append({
                "m": 0, "s": s["name"],
                "text": f"{s['name']} founded by the {people}"
                        + (" by the coast." if s["coastal"]
                           else " on fresh water." if s["river"] else "."),
            })
        self.timings["total"] = time.time() - t0

    def tick(self, months: int = 1):
        new_events = []
        founded = False
        months = max(1, min(int(months), 240))
        for _ in range(months):
            self.month += 1
            evs = settlements.tick_settlements(self._wdict, self.month, self.rng)
            new_events.extend(evs)
            col_evs, did = settlements.try_colonize(
                self._wdict, self.month, self.rng, self.cultures)
            if did:
                founded = True
                new_events.extend(col_evs)
        self.events.extend(new_events)
        self.events = self.events[-200:]
        return {"events": new_events, "founded": founded}

    # --- flags packed for the client ---
    def flags(self) -> np.ndarray:
        f = np.zeros((self.size, self.size), dtype=np.uint8)
        f |= self.rivers.astype(np.uint8) * 1
        f |= self.lakes.astype(np.uint8) * 2
        return f

    def meta(self):
        return {
            "seed": self.seed,
            "size": self.size,
            "month": self.month,
            "months": constants.MONTHS,
            "sea_level": 0.0,
            "metres_per_unit": constants.METRES_PER_UNIT,
            "world_name": self.world_name,
            "biomes": constants.biome_meta(),
            "resources": resources.resource_meta(),
            "deposits": self.deposits,
            "settlements": self.settlements,
            "cultures": self.cultures,
            "features": self.features,
            "routes": self.routes,
            "events": self.events[-60:],
            "timings": {k: round(v, 3) for k, v in self.timings.items()},
        }
