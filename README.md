# Calliope

A generated world: terrain, climate, hydrology, soils, resources, peoples, settlements and trade — simulated month by month and explored through an interactive map that explains itself.

Originally a Hy/cocos2d prototype (preserved under `game/game/*.hy`), now a Python 3.13 simulation engine served to a canvas-based web viewer.

## Running

```sh
uvicorn game.server:app --app-dir game --port 8080
```

Then open `http://localhost:8080/?seed=42&size=512`.

## The simulation

Generation pipeline (`game/game/world.py`), ~5 s for a 512×512 world:

1. **Terrain** — vectorized 3D Perlin fBm with domain warping and ridged multifractals; radial falloff shapes the continents.
2. **Climate** — latitude bands, altitude lapse rate, continentality, and seasonal amplitude give each cell a monthly temperature; moisture advection along trade winds and westerlies produces precipitation with rain shadows.
3. **Hydrology** — priority-flood depression filling, D8 flow routing, precipitation-weighted accumulation; rivers and lakes emerge from discharge.
4. **Biomes** — the original Whittaker-style height x moisture table from the Hy prototype.
5. **Fertility** (`agriculture.py`) — warmth and rainfall optima, slope penalty, alluvial silt along big rivers, lakeshore bonus; feeds settlement food scores and ships as a map layer.
6. **Toponymy** (`naming.py`) — connected-component analysis finds the ocean, seas, continents, isles, mountain ranges, deserts, forests, rivers and lakes; each gets a name from the world's "old tongue", anchored at its interior.
7. **Resources** — the original knowledge triples (`ISA`, `REQUIRES`, `FOUNDIN`, `ABUNDANCE`) drive suitability masks; noise thresholds place point deposits.
8. **Cultures** (`culture.py`) — settlements cluster into peoples by geography; the dominant homeland biome picks each culture's naming style (Hellenic, Nordic, arid, sylvan, steppe) and colours the political map.
9. **Settlements & trade** — founded on food, fertility, freshwater and resource suitability; each settlement works nearby deposits (plus grain from fertile soil) into a goods list and a chief export; A* routes over terrain cost link neighbours, weighted by traffic.

Each month: logistic population growth with winter shocks, plagues, golden harvests and a trade bonus; camps promote to villages, towns, cities. Crowded settlements send out settlers — colonies inherit their mother city's culture, take names in its tongue, and get linked into the trade network on the spot.

## The viewer (`game/web/`)

Layers: biomes, elevation, temperature, precipitation, hydrology, fertility, political (culture realms with borders). Overlays: rivers, seasonal snow/sea-ice, settlements, trade routes, resource deposits, geographic place names, prevailing winds, relief shading.

A month-by-month calendar with playback speeds, a population chronicle and sparkline, a peoples legend, a click-to-open settlement panel (culture, goods, exports, routes), and a hover inspector that explains the world — rain shadows behind named ranges, subtropical highs, equatorial rains, floodplain silt.

Data travels as a binary-packed payload (`/api/world`); ticks stream through `/api/tick` and return updated routes when colonies are founded.
