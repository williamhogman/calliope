//! World orchestration — port of world.py: generation pipeline + simulation.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use smallvec::{smallvec, SmallVec};


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

use crate::agriculture;
use crate::artifact;
use crate::biomes as biomes_mod;
use crate::chronicle::{self, ChronicleState};
use crate::climate;
use crate::culture::{self, Culture};
use crate::economy::{self, Market};
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::erosion;
use crate::geo;
use crate::hydrology;
use crate::ids::{CultureId, EntityId, SettlementId};
use crate::naming::{self, Feature};
use crate::noisegen::Perlin3;
use crate::patina::{self, Ruin};
use crate::politics::{self, Politics};
use crate::resources::{self, Deposit};
use crate::settlements::{self, Settlement};
use crate::snapshot::SentCache;
use crate::society::{self, Society};
use crate::telling;
use crate::trade::{self, Route};
use crate::util::{now_ms, round2};

/// Closed vocabulary of chronicle event kinds (E1.4). Displayed and
/// serialized as the same lowercase names the strings used, so the wire
/// format and the determinism hash are unchanged.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde_repr::Serialize_repr,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::EnumCount,
    strum::EnumIter,
)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum EventKind {
    Depletion,
    Disaster,
    Discovery,
    Economy,
    Famine,
    Festival,
    Found,
    Growth,
    Myth,
    Nature,
    Omen,
    Realm,
    Ruler,
    Society,
    Tech,
    Trade,
    War,
    Wonder,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// E2.3 — the event table: every kind's notification family, telling
/// weight (M6.5) and fortune lean (M6.7) declared in one row. `telling.rs`
/// and the generated JS constants (E2.4) both read this table. The
/// chronicle's prose intentionally stays at the emission sites in
/// `chronicle.rs` — each line is composed from live context (names, goods,
/// tallies) that no static template column could carry.
macro_rules! event_table {
    ($($kind:ident => family $fam:ident, weight $w:literal, fortune $f:literal;)+) => {
        impl EventKind {
            /// Filter/notification family: realm · war · economy · myth · nature.
            pub fn family(self) -> &'static str {
                match self { $(EventKind::$kind => stringify!($fam),)+ }
            }
            /// How loudly this kind rings down the years (M6.5).
            pub fn weight(self) -> i32 {
                match self { $(EventKind::$kind => $w,)+ }
            }
            /// Which way fortune leans for the subject: +1 rising, −1
            /// falling, 0 flat — the reversal detector counts sign changes.
            pub fn fortune(self) -> i32 {
                match self { $(EventKind::$kind => $f,)+ }
            }
        }
    };
}

event_table! {
    Depletion => family economy, weight 2, fortune -1;
    Disaster  => family nature,  weight 4, fortune -1;
    Discovery => family economy, weight 2, fortune 1;
    Economy   => family economy, weight 1, fortune 0;
    Famine    => family nature,  weight 3, fortune -1;
    Festival  => family myth,    weight 1, fortune 1;
    Found     => family realm,   weight 2, fortune 1;
    Growth    => family realm,   weight 1, fortune 1;
    Myth      => family myth,    weight 1, fortune 0;
    Nature    => family nature,  weight 1, fortune 0;
    Omen      => family myth,    weight 1, fortune 0;
    Realm     => family realm,   weight 3, fortune 0;
    Ruler     => family realm,   weight 2, fortune 0;
    Society   => family realm,   weight 1, fortune 0;
    Tech      => family realm,   weight 2, fortune 1;
    Trade     => family economy, weight 1, fortune 0;
    War       => family war,     weight 3, fortune -1;
    Wonder    => family realm,   weight 2, fortune 1;
}

/// E5.5 — inline storage for the common 0–2 entity ids per event.
pub type EventIds = SmallVec<[EntityId; 2]>;

#[derive(Serialize, Clone)]
pub struct Event {
    pub m: i64,
    pub s: String,
    pub k: EventKind,
    pub text: String,
    /// Entities this event speaks of (M6.1); the first id is the subject.
    /// E5.5 — SmallVec: most events name 0–2 entities, so the ids ride
    /// inline in the Event with no heap allocation; wire format unchanged.
    #[serde(skip_serializing_if = "SmallVec::is_empty")]
    pub ids: EventIds,
    /// Map anchor for fly-to, in grid cells; -1 = nowhere in particular.
    #[serde(skip_serializing_if = "neg")]
    pub x: i64,
    #[serde(skip_serializing_if = "neg")]
    pub y: i64,
    /// The mythologized rendering of great deeds (M6.9); empty = none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub legend: String,
    /// Withheld or disputed (M9.5): the telling admits it does not know.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub veiled: bool,
}

fn neg(v: &i64) -> bool {
    *v < 0
}

/// E4.8 — kinds worth a toast, picked engine-side; the client applies its
/// own notification-family preferences on top.
pub fn headline_worthy(k: EventKind) -> bool {
    matches!(
        k,
        EventKind::War
            | EventKind::Found
            | EventKind::Ruler
            | EventKind::Wonder
            | EventKind::Disaster
            | EventKind::Discovery
            | EventKind::Depletion
            | EventKind::Society
            | EventKind::Tech
            | EventKind::Myth
    )
}

/// E4.5 — one seat for "this wire section must reship on the next tick".
/// Systems mark bits as they change state; `tick_json` takes them.
#[derive(Default, Clone, Copy)]
pub struct Dirty(u8);

impl Dirty {
    pub const ROUTES: u8 = 1 << 0;
    pub const FEATURES: u8 = 1 << 1;
    pub const RUINS: u8 = 1 << 2;
    pub const TERRITORY: u8 = 1 << 3;
    pub const DEPOSITS: u8 = 1 << 4;

    pub fn mark(&mut self, bits: u8) {
        self.0 |= bits;
    }
    /// Read-and-clear: true when any of `bits` was set.
    pub fn take(&mut self, bits: u8) -> bool {
        let hit = self.0 & bits != 0;
        self.0 &= !bits;
        hit
    }
    pub fn clear(&mut self, bits: u8) {
        self.0 &= !bits;
    }
}


impl Default for Event {
    fn default() -> Self {
        Event {
            m: 0,
            s: String::new(),
            // never observed: every construction site sets `k` explicitly
            // (audited — 26 `..Default::default()` sites, all override it)
            k: EventKind::Growth,
            text: String::new(),
            ids: SmallVec::new(),
            x: -1,
            y: -1,
            legend: String::new(),
            veiled: false,
        }
    }
}

pub struct World {
    pub seed: i64,
    pub size: usize,  // grid rows (the generated square base)
    pub width: usize, // grid columns after ocean margins are added
    pub month: i64,

    // E11.3 — the load-bearing walls: state lives in four sub-structs so
    // subsystems can borrow disjoint walls instead of eight loose params.
    /// The land itself — every per-cell grid (E3.2: f32 at rest).
    pub fields: Fields,
    /// The peoples — settlements, cultures, societies.
    pub peoples: Peoples,
    /// The coin — market, areas, merchants, realized route flow.
    pub economy: Economy,
    /// The record — events, registry, artifacts, dynastic state.
    pub chronicle: Chronicle,

    pub deposits: Vec<Deposit>,
    pub features: Vec<Feature>,
    pub routes: Vec<Route>,
    pub world_name: String,
    /// M9.1 — where towns died: named remains on the map.
    pub ruins: Vec<Ruin>,
    /// Years each route has gone without realized flow (M9.4).
    route_idle: Vec<u16>,
    /// M6.4 — narrative heat: decaying sum of the month's weighted events;
    /// quiet years reach for omens, loud years let the wars speak.
    heat: f64,

    pub(crate) rng: Pcg64Mcg,
    taken: HashSet<String>,
    /// Statecraft: wars, opinion, dread, solidarity, vassals (M4).
    pub politics: Politics,
    /// E4.5 — which wire sections must reship on the next tick.
    pub(crate) dirty: Dirty,
    /// E4.2/E4.3 — hashes of the last-shipped JSON per wire surface.
    pub(crate) sent: SentCache,
    /// E5.8 — reused serialization scratch for the tick payload; keeps its
    /// high-water capacity so `tick_json` stops paying growth reallocations.
    pub(crate) wire_buf: Vec<u8>,
    /// Deterministic drought field over (space × year) — the famine die (M2.6).
    pub(crate) drought: Perlin3,
    /// Last year grain was shock-priced by famine, to spike at most once a year.
    pub(crate) grain_shock_year: i64,
    site_score: Array2<f64>,
    food_grid: Array2<f64>,
    near_fresh: Array2<bool>,
    coast: Array2<bool>,
    max_settlements: usize,
    trade: trade::TradeGrid,
    pub timings: Vec<(&'static str, f64)>,
}

/// E7.4/E7.5 — world generation as a resumable ladder of stages. Each
/// `step()` runs exactly one stage and returns, so the wasm worker can post
/// progress and honour an abort between stages without threads or unwinding.
/// `World::generate_scaled` drives the same ladder start to finish: one code
/// path for native and staged generation, so determinism holds by
/// construction rather than by parallel maintenance.
pub struct GenBuilder {
    seed: i64,
    size: usize,
    precip_scale: f64,
    stage: usize,
    t0: f64,
    timings: Vec<(&'static str, f64)>,
    // f64 physics intermediates (dropped as soon as their stage is done)
    height64: Option<Array2<f64>>,
    water: Option<Array2<bool>>,
    tmean64: Option<Array2<f64>>,
    tamp64: Option<Array2<f64>>,
    precip64: Option<Array2<f64>>,
    pamp64: Option<Array2<f64>>,
    hydro: Option<hydrology::Hydrology>,
    biome_map: Option<Array2<u8>>,
    crops: Option<Array2<u8>>,
    // resting-width f32 fields (after the E3.2 drop)
    height: Option<Array2<f32>>,
    tmean: Option<Array2<f32>>,
    tamp: Option<Array2<f32>>,
    precip: Option<Array2<f32>>,
    pamp: Option<Array2<f32>>,
    discharge: Option<Array2<f32>>,
    flow_amp: Option<Array2<f32>>,
    fertility: Option<Array2<f32>>,
    // human intermediates
    features: Option<Vec<Feature>>,
    world_name: Option<String>,
    deposits: Option<Vec<Deposit>>,
    world: Option<World>,
}

impl GenBuilder {
    /// The ladder, in running order. Names double as progress labels.
    pub const STAGES: [&'static str; 9] = [
        "terrain",
        "erosion",
        "climate",
        "hydrology",
        "biomes",
        "fertility",
        "naming",
        "resources",
        "dawn",
    ];

    pub fn new(seed: i64, size: usize, precip_scale: f64) -> GenBuilder {
        GenBuilder {
            seed,
            size,
            precip_scale,
            stage: 0,
            t0: now_ms(),
            timings: Vec::new(),
            height64: None,
            water: None,
            tmean64: None,
            tamp64: None,
            precip64: None,
            pamp64: None,
            hydro: None,
            biome_map: None,
            crops: None,
            height: None,
            tmean: None,
            tamp: None,
            precip: None,
            pamp: None,
            discharge: None,
            flow_amp: None,
            fertility: None,
            features: None,
            world_name: None,
            deposits: None,
            world: None,
        }
    }

    pub fn done(&self) -> bool {
        self.world.is_some()
    }

    /// Stages completed so far.
    pub fn stage_index(&self) -> usize {
        self.stage
    }

    /// Run the next stage; returns its name. Panics if generation is done.
    pub fn step(&mut self) -> &'static str {
        let name = Self::STAGES[self.stage];
        match self.stage {
            0 => self.stage_terrain(),
            1 => self.stage_erosion(),
            2 => self.stage_climate(),
            3 => self.stage_hydrology(),
            4 => self.stage_biomes(),
            5 => self.stage_fertility(),
            6 => self.stage_naming(),
            7 => self.stage_resources(),
            8 => self.stage_dawn(),
            _ => panic!("generation already complete"),
        }
        self.stage += 1;
        name
    }

    /// Hand over the finished world. Panics unless `done()`.
    pub fn finish(&mut self) -> World {
        self.world.take().expect("generation not complete")
    }

    fn stage_terrain(&mut self) {
        let t = now_ms();
        self.height64 = Some(geo::heightmap(self.seed, self.size));
        self.timings.push(("terrain", now_ms() - t));
    }

    fn stage_erosion(&mut self) {
        let te = now_ms();
        let height = self.height64.as_mut().unwrap();
        erosion::erode(height);
        let water = height.mapv(|h| h < 0.0);
        self.water = Some(water);
        self.timings.push(("erosion", now_ms() - te));
    }

    fn stage_climate(&mut self) {
        let t1 = now_ms();
        let height = self.height64.as_ref().unwrap();
        let water = self.water.as_ref().unwrap();
        let lat = climate::latitude_deg(self.size);
        // E5.11 — one continentality (EDT) shared by amplitude + monsoon.
        let cont = climate::continentality(water);
        let tmean = climate::temperature_mean(height, &lat);
        let tamp = climate::temperature_amplitude(&lat, &cont);
        let (mut precip, pamp) = climate::precipitation(height, water, &tmean, &lat, &cont);
        if self.precip_scale != 1.0 {
            let s = self.precip_scale;
            precip.mapv_inplace(|p| p * s);
        }
        self.tmean64 = Some(tmean);
        self.tamp64 = Some(tamp);
        self.precip64 = Some(precip);
        self.pamp64 = Some(pamp);
        self.timings.push(("climate", now_ms() - t1));
    }

    fn stage_hydrology(&mut self) {
        let t2 = now_ms();
        let hydro = hydrology::hydrology(
            self.height64.as_ref().unwrap(),
            self.water.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            self.pamp64.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
        );
        self.hydro = Some(hydro);
        self.timings.push(("hydrology", now_ms() - t2));
    }

    fn stage_biomes(&mut self) {
        let t3 = now_ms();
        let biome_map = biomes_mod::classify(
            self.height64.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            &self.hydro.as_ref().unwrap().lakes,
        );
        self.biome_map = Some(biome_map);
        self.timings.push(("biomes", now_ms() - t3));
    }

    fn stage_fertility(&mut self) {
        let t4 = now_ms();
        {
            let height = self.height64.as_ref().unwrap();
            let tmean = self.tmean64.as_ref().unwrap();
            let precip = self.precip64.as_ref().unwrap();
            let hydro = self.hydro.as_ref().unwrap();
            let fert = agriculture::fertility(
                height,
                tmean,
                precip,
                &hydro.rivers,
                &hydro.lakes,
                &hydro.discharge,
            );
            let crops =
                agriculture::crop_packages(height, tmean, precip, &hydro.rivers, &hydro.lakes);
            self.fertility = Some(fert.mapv(|x| x as f32));
            self.crops = Some(crops);
        }
        self.timings.push(("fertility", now_ms() - t4));

        // E3.2 — the physical stages are done; the world's float grids
        // drop to their resting f32 width here, and every human stage
        // below (naming, resources, settlements, trade, economy) reads
        // the same f32 the ticks will read.
        self.height = Some(self.height64.take().unwrap().mapv(|x| x as f32));
        self.tmean = Some(self.tmean64.take().unwrap().mapv(|x| x as f32));
        self.tamp = Some(self.tamp64.take().unwrap().mapv(|x| x as f32));
        self.precip = Some(self.precip64.take().unwrap().mapv(|x| x as f32));
        self.pamp = Some(self.pamp64.take().unwrap().mapv(|x| x as f32));
        let discharge = self.hydro.as_ref().unwrap().discharge.mapv(|x| x as f32);
        let flow_amp = self.hydro.as_ref().unwrap().flow_amp.mapv(|x| x as f32);
        self.discharge = Some(discharge);
        self.flow_amp = Some(flow_amp);
        self.water = None;
    }

    fn stage_naming(&mut self) {
        let t5 = now_ms();
        let (features, world_name) = naming::name_features(
            self.height.as_ref().unwrap(),
            self.biome_map.as_ref().unwrap(),
            &self.hydro.as_ref().unwrap().rivers,
            &self.hydro.as_ref().unwrap().lakes,
            self.discharge.as_ref().unwrap(),
            self.tmean.as_ref().unwrap(),
            self.precip.as_ref().unwrap(),
            self.seed,
        );
        self.features = Some(features);
        self.world_name = Some(world_name);
        self.timings.push(("naming", now_ms() - t5));
    }

    fn stage_resources(&mut self) {
        let t6 = now_ms();
        let deposits = resources::place_resources(
            self.biome_map.as_ref().unwrap(),
            self.height.as_ref().unwrap(),
            &self.hydro.as_ref().unwrap().rivers,
            &self.hydro.as_ref().unwrap().lakes,
            self.seed,
        );
        self.deposits = Some(deposits);
        self.timings.push(("resources", now_ms() - t6));
    }

    fn stage_dawn(&mut self) {
        let seed = self.seed;
        let size = self.size;
        let t0 = self.t0;
        let mut timings = std::mem::take(&mut self.timings);
        let height = self.height.take().unwrap();
        let tmean = self.tmean.take().unwrap();
        let tamp = self.tamp.take().unwrap();
        let precip = self.precip.take().unwrap();
        let pamp = self.pamp.take().unwrap();
        let discharge = self.discharge.take().unwrap();
        let flow_amp = self.flow_amp.take().unwrap();
        let fertility = self.fertility.take().unwrap();
        let biome_map = self.biome_map.take().unwrap();
        let crops = self.crops.take().unwrap();
        let hydro = self.hydro.take().unwrap();
        let mut features = self.features.take().unwrap();
        let world_name = self.world_name.take().unwrap();
        let mut deposits = self.deposits.take().unwrap();

        let t7 = now_ms();
        let mut taken: HashSet<String> = HashSet::new();
        let mut rng9000 = crate::util::rng(seed + 9000);
        let founded = settlements::found_settlements(
            &height,
            &biome_map,
            &tmean,
            &hydro.rivers,
            &hydro.lakes,
            &discharge,
            &deposits,
            &fertility,
            &mut rng9000,
            &mut taken,
        );
        let mut setts = founded.settlements;
        // The first peoples know the ground they chose: seams close to a
        // founding site begin the story already discovered.
        {
            use rand::Rng;
            let mut rng6 = crate::util::rng(seed + 6600);
            for d in deposits.iter_mut() {
                if d.known {
                    continue;
                }
                for s in &setts {
                    let r = settlements::work_radius(s.pop);
                    let dx = (d.x - s.x) as f64;
                    let dy = (d.y - s.y) as f64;
                    if dx * dx + dy * dy <= r * r && rng6.gen::<f64>() < 0.6 {
                        d.known = true;
                        break;
                    }
                }
            }
        }
        // Dawn carrying capacity from the crop grid (no arts yet: kaplan=1).
        for s in setts.iter_mut() {
            s.k = round2(settlements::capacity_at(
                &crops,
                &fertility,
                s.y as usize,
                s.x as usize,
                s.coastal,
                s.food,
                1.0,
                1.0,
            ));
        }
        let cultures = culture::assign_cultures(&biome_map, &mut setts, &mut taken, seed);
        trade::assign_goods(&mut setts, &deposits, &fertility);

        // E1.7 — fold the four hydrology masks into one CellFlags byte grid;
        // this is the exact byte the pack ships, so pack() is now a memcpy.
        let flags = {
            let mut f = Array2::<u8>::zeros(hydro.rivers.dim());
            for (o, (((&r, &l), &s), &sw)) in f.iter_mut().zip(
                hydro
                    .rivers
                    .iter()
                    .zip(hydro.lakes.iter())
                    .zip(hydro.salt.iter())
                    .zip(hydro.seasonal.iter()),
            ) {
                let mut c = CellFlags::empty();
                c.set(CellFlags::RIVER, r);
                c.set(CellFlags::LAKE, l);
                c.set(CellFlags::SALT, s);
                c.set(CellFlags::SEASONAL, sw);
                *o = c.bits();
            }
            f
        };

        let trade_grid = trade::TradeGrid::build(
            &height,
            &flags,
            &biome_map,
            &discharge,
            (size / 128).max(1),
        );
        let routes = trade::build_routes(
            &trade_grid,
            &mut setts,
            &height,
            &flags,
            &discharge,
            &flow_amp,
        );
        trade::mark_ports(&mut setts, &routes);
        // The roads themselves name the land: passes where they climb,
        // fords where they cross the great rivers.
        naming::name_route_features(
            &mut features,
            &mut rng9000,
            &mut taken,
            &routes,
            &height,
            &hydro.rivers,
            &discharge,
        );
        // M3.1/M3.4 — the peoples lay their tongues over the nearby land;
        // border features gain an exonym from the second-closest people.
        naming::culture_toponyms(&mut features, &setts, &cultures, &mut taken, seed);
        let societies = society::init(&cultures);
        let mut market = Market::default();
        economy::update_prices(&mut market, &setts);
        // the first carve of the market areas (M5.2)
        let sidx0 = economy::sidx(&setts);
        let mut areas = economy::build_areas(&setts, &routes, None, &sidx0);
        economy::update_area_prices(&mut areas, &setts, &market);
        timings.push(("settlements", now_ms() - t7));

        let mut rng = crate::util::rng(seed + 777);

        // The cast enters the telling: peoples, towns and the named land
        // each get one stable id for their whole life (M6.1). The world
        // itself is entity zero, so even the creation myth has a subject.
        let mut registry = Registry::default();
        registry.add(EntityKind::World, &world_name, 0, None, -1, -1);
        for c in &cultures {
            registry.add(EntityKind::Culture, &c.people, 0, Some(c.id), -1, -1);
        }
        let sett_ents: Vec<EntityId> = setts
            .iter()
            .map(|s| registry.add(EntityKind::Settlement, &s.name, 0, Some(s.culture), s.x, s.y))
            .collect();
        for f in &features {
            registry.add(EntityKind::Feature, &f.name, 0, None, f.x, f.y);
        }
        // The goods trade under their own names: market shocks, gluts and
        // strikes all speak of them, so they join the cast too (M6.1).
        // Legacy creation order: placeables first, then the produced goods.
        for g in resources::ALL_PLACEABLE.iter().copied().chain([
            resources::Good::Grain,
            resources::Good::Tools,
            resources::Good::Weapons,
            resources::Good::Jewelry,
        ]) {
            registry.add(EntityKind::Good, g.name(), 0, None, -1, -1);
        }

        let mut chron = ChronicleState::default();
        chron.rulers = chronicle::init_rulers(&mut rng, &cultures, &mut taken, &mut registry);

        let mut events: Vec<Event> =
            chronicle::founding_myths(&mut rng, &cultures, &features, &world_name);
        for (si, s) in setts.iter().enumerate() {
            let people = if !cultures.is_empty() {
                cultures[s.culture.idx()].people.clone()
            } else {
                "first peoples".to_string()
            };
            let suffix = if s.coastal {
                " by the coast."
            } else if s.river {
                " on fresh water."
            } else {
                "."
            };
            events.push(Event {
                m: 0,
                s: s.name.clone(),
                k: EventKind::Found,
                text: format!("{} founded by the {}{}", s.name, people, suffix),
                ids: smallvec![sett_ents[si]],
                x: s.x,
                y: s.y,
                ..Default::default()
            });
        }
        timings.push(("total", now_ms() - t0));

        let n_cultures = cultures.len();
        let mut world = World {
            seed,
            size,
            width: size,
            month: 0,
            height,
            tmean,
            tamp,
            precip,
            discharge,
            fertility,
            biomes: biome_map,
            crops,
            flags,
            pamp,
            flow_amp,
            strahler: hydro.strahler,
            deposits,
            settlements: setts,
            cultures,
            features,

            routes,
            events,
            world_name,
            societies,
            market,
            areas,
            merchants: Vec::new(),
            route_flow: Vec::new(),
            ruins: Vec::new(),
            route_idle: Vec::new(),
            registry,
            artifacts: Vec::new(),
            heat: 0.0,
            rng,
            taken,
            chron,
            politics: Politics::init(n_cultures),
            territory: Array2::from_elem((1, 1), -1),
            dirty: Dirty::default(),
            sent: SentCache::default(),
            wire_buf: Vec::new(),
            drought: Perlin3::new(seed + 4444),
            grain_shock_year: -1,
            site_score: founded.site_score,
            food_grid: founded.food_grid,
            near_fresh: founded.near_fresh,
            coast: founded.coast,
            max_settlements: founded.max_settlements,
            trade: trade_grid,
            timings,
        };
        // Open-ocean margins east and west: the world breathes a little wider.
        world.widen(size / 8);
        // The dawn's own entries join the telling: subjects resolved to
        // registry ids, coordinates backfilled, great deeds legendized (M6).
        let mut dawn = std::mem::take(&mut world.chronicle.events);
        world.resolve_events(0, &mut dawn);
        world.chronicle.events = dawn;
        // The first political map: who holds what, before any drum beats.
        world.recompute_territory();
        world.dirty.clear(Dirty::TERRITORY); // ships with the pack, not the first tick
        // Seed the delta baseline (E4.2/E4.3) to what bootstrap() ships.
        world.prime_sent();
        self.world = Some(world);
    }
}

impl World {
    pub fn generate(seed: i64, size: usize) -> World {
        World::generate_scaled(seed, size, 1.0)
    }

    /// `generate` with a rainfall multiplier — the metamorphic-testing knob
    /// (M8.2): the harness generates the same seed wetter and asserts the
    /// rivers do not shrink. 1.0 is the game; nothing else ships.
    /// Drives the `GenBuilder` ladder start to finish — the exact code path
    /// the staged wasm build runs, one stage per `step()`.
    pub fn generate_scaled(seed: i64, size: usize, precip_scale: f64) -> World {
        let mut b = GenBuilder::new(seed, size, precip_scale);
        while !b.done() {
            b.step();
        }
        b.finish()
    }

    /// M4.1 — redraw the influence map after borders move or towns grow.
    pub fn recompute_territory(&mut self) {
        self.fields.territory = politics::influence_map(
            &self.fields.height,
            &self.peoples.settlements,
            &self.peoples.societies,
            &self.politics.asab,
            self.peoples.cultures.len(),
        );
        self.dirty.mark(Dirty::TERRITORY);
    }

    /// Grow the map horizontally: every grid gains `pad` ocean columns on both
    /// sides and every coordinate shifts east by `pad`. The simulation keeps
    /// running in the widened frame, so colonies, routes and labels all agree.
    fn widen(&mut self, pad: usize) {
        if pad == 0 {
            return;
        }
        let (h, w) = self.fields.height.dim();
        let p = pad as isize;

        /// Widen any copyable grid: interior cells shift east by `pad`,
        /// margin cells get `margin(edge_value, t)` with t rising 0→1
        /// toward the map border. Serves both f32 world grids and the
        /// f64 tick caches.
        fn grow<T: Copy>(
            a: &Array2<T>,
            pad: usize,
            margin: impl Fn(T, f64) -> T,
        ) -> Array2<T> {
            let (h, w) = a.dim();
            let p = pad as isize;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    let (edge, k) = if xi < 0 {
                        (a[[y, 0]], (-xi) as f64)
                    } else {
                        (a[[y, w - 1]], (xi as usize - (w - 1)) as f64)
                    };
                    margin(edge, k / pad as f64)
                }
            })
        }
        fn grow_bool(a: &Array2<bool>, pad: usize) -> Array2<bool> {
            let (h, w) = a.dim();
            let p = pad as isize;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                xi >= 0 && (xi as usize) < w && a[[y, xi as usize]]
            })
        }

        // Bathymetry: slide from the coastal edge down toward open deep sea.
        self.fields.height = grow(&self.fields.height, pad, |e, t| {
            let shelf = e.min(-0.03);
            let deep = (-0.62_f32).min(e);
            shelf + (deep - shelf) * t as f32
        });
        // Climate margins keep zonal continuity by extending the edge column.
        self.fields.tmean = grow(&self.fields.tmean, pad, |e, _| e);
        self.fields.tamp = grow(&self.fields.tamp, pad, |e, _| e);
        self.fields.precip = grow(&self.fields.precip, pad, |e, _| e);
        self.fields.discharge = grow(&self.fields.discharge, pad, |_, _| 0.0);
        self.fields.fertility = grow(&self.fields.fertility, pad, |_, _| 0.0);
        self.site_score = grow(&self.site_score, pad, |_, _| 0.0);
        self.food_grid = grow(&self.food_grid, pad, |_, _| 0.0);
        self.fields.biomes = {
            let a = &self.fields.biomes;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0 // open water
                }
            })
        };
        self.fields.crops = {
            let a = &self.fields.crops;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0 // open water grows nothing
                }
            })
        };
        self.fields.flags = {
            let a = &self.fields.flags;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0 // open water: no river, lake, salt or wadi
                }
            })
        };
        self.near_fresh = grow_bool(&self.near_fresh, pad);
        self.coast = grow_bool(&self.coast, pad);
        self.fields.pamp = grow(&self.fields.pamp, pad, |e, _| e);
        self.fields.flow_amp = grow(&self.fields.flow_amp, pad, |_, _| 0.0);
        self.fields.strahler = {
            let a = &self.fields.strahler;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0
                }
            })
        };

        // Downsampled trade grid: margins are open blue-water lanes.
        let dpad = pad / self.trade.f;
        let (dh, dw) = self.trade.cost.dim();
        let dp = dpad as isize;
        let tc = self.trade.cost.clone();
        self.trade.cost = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                tc[[y, xi as usize]]
            } else {
                trade::OPEN_SEA_COST
            }
        });
        let ts = self.trade.sea.clone();
        self.trade.sea = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                ts[[y, xi as usize]]
            } else {
                true
            }
        });

        // Everything with an x slides east.
        let shift = pad as i64;
        for s in self.peoples.settlements.iter_mut() {
            s.x += shift;
        }
        for d in self.deposits.iter_mut() {
            d.x += shift;
        }
        for f in self.features.iter_mut() {
            f.x += shift;
        }
        for r in self.routes.iter_mut() {
            for pt in r.path.iter_mut() {
                pt[0] += shift;
            }
        }
        self.chronicle.registry.shift_x(shift);
        for e in self.chronicle.events.iter_mut() {
            if e.x >= 0 {
                e.x += shift;
            }
        }
        self.width = w + 2 * pad;
    }

    /// One month of growth for every settlement; returns events.
    fn tick_month(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        let month = month_abs.rem_euclid(12);
        let mods: Vec<society::Mods> =
            self.peoples.societies.iter().map(society::mods_for).collect();
        // M2.3: the seat of kings — each culture's greatest town keeps a
        // court, and courts import: grain barges, tribute, hungry retinues.
        // The head of the rank-size curve is political as much as economic.
        let mut seat: Vec<usize> = Vec::new();
        for (i, s) in self.peoples.settlements.iter().enumerate() {
            while seat.len() <= s.culture.0 {
                seat.push(usize::MAX);
            }
            if seat[s.culture.idx()] == usize::MAX
                || s.pop > self.peoples.settlements[seat[s.culture.idx()]].pop
            {
                seat[s.culture.idx()] = i;
            }
        }
        for (si, s) in self.peoples.settlements.iter_mut().enumerate() {
            let md = mods.get(s.culture.idx()).cloned().unwrap_or_default();
            let (y, x) = (s.y as usize, s.x as usize);
            let t_now =
                climate::month_temperature(self.fields.tmean[[y, x]] as f64, self.fields.tamp[[y, x]] as f64, month);
            // NOTE: this growth chain is mirrored by explain.rs — change both.
            // ~6%/yr at best: the world should still be filling in at year
            // 100, not saturated by year 45 with a century of flat plateau.
            let mut r = 0.005;
            if t_now < -8.0 {
                r *= 0.25;
            } else if t_now < 0.0 {
                r *= 0.6;
            }
            r *= 1.0 + 0.04 * (s.connections.min(4) as f64); // trade bonus
            r *= md.growth; // the plough, law, and the arts of peace
            r *= 1.0 + 0.04 * (s.wealth / (s.pop as f64 + 1.0)).min(1.0); // coin draws folk
            // M2.2: capacity from the crop package + arts. Single formula
            // site (settlements::capacity_at); explain.rs reads stored s.k.
            let mut k = settlements::capacity_at(
                &self.fields.crops,
                &self.fields.fertility,
                y,
                x,
                s.coastal,
                s.food,
                md.kaplan,
                md.capacity,
            );
            // M2.3: market towns import grain — the web of trade lifts K,
            // and the fat head of the rank-size curve lives in the hubs.
            k *= 1.0 + 0.26 * (s.connections.min(8) as f64);
            // NOTE: mirrored by explain.rs — the court term rides on s.k.
            if seat.get(s.culture.idx()) == Some(&si) {
                k *= 1.6; // the court eats what the realm sends
            }
            s.k = round2(k);
            let mut pop = s.pop;
            let mut growth = pop as f64 * r * (1.0 - pop as f64 / k);
            // harsh winter shock — softened by herb-lore and medicine
            if t_now < -14.0 && self.rng.gen::<f64>() < 0.10 && pop > 60 {
                let loss = (pop as f64 * self.rng.gen_range(0.02..0.06) * md.health) as i64;
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("A brutal winter grips {} — {} lost.", s.name, loss),
                    ..Default::default()
                });
            }
            // plague finds the crowded streets — aqueducts and physicians push back
            if pop > 2200 && self.rng.gen::<f64>() < 0.004 {
                let loss = (pop as f64 * self.rng.gen_range(0.06..0.16) * md.health) as i64;
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("Plague stalks the streets of {} — {} souls perish.", s.name, loss),
                    ..Default::default()
                });
            }
            // the earth shakes in the high country — dressed stone stands longer
            if self.fields.height[[y, x]] > 0.42 && pop > 120 && self.rng.gen::<f64>() < 0.0012 {
                let loss =
                    ((pop as f64 * self.rng.gen_range(0.03..0.09) * md.defense) as i64).max(3);
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("The earth shakes beneath {} — walls fall, {} are lost.", s.name, loss),
                    ..Default::default()
                });
            }
            // fire leaps the rooftops in the dry season — stone burns slower
            if (5..=7).contains(&month)
                && self.fields.precip[[y, x]] < 700.0
                && pop > 350
                && self.rng.gen::<f64>() < 0.0025
            {
                let loss =
                    ((pop as f64 * self.rng.gen_range(0.02..0.06) * md.defense) as i64).max(3);
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("Fire leaps the rooftops of {}; {} perish in the smoke.", s.name, loss),
                    ..Default::default()
                });
            }
            // the spring melt bursts the banks
            if s.river && (2..=4).contains(&month) && pop > 150 && self.rng.gen::<f64>() < 0.002 {
                let loss = ((pop as f64 * self.rng.gen_range(0.01..0.04)) as i64).max(2);
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("The river bursts its banks at {} — {} swept away in the brown water.", s.name, loss),
                    ..Default::default()
                });
            }
            // black autumn storms off the open sea
            if s.coastal && (8..=10).contains(&month) && pop > 150 && self.rng.gen::<f64>() < 0.002 {
                let loss = ((pop as f64 * self.rng.gen_range(0.01..0.05)) as i64).max(2);
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Disaster,
                    text: format!("A black storm off the open sea lashes {} — {} lost to the waves.", s.name, loss),
                    ..Default::default()
                });
            }
            // a golden harvest, in high summer, on good soil
            if month == 6 && s.food > 2.2 && self.rng.gen::<f64>() < 0.05 {
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Growth,
                    text: format!("The harvest overflows in {}; granaries groan.", s.name),
                    ..Default::default()
                });
                growth *= 2.0;
            }
            // markets overflow where many roads meet
            if s.connections >= 3 && pop > 400 && self.rng.gen::<f64>() < 0.0015 {
                let good = s.exports.unwrap_or(resources::Good::Grain);
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: EventKind::Trade,
                    text: format!("Caravans crowd the gates of {}; {} flows out to every shore.", s.name, good),
                    ..Default::default()
                });
                growth += pop as f64 * 0.004;
            }
            // a harbour draws trade, sailors and coin
            if s.port {
                growth += pop as f64 * 0.0012;
            }
            // M9.1 — the emigration spiral. A town pinned for years below
            // two-fifths of its own peak has lost the reason people stayed;
            // from there the young follow the trade that already left, and
            // each empty house empties the next. Entry demands persistence
            // — five straight years under the line with hysteresis — so a
            // plague year or a Gibrat dip never kills a town that would
            // have recovered; only the true has-beens, whose capacity fell
            // and stayed fallen, slide in. The slide itself takes a
            // generation — long enough for the chronicle to watch it die.
            if !s.failing && s.peak > 240 && month_abs - s.born >= 240 {
                if pop * 5 < s.peak * 2 {
                    s.ail = s.ail.saturating_add(1);
                } else if pop * 20 > s.peak * 9 {
                    s.ail = 0; // back over 45 %: the ailment lifts
                }
                if s.ail >= 60 {
                    s.failing = true;
                    events.push(Event {
                        m: month_abs,
                        s: s.name.clone(),
                        k: EventKind::Realm,
                        text: format!(
                            "The young of {} take the roads out; houses stand empty by the gate and no one bids on them.",
                            s.name
                        ),
                        x: s.x,
                        y: s.y,
                        ..Default::default()
                    });
                }
            }
            if s.failing && pop * 2 > s.peak {
                s.failing = false; // against the tide: the town found new life
                s.ail = 0;
            }
            if s.failing {
                // decay beats growth: −2.2 %/month, a ~19-year half-death
                growth = -(pop as f64 * 0.022).max(2.0);
            }
            // M2.3 Gibrat: a size-free multiplicative lottery on top of the
            // logistic mean — Zipf's rank-size law emerges from it, unforced.
            let gibrat = 1.0 + 0.045 * (2.0 * self.rng.gen::<f64>() - 1.0);
            pop = (((pop as f64 + growth) * gibrat).round() as i64).max(20);
            let old_tier = s.tier.clone();
            s.pop = pop;
            s.tier = settlements::tier(pop);
            if s.tier != old_tier {
                let rank = |t: &str| {
                    settlements::TIERS
                        .iter()
                        .position(|(_, n)| *n == t)
                        .unwrap_or(0)
                };
                if rank(&s.tier) > rank(&old_tier) {
                    events.push(Event {
                        m: month_abs,
                        s: s.name.clone(),
                        k: EventKind::Growth,
                        text: format!("{} has grown into a {}.", s.name, s.tier.to_lowercase()),
                        ..Default::default()
                    });
                    // rising tier: something worth singing about may be raised
                    let wonders = chronicle::wonder_for(
                        &mut self.chronicle.state,
                        &mut self.rng,
                        s,
                        &self.peoples.cultures,
                        month_abs,
                    );
                    events.extend(wonders);
                } else {
                    events.push(Event {
                        m: month_abs,
                        s: s.name.clone(),
                        k: EventKind::Disaster,
                        text: format!("{} dwindles to a {}.", s.name, s.tier.to_lowercase()),
                        ..Default::default()
                    });
                }
            }
        }
        events
    }

    /// Crowded settlements send out settlers to found colonies.
    /// The pull of unworked riches: known seams nobody yet mines project a
    /// price-weighted attraction for colonists. When the market runs hot a
    /// vein can outweigh thin soil, and the mining camps follow.
    fn resource_pull(&self) -> Array2<f64> {
        let (h, w) = self.site_score.dim();
        let mut pull = Array2::<f64>::zeros((h, w));
        const R: i64 = 5;
        for d in &self.deposits {
            if !d.known || d.left == 0.0 {
                continue;
            }
            // renewables draw no rush; it is metal, coal and stone that call
            if !d.r.is_mineral() {
                continue;
            }
            let claimed = self.peoples.settlements.iter().any(|s| {
                let r = settlements::work_radius(s.pop);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                dx * dx + dy * dy <= r * r
            });
            if claimed {
                continue;
            }
            // far ore must out-pull farmland or nobody ever leaves the plough
            let worth = self.economy.market.price(d.r) * d.rich * 2.2;
            for yy in (d.y - R).max(0)..=(d.y + R).min(h as i64 - 1) {
                for xx in (d.x - R).max(0)..=(d.x + R).min(w as i64 - 1) {
                    if self.fields.height[[yy as usize, xx as usize]] < 0.0 {
                        continue; // no camps on the water
                    }
                    let dist = (((yy - d.y).pow(2) + (xx - d.x).pow(2)) as f64).sqrt();
                    if dist > R as f64 {
                        continue;
                    }
                    let v = worth * (1.0 - dist / (R as f64 + 1.0));
                    let c = &mut pull[[yy as usize, xx as usize]];
                    *c = (*c + v).min(7.0);
                }
            }
        }
        pull
    }

    fn try_colonize(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        let mut founded = false;
        let mut pull: Option<Array2<f64>> = None;
        let initial = self.peoples.settlements.len();
        let mods_v: Vec<society::Mods> =
            self.peoples.societies.iter().map(society::mods_for).collect();
        // ore-led ventures may spill past the cap into a reserved band:
        // the seams don't care that the census is full.
        let hard_cap = self.max_settlements + self.max_settlements / 4;
        for pi in 0..initial {
            if self.peoples.settlements.len() >= hard_cap {
                break;
            }
            let (ppop, pcap, pname) = {
                let p = &self.peoples.settlements[pi];
                // the hunger for land is measured against what the LAND
                // carries — not the import-lifted ceiling stored in s.k
                // (hub and court terms), which would gate colonists on
                // grain barges that feed the city just fine.
                let md = mods_v.get(p.culture.idx()).cloned().unwrap_or_default();
                let kland = settlements::capacity_at(
                    &self.fields.crops,
                    &self.fields.fertility,
                    p.y as usize,
                    p.x as usize,
                    p.coastal,
                    p.food,
                    md.kaplan,
                    md.capacity,
                );
                (p.pop, kland.max(180.0), p.name.clone())
            };
            if ppop < 380 || (ppop as f64) < 0.72 * pcap {
                continue;
            }
            if self.rng.gen::<f64>() > 0.02 {
                continue;
            }
            if pull.is_none() {
                pull = Some(self.resource_pull());
            }
            let site = {
                let parent = self.peoples.settlements[pi].clone();
                let range = self
                    .societies
                    .get(parent.culture.idx())
                    .map(|so| society::mods_for(so).colony_range)
                    .unwrap_or(1.0);
                settlements::colony_site(
                    &self.site_score,
                    pull.as_ref().unwrap(),
                    &self.peoples.settlements,
                    &parent,
                    3600.0 * range * range,
                )
            };
            let Some((y, x)) = site else { continue };
            // an ore-led venture: the seams called louder than the soil
            let ore_led = pull.as_ref().unwrap()[[y, x]] > self.site_score[[y, x]].max(0.0);
            // past the soft cap only miners still sail
            if self.peoples.settlements.len() >= self.max_settlements && !ore_led {
                continue;
            }
            let migrants = ((ppop as f64 * self.rng.gen_range(0.08..0.14)) as i64).max(40);
            self.peoples.settlements[pi].pop = (ppop - migrants).max(60);
            let cid = self.peoples.settlements[pi].culture;
            let idx = self.found_settlement(y, x, migrants, cid);
            founded = true;
            let name = self.peoples.settlements[idx].name.clone();
            let coastal = self.peoples.settlements[idx].coastal;
            let river = self.peoples.settlements[idx].river;
            let text = if ore_led {
                format!(
                    "Settlers out of {} drive the mining camp of {} into hungry country — the seams there outweigh the thin soil.",
                    pname, name
                )
            } else {
                let place = if coastal {
                    " by the sea."
                } else if river {
                    " on fresh water."
                } else {
                    " in the wilds."
                };
                format!("Settlers out of {} raise {}{}", pname, name, place)
            };
            events.push(Event {
                m: month_abs,
                s: name.clone(),
                k: EventKind::Found,
                text,
                ..Default::default()
            });
        }
        (events, founded)
    }

    /// Raise a new settlement at (y, x): coin a name in the founding
    /// culture's style, list its goods, size its land, and wire it into
    /// the trade web. Shared by colonists and rush camps alike.
    fn found_settlement(&mut self, y: usize, x: usize, migrants: i64, cid: CultureId) -> usize {
        let style = if !self.peoples.cultures.is_empty() {
            self.peoples.cultures[cid.0].style.clone()
        } else {
            "hellenic".to_string()
        };
        let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
        let new_id = SettlementId(self.peoples.settlements.iter().map(|o| o.id.0).max().unwrap_or(-1) + 1);
        let mut s = Settlement {
            id: new_id,
            name: coined.word.clone(),
            x: x as i64,
            y: y as i64,
            pop: migrants,
            tier: settlements::tier(migrants),
            food: settlements::site_food(
                &self.food_grid,
                &self.fields.fertility,
                &self.near_fresh,
                &self.coast,
                y,
                x,
            ),
            k: 0.0,
            coastal: self.coast[[y, x]],
            river: self.near_fresh[[y, x]],
            culture: cid,
            namer: cid,
            connections: 0,
            goods: resources::Goods::new(),
            exports: None,
            wealth: round2(migrants as f64 * 0.2),
            port: false,
            ety: coined.ety,
            fort: 0,
            formerly: Vec::new(),
            peak: migrants,
            born: self.month,
            failing: false,
            ail: 0,
        };
        trade::goods_for(&mut s, &self.deposits, &self.fields.fertility);
        let mdc = self
            .societies
            .get(cid.0)
            .map(society::mods_for)
            .unwrap_or_default();
        s.k = round2(settlements::capacity_at(
            &self.fields.crops,
            &self.fields.fertility,
            y,
            x,
            s.coastal,
            s.food,
            mdc.kaplan,
            mdc.capacity,
        ));
        self.peoples.settlements.push(s);
        let idx = self.peoples.settlements.len() - 1;
        // the new town enters the telling (M6.1)
        {
            let t = &self.peoples.settlements[idx];
            self.chronicle.registry
                .add(EntityKind::Settlement, &t.name, self.month, Some(t.culture), t.x, t.y);
        }
        trade::connect_settlement(
            idx,
            &mut self.peoples.settlements,
            &mut self.routes,
            &self.trade,
            &self.fields.height,
            &self.fields.flags,
            &self.fields.discharge,
            &self.fields.flow_amp,
        );
        idx
    }

    /// The rush: a rich seam, known but unworked, calls chancers on its
    /// own. Where colonists weigh soil against distance, rushers weigh
    /// only the price of metal — a camp springs up hard by the diggings,
    /// peopled from the nearest town. This is the channel that reaches
    /// ore struck by far ventures in country no crowded parent would
    /// ever pick: found metal must reach the market, not rust in the hills.
    fn try_rush_camps(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        use rand::Rng;
        let mut events = Vec::new();
        let mut founded = false;
        let hard_cap = self.max_settlements + self.max_settlements / 4;
        let (rows, cols) = self.site_score.dim();
        for di in 0..self.deposits.len() {
            if self.peoples.settlements.len() >= hard_cap {
                break;
            }
            let d = &self.deposits[di];
            if !d.known || d.left == 0.0 {
                continue;
            }
            if !d.r.is_mineral() {
                continue;
            }
            // a claimed seam already has crews — no rush to a worked pit
            let claimed = self.peoples.settlements.iter().any(|s| {
                let r = settlements::work_radius(s.pop);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                dx * dx + dy * dy <= r * r
            });
            if claimed {
                continue;
            }
            // the pull of the price: dearer metal, richer seam, faster rush
            let worth = (self.economy.market.price(d.r) * d.rich / 2.0).clamp(0.2, 3.0);
            let (dx0, dy0) = (d.x, d.y);
            let kind = d.r;
            if self.rng.gen::<f64>() >= 0.0045 * worth {
                continue;
            }
            // the best land within a day's walk of the diggings, clear of towns
            const R: i64 = 6;
            let min_d2 =
                settlements::MIN_TOWN_SPACING_CELLS * settlements::MIN_TOWN_SPACING_CELLS;
            let mut best = f64::NEG_INFINITY;
            let mut site: Option<(usize, usize)> = None;
            for yy in (dy0 - R).max(0)..=(dy0 + R).min(rows as i64 - 1) {
                for xx in (dx0 - R).max(0)..=(dx0 + R).min(cols as i64 - 1) {
                    if self.fields.height[[yy as usize, xx as usize]] < 0.0 {
                        continue;
                    }
                    let clear = self.peoples.settlements.iter().all(|o| {
                        let ddy = yy as f64 - o.y as f64;
                        let ddx = xx as f64 - o.x as f64;
                        ddy * ddy + ddx * ddx >= min_d2
                    });
                    if !clear {
                        continue;
                    }
                    let sc = self.site_score[[yy as usize, xx as usize]];
                    if sc > best {
                        best = sc;
                        site = Some((yy as usize, xx as usize));
                    }
                }
            }
            let Some((y, x)) = site else { continue };
            // souls from the nearest town — every rush empties somebody's inn
            let Some(src) = (0..self.peoples.settlements.len()).min_by_key(|&i| {
                let s = &self.peoples.settlements[i];
                (s.x - dx0).pow(2) + (s.y - dy0).pow(2)
            }) else {
                continue;
            };
            let spop = self.peoples.settlements[src].pop;
            if spop < 240 {
                continue;
            }
            let migrants = ((spop as f64 * 0.08) as i64).clamp(60, 240);
            self.peoples.settlements[src].pop = spop - migrants;
            let cid = self.peoples.settlements[src].culture;
            let sname = self.peoples.settlements[src].name.clone();
            let idx = self.found_settlement(y, x, migrants, cid);
            founded = true;
            let name = self.peoples.settlements[idx].name.clone();
            events.push(Event {
                m: month_abs,
                s: name.clone(),
                k: EventKind::Found,
                text: format!(
                    "Word of the {} above {} draws chancers and mule-trains — the camp of {} springs up hard by the diggings.",
                    kind, sname, name
                ),
                ..Default::default()
            });
        }
        (events, founded)
    }


    // ================= M9 — the patina =================

    /// Everything time does to the map (M9): battlefields earn names and
    /// conquerors lay new ones as politics hands them over; once a year
    /// the dying are gathered to ruin and the idle roads counted; twice a
    /// century speech wears the oldest names smooth; and rarely the world
    /// does something it will never explain.
    fn patina_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut evs: Vec<Event> = Vec::new();
        self.drain_battle_marks(month_abs, &mut evs);
        self.drain_conquest_renames(month_abs, &mut evs);
        if month_abs.rem_euclid(12) == 5 {
            self.abandonment_pass(month_abs, &mut evs);
        }
        if month_abs.rem_euclid(12) == 9 {
            self.route_age_pass(month_abs, &mut evs);
        }
        if month_abs.rem_euclid(1200) == 600 {
            self.erosion_pass(month_abs, &mut evs);
        }
        // Berúthiel emissions (M9.5): rare, anchored, never explained.
        if !self.peoples.settlements.is_empty() && self.rng.gen::<f64>() < 0.007 {
            let i = self.rng.gen_range(0..self.peoples.settlements.len());
            let (name, people, x, y) = {
                let s = &self.peoples.settlements[i];
                (
                    s.name.clone(),
                    self.peoples.cultures[s.culture.idx()].people.clone(),
                    s.x,
                    s.y,
                )
            };
            let t = patina::UNEXPLAINED
                [self.rng.gen_range(0..patina::UNEXPLAINED.len())];
            let ent = self.chronicle.registry.find_alive(EntityKind::Settlement, x, y);
            evs.push(Event {
                m: month_abs,
                s: name.clone(),
                k: EventKind::Myth,
                text: t.replace("{T}", &name).replace("{P}", &people),
                ids: ent.into_iter().collect(),
                x,
                y,
                veiled: true,
                ..Default::default()
            });
        }
        evs
    }

    /// M9.4 — a decisive field becomes a named place. Politics queued the
    /// coordinates; the world writes them onto the map as features.
    fn drain_battle_marks(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        if self.politics.marks.is_empty() {
            return;
        }
        let marks = std::mem::take(&mut self.politics.marks);
        for (x, y, m, town, winner) in marks {
            let name = format!("Field of {}", town);
            if self.taken.contains(&name) {
                continue;
            }
            self.taken.insert(name.clone());
            let people = self.peoples.cultures[winner.0].people.clone();
            let eid = self.chronicle.registry.add(EntityKind::Feature, &name, m, Some(winner), x, y);
            self.features.push(Feature {
                t: "battlefield".to_string(),
                name: name.clone(),
                x,
                y,
                size: 1,
                ety: format!("the ground before {} where the {} carried the day", town, people),
                people,
                ..Default::default()
            });
            self.dirty.mark(Dirty::FEATURES);
            evs.push(Event {
                m: month_abs,
                s: name.clone(),
                k: EventKind::War,
                text: format!(
                    "The dead are buried and the ground remembered: the country folk speak now of the {}.",
                    name
                ),
                ids: smallvec![eid],
                x,
                y,
                ..Default::default()
            });
        }
    }

    /// M9.2 — bounded conquest name-layers: sometimes the conqueror lays
    /// a name in its own tongue over a taken town. The old name is kept
    /// in the strata (`formerly`), never erased; rivers and features are
    /// never touched — hydronyms are conserved.
    fn drain_conquest_renames(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        if self.politics.transfers.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut self.politics.transfers);
        for sid in ids {
            let Some(i) = self.peoples.settlements.iter().position(|s| s.id == sid) else {
                continue;
            };
            let to = self.peoples.settlements[i].culture;
            // a people does not rename what already speaks its tongue,
            // and a place carries at most two former names (bounded strata)
            if self.peoples.settlements[i].namer == to || self.peoples.settlements[i].formerly.len() >= 2 {
                continue;
            }
            if self.rng.gen::<f64>() >= 0.35 {
                continue;
            }
            let style = self.peoples.cultures[to.0].style.clone();
            let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
            let old = self.peoples.settlements[i].name.clone();
            let people = self.peoples.cultures[to.0].people.clone();
            let (x, y) = (self.peoples.settlements[i].x, self.peoples.settlements[i].y);
            {
                let s = &mut self.peoples.settlements[i];
                s.formerly.push(old.clone());
                s.name = coined.word.clone();
                s.ety = coined.ety.clone();
                s.namer = to;
            }
            let ent = self.chronicle.registry.find_alive(EntityKind::Settlement, x, y);
            if let Some(id) = ent {
                self.chronicle.registry.rename(id, &coined.word);
            }
            evs.push(Event {
                m: month_abs,
                s: coined.word.clone(),
                k: EventKind::Society,
                text: format!(
                    "The {} lay their own name over {} — on the new rolls it is written {}.",
                    people, old, coined.word
                ),
                ids: ent.into_iter().collect(),
                x,
                y,
                ..Default::default()
            });
        }
    }

    /// M9.1 — settlement death. Once a year the world looks for a town
    /// past saving — starved, hollowed to a third of its peak, or a camp
    /// whose seams gave out — and lets it go. The town leaves a named
    /// ruin, its entity is closed, its routes are cut, and the web is
    /// re-knit so the one-component property (M8.1) holds.
    fn abandonment_pass(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        for s in self.peoples.settlements.iter_mut() {
            if s.pop > s.peak {
                s.peak = s.pop;
            }
        }
        if self.peoples.settlements.len() <= 6 {
            return;
        }
        let mut counts = vec![0usize; self.peoples.cultures.len()];
        for s in &self.peoples.settlements {
            counts[s.culture.idx()] += 1;
        }
        let besieged: HashSet<SettlementId> = self
            .politics
            .wars
            .iter()
            .filter_map(|w| w.siege.as_ref().map(|sg| sg.target))
            .collect();
        let mut worst: Option<usize> = None;
        for (i, s) in self.peoples.settlements.iter().enumerate() {
            if month_abs - s.born < 240 {
                continue; // twenty years' grace: young colonies struggle honestly
            }
            if counts[s.culture.idx()] <= 1 {
                continue; // never a people's last hearth
            }
            if besieged.contains(&s.id) {
                continue; // sieges resolve their own endings
            }
            let starving = s.pop < 60;
            let hollowed = s.pop < 110 && s.pop * 3 < s.peak;
            let played_out =
                s.pop < 90 && s.goods.is_empty() && s.exports.is_none() && s.wealth < 10.0;
            // a failing town (M9.1 spiral) wraps up once it is a husk —
            // no need to wait for the literal last sixty souls
            let spent = s.failing && s.pop < 400;
            if !(starving || hollowed || played_out || spent) {
                continue;
            }
            if worst.map_or(true, |w: usize| s.pop < self.peoples.settlements[w].pop) {
                worst = Some(i);
            }
        }
        let Some(i) = worst else { return };
        let dead = self.peoples.settlements[i].clone();
        let cause = if dead.goods.is_empty() && dead.exports.is_none() {
            "mines"
        } else if dead.fort > 0 && dead.pop * 3 < dead.peak {
            "war"
        } else if dead.failing {
            "decline" // the slow kind: ruin_why's default reading
        } else {
            "famine"
        };
        let why = patina::ruin_why(cause);
        let ent = self.chronicle.registry.find_alive(EntityKind::Settlement, dead.x, dead.y);
        if let Some(id) = ent {
            self.chronicle.registry
                .close(id, month_abs, &format!("abandoned — {}", why));
        }
        let ruin_name = format!("Ruins of {}", dead.name);
        let rid = self
            .registry
            .add(EntityKind::Ruin, &ruin_name, month_abs, Some(dead.culture), dead.x, dead.y);
        self.ruins.push(Ruin {
            name: ruin_name.clone(),
            of: dead.name.clone(),
            x: dead.x,
            y: dead.y,
            since: month_abs,
            why: why.to_string(),
            people: self.peoples.cultures[dead.culture.idx()].people.clone(),
            ety: dead.ety.clone(),
            eid: rid,
        });
        self.dirty.mark(Dirty::RUINS);
        // cut the dead town's routes, keeping the flow/idle ledgers aligned
        let keep: Vec<bool> = self
            .routes
            .iter()
            .map(|r| r.a != dead.id && r.b != dead.id)
            .collect();
        let mut k1 = keep.iter();
        self.routes.retain(|_| *k1.next().unwrap());
        if self.economy.route_flow.len() == keep.len() {
            let mut k2 = keep.iter();
            self.economy.route_flow.retain(|_| *k2.next().unwrap());
        } else {
            self.economy.route_flow.clear();
        }
        if self.route_idle.len() == keep.len() {
            let mut k3 = keep.iter();
            self.route_idle.retain(|_| *k3.next().unwrap());
        } else {
            self.route_idle.clear();
        }
        self.peoples.settlements.remove(i);
        // re-knit the web: the one-component property must survive death
        trade::recount_connections(&mut self.peoples.settlements, &self.routes);
        trade::rescue_unconnected(
            &mut self.peoples.settlements,
            &mut self.routes,
            &self.trade,
            &self.fields.height,
            &self.fields.flags,
            &self.fields.discharge,
            &self.fields.flow_amp,
        );
        trade::bridge_components(
            &mut self.peoples.settlements,
            &mut self.routes,
            &self.trade,
            &self.fields.height,
            &self.fields.flags,
            &self.fields.discharge,
            &self.fields.flow_amp,
        );
        trade::recount_connections(&mut self.peoples.settlements, &self.routes);
        trade::mark_ports(&mut self.peoples.settlements, &self.routes);
        self.dirty.mark(Dirty::ROUTES);
        self.recompute_territory();
        evs.push(Event {
            m: month_abs,
            s: dead.name.clone(),
            k: EventKind::Realm,
            text: format!(
                "The last hearth goes cold in {} — {}. Within ten years the roofs are fallen and the road grows grass; travellers call the place the {}.",
                dead.name, why, ruin_name
            ),
            ids: smallvec![rid],
            x: dead.x,
            y: dead.y,
            ..Default::default()
        });
    }

    /// M9.4 — roads fall disused. A route that has carried nothing for a
    /// generation fades on the map (and wakes again if the wagons return).
    fn route_age_pass(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        if self.economy.route_flow.len() != self.routes.len() {
            return; // ledgers momentarily out of step (new towns); next year
        }
        self.route_idle.resize(self.routes.len(), 0);
        for i in 0..self.routes.len() {
            if self.economy.route_flow[i] > 0.05 {
                self.route_idle[i] = 0;
                if self.routes[i].old {
                    self.routes[i].old = false; // the wagons came back
                    self.dirty.mark(Dirty::ROUTES);
                }
                continue;
            }
            self.route_idle[i] = self.route_idle[i].saturating_add(1);
            if self.route_idle[i] == 25 && !self.routes[i].old {
                self.routes[i].old = true;
                self.dirty.mark(Dirty::ROUTES);
                let an = self
                    .settlements
                    .iter()
                    .find(|s| s.id == self.routes[i].a)
                    .map(|s| s.name.clone());
                let bn = self
                    .settlements
                    .iter()
                    .find(|s| s.id == self.routes[i].b)
                    .map(|s| s.name.clone());
                if let (Some(an), Some(bn)) = (an, bn) {
                    evs.push(Event {
                        m: month_abs,
                        s: an.clone(),
                        k: EventKind::Economy,
                        text: format!(
                            "Grass closes over the way between {} and {} — no load has passed in a generation, and the itineraries drop it.",
                            an, bn
                        ),
                        ..Default::default()
                    });
                }
            }
        }
    }

    /// M9.3 — age-keyed name erosion. Twice a century, speech wears one
    /// or two of the oldest, longest names smooth (Aldenford → Aldford).
    /// The full form is kept in the strata; the etymology records the
    /// wearing. Rivers are never worn: hydronyms are conserved (M9.2).
    fn erosion_pass(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        let seed = self.seed as u64;
        let mut done = 0;
        for i in 0..self.peoples.settlements.len() {
            if done >= 2 {
                break;
            }
            let (name, born, layers) = {
                let s = &self.peoples.settlements[i];
                (s.name.clone(), s.born, s.formerly.len())
            };
            if month_abs - born < 960 || layers >= 2 {
                continue; // only names spoken for eighty years and more
            }
            if telling::det_hash(seed, &name) % 7 != 3 {
                continue; // most names hold for another century
            }
            let Some(worn) = patina::erode_word(&name) else { continue };
            if self.taken.contains(&worn) {
                continue;
            }
            self.taken.insert(worn.clone());
            let (x, y) = (self.peoples.settlements[i].x, self.peoples.settlements[i].y);
            {
                let s = &mut self.peoples.settlements[i];
                s.formerly.push(name.clone());
                if !s.ety.is_empty() {
                    s.ety = format!("{}; worn from {}", s.ety, name);
                } else {
                    s.ety = format!("worn from {}", name);
                }
                s.name = worn.clone();
            }
            let ent = self.chronicle.registry.find_alive(EntityKind::Settlement, x, y);
            if let Some(id) = ent {
                self.chronicle.registry.rename(id, &worn);
            }
            evs.push(Event {
                m: month_abs,
                s: worn.clone(),
                k: EventKind::Society,
                text: format!(
                    "A lifetime of speech wears {} smooth — travellers and tax rolls alike now write {}.",
                    name, worn
                ),
                ids: ent.into_iter().collect(),
                x,
                y,
                ..Default::default()
            });
            done += 1;
        }
        // and at most one feature — never a river (M9.2)
        for f in self.features.iter_mut() {
            if f.t == "river" || !f.formerly.is_empty() {
                continue;
            }
            if telling::det_hash(seed, &f.name) % 11 != 5 {
                continue;
            }
            let Some((worn_phrase, old_tok, new_tok)) = patina::erode_phrase(&f.name) else {
                continue;
            };
            if self.taken.contains(&worn_phrase) {
                continue;
            }
            self.taken.insert(worn_phrase.clone());
            let old_name = f.name.clone();
            f.formerly = old_name.clone();
            f.name = worn_phrase.clone();
            if !f.ety.is_empty() {
                f.ety = format!("{}; {} worn to {}", f.ety, old_tok, new_tok);
            } else {
                f.ety = format!("{} worn to {}", old_tok, new_tok);
            }
            self.dirty.mark(Dirty::FEATURES);
            evs.push(Event {
                m: month_abs,
                s: worn_phrase.clone(),
                k: EventKind::Society,
                text: format!(
                    "The maps catch up with the tongue: {} appears on the new charts as {}.",
                    old_name, worn_phrase
                ),
                x: f.x,
                y: f.y,
                ..Default::default()
            });
            break;
        }
    }

    /// M9.5 — the withheld. A bounded share of the quieter entries close
    /// on an admission that the record does not know. Deterministic per
    /// event text; the 2–8 % band is enforced by the harness.
    fn veil_pass(&mut self, events: &mut [Event], from: usize) {
        let seed = self.seed as u64;
        for e in events[from..].iter_mut() {
            if e.veiled {
                continue;
            }
            // the flavor families — entries whose truth the record can
            // afford to doubt; wars, foundings and realm acts stay firm
            if !matches!(
                e.k,
                EventKind::Myth
                    | EventKind::Omen
                    | EventKind::Wonder
                    | EventKind::Society
                    | EventKind::Nature
                    | EventKind::Disaster
                    | EventKind::Famine
                    | EventKind::Growth
            ) {
                continue;
            }
            // 7 % of the eligible families lands the whole-chronicle share
            // inside the 2-8 % band across seeds whose family mixes differ
            // (myth-heavy seeds ran over band at 11 %).
            if telling::det_hash(seed, &e.text) % 100 < 7 {
                e.veiled = true;
                e.text.push_str(patina::coda_for(seed, &e.text));
            }
        }
    }

    pub fn tick(&mut self, months: i64) -> (Vec<Event>, bool, bool) {
        let months = months.clamp(1, 240).max(1);
        let mut new_events: Vec<Event> = Vec::new();
        let mut founded = false;
        let mut deposits_changed = false;
        for _ in 0..months {
            let month_start = new_events.len();
            self.month += 1;
            let evs = self.tick_month(self.month);
            new_events.extend(evs);
            let fam_evs = self.famine_pass(self.month);
            new_events.extend(fam_evs);
            let (col_evs, did) = self.try_colonize(self.month);
            if did {
                founded = true;
                new_events.extend(col_evs);
            }
            let (pr_evs, dep_changed) = self.prospect_and_deplete(self.month);
            if dep_changed {
                deposits_changed = true;
            }
            new_events.extend(pr_evs);
            // rushes ride behind the strikes: unworked seams call their own camps
            let (rush_evs, rush_founded) = self.try_rush_camps(self.month);
            if rush_founded {
                founded = true;
            }
            new_events.extend(rush_evs);
            // once a year every town re-reads its hinterland: territories
            // grow with population, and a seam struck beyond yesterday's
            // reach must not rust in the hills once the town has grown to it
            if self.month.rem_euclid(12) == 0 {
                trade::assign_goods(&mut self.peoples.settlements, &self.deposits, &self.fields.fertility);
            }
            // once a decade the tongues catch up with the map: a people
            // spread near a named feature coins its own word for it (M3.4)
            if self.month.rem_euclid(120) == 0 {
                let doubled = naming::exonym_pass(
                    &mut self.features,
                    &self.peoples.settlements,
                    &self.peoples.cultures,
                    &mut self.taken,
                    &mut self.rng,
                );
                for (fname, people, alt) in doubled {
                    self.dirty.mark(Dirty::FEATURES);
                    new_events.push(Event {
                        m: self.month,
                        s: fname.clone(),
                        k: EventKind::Society,
                        text: format!(
                            "Spread now into that country, the {} keep their own word for {} — in their tongue it is {}.",
                            people, fname, alt
                        ),
                        ..Default::default()
                    });
                }
            }
            let soc_evs = society::monthly(
                &mut self.peoples.societies,
                &self.peoples.settlements,
                &self.deposits,
                &self.peoples.cultures,
                self.month,
                &mut self.rng,
            );
            new_events.extend(soc_evs);
            // E5.2: one id→index map for every pass this month — settlement
            // membership is fixed from here to the end of the economy block
            // (the passes below take slices, which cannot grow or shrink)
            let sidx = economy::sidx(&self.peoples.settlements);
            // M5.2: re-carve the market areas when towns appeared, and
            // refresh every other year as the route web thickens
            if self.economy.areas.area.len() != self.peoples.settlements.len()
                || self.month.rem_euclid(24) == 2
            {
                self.economy.areas =
                    economy::build_areas(&self.peoples.settlements, &self.routes, Some(&self.economy.areas), &sidx);
            }
            // M5.1: forges light where ore, fuel, hands and the art meet
            let craft_evs = economy::craft_pass(
                &mut self.peoples.settlements,
                &self.peoples.societies,
                &self.economy.areas,
                self.month,
                &mut self.rng,
            );
            new_events.extend(craft_evs);
            let eco_evs = economy::monthly(
                &mut self.peoples.settlements,
                &self.routes,
                &mut self.economy.market,
                &mut self.economy.areas,
                &mut self.economy.route_flow,
                &mut self.peoples.societies,
                self.month,
                &mut self.rng,
                &sidx,
            );
            new_events.extend(eco_evs);
            // M5.5: the merchants ride the widest gaps
            let mer_evs = economy::merchant_pass(
                &mut self.economy.merchants,
                &mut self.peoples.settlements,
                &mut self.economy.areas,
                &self.routes,
                &self.peoples.societies,
                &self.peoples.cultures,
                &mut self.taken,
                self.month,
                &mut self.rng,
                &mut self.chronicle.registry,
                &sidx,
            );
            new_events.extend(mer_evs);
            // statecraft: wars that move borders, dread, risings (M4)
            let (pol_evs, borders_changed) = politics::monthly(
                &mut self.politics,
                &mut self.chronicle.state,
                &mut self.rng,
                &mut self.taken,
                self.month,
                &mut self.peoples.settlements,
                &mut self.peoples.cultures,
                &mut self.peoples.societies,
                &self.fields.territory,
                &mut self.chronicle.registry,
            );
            new_events.extend(pol_evs);
            // the patina settles behind the drums: battlefields earn names,
            // conquerors rename, towns die to ruin, roads fade, names wear (M9)
            let pat_evs = self.patina_pass(self.month);
            new_events.extend(pat_evs);
            // redraw the political map when land changed hands, and once a
            // year regardless — growing towns push their reach outward
            if borders_changed || self.month.rem_euclid(12) == 6 {
                self.recompute_territory();
            }
            // the human pulse, paced by how loud the world already is (M6.4)
            let pace = (1.30 - 0.22 * self.heat).clamp(0.55, 1.30);
            let chron_evs = chronicle::monthly(
                &mut self.chronicle.state,
                &mut self.rng,
                &mut self.taken,
                self.month,
                &mut self.peoples.settlements,
                &self.peoples.cultures,
                &self.features,
                &self.world_name,
                &mut self.chronicle.registry,
                pace,
            );
            new_events.extend(chron_evs);
            // the relics ride the month's tides: forged, plundered, lost
            // (M6.3) — read straight off the month's slice, no clone (E5.6)
            let art_evs = artifact::monthly(
                &mut self.chronicle.artifacts,
                &mut self.chronicle.registry,
                &new_events[month_start..],
                &self.peoples.settlements,
                &self.peoples.cultures,
                &self.peoples.societies,
                &mut self.taken,
                self.month,
                &mut self.rng,
            );
            new_events.extend(art_evs);
            // second reading: back-fill ids, anchor coordinates, and let
            // the great deeds pass into legend (M6.1, M6.9)
            self.resolve_events(month_start, &mut new_events);
            // third reading: the record admits what it does not know (M9.5)
            self.veil_pass(&mut new_events, month_start);
            // narrative heat: the month's weighted noise, slowly cooling (M6.4)
            let m_heat: i32 = new_events[month_start..]
                .iter()
                .map(|e| telling::weight(e.k) - 1)
                .sum();
            self.heat = self.heat * 0.94 + (m_heat as f64 / 6.0) * 0.06;
        }
        // change tracking for the wire (E4.5): foundings reship routes,
        // strikes and dead mines reship the mineral ledger
        if founded {
            self.dirty.mark(Dirty::ROUTES);
        }
        if deposits_changed {
            self.dirty.mark(Dirty::DEPOSITS);
        }
        // the full log is the chronicle's memory — the sifter reads all of it,
        // and the client pages it with events_range (M6)
        self.chronicle.events.extend(new_events.iter().cloned());
        (new_events, founded, deposits_changed)
    }

    /// The second reading of the month's events (M6): any entry whose
    /// subject the registry knows gets its ids back-filled, any entry
    /// without a map anchor inherits its subject's, and the loudest
    /// entries pass into legend (M6.9). Purely derived — no rng, no
    /// state change beyond the event fields and mention counters.
    fn resolve_events(&mut self, from: usize, events: &mut [Event]) {
        for e in events[from..].iter_mut() {
            if e.ids.is_empty() {
                let found = [
                    EntityKind::Settlement,
                    EntityKind::War,
                    EntityKind::Culture,
                    EntityKind::Person,
                    EntityKind::Artifact,
                    EntityKind::Feature,
                ]
                .iter()
                .find_map(|&k| self.chronicle.registry.find_kind(k, &e.s))
                    .or_else(|| self.chronicle.registry.find(&e.s));
                if let Some(id) = found {
                    e.ids.push(id);
                }
            }
            if e.ids.is_empty() && e.x >= 0 {
                // the subject may have been renamed this very tick (M9.2):
                // fall back to the one thing a rename never moves — its ground
                let found = [EntityKind::Settlement, EntityKind::Ruin, EntityKind::Feature]
                    .iter()
                    .find_map(|&k| self.chronicle.registry.find_alive(k, e.x, e.y));
                if let Some(id) = found {
                    e.ids.push(id);
                }
            }
            if e.x < 0 {
                if let Some(ent) = e.ids.first().and_then(|&id| self.chronicle.registry.get(id)) {
                    if ent.x >= 0 {
                        e.x = ent.x;
                        e.y = ent.y;
                    }
                }
            }
            if e.legend.is_empty() && telling::weight(e.k) >= 3 {
                e.legend = telling::legendize(e);
            }
        }
    }






    /// Boolean view of one `CellFlags` bit — offline tooling convenience;
    /// hot paths test bits on `self.fields.flags` directly.
    pub fn mask(&self, f: CellFlags) -> Array2<bool> {
        self.fields.flags.mapv(|b| b & f.bits() != 0)
    }

    // The field registry itself is declared once, below `pack()`, via the
    // `field_registry!` macro (E2.1) — it expands to both the static
    // `FIELD_SPECS` table (for codegen, E2.4) and `World::fields()`.

}


// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): the engine's speed and wire budget.
pub const BANDS: &[Band] = &[
    Band { name: "512 generation time", sweet: (0.0, 3000.0), hard: (0.0, 8000.0), target: "sweet ≤3s · hard ≤8s (wasm ≈ 2× native)" },
    Band { name: "tick rate", sweet: (100.0, f64::INFINITY), hard: (25.0, f64::INFINITY), target: "sweet ≥100 mo/s · hard ≥25" },
    Band { name: "pack bytes per cell", sweet: (0.0, 21.0), hard: (0.0, 24.0), target: "sweet ≤21 · hard ≤24 (8×u16 + 4×u8 = 20 B/cell + header)" },
    Band { name: "median tick payload", sweet: (0.0, 4096.0), hard: (0.0, 16384.0), target: "sweet ≤4 KB · hard ≤16 KB (E4: ship what changed)" },
    Band { name: "allocations per month", sweet: (0.0, 350.0), hard: (0.0, 1500.0), target: "sweet ≤350 · hard ≤1500 (baseline 183/mo — deterministic per seed, E5.10)" },
    // E10.1 — per-stage generation budgets at 512, native release ms,
    // asserted against the WORST seed of the sweep (a budget that only
    // holds on the friendly seed is not a budget).
    Band { name: "stage terrain ms", sweet: (0.0, 400.0), hard: (0.0, 900.0), target: "E10.1: plate+plume noise, warp, falloff" },
    Band { name: "stage erosion ms", sweet: (0.0, 500.0), hard: (0.0, 1200.0), target: "E10.1: thermal+fluvial passes" },
    Band { name: "stage climate ms", sweet: (0.0, 200.0), hard: (0.0, 500.0), target: "E10.1: temperature, advection precip" },
    Band { name: "stage hydrology ms", sweet: (0.0, 250.0), hard: (0.0, 600.0), target: "E10.1: D8 routing, accumulation, lakes" },
    Band { name: "stage biomes ms", sweet: (0.0, 80.0), hard: (0.0, 250.0), target: "E10.1: Whittaker classification" },
    Band { name: "stage fertility ms", sweet: (0.0, 80.0), hard: (0.0, 250.0), target: "E10.1: soil + floodplain kernel" },
    Band { name: "stage naming ms", sweet: (0.0, 120.0), hard: (0.0, 350.0), target: "E10.1: feature detection + toponymy" },
    Band { name: "stage resources ms", sweet: (0.0, 400.0), hard: (0.0, 900.0), target: "E10.1: deposit placement + suitability scan (baseline ~300 ms)" },
    Band { name: "stage settlements ms", sweet: (0.0, 250.0), hard: (0.0, 700.0), target: "E10.1: founding, cultures, goods, routes" },
    Band { name: "gen total ms", sweet: (0.0, 1600.0), hard: (0.0, 3500.0), target: "E10.1: native total; wasm ≈ 2× rides the 512-generation band" },
    // E10.2 — tick rate on a young world and the heavier year-100 world.
    Band { name: "tick rate year 0", sweet: (1000.0, f64::INFINITY), hard: (200.0, f64::INFINITY), target: "E10.2: sweet ≥1000 mo/s · hard ≥200 (native)" },
    Band { name: "tick rate year 100", sweet: (400.0, f64::INFINITY), hard: (100.0, f64::INFINITY), target: "E10.2: sweet ≥400 mo/s · hard ≥100 (grown-in world, ~103 towns; baseline 459–544)" },
    // E10.6 — memory ceiling with the whole seed sweep resident.
    Band { name: "native peak RSS", sweet: (0.0, 1500.0), hard: (0.0, 2500.0), target: "E10.6: sweet ≤1.5 GiB · hard ≤2.5 GiB (3 × 512² worlds + histories)" },
];
