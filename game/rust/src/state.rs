//! The world's state, split into load-bearing walls (E11.3).
//!
//! `World` owns four sub-structs instead of forty loose fields: the land
//! (`Fields`), the peoples (`Peoples`), the coin (`Economy`) and the record
//! (`Chronicle`). Subsystems borrow the walls they need — disjoint borrows
//! the compiler can see — instead of eight loose parameters.

use ndarray::Array2;

bitflags::bitflags! {
    /// Per-cell water flags (E1.7) — one byte per cell, stored directly in
    /// `World.flags` and shipped verbatim in the pack (bit layout is the
    /// wire contract the JS client already reads).
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct CellFlags: u8 {
        const RIVER    = 1 << 0;
        const LAKE     = 1 << 1;
        const SALT     = 1 << 2;
        const SEASONAL = 1 << 3;
    }
}


use crate::artifact::Artifact;
use crate::chronicle::ChronicleState;
use crate::culture::People;
use crate::economy::{Market, MarketAreas, Merchant};
use crate::entity::Registry;
use crate::politics::Realm;
use crate::settlements::Settlement;
use crate::society::Society;
use crate::civ::Civ;
use crate::event::Event;

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
    /// Basement geology per cell (M18): 0 shield · 1 basin · 2 fold belt
    /// · 3 volcanic. Frozen at genesis, read by deposit placement (M19).
    pub rock: Array2<u8>,
    /// Influence-map territory: owner REALM per cell, −1 wilderness
    /// (M4.1, realm axis per ADR-0018).
    pub territory: Array2<i16>,
    /// The people-axis influence map for the culture layer (M10.6):
    /// dominant people per cell, −1 wilderness.
    pub peoples_map: Array2<i16>,
}

/// The peoples and their crowns — settlements, peoples, realms, arts.
pub struct Peoples {
    pub settlements: Vec<Settlement>,
    /// The generational axis (ADR-0018): tongue, gods, name bank.
    pub peoples: Vec<People>,
    /// The political axis (ADR-0018): crown, house, seat, treasury.
    pub realms: Vec<Realm>,
    /// Arts and lore, keyed by people — knowledge travels with the tongue.
    pub societies: Vec<Society>,
    /// M13/ADR-0019 — the derived tier: civilizations named over the
    /// kinship-closure of peoples. Recomputed yearly by the civ pass;
    /// rows are never deleted (`alive` flips on collapse).
    pub civs: Vec<Civ>,
    /// M12.1 — months people A's towns have spent under crowns of people
    /// B, exposure-weighted (a year under one crown with every town adds
    /// 12). Directional; the kinship metric reads both ways. Grows a row
    /// and column when a people diverges.
    pub coresidence: Vec<Vec<f64>>,
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
