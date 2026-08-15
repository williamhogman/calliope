# Calliope

A generated world: terrain, climate, hydrology, resources, settlements and trade — simulated month by month and explored through an interactive map.

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
5. **Resources** — the original knowledge triples (`ISA`, `REQUIRES`, `FOUNDIN`, `ABUNDANCE`) drive suitability masks; noise thresholds place point deposits.
6. **Settlements & trade** — founded on food, freshwater and resource suitability; A* routes over terrain cost link neighbors; population grows logistically with seasonal penalties and a trade bonus, promoting camps to villages to towns.

## The viewer (`game/web/`)

Layers: biomes, elevation, temperature, precipitation, hydrology, political. Overlays: rivers, seasonal snow/sea-ice, settlements, trade routes, resource deposits, relief shading. A month-by-month calendar with playback speeds, a population chronicle, and a hover inspector for any cell.

Data travels as a binary-packed payload (`/api/world`); ticks stream through `/api/tick`.
