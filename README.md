# Calliope

A generated world: terrain, climate, hydrology, soils, resources, peoples, settlements and trade — simulated month by month and explored through an interactive map that explains itself.

Originally a Hy/cocos2d prototype (preserved under `game/game/*.hy`), later a Python 3.13 engine (preserved under `game/game/*.py`), now a **Rust simulation core compiled to WebAssembly** — the whole world generates and ticks inside the browser, no backend.

## Running

Any static file server over `game/web/`:

```sh
python3 -m http.server 8080 --directory game/web
```

Then open `http://localhost:8080/?seed=42&size=512`.

## Building the engine

The Rust crate lives in `game/rust/`. `scripts/build.sh` rebuilds the WASM when any Rust source is newer than the shipped artifact, then assembles `dist/`:

```sh
bash scripts/build.sh          # wasm-pack build --target web + copy into game/web/js/wasm/
```

Native benchmark binary: `cargo run --release --bin worldgen` (512×512 in ~0.9 s native, ~1.5 s in-browser WASM; 256×256 in ~0.4 s).

## The simulation (`game/rust/src/`)

Generation pipeline (`world.rs`):

1. **Terrain** (`geo.rs`, `noisegen.rs`) — 3D Perlin fBm with domain warping and ridged multifractals; radial falloff shapes the continents.
2. **Climate** (`climate.rs`) — latitude bands, altitude lapse rate, continentality, and seasonal amplitude give each cell a monthly temperature; moisture advection along trade winds and westerlies produces precipitation with rain shadows.
3. **Hydrology** (`hydrology.rs`) — priority-flood depression filling, D8 flow routing, precipitation-weighted accumulation; rivers and lakes emerge from discharge.
4. **Biomes** (`biomes.rs`) — the original Whittaker-style height × moisture table from the Hy prototype.
5. **Fertility** (`agriculture.rs`) — warmth and rainfall optima, slope penalty, alluvial silt along big rivers, lakeshore bonus; feeds settlement food scores and ships as a map layer.
6. **Toponymy** (`naming.rs`) — connected-component analysis finds the ocean, seas, continents, isles, mountain ranges, deserts, forests, rivers and lakes; each gets a name from the world's "old tongue", anchored at its interior.
7. **Resources** (`resources.rs`) — the original knowledge triples (`ISA`, `REQUIRES`, `FOUNDIN`, `ABUNDANCE`) drive suitability masks; noise thresholds place point deposits.
8. **Cultures** (`culture.rs`) — settlements cluster into peoples by geography; the dominant homeland biome picks each culture's naming style (Hellenic, Nordic, arid, sylvan, steppe) and colours the political map.
9. **Settlements & trade** (`settlements.rs`, `trade.rs`) — founded on food, fertility, freshwater and resource suitability; each settlement works nearby deposits (plus grain from fertile soil) into a goods list and a chief export; A* routes over terrain cost link neighbours, weighted by traffic.

`ndimage.rs` reimplements the scipy primitives the pipeline needs: separable Gaussian blur, exact Euclidean distance transform (Felzenszwalb–Huttenlocher), binary dilation, connected-component labeling, and maximum filter.

Each month: logistic population growth with winter shocks, plagues, golden harvests and a trade bonus; camps promote to villages, towns, cities. Crowded settlements send out settlers — colonies inherit their mother city's culture, take names in its tongue, and get linked into the trade network on the spot.

## The viewer (`game/web/`)

The engine runs in a Web Worker (`js/worker.js`) via wasm-bindgen; `js/net.js` bridges worker messages and unpacks the binary world payload. Generation and ticks never block the UI thread.

Layers: biomes, elevation, temperature, precipitation, hydrology, fertility, political (culture realms with borders). Overlays: rivers, seasonal snow/sea-ice, settlements, trade routes, resource deposits, geographic place names, prevailing winds, relief shading.

A month-by-month calendar with playback speeds, a population chronicle and sparkline, a peoples legend, a click-to-open settlement panel (culture, goods, exports, routes), and a hover inspector that explains the world — rain shadows behind named ranges, subtropical highs, equatorial rains, floodplain silt.
