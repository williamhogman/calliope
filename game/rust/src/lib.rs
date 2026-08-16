//! Calliope — the world simulation, compiled to WebAssembly.
//!
//! The whole pipeline (terrain, climate, hydrology, biomes, fertility,
//! toponymy, resources, cultures, settlements, trade) runs in the browser;
//! `pack()` emits the same binary format the Python server used, so the
//! client unpacker is unchanged.

pub mod agriculture;
pub mod artifact;
pub mod biomes;
pub mod chronicle;
pub mod climate;
pub mod constants;
pub mod culture;
pub mod economy;
pub mod entity;
pub mod erosion;
pub mod explain;
pub mod geo;
pub mod hydrology;
pub mod ids;
pub mod naming;
pub mod ndimage;
pub mod noisegen;
pub mod patina;
pub mod politics;

#[cfg(target_arch = "wasm32")]
pub mod render;
pub mod resources;
pub mod settlements;
pub mod society;
pub mod telling;
pub mod trade;
pub mod util;
pub mod world;

use wasm_bindgen::prelude::*;

/// E6.2 — debug builds only: route panic messages to the console before
/// the abort. Release builds ship `panic = "abort"` with no hook at all,
/// so none of this machinery reaches the production binary.
#[cfg(all(target_arch = "wasm32", debug_assertions))]
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("calliope panic: {info}").into());
    }));
}

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

    /// Binary world payload, pack v2: [u32 header_len][header json][blob]
    /// — crc-stamped, quantized u16 float grids, territory as header RLE.
    pub fn pack(&self) -> Vec<u8> {
        self.inner.pack()
    }

    /// Generation stage timings as JSON pairs — debug side channel (E3.9);
    /// wall-clock never rides the pack itself.
    pub fn timings(&self) -> String {
        self.inner.timings_json()
    }

    /// Once-per-world bootstrap JSON (E3.1): vocabulary tables and entity
    /// state — the pack header stays lean.
    pub fn bootstrap(&self) -> String {
        self.inner.bootstrap_json()
    }

    /// Advance the simulation; returns {month, settlements, events, routes?} JSON.
    pub fn tick(&mut self, months: u32) -> String {
        self.inner.tick_json(months as i64)
    }

    /// Term ledger for a derived quantity ("why is this so?") as JSON.
    /// kind: "settlement" (key = id) | "good" (key = good name).
    pub fn explain(&self, kind: String, key: String) -> String {
        explain::explain(&self.inner, &kind, &key)
    }

    /// How many entries the full chronicle holds (M6).
    pub fn events_len(&self) -> u32 {
        self.inner.events.len() as u32
    }

    /// One page of the full chronicle, `[from, to)`, as a JSON array (M6).
    pub fn events_range(&self, from: u32, to: u32) -> String {
        let n = self.inner.events.len();
        let a = (from as usize).min(n);
        let b = (to as usize).clamp(a, n);
        serde_json::to_string(&self.inner.events[a..b]).unwrap_or_else(|_| "[]".into())
    }

    /// The chronicle's cast — every named entity, alive and dead (M6.1).
    pub fn entities(&self) -> String {
        serde_json::to_string(&self.inner.registry.items).unwrap_or_else(|_| "[]".into())
    }

    /// Every chronicle entry that speaks of one entity, oldest first (M6.6).
    pub fn entity_log(&self, id: i64) -> String {
        let evs: Vec<&world::Event> = self
            .inner
            .events
            .iter()
            .filter(|e| e.ids.contains(&ids::EntityId(id)))
            .collect();
        serde_json::to_string(&evs).unwrap_or_else(|_| "[]".into())
    }

    /// The story sifter (M6.5/M6.7): ranked microstories lifted from the
    /// full log — rises and falls, rivalries, curses, relic roads.
    pub fn stories(&self) -> String {
        let stories = telling::sift(&self.inner.events, &self.inner.registry);
        serde_json::to_string(&stories).unwrap_or_else(|_| "[]".into())
    }

    /// The relics and their provenance (M6.3).
    pub fn artifacts(&self) -> String {
        serde_json::to_string(&self.inner.artifacts).unwrap_or_else(|_| "[]".into())
    }
}
