//! World orchestration — port of world.py: generation pipeline + simulation.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use serde_json::{json, Value};

use strum::IntoEnumIterator;

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
use crate::constants;
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
use crate::society::{self, Society};
use crate::telling;
use crate::trade::{self, Route};
use crate::util::{now_ms, round2, round3};

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

#[derive(Serialize, Clone)]
pub struct Event {
    pub m: i64,
    pub s: String,
    pub k: EventKind,
    pub text: String,
    /// Entities this event speaks of (M6.1); the first id is the subject.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<EntityId>,
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

/// E4.2/E4.3 — FNV hashes of the last-shipped JSON per wire surface; a
/// section crosses the boundary only when its bytes moved. Seeded by
/// `prime_sent()` to the freshly generated world, which is exactly what
/// `bootstrap()` ships.
#[derive(Default)]
struct SentCache {
    /// (settlement id, cold-form hash, heartbeat quanta), engine order.
    /// The cold form zeroes the monthly heartbeat (pop/food/k/wealth); the
    /// quanta are the heartbeat at wire precision (pop · food×10 · k ·
    /// wealth×10), so a town ships a per-field patch only when a value the
    /// client can actually see has moved (E4.2).
    settlements: Vec<(i64, u64, [i64; 4])>,
    /// cultures · wars · merchants (full-form hashes).
    blocks: [u64; 3],
    /// Cold-form hash of the cultures block (heartbeat stripped).
    cultures_cold: u64,
    /// Per-good market row hashes — the ledger reships whole only when
    /// the set of priced goods changes (E4.3).
    market_rows: Vec<(String, u64)>,
    /// Per-hub delta state for the market-areas block, keyed by hub id
    /// (E4.3): cold hash (id·name·n) and price bits per good.
    areas_hubs: Vec<(i64, u64, Vec<(String, u64)>)>,
    /// Hash of the area assignment vector — when this moves, the hub set
    /// itself changed and the whole block reships.
    areas_of: u64,
    /// Hash of the price-spread rows.
    areas_spread: u64,
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
            ids: Vec::new(),
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

    // E3.2 — the eight float grids live as f32 at rest: generation math
    // runs in f64 (geo → climate → hydrology → biomes → agriculture), the
    // result is stored once as f32, and every human-stage consumer
    // (naming, resources, settlements, trade, economy) reads f32. Halves
    // grid memory and makes pack() a straight memcpy per field.
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

    pub deposits: Vec<Deposit>,
    pub settlements: Vec<Settlement>,
    pub cultures: Vec<Culture>,
    pub features: Vec<Feature>,
    pub routes: Vec<Route>,
    pub events: Vec<Event>,
    pub world_name: String,
    pub societies: Vec<Society>,
    pub market: Market,
    /// M5.2 — the route web carved into market areas, each with its own
    /// price list; rebuilt when towns are founded and refreshed yearly.
    pub areas: economy::MarketAreas,
    /// M5.5 — named traders riding the price gaps between areas.
    pub merchants: Vec<economy::Merchant>,
    /// Last month's realized flow per route (gravity cross-check, M5.4).
    pub route_flow: Vec<f64>,
    /// M9.1 — where towns died: named remains on the map.
    pub ruins: Vec<Ruin>,
    /// Years each route has gone without realized flow (M9.4).
    route_idle: Vec<u16>,
    /// M6.1 — the chronicle's cast: every named thing, one stable id.
    pub registry: Registry,
    /// M6.3 — relics with provenance: forged, plundered, lost, found.
    pub artifacts: Vec<artifact::Artifact>,
    /// M6.4 — narrative heat: decaying sum of the month's weighted events;
    /// quiet years reach for omens, loud years let the wars speak.
    heat: f64,

    rng: Pcg64Mcg,
    taken: HashSet<String>,
    chron: ChronicleState,
    /// Statecraft: wars, opinion, dread, solidarity, vassals (M4).
    pub politics: Politics,
    /// Influence-map territory: owner culture per cell, −1 wilderness (M4.1).
    pub territory: Array2<i16>,
    /// E4.5 — which wire sections must reship on the next tick.
    dirty: Dirty,
    /// E4.2/E4.3 — hashes of the last-shipped JSON per wire surface.
    sent: SentCache,
    /// Deterministic drought field over (space × year) — the famine die (M2.6).
    drought: Perlin3,
    /// Last year grain was shock-priced by famine, to spike at most once a year.
    grain_shock_year: i64,
    site_score: Array2<f64>,
    food_grid: Array2<f64>,
    near_fresh: Array2<bool>,
    coast: Array2<bool>,
    max_settlements: usize,
    trade: trade::TradeGrid,
    pub timings: Vec<(&'static str, f64)>,
}

impl World {
    pub fn generate(seed: i64, size: usize) -> World {
        World::generate_scaled(seed, size, 1.0)
    }

    /// `generate` with a rainfall multiplier — the metamorphic-testing knob
    /// (M8.2): the harness generates the same seed wetter and asserts the
    /// rivers do not shrink. 1.0 is the game; nothing else ships.
    pub fn generate_scaled(seed: i64, size: usize, precip_scale: f64) -> World {
        let t0 = now_ms();
        let mut timings: Vec<(&'static str, f64)> = Vec::new();

        let mut height = geo::heightmap(seed, size);
        timings.push(("terrain", now_ms() - t0));

        let te = now_ms();
        erosion::erode(&mut height);
        let water = height.mapv(|h| h < 0.0);
        timings.push(("erosion", now_ms() - te));

        let t1 = now_ms();
        let lat = climate::latitude_deg(size);
        let tmean = climate::temperature_mean(&height, &lat);
        let tamp = climate::temperature_amplitude(&lat, &water);
        let (mut precip, pamp) = climate::precipitation(&height, &water, &tmean, &lat);
        if precip_scale != 1.0 {
            precip.mapv_inplace(|p| p * precip_scale);
        }
        timings.push(("climate", now_ms() - t1));

        let t2 = now_ms();
        let hydro = hydrology::hydrology(&height, &water, &precip, &pamp, &tmean);
        timings.push(("hydrology", now_ms() - t2));

        let t3 = now_ms();
        let biome_map = biomes_mod::classify(&height, &tmean, &precip, &hydro.lakes);
        timings.push(("biomes", now_ms() - t3));

        let t4 = now_ms();
        let fertility = agriculture::fertility(
            &height,
            &tmean,
            &precip,
            &hydro.rivers,
            &hydro.lakes,
            &hydro.discharge,
        );
        let crops = agriculture::crop_packages(
            &height,
            &tmean,
            &precip,
            &hydro.rivers,
            &hydro.lakes,
        );
        timings.push(("fertility", now_ms() - t4));

        // E3.2 — the physical stages are done; the world's float grids
        // drop to their resting f32 width here, and every human stage
        // below (naming, resources, settlements, trade, economy) reads
        // the same f32 the ticks will read.
        let height = height.mapv(|x| x as f32);
        let tmean = tmean.mapv(|x| x as f32);
        let tamp = tamp.mapv(|x| x as f32);
        let precip = precip.mapv(|x| x as f32);
        let pamp = pamp.mapv(|x| x as f32);
        let discharge = hydro.discharge.mapv(|x| x as f32);
        let flow_amp = hydro.flow_amp.mapv(|x| x as f32);
        let fertility = fertility.mapv(|x| x as f32);

        let t5 = now_ms();
        let (mut features, world_name) = naming::name_features(
            &height,
            &biome_map,
            &hydro.rivers,
            &hydro.lakes,
            &discharge,
            &tmean,
            &precip,
            seed,
        );
        timings.push(("naming", now_ms() - t5));

        let t6 = now_ms();
        let mut deposits =
            resources::place_resources(&biome_map, &height, &hydro.rivers, &hydro.lakes, seed);
        timings.push(("resources", now_ms() - t6));

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
                ids: vec![sett_ents[si]],
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
        let mut dawn = std::mem::take(&mut world.events);
        world.resolve_events(0, &mut dawn);
        world.events = dawn;
        // The first political map: who holds what, before any drum beats.
        world.recompute_territory();
        world.dirty.clear(Dirty::TERRITORY); // ships with the pack, not the first tick
        // Seed the delta baseline (E4.2/E4.3) to what bootstrap() ships.
        world.prime_sent();
        world
    }

    /// M4.1 — redraw the influence map after borders move or towns grow.
    pub fn recompute_territory(&mut self) {
        self.territory = politics::influence_map(
            &self.height,
            &self.settlements,
            &self.societies,
            &self.politics.asab,
            self.cultures.len(),
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
        let (h, w) = self.height.dim();
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
        self.height = grow(&self.height, pad, |e, t| {
            let shelf = e.min(-0.03);
            let deep = (-0.62_f32).min(e);
            shelf + (deep - shelf) * t as f32
        });
        // Climate margins keep zonal continuity by extending the edge column.
        self.tmean = grow(&self.tmean, pad, |e, _| e);
        self.tamp = grow(&self.tamp, pad, |e, _| e);
        self.precip = grow(&self.precip, pad, |e, _| e);
        self.discharge = grow(&self.discharge, pad, |_, _| 0.0);
        self.fertility = grow(&self.fertility, pad, |_, _| 0.0);
        self.site_score = grow(&self.site_score, pad, |_, _| 0.0);
        self.food_grid = grow(&self.food_grid, pad, |_, _| 0.0);
        self.biomes = {
            let a = &self.biomes;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0 // open water
                }
            })
        };
        self.crops = {
            let a = &self.crops;
            Array2::from_shape_fn((h, w + 2 * pad), |(y, x)| {
                let xi = x as isize - p;
                if xi >= 0 && (xi as usize) < w {
                    a[[y, xi as usize]]
                } else {
                    0 // open water grows nothing
                }
            })
        };
        self.flags = {
            let a = &self.flags;
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
        self.pamp = grow(&self.pamp, pad, |e, _| e);
        self.flow_amp = grow(&self.flow_amp, pad, |_, _| 0.0);
        self.strahler = {
            let a = &self.strahler;
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
        for s in self.settlements.iter_mut() {
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
        self.registry.shift_x(shift);
        for e in self.events.iter_mut() {
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
            self.societies.iter().map(society::mods_for).collect();
        // M2.3: the seat of kings — each culture's greatest town keeps a
        // court, and courts import: grain barges, tribute, hungry retinues.
        // The head of the rank-size curve is political as much as economic.
        let mut seat: Vec<usize> = Vec::new();
        for (i, s) in self.settlements.iter().enumerate() {
            while seat.len() <= s.culture.0 {
                seat.push(usize::MAX);
            }
            if seat[s.culture.idx()] == usize::MAX
                || s.pop > self.settlements[seat[s.culture.idx()]].pop
            {
                seat[s.culture.idx()] = i;
            }
        }
        for (si, s) in self.settlements.iter_mut().enumerate() {
            let md = mods.get(s.culture.idx()).cloned().unwrap_or_default();
            let (y, x) = (s.y as usize, s.x as usize);
            let t_now =
                climate::month_temperature(self.tmean[[y, x]] as f64, self.tamp[[y, x]] as f64, month);
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
                &self.crops,
                &self.fertility,
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
            if self.height[[y, x]] > 0.42 && pop > 120 && self.rng.gen::<f64>() < 0.0012 {
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
                && self.precip[[y, x]] < 700.0
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
                        &mut self.chron,
                        &mut self.rng,
                        s,
                        &self.cultures,
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
            let claimed = self.settlements.iter().any(|s| {
                let r = settlements::work_radius(s.pop);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                dx * dx + dy * dy <= r * r
            });
            if claimed {
                continue;
            }
            // far ore must out-pull farmland or nobody ever leaves the plough
            let worth = self.market.price(d.r) * d.rich * 2.2;
            for yy in (d.y - R).max(0)..=(d.y + R).min(h as i64 - 1) {
                for xx in (d.x - R).max(0)..=(d.x + R).min(w as i64 - 1) {
                    if self.height[[yy as usize, xx as usize]] < 0.0 {
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
        let initial = self.settlements.len();
        let mods_v: Vec<society::Mods> =
            self.societies.iter().map(society::mods_for).collect();
        // ore-led ventures may spill past the cap into a reserved band:
        // the seams don't care that the census is full.
        let hard_cap = self.max_settlements + self.max_settlements / 4;
        for pi in 0..initial {
            if self.settlements.len() >= hard_cap {
                break;
            }
            let (ppop, pcap, pname) = {
                let p = &self.settlements[pi];
                // the hunger for land is measured against what the LAND
                // carries — not the import-lifted ceiling stored in s.k
                // (hub and court terms), which would gate colonists on
                // grain barges that feed the city just fine.
                let md = mods_v.get(p.culture.idx()).cloned().unwrap_or_default();
                let kland = settlements::capacity_at(
                    &self.crops,
                    &self.fertility,
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
                let parent = self.settlements[pi].clone();
                let range = self
                    .societies
                    .get(parent.culture.idx())
                    .map(|so| society::mods_for(so).colony_range)
                    .unwrap_or(1.0);
                settlements::colony_site(
                    &self.site_score,
                    pull.as_ref().unwrap(),
                    &self.settlements,
                    &parent,
                    3600.0 * range * range,
                )
            };
            let Some((y, x)) = site else { continue };
            // an ore-led venture: the seams called louder than the soil
            let ore_led = pull.as_ref().unwrap()[[y, x]] > self.site_score[[y, x]].max(0.0);
            // past the soft cap only miners still sail
            if self.settlements.len() >= self.max_settlements && !ore_led {
                continue;
            }
            let migrants = ((ppop as f64 * self.rng.gen_range(0.08..0.14)) as i64).max(40);
            self.settlements[pi].pop = (ppop - migrants).max(60);
            let cid = self.settlements[pi].culture;
            let idx = self.found_settlement(y, x, migrants, cid);
            founded = true;
            let name = self.settlements[idx].name.clone();
            let coastal = self.settlements[idx].coastal;
            let river = self.settlements[idx].river;
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
        let style = if !self.cultures.is_empty() {
            self.cultures[cid.0].style.clone()
        } else {
            "hellenic".to_string()
        };
        let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
        let new_id = SettlementId(self.settlements.iter().map(|o| o.id.0).max().unwrap_or(-1) + 1);
        let mut s = Settlement {
            id: new_id,
            name: coined.word.clone(),
            x: x as i64,
            y: y as i64,
            pop: migrants,
            tier: settlements::tier(migrants),
            food: settlements::site_food(
                &self.food_grid,
                &self.fertility,
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
        trade::goods_for(&mut s, &self.deposits, &self.fertility);
        let mdc = self
            .societies
            .get(cid.0)
            .map(society::mods_for)
            .unwrap_or_default();
        s.k = round2(settlements::capacity_at(
            &self.crops,
            &self.fertility,
            y,
            x,
            s.coastal,
            s.food,
            mdc.kaplan,
            mdc.capacity,
        ));
        self.settlements.push(s);
        let idx = self.settlements.len() - 1;
        // the new town enters the telling (M6.1)
        {
            let t = &self.settlements[idx];
            self.registry
                .add(EntityKind::Settlement, &t.name, self.month, Some(t.culture), t.x, t.y);
        }
        trade::connect_settlement(
            idx,
            &mut self.settlements,
            &mut self.routes,
            &self.trade,
            &self.height,
            &self.flags,
            &self.discharge,
            &self.flow_amp,
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
            if self.settlements.len() >= hard_cap {
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
            let claimed = self.settlements.iter().any(|s| {
                let r = settlements::work_radius(s.pop);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                dx * dx + dy * dy <= r * r
            });
            if claimed {
                continue;
            }
            // the pull of the price: dearer metal, richer seam, faster rush
            let worth = (self.market.price(d.r) * d.rich / 2.0).clamp(0.2, 3.0);
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
                    if self.height[[yy as usize, xx as usize]] < 0.0 {
                        continue;
                    }
                    let clear = self.settlements.iter().all(|o| {
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
            let Some(src) = (0..self.settlements.len()).min_by_key(|&i| {
                let s = &self.settlements[i];
                (s.x - dx0).pow(2) + (s.y - dy0).pow(2)
            }) else {
                continue;
            };
            let spop = self.settlements[src].pop;
            if spop < 240 {
                continue;
            }
            let migrants = ((spop as f64 * 0.08) as i64).clamp(60, 240);
            self.settlements[src].pop = spop - migrants;
            let cid = self.settlements[src].culture;
            let sname = self.settlements[src].name.clone();
            let idx = self.found_settlement(y, x, migrants, cid);
            founded = true;
            let name = self.settlements[idx].name.clone();
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
        if !self.settlements.is_empty() && self.rng.gen::<f64>() < 0.007 {
            let i = self.rng.gen_range(0..self.settlements.len());
            let (name, people, x, y) = {
                let s = &self.settlements[i];
                (
                    s.name.clone(),
                    self.cultures[s.culture.idx()].people.clone(),
                    s.x,
                    s.y,
                )
            };
            let t = patina::UNEXPLAINED
                [self.rng.gen_range(0..patina::UNEXPLAINED.len())];
            let ent = self.registry.find_alive(EntityKind::Settlement, x, y);
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
            let people = self.cultures[winner.0].people.clone();
            let eid = self.registry.add(EntityKind::Feature, &name, m, Some(winner), x, y);
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
                ids: vec![eid],
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
            let Some(i) = self.settlements.iter().position(|s| s.id == sid) else {
                continue;
            };
            let to = self.settlements[i].culture;
            // a people does not rename what already speaks its tongue,
            // and a place carries at most two former names (bounded strata)
            if self.settlements[i].namer == to || self.settlements[i].formerly.len() >= 2 {
                continue;
            }
            if self.rng.gen::<f64>() >= 0.35 {
                continue;
            }
            let style = self.cultures[to.0].style.clone();
            let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
            let old = self.settlements[i].name.clone();
            let people = self.cultures[to.0].people.clone();
            let (x, y) = (self.settlements[i].x, self.settlements[i].y);
            {
                let s = &mut self.settlements[i];
                s.formerly.push(old.clone());
                s.name = coined.word.clone();
                s.ety = coined.ety.clone();
                s.namer = to;
            }
            let ent = self.registry.find_alive(EntityKind::Settlement, x, y);
            if let Some(id) = ent {
                self.registry.rename(id, &coined.word);
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
        for s in self.settlements.iter_mut() {
            if s.pop > s.peak {
                s.peak = s.pop;
            }
        }
        if self.settlements.len() <= 6 {
            return;
        }
        let mut counts = vec![0usize; self.cultures.len()];
        for s in &self.settlements {
            counts[s.culture.idx()] += 1;
        }
        let besieged: HashSet<SettlementId> = self
            .politics
            .wars
            .iter()
            .filter_map(|w| w.siege.as_ref().map(|sg| sg.target))
            .collect();
        let mut worst: Option<usize> = None;
        for (i, s) in self.settlements.iter().enumerate() {
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
            if worst.map_or(true, |w: usize| s.pop < self.settlements[w].pop) {
                worst = Some(i);
            }
        }
        let Some(i) = worst else { return };
        let dead = self.settlements[i].clone();
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
        let ent = self.registry.find_alive(EntityKind::Settlement, dead.x, dead.y);
        if let Some(id) = ent {
            self.registry
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
            people: self.cultures[dead.culture.idx()].people.clone(),
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
        if self.route_flow.len() == keep.len() {
            let mut k2 = keep.iter();
            self.route_flow.retain(|_| *k2.next().unwrap());
        } else {
            self.route_flow.clear();
        }
        if self.route_idle.len() == keep.len() {
            let mut k3 = keep.iter();
            self.route_idle.retain(|_| *k3.next().unwrap());
        } else {
            self.route_idle.clear();
        }
        self.settlements.remove(i);
        // re-knit the web: the one-component property must survive death
        trade::recount_connections(&mut self.settlements, &self.routes);
        trade::rescue_unconnected(
            &mut self.settlements,
            &mut self.routes,
            &self.trade,
            &self.height,
            &self.flags,
            &self.discharge,
            &self.flow_amp,
        );
        trade::bridge_components(
            &mut self.settlements,
            &mut self.routes,
            &self.trade,
            &self.height,
            &self.flags,
            &self.discharge,
            &self.flow_amp,
        );
        trade::recount_connections(&mut self.settlements, &self.routes);
        trade::mark_ports(&mut self.settlements, &self.routes);
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
            ids: vec![rid],
            x: dead.x,
            y: dead.y,
            ..Default::default()
        });
    }

    /// M9.4 — roads fall disused. A route that has carried nothing for a
    /// generation fades on the map (and wakes again if the wagons return).
    fn route_age_pass(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        if self.route_flow.len() != self.routes.len() {
            return; // ledgers momentarily out of step (new towns); next year
        }
        self.route_idle.resize(self.routes.len(), 0);
        for i in 0..self.routes.len() {
            if self.route_flow[i] > 0.05 {
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
        for i in 0..self.settlements.len() {
            if done >= 2 {
                break;
            }
            let (name, born, layers) = {
                let s = &self.settlements[i];
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
            let (x, y) = (self.settlements[i].x, self.settlements[i].y);
            {
                let s = &mut self.settlements[i];
                s.formerly.push(name.clone());
                if !s.ety.is_empty() {
                    s.ety = format!("{}; worn from {}", s.ety, name);
                } else {
                    s.ety = format!("worn from {}", name);
                }
                s.name = worn.clone();
            }
            let ent = self.registry.find_alive(EntityKind::Settlement, x, y);
            if let Some(id) = ent {
                self.registry.rename(id, &worn);
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
                trade::assign_goods(&mut self.settlements, &self.deposits, &self.fertility);
            }
            // once a decade the tongues catch up with the map: a people
            // spread near a named feature coins its own word for it (M3.4)
            if self.month.rem_euclid(120) == 0 {
                let doubled = naming::exonym_pass(
                    &mut self.features,
                    &self.settlements,
                    &self.cultures,
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
                &mut self.societies,
                &self.settlements,
                &self.deposits,
                &self.cultures,
                self.month,
                &mut self.rng,
            );
            new_events.extend(soc_evs);
            // E5.2: one id→index map for every pass this month — settlement
            // membership is fixed from here to the end of the economy block
            // (the passes below take slices, which cannot grow or shrink)
            let sidx = economy::sidx(&self.settlements);
            // M5.2: re-carve the market areas when towns appeared, and
            // refresh every other year as the route web thickens
            if self.areas.area.len() != self.settlements.len()
                || self.month.rem_euclid(24) == 2
            {
                self.areas =
                    economy::build_areas(&self.settlements, &self.routes, Some(&self.areas), &sidx);
            }
            // M5.1: forges light where ore, fuel, hands and the art meet
            let craft_evs = economy::craft_pass(
                &mut self.settlements,
                &self.societies,
                &self.areas,
                self.month,
                &mut self.rng,
            );
            new_events.extend(craft_evs);
            let eco_evs = economy::monthly(
                &mut self.settlements,
                &self.routes,
                &mut self.market,
                &mut self.areas,
                &mut self.route_flow,
                &mut self.societies,
                self.month,
                &mut self.rng,
                &sidx,
            );
            new_events.extend(eco_evs);
            // M5.5: the merchants ride the widest gaps
            let mer_evs = economy::merchant_pass(
                &mut self.merchants,
                &mut self.settlements,
                &mut self.areas,
                &self.routes,
                &self.societies,
                &self.cultures,
                &mut self.taken,
                self.month,
                &mut self.rng,
                &mut self.registry,
                &sidx,
            );
            new_events.extend(mer_evs);
            // statecraft: wars that move borders, dread, risings (M4)
            let (pol_evs, borders_changed) = politics::monthly(
                &mut self.politics,
                &mut self.chron,
                &mut self.rng,
                &mut self.taken,
                self.month,
                &mut self.settlements,
                &mut self.cultures,
                &mut self.societies,
                &self.territory,
                &mut self.registry,
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
                &mut self.chron,
                &mut self.rng,
                &mut self.taken,
                self.month,
                &mut self.settlements,
                &self.cultures,
                &self.features,
                &self.world_name,
                &mut self.registry,
                pace,
            );
            new_events.extend(chron_evs);
            // the relics ride the month's tides: forged, plundered, lost
            // (M6.3) — read straight off the month's slice, no clone (E5.6)
            let art_evs = artifact::monthly(
                &mut self.artifacts,
                &mut self.registry,
                &new_events[month_start..],
                &self.settlements,
                &self.cultures,
                &self.societies,
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
        self.events.extend(new_events.iter().cloned());
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
                .find_map(|&k| self.registry.find_kind(k, &e.s))
                    .or_else(|| self.registry.find(&e.s));
                if let Some(id) = found {
                    e.ids.push(id);
                }
            }
            if e.ids.is_empty() && e.x >= 0 {
                // the subject may have been renamed this very tick (M9.2):
                // fall back to the one thing a rename never moves — its ground
                let found = [EntityKind::Settlement, EntityKind::Ruin, EntityKind::Feature]
                    .iter()
                    .find_map(|&k| self.registry.find_alive(k, e.x, e.y));
                if let Some(id) = found {
                    e.ids.push(id);
                }
            }
            if e.x < 0 {
                if let Some(ent) = e.ids.first().and_then(|&id| self.registry.get(id)) {
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

    /// M2.6 — the harvest verdict. Once a year, in the eighth month, every
    /// rain-fed farming town faces the sky it actually got: a deterministic
    /// drought field (seeded noise over space × year) decides where the rains
    /// failed. Failure starves, spikes grain, and sends folk down the roads.
    /// Floodplains irrigate, paddies flood, herders walk to the grass and
    /// fishers never planted — only wheat and maize under open sky can fail.
    fn famine_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 7 {
            return events;
        }
        let year = month_abs / 12;
        let dry = |x: i64, y: i64| -> f64 {
            self.drought
                .fbm(x as f64 * 0.045, y as f64 * 0.045, year as f64 * 0.83, 2)
        };
        let mut migrations: Vec<(usize, i64)> = Vec::new();
        let mut worst = 0.0f64;
        // settlement bucket grid for the kin-town search (E5.3) — positions
        // are fixed for the whole pass, only populations move
        let town_buckets = crate::util::Buckets::build(
            self.settlements.iter().map(|s| (s.x as f64, s.y as f64)).collect(),
            32.0,
        );
        for i in 0..self.settlements.len() {
            let (y, x, pop, culture, river, name) = {
                let s = &self.settlements[i];
                (s.y, s.x, s.pop, s.culture, s.river, s.name.clone())
            };
            let pack = self.crops[[y as usize, x as usize]];
            let rainfed = (pack == agriculture::CropPackage::Wheat.code()
                || pack == agriculture::CropPackage::Maize.code())
                && !river;
            if !rainfed || pop <= 90 {
                continue;
            }
            let d = dry(x, y);
            if d >= -0.30 {
                continue;
            }
            let shortfall = (((-d) - 0.30) / 0.30).min(1.0);
            worst = worst.max(shortfall);
            // granaries (pottery) blunt a failed year
            let granary = if self
                .societies
                .get(culture.0)
                .map_or(false, |so| so.knows(society::TechId::Pottery))
            {
                0.75
            } else {
                1.0
            };
            let hit = ((pop as f64) * (0.05 + 0.16 * shortfall) * granary) as i64;
            if hit < 4 {
                continue;
            }
            let dead = (hit as f64 * 0.55) as i64;
            let walked = hit - dead;
            self.settlements[i].pop = (pop - hit).max(30);
            // the hungry walk to the nearest kin-town outside the blight —
            // ring search over the bucket grid (E5.3), same winner as the
            // old full scan: nearest by (distance², index)
            let target = town_buckets.nearest(x as f64, y as f64, |j| {
                let o = &self.settlements[j];
                j != i && o.culture == culture && !(dry(o.x, o.y) < -0.30)
            });
            let text = if let Some((j, _)) = target {
                migrations.push((j, walked));
                format!(
                    "The rains fail over {} — {} starve, and {} take the road to {}.",
                    name, dead, walked, self.settlements[j].name
                )
            } else {
                format!(
                    "The rains fail over {} — {} starve in the dust of a dead harvest.",
                    name,
                    dead + walked
                )
            };
            events.push(Event {
                m: month_abs,
                s: name,
                k: EventKind::Famine,
                text,
                ..Default::default()
            });
        }
        for (j, souls) in migrations {
            self.settlements[j].pop += souls;
        }
        // scarcity is priced at once: one grain spike per failed year
        if worst > 0.0 && self.grain_shock_year != year {
            self.grain_shock_year = year;
            self.market.shock(resources::Good::Grain, 1.0 + 0.30 * worst);
        }
        events
    }

    /// Prospectors comb the hinterlands for hidden seams; worked mines thin
    /// toward exhaustion. Returns events and whether the deposit list changed.
    fn prospect_and_deplete(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        use rand::Rng;
        let mut events = Vec::new();
        let mut changed = false;
        let mods: Vec<society::Mods> = self.societies.iter().map(society::mods_for).collect();

        // --- discovery: hidden seams within a town's ranging distance may
        // come to light; rarer metals hide longer, better arts search wider.
        // Beyond the home range there is a second, thinner channel — the far
        // venture: prospecting parties that push into wild country, so even
        // mountain gold no town can reach is found in the fullness of time.
        //
        // E5.3: the deposit bucket grid inverts the old O(deposits × towns)
        // scan — each town only touches seams inside its own venture range.
        // Beyond 3.5 ranges the chance is zero, so an uncovered deposit and
        // a covered-but-too-far one are the same outcome (no rng drawn) and
        // the random stream is untouched.
        let deposit_buckets = crate::util::Buckets::build(
            self.deposits.iter().map(|d| (d.x as f64, d.y as f64)).collect(),
            32.0,
        );
        let mut best: Vec<Option<(f64, usize)>> = vec![None; self.deposits.len()];
        let mut cand: Vec<usize> = Vec::new();
        for (si, s) in self.settlements.iter().enumerate() {
            let reach = settlements::territory_radius(s.pop)
                * 2.4
                * mods.get(s.culture.idx()).map(|m| m.prospecting).unwrap_or(1.0);
            let reach = reach.max(1e-9);
            deposit_buckets.candidates(s.x as f64, s.y as f64, 3.5 * reach + 1.0, &mut cand);
            for &di in &cand {
                let d = &self.deposits[di];
                if d.known {
                    continue;
                }
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                let ratio = (dx * dx + dy * dy).sqrt() / reach;
                if best[di].map_or(true, |(b, _)| ratio < b) {
                    best[di] = Some((ratio, si));
                }
            }
        }
        let mut found: Vec<(usize, usize)> = Vec::new();
        for (di, d) in self.deposits.iter().enumerate() {
            if d.known {
                continue;
            }
            let Some((ratio, si)) = best[di] else { continue };
            let rarity = match d.r.abundance() {
                resources::Abundance::Uncommon => 0.6,
                resources::Abundance::Rare => 0.35,
                resources::Abundance::Legendary => 0.12,
                resources::Abundance::Common => 1.0,
            };
            let p = if ratio <= 1.0 {
                0.012 * rarity * (1.0 - 0.65 * ratio) // combing the home range
            } else if ratio <= 3.5 {
                0.0018 * rarity // a far venture into unclaimed country
            } else {
                0.0
            };
            if p > 0.0 && self.rng.gen::<f64>() < p {
                found.push((di, si));
            }
        }
        for (di, si) in found {
            self.deposits[di].known = true;
            changed = true;
            let kind = self.deposits[di].r;
            let rich = self.deposits[di].rich;
            let precious = matches!(
                kind,
                resources::Good::Silver | resources::Good::Gold | resources::Good::Mithril
            );
            // the market feels the strike before the first cart arrives —
            // hardest where the ore actually is (M5.2: local first)
            self.market.shock(kind, if precious { 0.88 } else { 0.84 });
            let ka = self.areas.area_of(si);
            if let Some(mk) = self.areas.markets.get_mut(ka) {
                mk.shock(kind, if precious { 0.80 } else { 0.75 });
            }
            let sname = self.settlements[si].name.clone();
            let strike = 12.0 + 45.0 * rich * economy::base_value(kind);
            self.settlements[si].wealth = round2(self.settlements[si].wealth + strike);
            if precious {
                // a rush: prospectors, chancers and mule-trains pour in
                let influx = ((self.settlements[si].pop as f64 * 0.05) as i64).max(10);
                self.settlements[si].pop += influx;
            }
            let text = match kind {
                resources::Good::Gold => format!(
                    "Gold! Panners out of {} lift bright dust from the gravels — word runs faster than horses.",
                    sname
                ),
                resources::Good::Silver => format!(
                    "A silver seam glitters by torchlight in the diggings above {}.",
                    sname
                ),
                resources::Good::Mithril => format!(
                    "Beneath the mountain-roots, miners of {} strike mithril — truesilver, the dream of every smith.",
                    sname
                ),
                _ => format!(
                    "Prospectors out of {} strike {} in the hills; the seam runs deep and true.",
                    sname, kind
                ),
            };
            events.push(Event {
                m: month_abs,
                s: sname,
                k: EventKind::Discovery,
                text,
                ..Default::default()
            });
            self.refresh_goods_near(di);
        }

        // --- depletion: a worked seam only lasts so many carts
        let mut spent: Vec<usize> = Vec::new();
        for di in 0..self.deposits.len() {
            let d = &self.deposits[di];
            if !d.known || d.left <= 0.0 {
                continue;
            }
            let mut crews = 0.0;
            for s in &self.settlements {
                if !s.goods.iter().any(|g| *g == d.r) {
                    continue;
                }
                let r = settlements::work_radius(s.pop);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                if dx * dx + dy * dy <= r * r {
                    crews += 1.0 + (s.pop as f64 / 9000.0).min(1.0);
                }
            }
            if crews == 0.0 {
                continue;
            }
            let d = &mut self.deposits[di];
            d.left = round2((d.left - crews).max(0.0));
            if d.left == 0.0 {
                spent.push(di);
            }
        }
        for di in spent {
            changed = true;
            let kind = self.deposits[di].r;
            let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
            let near_i = self
                .settlements
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| (s.x - dx0).pow(2) + (s.y - dy0).pow(2))
                .map(|(i, _)| i);
            let near = near_i
                .map(|i| self.settlements[i].name.clone())
                .unwrap_or_else(|| "the wilds".to_string());
            self.market.shock(kind, 1.22);
            // the pit's own market feels the silence first (M5.2)
            if let Some(i) = near_i {
                let ka = self.areas.area_of(i);
                if let Some(mk) = self.areas.markets.get_mut(ka) {
                    mk.shock(kind, 1.35);
                }
            }
            events.push(Event {
                m: month_abs,
                s: near.clone(),
                k: EventKind::Depletion,
                text: format!(
                    "The last good ore comes up from the {} pits near {}; the galleries fall silent.",
                    kind, near
                ),
                ..Default::default()
            });
            self.refresh_goods_near(di);
        }
        (events, changed)
    }

    /// Re-list goods for every settlement whose hinterland covers deposit `di`.
    fn refresh_goods_near(&mut self, di: usize) {
        let (dx0, dy0) = (self.deposits[di].x, self.deposits[di].y);
        for i in 0..self.settlements.len() {
            let s = &self.settlements[i];
            let r = settlements::work_radius(s.pop);
            let ddx = (dx0 - s.x) as f64;
            let ddy = (dy0 - s.y) as f64;
            if ddx * ddx + ddy * ddy <= r * r {
                trade::goods_for(&mut self.settlements[i], &self.deposits, &self.fertility);
            }
        }
    }

    /// Deposits the world has actually found — all the client ever sees.
    fn known_deposits(&self) -> Vec<&Deposit> {
        self.deposits.iter().filter(|d| d.known).collect()
    }

    /// Cultures with ruler, era, polity, arts and treasury attached.
    fn cultures_json(&self) -> Value {
        let arr: Vec<Value> = self
            .cultures
            .iter()
            .map(|c| {
                let mut v = serde_json::to_value(c).unwrap();
                let polity = self.societies.get(c.id.0).map(|s| s.polity).unwrap_or(0);
                if let Some(r) = self.chron.rulers.iter().find(|r| r.culture == c.id) {
                    let title = society::RULER_TITLES[polity];
                    v["ruler"] = if title.is_empty() {
                        json!(r.title())
                    } else {
                        json!(format!("{} {}", title, r.title()))
                    };
                }
                if let Some(soc) = self.societies.get(c.id.0) {
                    v["era"] = json!(society::ERAS[soc.era]);
                    v["polity"] = json!(society::POLITIES[soc.polity]);
                    v["treasury"] = json!(round2(soc.treasury));
                    let names: Vec<&'static str> = soc
                        .techs
                        .iter()
                        .map(|&id| society::tech(id).name)
                        .collect();
                    v["techs"] = json!(names);
                }
                // statecraft readouts (M4): solidarity, the crown's standing,
                // whose leash they wear, and whether the realm still stands
                if let Some(a) = self.politics.asab.get(c.id.0) {
                    v["asab"] = json!(round2(*a));
                }
                if let Some(l) = self.politics.legit.get(c.id.0) {
                    v["legit"] = json!(round2(*l));
                }
                if let Some(Some(suz)) = self.politics.vassal_of.get(c.id.0) {
                    v["vassal_of"] = json!(self.cultures[suz.0].people.clone());
                }
                v["alive"] = json!(politics::alive(&self.settlements, c.id));
                v
            })
            .collect();
        Value::Array(arr)
    }

    /// E4.2 hot/cold split, town side: the monthly heartbeat zeroed out.
    /// A town whose full form moved but whose cold form did not ships a
    /// tiny heartbeat patch instead of the whole object.
    fn settlement_cold_sig(s: &Settlement) -> u64 {
        let mut c = s.clone();
        c.pop = 0;
        c.food = 0.0;
        c.k = 0.0;
        c.wealth = 0.0;
        crate::util::fnv1a64(serde_json::to_string(&c).unwrap().as_bytes())
    }

    /// Decompose a market-area hub row for delta gating (E4.3): hub id,
    /// cold hash over id·name·member-count, and price bits per good at
    /// wire precision.
    fn hub_wire(h: &Value) -> (i64, u64, Vec<(String, u64)>) {
        let id = h["id"].as_i64().unwrap();
        let cold =
            crate::util::fnv1a64(format!("{}|{}|{}", id, h["name"], h["n"]).as_bytes());
        let pbits = h["p"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(g, v)| (g.clone(), v.as_f64().unwrap_or(0.0).to_bits()))
                    .collect()
            })
            .unwrap_or_default();
        (id, cold, pbits)
    }


    /// E4.2 hot/cold split, culture side: (full string, cold string, hot
    /// patch rows). treasury/asab/legit are the heartbeat; the cold form
    /// strips them, the hot rows carry them keyed by array index.
    fn cultures_split(&self) -> (String, String, String) {
        let full = self.cultures_json();
        let mut cold = full.clone();
        let mut rows: Vec<Value> = Vec::new();
        if let Value::Array(items) = &mut cold {
            for (i, cv) in items.iter_mut().enumerate() {
                if let Value::Object(o) = cv {
                    let mut row = serde_json::Map::new();
                    row.insert("i".into(), json!(i));
                    for kf in ["treasury", "asab", "legit"] {
                        if let Some(v) = o.remove(kf) {
                            row.insert(kf.into(), v);
                        }
                    }
                    if row.len() > 1 {
                        rows.push(Value::Object(row));
                    }
                }
            }
        }
        (
            full.to_string(),
            cold.to_string(),
            serde_json::to_string(&rows).unwrap(),
        )
    }

    /// The tick payload, v2 (E4.1–E4.4): month and chronicle cursor always;
    /// every other section rides only when its content moved since it last
    /// crossed (E4.2/E4.3 hashes, E4.5 dirty bits). One direct-serialize
    /// struct of pre-serialized `RawValue` sections — nothing is built
    /// twice. Absent key = "you already hold the truth"; the client merges.
    pub fn tick_json(&mut self, months: i64) -> String {
        use serde_json::value::RawValue;

        #[derive(Serialize)]
        struct Payload {
            month: i64,
            /// Chronicle cursor [from, to): the client pulls the slice via
            /// `events_range` (E4.4) — event arrays left the tick payload.
            ev: [u64; 2],
            /// Toast-worthy picks as indices into the [from, to) slice
            /// (E4.8) — no event ever ships twice.
            #[serde(skip_serializing_if = "Vec::is_empty")]
            headlines: Vec<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            settlements: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            settlements_gone: Vec<i64>,
            /// Heartbeat patches (E4.2): towns whose only news is
            /// pop/food/k/wealth — merged over the held object client-side.
            #[serde(skip_serializing_if = "Option::is_none")]
            s_hot: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            cultures: Option<Box<RawValue>>,
            /// Heartbeat patches for cultures (treasury/asab/legit), by
            /// array index — ships when only the heartbeat moved (E4.2).
            #[serde(skip_serializing_if = "Option::is_none")]
            c_hot: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            wars: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            market: Option<Box<RawValue>>,
            /// Per-good market row patches (E4.3), merged by `g`.
            #[serde(skip_serializing_if = "Option::is_none")]
            m_hot: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            areas: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            merchants: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            routes: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            deposits: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            deposits_hidden: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            features: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            ruins: Option<Box<RawValue>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            territory: Option<Box<RawValue>>,
        }

        fn raw(s: String) -> Option<Box<RawValue>> {
            Some(RawValue::from_string(s).unwrap())
        }

        let ev_from = self.events.len();
        let _ = self.tick(months); // change tracking rides self.dirty (E4.5)
        let ev_to = self.events.len();

        // E4.8 — the toast-worthy picks, as indices into the [from, to)
        // chronicle slice the client pulls anyway; no event ships twice.
        let heads: Vec<u32> = self.events[ev_from..ev_to]
            .iter()
            .enumerate()
            .filter(|(_, e)| headline_worthy(e.k))
            .map(|(i, _)| i as u32)
            .take(6)
            .collect();

        // E4.2 — settlements cross only when their cold form moved; when
        // only the heartbeat moved, the fields that moved cross as a patch.
        let mut changed: Vec<String> = Vec::new();
        let mut hot: Vec<String> = Vec::new();
        let mut cache: Vec<(i64, u64, [i64; 4])> =
            Vec::with_capacity(self.settlements.len());
        for s in &self.settlements {
            let cold = Self::settlement_cold_sig(s);
            // the heartbeat at wire precision: pop · food(0.1) · k(1) ·
            // wealth(1) — each matches what the client actually displays
            let hotq = [
                s.pop,
                (s.food * 10.0).round() as i64,
                s.k.round() as i64,
                s.wealth.round() as i64,
            ];
            let prev = self
                .sent
                .settlements
                .iter()
                .find(|(id, _, _)| *id == s.id.0)
                .map(|&(_, c, q)| (c, q));
            match prev {
                Some((pc, pq)) if pc == cold && pq == hotq => {}
                Some((pc, pq)) if pc == cold => {
                    // positional heartbeat row (E4.2): [id, pop, food, k,
                    // wealth], null = unchanged — keys carry no information
                    // the slot position doesn't already carry
                    let mut row =
                        vec![json!(s.id.0), Value::Null, Value::Null, Value::Null, Value::Null];
                    if pq[0] != hotq[0] {
                        row[1] = json!(s.pop);
                    }
                    if pq[1] != hotq[1] {
                        row[2] = json!(hotq[1] as f64 / 10.0);
                    }
                    if pq[2] != hotq[2] {
                        row[3] = json!(hotq[2]);
                    }
                    if pq[3] != hotq[3] {
                        row[4] = json!(hotq[3]);
                    }
                    hot.push(Value::Array(row).to_string());
                }
                _ => changed.push(serde_json::to_string(s).unwrap()),
            }
            cache.push((s.id.0, cold, hotq));
        }
        let gone: Vec<i64> = self
            .sent
            .settlements
            .iter()
            .map(|&(id, _, _)| id)
            .filter(|id| !cache.iter().any(|(cid, _, _)| cid == id))
            .collect();
        self.sent.settlements = cache;

        // E4.3 — whole blocks gated by content hash: serialized once for
        // the gate, reused verbatim as the wire bytes when they moved.
        // Cultures get the hot/cold split: when only the heartbeat moved,
        // the tiny c_hot rows cross instead of the whole block (E4.2).
        let (cul_full, cul_cold, cul_hot) = self.cultures_split();
        let cul_full_h = crate::util::fnv1a64(cul_full.as_bytes());
        let cul_cold_h = crate::util::fnv1a64(cul_cold.as_bytes());
        let mut cultures = None;
        let mut c_hot = None;
        if self.sent.blocks[0] != cul_full_h {
            if self.sent.cultures_cold != cul_cold_h {
                cultures = raw(cul_full);
            } else {
                c_hot = raw(cul_hot);
            }
            self.sent.blocks[0] = cul_full_h;
            self.sent.cultures_cold = cul_cold_h;
        }

        let block_strings = [
            serde_json::to_string(&self.politics.wars).unwrap(),
            serde_json::to_string(&self.merchants).unwrap(),
        ];
        let mut gated: [Option<Box<RawValue>>; 2] = [None, None];
        for (i, s) in block_strings.into_iter().enumerate() {
            let h = crate::util::fnv1a64(s.as_bytes());
            if self.sent.blocks[i + 1] != h {
                self.sent.blocks[i + 1] = h;
                gated[i] = raw(s);
            }
        }
        let [wars, merchants] = gated;

        // E4.3 — the market ledger, gated per row: the whole list reships
        // only when the set of priced goods changed; otherwise the rows
        // whose content moved cross as m_hot and the client merges by good.
        let market_v = self.market.snapshot();
        let market_rows: Vec<(String, String)> = market_v
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["g"].as_str().unwrap().to_string(), r.to_string()))
            .collect();
        let mut names: Vec<&String> = market_rows.iter().map(|(g, _)| g).collect();
        names.sort();
        let mut prev_names: Vec<&String> =
            self.sent.market_rows.iter().map(|(g, _)| g).collect();
        prev_names.sort();
        let (market, m_hot) = if names != prev_names {
            self.sent.market_rows = market_rows
                .iter()
                .map(|(g, s)| (g.clone(), crate::util::fnv1a64(s.as_bytes())))
                .collect();
            (raw(market_v.to_string()), None)
        } else {
            let mut out: Vec<&str> = Vec::new();
            let mut fresh: Vec<(String, u64)> = Vec::with_capacity(market_rows.len());
            for (g, s) in &market_rows {
                let h = crate::util::fnv1a64(s.as_bytes());
                let prev = self
                    .sent
                    .market_rows
                    .iter()
                    .find(|(pg, _)| pg == g)
                    .map(|&(_, h)| h);
                if prev != Some(h) {
                    out.push(s);
                }
                fresh.push((g.clone(), h));
            }
            self.sent.market_rows = fresh;
            if out.is_empty() {
                (None, None)
            } else {
                (None, raw(format!("[{}]", out.join(","))))
            }
        };

        // E4.3 — market areas, gated per hub and per good: the whole block
        // reships only when the hub set changed (the "of" vector moved).
        // Otherwise a hub whose cold half (name, member count) moved ships
        // its full row; a hub where only prices moved ships {id, p: {only
        // the goods that moved}}; spread rows ride along when they moved.
        let areas_v = economy::areas_json(&self.areas, &self.settlements);
        let of_h = crate::util::fnv1a64(areas_v["of"].to_string().as_bytes());
        let spread_s = areas_v["spread"].to_string();
        let spread_h = crate::util::fnv1a64(spread_s.as_bytes());
        let hubs_v = areas_v["hubs"].as_array().unwrap();
        let areas = if of_h != self.sent.areas_of {
            self.sent.areas_of = of_h;
            self.sent.areas_spread = spread_h;
            self.sent.areas_hubs = hubs_v.iter().map(Self::hub_wire).collect();
            raw(areas_v.to_string())
        } else {
            let mut rows: Vec<String> = Vec::new();
            let mut fresh: Vec<(i64, u64, Vec<(String, u64)>)> =
                Vec::with_capacity(hubs_v.len());
            for h in hubs_v {
                let (id, cold, pbits) = Self::hub_wire(h);
                let prev = self.sent.areas_hubs.iter().find(|(pid, _, _)| *pid == id);
                match prev {
                    Some((_, pcold, ppb))
                        if *pcold == cold
                            && ppb.len() == pbits.len()
                            && ppb.iter().zip(pbits.iter()).all(|(a, b)| a.0 == b.0) =>
                    {
                        let mut pm = serde_json::Map::new();
                        for ((g, bits), (_, pb)) in pbits.iter().zip(ppb.iter()) {
                            if bits != pb {
                                pm.insert(g.clone(), json!(f64::from_bits(*bits)));
                            }
                        }
                        if !pm.is_empty() {
                            rows.push(json!({ "id": id, "p": Value::Object(pm) }).to_string());
                        }
                    }
                    _ => rows.push(h.to_string()),
                }
                fresh.push((id, cold, pbits));
            }
            self.sent.areas_hubs = fresh;
            let spread_moved = spread_h != self.sent.areas_spread;
            self.sent.areas_spread = spread_h;
            if rows.is_empty() && !spread_moved {
                None
            } else {
                let mut out = format!("{{\"hubs\":[{}]", rows.join(","));
                if spread_moved {
                    out.push_str(",\"spread\":");
                    out.push_str(&spread_s);
                }
                out.push('}');
                raw(out)
            }
        };

        let dep = self.dirty.take(Dirty::DEPOSITS);
        let payload = Payload {
            month: self.month,
            ev: [ev_from as u64, ev_to as u64],
            headlines: heads,
            settlements: if changed.is_empty() {
                None
            } else {
                raw(format!("[{}]", changed.join(",")))
            },
            settlements_gone: gone,
            s_hot: if hot.is_empty() {
                None
            } else {
                raw(format!("[{}]", hot.join(",")))
            },
            cultures,
            c_hot,
            wars,
            market,
            m_hot,
            areas,
            merchants,
            routes: if self.dirty.take(Dirty::ROUTES) {
                raw(serde_json::to_string(&self.routes).unwrap())
            } else {
                None
            },
            deposits: if dep {
                raw(serde_json::to_string(&self.known_deposits()).unwrap())
            } else {
                None
            },
            deposits_hidden: if dep {
                Some(self.deposits.iter().filter(|d| !d.known).count())
            } else {
                None
            },
            features: if self.dirty.take(Dirty::FEATURES) {
                raw(serde_json::to_string(&self.features).unwrap())
            } else {
                None
            },
            ruins: if self.dirty.take(Dirty::RUINS) {
                raw(serde_json::to_string(&self.ruins).unwrap())
            } else {
                None
            },
            territory: if self.dirty.take(Dirty::TERRITORY) {
                raw(serde_json::to_string(&politics::territory_rle(&self.territory)).unwrap())
            } else {
                None
            },
        };
        serde_json::to_string(&payload).unwrap()
    }

    /// E4.2/E4.3 — seed the delta baseline to the freshly generated world,
    /// which is exactly what `bootstrap()` ships; the first tick then
    /// carries only what actually moved after month 0.
    fn prime_sent(&mut self) {
        self.sent.settlements = self
            .settlements
            .iter()
            .map(|s| {
                (
                    s.id.0,
                    Self::settlement_cold_sig(s),
                    [
                        s.pop,
                        (s.food * 10.0).round() as i64,
                        s.k.round() as i64,
                        s.wealth.round() as i64,
                    ],
                )
            })
            .collect();
        let (cul_full, cul_cold, _) = self.cultures_split();
        self.sent.cultures_cold = crate::util::fnv1a64(cul_cold.as_bytes());
        self.sent.blocks = [
            crate::util::fnv1a64(cul_full.as_bytes()),
            crate::util::fnv1a64(serde_json::to_string(&self.politics.wars).unwrap().as_bytes()),
            crate::util::fnv1a64(serde_json::to_string(&self.merchants).unwrap().as_bytes()),
        ];
        self.sent.market_rows = self
            .market
            .snapshot()
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["g"].as_str().unwrap().to_string(),
                    crate::util::fnv1a64(r.to_string().as_bytes()),
                )
            })
            .collect();
        let areas_v = economy::areas_json(&self.areas, &self.settlements);
        self.sent.areas_of = crate::util::fnv1a64(areas_v["of"].to_string().as_bytes());
        self.sent.areas_spread =
            crate::util::fnv1a64(areas_v["spread"].to_string().as_bytes());
        self.sent.areas_hubs = areas_v["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .map(Self::hub_wire)
            .collect();
    }

    /// Minimal pack-header meta (E3.1): identity, dimensions and physical
    /// constants only — everything entity-shaped rides `bootstrap()`.
    fn pack_meta(&self) -> Value {
        json!({
            "seed": self.seed,
            "size": self.size,
            "width": self.width,
            "height_cells": self.size,
            "month": self.month,
            "months": constants::MONTHS,
            "sea_level": 0.0,
            "metres_per_unit": constants::METRES_PER_UNIT,
            "km_per_cell": constants::KM_PER_CELL,
            "world_name": self.world_name,
        })
    }

    /// The once-per-world bootstrap (E3.1): vocabulary tables plus the
    /// entity state ticks also carry. Its own small JSON call — the
    /// multi-megabyte pack header stops duplicating the tick payload.
    pub fn bootstrap(&self) -> Value {
        let ev_start = self.events.len().saturating_sub(60);
        json!({
            "biomes": constants::biome_meta(),
            // E1.12 — wire enums ship as small ints; these tables give them names
            "event_kinds": EventKind::iter().map(|k| k.name()).collect::<Vec<_>>(),
            "entity_kinds": crate::entity::EntityKind::iter().map(|k| k.name()).collect::<Vec<_>>(),
            "crop_packages": agriculture::CropPackage::iter()
                .map(|p| json!({
                    "id": p.code(),
                    "name": p.name(),
                    "density": p.density(),
                }))
                .collect::<Vec<Value>>(),
            "resources": resources::resource_meta(),
            "deposits": self.known_deposits(),
            "deposits_hidden": self.deposits.iter().filter(|d| !d.known).count(),
            "settlements": self.settlements,
            "cultures": self.cultures_json(),
            "features": self.features,
            "routes": self.routes,
            "ruins": self.ruins,
            "wars": self.politics.wars,
            "market": self.market.snapshot(),
            "areas": economy::areas_json(&self.areas, &self.settlements),
            "merchants": self.merchants,
            "events": self.events[ev_start..],
        })
    }

    pub fn bootstrap_json(&self) -> String {
        self.bootstrap().to_string()
    }

    /// Merged view — exactly what the client holds after unpack +
    /// bootstrap. Native tooling (genjs, worldgen) reads this.
    pub fn meta(&self) -> Value {
        let mut m = self.pack_meta();
        for (k, v) in self.bootstrap().as_object().unwrap() {
            m[k.as_str()] = v.clone();
        }
        m
    }

    /// Generation stage timings, seconds, in stage order — the debug side
    /// channel (E3.9). Wall-clock was the one nondeterministic region of
    /// the pack header; it no longer rides the pack at all.
    pub fn timings_json(&self) -> String {
        let pairs: Vec<Value> = self
            .timings
            .iter()
            .map(|(k, v)| json!([k, round3(*v / 1000.0)]))
            .collect();
        serde_json::to_string(&pairs).unwrap()
    }

    /// Boolean view of one `CellFlags` bit — offline tooling convenience;
    /// hot paths test bits on `self.flags` directly.
    pub fn mask(&self, f: CellFlags) -> Array2<bool> {
        self.flags.mapv(|b| b & f.bits() != 0)
    }

    // The field registry itself is declared once, below `pack()`, via the
    // `field_registry!` macro (E2.1) — it expands to both the static
    // `FIELD_SPECS` table (for codegen, E2.4) and `World::fields()`.

    /// Pack v2 (E3.3–E3.6): `[u32 header_len][header json (padded to 4)][blob]`.
    /// The header carries `pack: 2`, a CRC-32 of the blob (E3.6), and the
    /// territory grid as RLE instead of a raw section (E3.5); float grids
    /// ride as quantized u16 where the registry says so (E3.4). The blob is
    /// written once, straight from grid storage — no per-field temporary
    /// buffers (E3.3). Section order comes from the field registry (E2.2).
    pub fn pack(&self) -> Vec<u8> {
        let fields = self.fields();
        let cells = self.size * self.width;
        let mut blob: Vec<u8> = Vec::with_capacity(cells * 20 + 64);
        let mut entries: Vec<Value> = Vec::new();
        for f in &fields {
            // territory rides the header as RLE (E3.5): contiguous realms
            // compress ~1000×, and the client already speaks this encoding
            // for tick patches.
            if f.name == "territory" {
                continue;
            }
            let offset = blob.len();
            let mut entry = json!({
                "name": f.name,
                "dtype": f.data.dtype(),
                "shape": [self.size, self.width],
            });
            match (&f.data, f.quant) {
                (FieldData::F32(a), Quant::Linear) => {
                    let s = a.as_slice().expect("registry grids are contiguous");
                    let (lo, hi) = min_max(s);
                    let (scale, inv) = quant_steps(lo, hi);
                    blob.reserve(s.len() * 2);
                    for &v in s {
                        let q = ((v as f64 - lo) * inv).round().clamp(0.0, 65535.0) as u16;
                        blob.extend_from_slice(&q.to_le_bytes());
                    }
                    entry["dtype"] = json!("uint16");
                    entry["q"] = json!({ "scale": scale, "offset": lo, "xform": "linear" });
                }
                (FieldData::F32(a), Quant::Sqrt) => {
                    // 16 bits spent in sqrt-space: low flows keep relative
                    // precision even though discharge spans ~6 decades.
                    let s = a.as_slice().expect("registry grids are contiguous");
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for &v in s {
                        let t = (v.max(0.0) as f64).sqrt();
                        if t < lo { lo = t; }
                        if t > hi { hi = t; }
                    }
                    if !lo.is_finite() {
                        lo = 0.0;
                        hi = 0.0;
                    }
                    let (scale, inv) = quant_steps(lo, hi);
                    blob.reserve(s.len() * 2);
                    for &v in s {
                        let t = (v.max(0.0) as f64).sqrt();
                        let q = ((t - lo) * inv).round().clamp(0.0, 65535.0) as u16;
                        blob.extend_from_slice(&q.to_le_bytes());
                    }
                    entry["dtype"] = json!("uint16");
                    entry["q"] = json!({ "scale": scale, "offset": lo, "xform": "sqrt" });
                }
                (data, _) => data.write_into(&mut blob),
            }
            entry["offset"] = json!(offset);
            entry["nbytes"] = json!(blob.len() - offset);
            entries.push(entry);
        }

        let mut header = self.pack_meta();
        header["id"] = json!(format!("{}-{}", self.seed, self.size));
        header["pack"] = json!(PACK_VERSION);
        header["crc32"] = json!(crate::util::crc32(&blob));
        header["territory"] = json!(politics::territory_rle(&self.territory));
        header["arrays"] = Value::Array(entries);
        let mut hjson = serde_json::to_string(&header).unwrap().into_bytes();
        while hjson.len() % 4 != 0 {
            hjson.push(b' ');
        }

        let mut out = Vec::with_capacity(4 + hjson.len() + blob.len());
        out.extend_from_slice(&(hjson.len() as u32).to_le_bytes());
        out.extend_from_slice(&hjson);
        out.extend_from_slice(&blob);
        out
    }
}

/// Pack protocol version — the client refuses any other (E3.6).
pub const PACK_VERSION: u32 = 2;

fn min_max(s: &[f32]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &v in s {
        let v = v as f64;
        if v < lo { lo = v; }
        if v > hi { hi = v; }
    }
    if lo.is_finite() { (lo, hi) } else { (0.0, 0.0) }
}

/// `(scale, 1/scale)` for a u16 span over `[lo, hi]`; constant fields get 0.
fn quant_steps(lo: f64, hi: f64) -> (f64, f64) {
    if hi > lo {
        let scale = (hi - lo) / 65535.0;
        (scale, 1.0 / scale)
    } else {
        (0.0, 0.0)
    }
}

/// One grid's declaration in the field registry (E2.1).
pub struct FieldDecl<'a> {
    /// Wire + registry name; also the JS-side array key.
    pub name: &'static str,
    /// Human units, for docs and generated constants.
    pub units: &'static str,
    /// Included in the diagnostics state hash (mutable-by-tick grids yes;
    /// grids derivable from them or static after generation, no).
    pub in_hash: bool,
    /// True when Orbital uploads this grid as a texture (E2.2: the upload
    /// list on the JS side derives from the generated constants).
    pub gpu: bool,
    /// Wire quantization (E3.4) — storage and the determinism hash always
    /// see full f32; quantization is strictly a wire concern.
    pub quant: Quant,
    pub data: FieldData<'a>,
}

/// Data-free registry row — what codegen and offline tooling see (E2.4).
pub struct FieldSpec {
    pub name: &'static str,
    pub dtype: &'static str,
    pub units: &'static str,
    pub in_hash: bool,
    pub gpu: bool,
}

/// E2.1 — the field registry macro: every per-cell grid the world owns,
/// declared exactly once with name, storage kind, units, hash inclusion and
/// GPU upload flag. Expands to the static `FIELD_SPECS` table (codegen) and
/// `World::fields()` (pack + hash). A grid added here is a grid added
/// everywhere; field-order drift dies structurally (E2.2).
///
/// Order is the pack order and is a versioned contract (ADR-0007).
macro_rules! dtype_name {
    (F32) => { "float32" };
    (U8) => { "uint8" };
    (I16) => { "int16" };
}

// The `wire` column: how the grid crosses WASM→JS (E3.4). `raw` ships
// storage bytes verbatim; `u16` is linear 16-bit quantization over the
// field's live range; `u16sqrt` quantizes in sqrt-space (wide-dynamic-range
// fields keep relative precision at the low end). The client dequantizes
// back to float32 at the unpack edge, so everything downstream is unchanged.
macro_rules! quant_mode {
    (raw) => { Quant::None };
    (u16) => { Quant::Linear };
    (u16sqrt) => { Quant::Sqrt };
}

macro_rules! field_registry {
    ($($field:ident : $kind:ident, units $units:literal, hash $h:literal, gpu $g:literal, wire $wire:ident;)+) => {
        /// Static view of the field registry, in pack order (E2.1/E2.4).
        /// `dtype` is the *decoded* type the client ends up holding.
        pub const FIELD_SPECS: &[FieldSpec] = &[$(
            FieldSpec {
                name: stringify!($field),
                dtype: dtype_name!($kind),
                units: $units,
                in_hash: $h,
                gpu: $g,
            },
        )+];

        impl World {
            /// The live registry: specs bound to this world's grids.
            pub fn fields(&self) -> Vec<FieldDecl<'_>> {
                vec![$(
                    FieldDecl {
                        name: stringify!($field),
                        units: $units,
                        in_hash: $h,
                        gpu: $g,
                        quant: quant_mode!($wire),
                        data: FieldData::$kind(&self.$field),
                    },
                )+]
            }
        }
    };
}

field_registry! {
    height:    F32, units "rel. elevation (0 = sea)",        hash true,  gpu true,  wire u16;
    tmean:     F32, units "°C annual mean",                  hash false, gpu true,  wire u16;
    tamp:      F32, units "°C seasonal amplitude",           hash false, gpu true,  wire u16;
    precip:    F32, units "mm/yr",                           hash false, gpu true,  wire u16;
    pamp:      F32, units "signed monsoon share −1..1",      hash true,  gpu false, wire u16;
    discharge: F32, units "flow accumulation (cells·rain)",  hash false, gpu true,  wire u16sqrt;
    flow_amp:  F32, units "signed seasonal swing −1..1",     hash true,  gpu false, wire u16;
    fertility: F32, units "0..1 arable index",               hash false, gpu true,  wire u16;
    biomes:    U8,  units "biome id",                        hash true,  gpu false, wire raw;
    crops:     U8,  units "crop package id",                 hash true,  gpu false, wire raw;
    strahler:  U8,  units "stream order, 0 off-river",       hash true,  gpu true,  wire raw;
    flags:     U8,  units "CellFlags bits",                  hash true,  gpu true,  wire raw;
    territory: I16, units "owner culture, −1 wild",          hash false, gpu false, wire raw;
}

/// Wire quantization mode for a registry field (E3.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quant {
    /// Storage bytes ride verbatim.
    None,
    /// Linear u16 over the field's live `[min, max]` span.
    Linear,
    /// Linear u16 in sqrt-space — for wide-dynamic-range fields.
    Sqrt,
}

/// Borrowed grid storage behind a registry entry. Storage is f32 at rest
/// (E3.2); the wire may narrow further via `Quant` (E3.4).
pub enum FieldData<'a> {
    F32(&'a Array2<f32>),
    U8(&'a Array2<u8>),
    I16(&'a Array2<i16>),
}

impl FieldData<'_> {
    /// Decoded dtype name as the JS client ends up holding it.
    pub fn dtype(&self) -> &'static str {
        match self {
            FieldData::F32(_) => "float32",
            FieldData::U8(_) => "uint8",
            FieldData::I16(_) => "int16",
        }
    }

    /// Append raw little-endian bytes straight from grid storage — the
    /// no-temporaries path of pack v2 (E3.3).
    pub fn write_into(&self, out: &mut Vec<u8>) {
        match self {
            FieldData::F32(a) => out.extend_from_slice(bytemuck::cast_slice(
                a.as_slice().expect("registry grids are contiguous"),
            )),
            FieldData::U8(a) => {
                out.extend_from_slice(a.as_slice().expect("registry grids are contiguous"))
            }
            FieldData::I16(a) => out.extend_from_slice(bytemuck::cast_slice(
                a.as_slice().expect("registry grids are contiguous"),
            )),
        }
    }

    /// Exact-width storage bytes for the determinism hash — the hash sees
    /// every bit the simulation sees (f32 at rest since E3.2).
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        match self {
            FieldData::F32(a) => {
                for &v in a.iter() {
                    out.extend_from_slice(&v.to_bits().to_le_bytes());
                }
            }
            FieldData::U8(a) => out.extend(a.iter().cloned()),
            FieldData::I16(a) => {
                for &v in a.iter() {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

// ---------------------------------------------------------------- bands

use crate::util::Band;

/// Diagnostics bands (E2.7): the engine's speed and wire budget.
pub const BANDS: &[Band] = &[
    Band { name: "512 generation time", sweet: (0.0, 3000.0), hard: (0.0, 8000.0), target: "sweet ≤3s · hard ≤8s (wasm ≈ 2× native)" },
    Band { name: "tick rate", sweet: (100.0, f64::INFINITY), hard: (25.0, f64::INFINITY), target: "sweet ≥100 mo/s · hard ≥25" },
    Band { name: "pack bytes per cell", sweet: (0.0, 21.0), hard: (0.0, 24.0), target: "sweet ≤21 · hard ≤24 (8×u16 + 4×u8 = 20 B/cell + header)" },
    Band { name: "median tick payload", sweet: (0.0, 4096.0), hard: (0.0, 16384.0), target: "sweet ≤4 KB · hard ≤16 KB (E4: ship what changed)" },
];
