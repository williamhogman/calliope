//! The world's state, split into load-bearing walls (E11.3).
//!
//! `World` owns four sub-structs instead of forty loose fields: the land
//! (`Fields`), the peoples (`Peoples`), the coin (`Economy`) and the record
//! (`Chronicle`). Subsystems borrow the walls they need — disjoint borrows
//! the compiler can see — instead of eight loose parameters.

use ndarray::Array2;

use crate::artifact::Artifact;
use crate::chronicle::ChronicleState;
use crate::culture::Culture;
use crate::economy::{Market, MarketAreas, Merchant};
use crate::entity::Registry;
use crate::settlements::Settlement;
use crate::society::Society;
use crate::world::Event;

/// The land itself — every per-cell grid (E3.2: f32 at rest).
pub struct Fields {
    pub height: Array2<f32>,
    pub tmean: Array2<f32>,
    pub tamp: Array2<f32>,
    pub precip: Array2<f32>,
    pub discharge: Array2<f32>,
    pub fertility: Array2<f32>,
    pub biomes: Array2<u8>,
    /// Crop package per cell (M2.1): 0 wild · 1 wheat · 2 rice · 3 maize · 4 pastoral.
    pub crops: Array2<u8>,
    /// Per-cell `CellFlags` bits: river / lake / salt / seasonal.
    pub flags: Array2<u8>,
    /// Signed monsoon share of the year's rain (positive peaks month 0).
    pub pamp: Array2<f32>,
    /// Signed seasonal discharge swing per cell, -1..1.
    pub flow_amp: Array2<f32>,
    /// Strahler stream order, 0 off-river.
    pub strahler: Array2<u8>,
    /// Influence-map territory: owner culture per cell, −1 wilderness (M4.1).
    pub territory: Array2<i16>,
}

/// The peoples — settlements, cultures and their arts.
pub struct Peoples {
    pub settlements: Vec<Settlement>,
    pub cultures: Vec<Culture>,
    pub societies: Vec<Society>,
}

/// The coin — market, areas, merchants, realized flow.
pub struct Economy {
    pub market: Market,
    /// M5.2 — the route web carved into market areas, each with its own
    /// price list; rebuilt when towns are founded and refreshed yearly.
    pub areas: MarketAreas,
    /// M5.5 — named traders riding the price gaps between areas.
    pub merchants: Vec<Merchant>,
    /// Last month's realized flow per route (gravity cross-check, M5.4).
    pub route_flow: Vec<f64>,
}

/// The record — the chronicle's memory and cast.
pub struct Chronicle {
    /// The full log — the sifter reads all of it (M6).
    pub events: Vec<Event>,
    /// M6.1 — the chronicle's cast: every named thing, one stable id.
    pub registry: Registry,
    /// M6.3 — relics with provenance: forged, plundered, lost, found.
    pub artifacts: Vec<Artifact>,
    /// Dynasties, rulers, wonder cooldowns — the tellers' working state.
    pub(crate) state: ChronicleState,
}
