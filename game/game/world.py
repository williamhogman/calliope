"""World orchestration — the modern generate-layers (geo.hy) plus simulation."""

import time

import numpy as np

from game import biomes as biomes_mod
from game import climate, constants, geo, hydrology, resources, settlements, trade


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
        wdict = self._as_dict()
        self.deposits = resources.place_resources(wdict, seed)
        wdict["deposits"] = self.deposits
        self.timings["resources"] = time.time() - t4

        t5 = time.time()
        self.settlements = settlements.found_settlements(wdict, seed)
        self.routes = trade.build_routes(wdict, self.settlements)
        self.timings["settlements"] = time.time() - t5

        for s in self.settlements:
            self.events.append({
                "m": 0, "s": s["name"],
                "text": f"{s['name']} founded"
                        + (" by the coast." if s["coastal"]
                           else " on fresh water." if s["river"] else "."),
            })
        self.timings["total"] = time.time() - t0

    def _as_dict(self):
        return {
            "height": self.height, "biomes": self.biomes, "tmean": self.tmean,
            "tamp": self.tamp, "precip": self.precip, "rivers": self.rivers,
            "lakes": self.lakes, "deposits": getattr(self, "deposits", []),
            "settlements": getattr(self, "settlements", []),
        }

    def tick(self, months: int = 1):
        new_events = []
        months = max(1, min(int(months), 240))
        for _ in range(months):
            self.month += 1
            evs = settlements.tick_settlements(
                self._as_dict(), self.month, self.rng)
            new_events.extend(evs)
        self.events.extend(new_events)
        self.events = self.events[-200:]
        return new_events

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
            "biomes": constants.biome_meta(),
            "resources": resources.resource_meta(),
            "deposits": self.deposits,
            "settlements": self.settlements,
            "routes": self.routes,
            "events": self.events[-60:],
            "timings": {k: round(v, 3) for k, v in self.timings.items()},
        }
