//! Biome constants — ported from constants.py / constants.hy.

use serde_json::{json, Value};

/// The twelve biomes (E1.8, M38) — one declaration: codes, display
/// names and map colors live on the variants; `biome_meta()` is
/// generated from it. Codes are stable — they ship in the pack.
#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::Display, strum::EnumIter, strum::EnumCount, strum::IntoStaticStr)]
#[repr(u8)]
pub enum Biome {
    Water = 0,
    Desert = 1,
    Savanna = 2,
    #[strum(serialize = "Tropical Rainforest")]
    TropicalRainForest = 3,
    Grasslands = 4,
    Woodlands = 5,
    #[strum(serialize = "Seasonal Rainforest")]
    SeasonalRainForest = 6,
    #[strum(serialize = "Temperate Rainforest")]
    TemperateRainForest = 7,
    #[strum(serialize = "Boreal Forest")]
    BorealForest = 8,
    Tundra = 9,
    /// M38 — sedge-and-moss mire over a shallow permafrost table;
    /// code 9 stays the dry fell-field default.
    #[strum(serialize = "Wet Tundra")]
    WetTundra = 11,
    Ice = 10,
}

impl Biome {
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        self.into()
    }

    #[inline]
    pub fn from_code(c: u8) -> Biome {
        match c {
            1 => Biome::Desert,
            2 => Biome::Savanna,
            3 => Biome::TropicalRainForest,
            4 => Biome::Grasslands,
            5 => Biome::Woodlands,
            6 => Biome::SeasonalRainForest,
            7 => Biome::TemperateRainForest,
            8 => Biome::BorealForest,
            9 => Biome::Tundra,
            10 => Biome::Ice,
            11 => Biome::WetTundra,
            _ => Biome::Water,
        }
    }

    pub const fn color(self) -> [u8; 3] {
        match self {
            Biome::Water => [38, 84, 148],
            Biome::Desert => [231, 196, 132],
            Biome::Savanna => [196, 168, 83],
            Biome::TropicalRainForest => [22, 108, 48],
            Biome::Grasslands => [144, 189, 102],
            Biome::Woodlands => [104, 156, 74],
            Biome::SeasonalRainForest => [55, 128, 62],
            Biome::TemperateRainForest => [32, 104, 84],
            Biome::BorealForest => [58, 92, 62],
            Biome::Tundra => [148, 145, 122],
            Biome::WetTundra => [121, 139, 108],
            Biome::Ice => [235, 242, 246],
        }
    }
}

// Grid codes for the biome field (`Array2<u8>` — the pack contract):
// defined from the enum, so there is exactly one source of truth.
pub const WATER: u8 = Biome::Water.code();
pub const DESERT: u8 = Biome::Desert.code();
pub const SAVANNA: u8 = Biome::Savanna.code();
pub const TROPICAL_RAIN_FOREST: u8 = Biome::TropicalRainForest.code();
pub const GRASSLAND: u8 = Biome::Grasslands.code();
pub const WOODLAND: u8 = Biome::Woodlands.code();
pub const SEASONAL_RAIN_FOREST: u8 = Biome::SeasonalRainForest.code();
pub const TEMPERATE_RAIN_FOREST: u8 = Biome::TemperateRainForest.code();
pub const BOREAL_FOREST: u8 = Biome::BorealForest.code();
pub const TUNDRA: u8 = Biome::Tundra.code();
pub const WET_TUNDRA: u8 = Biome::WetTundra.code();
pub const ICE: u8 = Biome::Ice.code();

/// Height unit: 1.0 == 4000 m (from geo.hy)
pub const METRES_PER_UNIT: f64 = 4000.0;

/// Horizontal scale: one grid cell spans this many kilometres.
pub const KM_PER_CELL: f64 = 4.0;

/// Attic calendar, index 0 ~ January
pub const MONTHS: [&str; 12] = [
    "Gamelion",
    "Anthesterion",
    "Elaphebolion",
    "Mounichion",
    "Thargelion",
    "Skirophorion",
    "Hekatombaion",
    "Metageitnion",
    "Boedromion",
    "Pyanepsion",
    "Maimakterion",
    "Poseideon",
];

pub fn biome_meta() -> Value {
    use strum::IntoEnumIterator;
    let list: Vec<Value> = Biome::iter()
        .map(|b| {
            json!({
                "id": b.code(),
                "name": b.to_string(),
                "color": b.color(),
            })
        })
        .collect();
    Value::Array(list)
}

/// Raster layer ids exactly as the Orbital WGSL shader branches on them
/// (`render.rs`). The generated JS constants (E2.4) mirror this table, so
/// the client can never drift from the shader.
pub const LAYERS: &[(&str, u8)] = &[
    ("biomes", 0),
    ("political", 1),
    ("elevation", 2),
    ("temperature", 3),
    ("precip", 4),
    ("hydro", 5),
    ("fertility", 6),
    // M63 — the deep-earth lenses read the stack that built the ground
    ("geology", 7),
    ("soils", 8),
    ("landform", 9),
];
