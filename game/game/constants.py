"""Biome constants — ported from constants.hy.

There are a finite number of biomes in the game.
"""

WATER = 0
DESERT = 1
SAVANNA = 2
TROPICAL_RAIN_FOREST = 3
GRASSLAND = 4
WOODLAND = 5
SEASONAL_RAIN_FOREST = 6
TEMPERATE_RAIN_FOREST = 7
BOREAL_FOREST = 8
TUNDRA = 9
ICE = 10

PRETTY_BIOMES = {
    WATER: "Water",
    DESERT: "Desert",
    SAVANNA: "Savanna",
    TROPICAL_RAIN_FOREST: "Tropical Rainforest",
    GRASSLAND: "Grasslands",
    WOODLAND: "Woodlands",
    SEASONAL_RAIN_FOREST: "Seasonal Rainforest",
    TEMPERATE_RAIN_FOREST: "Temperate Rainforest",
    BOREAL_FOREST: "Boreal Forest",
    TUNDRA: "Tundra",
    ICE: "Ice",
}

# Refined palette (the color.hy original had placeholder duplicates);
# hues stay in the same family, every biome now distinct.
BIOME_COLORS = {
    WATER: (38, 84, 148),
    DESERT: (231, 196, 132),
    SAVANNA: (196, 168, 83),
    TROPICAL_RAIN_FOREST: (22, 108, 48),
    GRASSLAND: (144, 189, 102),
    WOODLAND: (104, 156, 74),
    SEASONAL_RAIN_FOREST: (55, 128, 62),
    TEMPERATE_RAIN_FOREST: (32, 104, 84),
    BOREAL_FOREST: (58, 92, 62),
    TUNDRA: (148, 145, 122),
    ICE: (235, 242, 246),
}

# Height unit: 1.0 == 4000 m (from geo.hy)
METRES_PER_UNIT = 4000.0

# Attic calendar, index 0 ~ January
MONTHS = [
    "Gamelion", "Anthesterion", "Elaphebolion", "Mounichion",
    "Thargelion", "Skirophorion", "Hekatombaion", "Metageitnion",
    "Boedromion", "Pyanepsion", "Maimakterion", "Poseideon",
]

def biome_meta():
    return [
        {"id": b, "name": PRETTY_BIOMES[b], "color": list(BIOME_COLORS[b])}
        for b in sorted(PRETTY_BIOMES)
    ]
