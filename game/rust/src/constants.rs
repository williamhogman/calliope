//! Biome constants — ported from constants.py / constants.hy.

use serde_json::{json, Value};

pub const WATER: u8 = 0;
pub const DESERT: u8 = 1;
pub const SAVANNA: u8 = 2;
pub const TROPICAL_RAIN_FOREST: u8 = 3;
pub const GRASSLAND: u8 = 4;
pub const WOODLAND: u8 = 5;
pub const SEASONAL_RAIN_FOREST: u8 = 6;
pub const TEMPERATE_RAIN_FOREST: u8 = 7;
pub const BOREAL_FOREST: u8 = 8;
pub const TUNDRA: u8 = 9;
pub const ICE: u8 = 10;

pub const PRETTY_BIOMES: [&str; 11] = [
    "Water",
    "Desert",
    "Savanna",
    "Tropical Rainforest",
    "Grasslands",
    "Woodlands",
    "Seasonal Rainforest",
    "Temperate Rainforest",
    "Boreal Forest",
    "Tundra",
    "Ice",
];

pub const BIOME_COLORS: [[u8; 3]; 11] = [
    [38, 84, 148],
    [231, 196, 132],
    [196, 168, 83],
    [22, 108, 48],
    [144, 189, 102],
    [104, 156, 74],
    [55, 128, 62],
    [32, 104, 84],
    [58, 92, 62],
    [148, 145, 122],
    [235, 242, 246],
];

/// Height unit: 1.0 == 4000 m (from geo.hy)
pub const METRES_PER_UNIT: f64 = 4000.0;

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
    let list: Vec<Value> = (0..11u8)
        .map(|b| {
            json!({
                "id": b,
                "name": PRETTY_BIOMES[b as usize],
                "color": BIOME_COLORS[b as usize],
            })
        })
        .collect();
    Value::Array(list)
}
