//! Calliope — the world simulation, compiled to WebAssembly.
//!
//! The whole pipeline (terrain, climate, hydrology, biomes, fertility,
//! toponymy, resources, cultures, settlements, trade) runs in the browser;
//! `pack()` emits the same binary format the Python server used, so the
//! client unpacker is unchanged.

pub mod agriculture;
pub mod biomes;
pub mod chronicle;
pub mod climate;
pub mod constants;
pub mod culture;
pub mod economy;
pub mod geo;
pub mod hydrology;
pub mod naming;
pub mod ndimage;
pub mod noisegen;
#[cfg(target_arch = "wasm32")]
pub mod render;
pub mod resources;
pub mod settlements;
pub mod society;
pub mod trade;
pub mod util;
pub mod world;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WasmWorld {
    inner: world::World,
}

#[wasm_bindgen]
impl WasmWorld {
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32, size: u32) -> Result<WasmWorld, JsValue> {
        let size = size as usize;
        if !matches!(size, 256 | 384 | 512 | 640 | 768) {
            return Err(JsValue::from_str("size must be 256, 384, 512, 640 or 768"));
        }
        Ok(WasmWorld {
            inner: world::World::generate(seed as i64, size),
        })
    }

    /// Binary world payload: [u32 header_len][header json][raw arrays].
    pub fn pack(&self) -> Vec<u8> {
        self.inner.pack()
    }

    /// Advance the simulation; returns {month, settlements, events, routes?} JSON.
    pub fn tick(&mut self, months: u32) -> String {
        self.inner.tick_json(months as i64)
    }
}
