//! World orchestration — port of world.py: generation pipeline + simulation.

use std::collections::{BTreeMap, HashSet};

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use smallvec::smallvec;



use crate::agriculture;
use crate::biomes as biomes_mod;
use crate::chronicle::{self, ChronicleState};
use crate::climate;
use crate::culture::{self};
use crate::drought::{
    DroughtEvent, Droughts, CAL_STRIDE, CAL_YEARS, DRY_HOLD, FORMS, MEM, MEMO_YEARS, MIN_CORE,
    MIN_NODES, NODE_KM2, NORM,
    STRIDE,
};
use crate::economy::{self, Market};
use crate::entity::EntityKind;
use crate::entity::Registry;
use crate::erosion;
use crate::famine::DROUGHT_Z;
use crate::geo;
use crate::hydrology;
use crate::ids::{EntityId, PeopleId, RealmId, SettlementId};
use crate::naming::{self, Feature};
use crate::noisegen::Perlin3;
use crate::patina::{self, Ruin};
use crate::politics::{self, Politics};
use crate::resources::{self, Deposit, Good};
use crate::settlements::{self, Settlement};
use crate::snapshot::SentCache;
use crate::state::{Chronicle, Economy, Fields, Peoples};
pub use crate::event::{headline_worthy, Event, EventIds, EventKind};
pub use crate::state::CellFlags;
use crate::systems::{EventSink, SimCtx, SYSTEMS};
use crate::society::{self};
use crate::telling;
use crate::trade::{self, Route};
use crate::util::{now_ms, round2};

#[inline]
fn within_fell_radius(distance: f64, radius: f64) -> bool {
    radius > 0.0 && distance <= radius
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



/// M72 — one row per famine, written by the harvest verdict as it strikes.
/// Diagnostics-only observation; never hashed, never packed.
#[derive(Clone, Copy, Debug)]
pub struct FamineRow {
    /// absolute month of the verdict
    pub m: i64,
    pub x: i64,
    pub y: i64,
    /// souls the pass weighed before it took its toll
    pub pop: i64,
    /// the standardized rain anomaly the pass read at this cell
    pub z: f64,
    /// the shortfall it derived from that anomaly (0 at SPI −1, 1 at −2)
    pub shortfall: f64,
    /// the granary factor its people's craft earned (1.0 or 0.75)
    pub granary: f64,
    /// the toll: struck = dead + walked
    pub hit: i64,
    pub dead: i64,
    /// M92 — true when the verdict was the monsoon's, not the SPI's
    pub monsoon: bool,
    /// M92 — the monsoon-strength index the pass read (1.0 = a normal
    /// year); 0.0 on SPI rows, which never consult it
    pub msi: f64,
}

/// M90 — one row per yearly fields pass whose forcing moved: what the
/// margin did that year, measured at the mechanism. Diagnostics-only
/// observation; never hashed, never packed.
#[derive(Clone, Copy, Debug)]
pub struct FieldsRow {
    /// calendar year of the pass
    pub year: i64,
    /// the composed forcing the pass applied, °C
    pub f: f64,
    /// ledger cells flipped to farmable this pass
    pub opened: usize,
    /// ledger cells flipped back to wildland this pass
    pub shut: usize,
    /// dawn-wildland ledger cells farming after the pass
    pub open_now: usize,
    /// all ledger cells farming after the pass
    pub farm_now: usize,
}

/// M91 — one row per yearly ice pass that moved the edge: the year,
/// the forcing read, the flips taken, and the law's whole extent
/// after — the atlas's extent snapshots. Diagnostics-only
/// observation; never hashed, never packed.
#[derive(Clone, Copy, Debug)]
pub struct IceRow {
    /// calendar year of the pass
    pub year: i64,
    /// the composed forcing the pass applied, °C
    pub f: f64,
    /// glacier extent after the pass, cells (stable core + margin)
    pub extent: u64,
    /// margin cells the ice took this pass
    pub advanced: usize,
    /// margin cells the ice gave back this pass
    pub retreated: usize,
}

/// M90 — a town claims margin flips within this many cells of itself:
/// twice the work radius of a grown town, the day's walk to an upland
/// field.
pub const FIELDS_TOWN_REACH: f64 = 6.0;
/// M90 — the fewest one-way flips a town's margin must gather in one
/// pass before the chronicle speaks of that town.
pub const FIELDS_EVENT_MIN: usize = 3;
/// M90 — the fewest one-way flips a pass must gather for the world row.
pub const FIELDS_WORLD_MIN: usize = 8;


/// M79 diagnostics: one direct-harbour landfall considered for permanent
/// felling, with the local empirical evidence the exceptionality rule read.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
pub struct StormFellProbe {
    pub month: i64,
    pub settlement: SettlementId,
    pub bite: f64,
    pub damage: f64,
    pub age: i64,
    pub local: usize,
    pub exceed: usize,
    pub eligible: bool,
    pub felled: bool,
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
    /// M16/ADR-0024 — the plate-history sketch: frozen prehistory, consumed
    /// by generation, hashed, never advanced in tick time.
    pub plates: crate::plates::Plates,
    /// M22 — fault seams derived from the sketch, plus the renewal clocks
    /// and the quake log. Own RNG stream: histories replay unchanged.
    pub seismic: crate::seismic::Seismic,
    /// M23 — live volcanism: cones read off the volcanic province, their
    /// reload clocks, the eruption record and the ash ledger. Own stream.
    pub volcanism: crate::seismic::Volcanism,
    /// M25 — sea-level history: the glacial-cycle freeze point, eustatic
    /// stand and isostatic row profile. Frozen prehistory (ADR-0024
    /// discipline): consumed at genesis, hashed, never ticked.
    pub sealevel: crate::sealevel::SeaLevel,
    /// M28 — the LGM ice-sheet footprint: per-cell peak thickness and
    /// the ELA row profile. Frozen prehistory (ADR-0024): computed at
    /// the dawn from the final height field, hashed, never ticked.
    pub ice: crate::ice::Ice,
    /// M33 — the frozen-ground ledger: permafrost extent class and
    /// patterned-ground micro-texture. Pure derived state off the final
    /// temperature and height fields (like `landform`): recomputed at
    /// the dawn, folded into `hash_state`, never ticked.
    pub permafrost: crate::permafrost::Permafrost,
    /// M40 — wind-driven gyres: basin-scale surface currents solved
    /// from the zonal wind bands over the final coastline geometry.
    /// Pure derived state (like `landform`/`permafrost`): recomputed
    /// at the dawn, folded into `hash_state`, never ticked.
    pub currents: crate::currents::Currents,
    /// M43 — the tides: per-cell range and basin-enclosure class
    /// solved from the final coastline geometry (Green's-law shoaling,
    /// quarter-wave funnel resonance, landlocked seas near-still).
    /// Pure derived state (like `currents`): recomputed at the dawn,
    /// folded into `hash_state`, never ticked.
    pub tides: crate::tides::Tides,
    /// M44 — longshore drift: CoastForm per cell (spit / barrier /
    /// lagoon) and the deposit ledger, written at the glacial stage's
    /// close — the deposits ARE the height field, so climate, rivers,
    /// tides and settlements all read the grown coast. Widened at the
    /// dawn, folded into `hash_state`, never ticked.
    pub coastform: crate::coast::Coast,
    /// M59 — the sediment budget: the fluvial passes' closed books
    /// (detached = settled + delta fill + abyssal) and their footprint
    /// on the map — deposition depth, delta land, the mouth ledger.
    /// Written at the erosion stage, widened at the dawn, folded into
    /// `hash_state` and the deep-earth identity line, never ticked.
    pub sediment: crate::erosion::Sediment,
    /// M45 — harbor shelter per cell: enclosure + fetch + the drift's
    /// forms, the coast read as sailors read it (settlements::
    /// shelter_score). Computed pre-widen at the dawn so founding and
    /// colony siting price the anchorage; exactly 0.0 off the coastal
    /// band, and since M59 shoaled where fan silt lies in the anchorage
    /// window. Widened at the dawn, folded into `hash_state`, never ticked.
    pub shelter: ndarray::Array2<f32>,
    pub features: Vec<Feature>,
    pub routes: Vec<Route>,
    pub world_name: String,
    /// M9.1 — where towns died: named remains on the map.
    pub ruins: Vec<Ruin>,
    /// M24 — completed rebuild arcs: months from disaster damage back to
    /// the pre-disaster population. Diagnostics ledger; never on the wire.
    pub rebuild_log: Vec<u32>,
    /// M14.8 — timber scars on the biome map: (deposit, y, x, original
    /// biome code), remembered so recovery can restore the forest.
    pub scars: Vec<(usize, i64, i64, u8)>,
    /// M15.6 — cumulative flow meters per deposit; see `resources::Flows`.
    pub flows: resources::Flows,
    /// Years each route has gone without realized flow (M9.4).
    route_idle: Vec<u16>,
    /// M89 — the forcing the route ice calendars were last frozen under,
    /// and the route count at that freeze: the Margins system refreezes
    /// only when either moved. Bookkeeping, never hashed or packed —
    /// the calendars themselves are the state.
    pub(crate) margins_dt: f64,
    pub(crate) margins_web: u64,
    /// M6.4 — narrative heat: decaying sum of the month's weighted events;
    /// quiet years reach for omens, loud years let the wars speak.
    pub(crate) heat: f64,

    pub(crate) rng: Pcg64Mcg,
    pub(crate) taken: HashSet<String>,
    /// Statecraft: wars, opinion, dread, solidarity, vassals (M4).
    pub politics: Politics,
    /// E4.5 — which wire sections must reship on the next tick.
    pub(crate) dirty: Dirty,
    /// E4.2/E4.3 — hashes of the last-shipped JSON per wire surface.
    pub(crate) sent: SentCache,
    /// E5.8 — reused serialization scratch for the tick payload; keeps its
    /// high-water capacity so `tick_json` stops paying growth reallocations.
    pub(crate) wire_buf: Vec<u8>,
    /// M71 — the interannual variability field over (space × year): its own
    /// stream, so the sky's noise and the famine die never share a draw.
    pub(crate) variability: Perlin3,
    /// M74 — the slow lean of the seas, drawn once from the seed.
    pub(crate) oscillation: crate::oscillation::Oscillation,
    /// M83 — the century's slow temperature drift: a bounded, mean-
    /// reverting walk around the baseline `tmean`, drawn once from the
    /// seed. Derived state (ADR-0003): the curve is re-run on demand,
    /// never stored, hashed or packed.
    pub(crate) drift: crate::climate::Drift,
    /// M83 — the years' drift values, cached so the sim pays the O(year)
    /// walk once per year instead of once per inhabited site. A map, not
    /// a slot, since M84: the drought window and the storm ledger read
    /// several adjacent years in one tick, and a one-slot memo would
    /// thrash the walk. One f64 per simulated year — bounded and tiny.
    pub(crate) year_drift_memo: std::sync::Mutex<BTreeMap<i64, f64>>,
    /// M86 — the cold-age schedule: multidecadal winters drawn once from
    /// the seed. Derived law like the drift (ADR-0003): never stored,
    /// hashed or packed; `year_forcing` composes it over the walk.
    pub(crate) ages: crate::ages::Ages,
    /// M79 — the storm field, solved lazily the first time a month asks
    /// the coast what hit it. Frozen once built (pure in the finished
    /// climate), so the landfall ledger of any year can be re-derived.
    pub(crate) storm_clim: Option<Box<crate::storms::StormClimatology>>,
    /// The calendar year `storm_now` holds, or `i64::MIN` before any.
    pub(crate) storm_year: i64,
    /// This year's landfalls and last year's — a storm born in December
    /// comes ashore in January, so the previous ledger stays alive.
    pub(crate) storm_now: Vec<crate::storms::Landfall>,
    pub(crate) storm_prev: Vec<crate::storms::Landfall>,
    /// M79 — the coast's memory: `(month, settlement, wound)` for every
    /// harbour a storm actually broke. Diagnostics ledger and the gate's
    /// evidence; bounded, never on the wire.
    pub storm_marks: Vec<(i64, SettlementId, f64)>,
    /// M80 — the drought ledger: the accumulated-shortfall lattice and
    /// every named drought the world has lived through. The index itself
    /// is derived (a pure read of the sky); the *events* — names, spans,
    /// ground — are state, and ride the replay identity line.
    pub droughts: crate::drought::Droughts,
    /// M81 — the flood ledger: every spate that overtopped a town's
    /// levees, and the silt those spates left for the season after. The
    /// year's water is derived (a pure read of the sky); the *floods* —
    /// who drowned, how deep, what the ground was given — are state, and
    /// ride the replay identity line.
    pub floods: crate::flood::Floods,
    /// M93 — the lake ledger: every terminal basin's level, struck once
    /// a year from inflow minus evaporation under the year's sky, with
    /// the exact records it has reached and the strandlines it dated.
    /// The geometry is the dawn's; the *level* remembers, so the ledger
    /// rides the replay identity line.
    pub lakes: hydrology::Lakes,
    /// M90 — the margin ledger: every cell whose farmable verdict is a
    /// single solved threshold on the composed forcing. Derived state —
    /// a pure function of the dawn grids, regenerated bit-identically —
    /// never hashed, never packed; the flips it drives land in
    /// `fields.crops`, which is both.
    pub fields_ledger: crate::agriculture::MarginLedger,
    /// The forcing `fields.crops` currently stands at: bit-exact the
    /// last value `fields_pass` applied. 0.0 is the dawn.
    pub fields_sky: f64,
    /// M90 — one row per yearly fields pass that moved anything: the
    /// forcing read, the flips taken, the ledger's standing after.
    /// Diagnostics observation, never hashed, never packed.
    pub fields_log: Vec<FieldsRow>,
    /// M91 — the ice-margin ledger: every cell whose glacier verdict
    /// is a single solved threshold on the composed forcing. Derived
    /// state — a pure function of the dawn grids, regenerated
    /// bit-identically — never hashed, never packed; the flips it
    /// drives land in `fields.flags`' GLACIER bit, which is both.
    pub ice_ledger: crate::ice::IceLedger,
    /// The forcing the GLACIER flags currently stand at: bit-exact
    /// the last value `ice_pass` applied. 0.0 is the dawn.
    pub ice_sky: f64,
    /// M91 — one row per yearly ice pass that moved the edge: extent
    /// snapshots for the atlas. Never hashed, never packed.
    pub ice_log: Vec<IceRow>,
    /// M79 — local strike history at event strength, before an existing
    /// harbour wound is added. Permanent felling reads this ledger's
    /// empirical severe-hit return interval; bounded and deterministic.
    pub storm_bites: Vec<(i64, SettlementId, f64)>,
    /// Native diagnostics-only decision ledger for the permanent-felling
    /// rule. The shipped WASM runs the identical eligibility decision but
    /// does not retain this write-only instrumentation.
    #[cfg(not(target_arch = "wasm32"))]
    pub storm_fell_probe: Vec<StormFellProbe>,
    /// M71 — the current year's weather, memoized: `(year, dt °C, dp share)`.
    /// Derived state (a pure function of seed × year), never hashed, never
    /// packed; recomputed the first time a year is asked for.
    /// M72 adds a third lane, `dq`: the same rain anomaly integrated over
    /// a catchment-sized neighbourhood — what the rivers carry that year.
    pub(crate) year_weather:
        std::sync::Mutex<Option<(i64, Array2<f64>, Array2<f64>, Array2<f64>)>>,
    /// M72 tick-path memo: the exact full-grid law evaluated only at inhabited
    /// cells. BTreeMap keeps lookup/order deterministic; the map turns over
    /// with the year and is derived state, never hashed or packed.
    pub(crate) year_site_weather:
        std::sync::Mutex<Option<(i64, BTreeMap<(usize, usize), (f64, f64, Option<f64>)>)>>,
    /// Last year grain was shock-priced by famine, to spike at most once a year.
    pub(crate) grain_shock_year: i64,
    /// M72 — the harvest verdict's own ledger: one row per famine, written
    /// by the pass as it strikes. Diagnostics-only observation of state the
    /// pass already computed (souls at risk, the standardized shortfall it
    /// read, the toll it took) — it changes no behaviour, is never hashed,
    /// never packed. Without it a severity gate can only reconstruct the
    /// dose from prose; with it the dose is measured at the mechanism.
    pub famine_ledger: Vec<FamineRow>,

    /// pub since M55: diagnostics weigh dry ground against watered ground.
    pub site_score: Array2<f64>,
    food_grid: Array2<f64>,
    near_fresh: Array2<bool>,
    /// M55 — arid ground with no reachable surface water; pub so
    /// diagnostics gate founding against the same mask the world uses.
    pub arid_dry: Array2<bool>,
    /// M55 — the site score that dry ground would carry once a well
    /// reaches its table.
    pub dry_site_score: Array2<f64>,
    /// M55 — diagnostics-only counterfactual: when set, every people is
    /// treated as if its wells reached this deep. `None` in every shipped
    /// path (the wire and the wasm never set it), so the simulation stays
    /// a pure function of the seed; the harness uses it to run the same
    /// world with the dry-frontier veto lifted and see what changes.
    pub dry_reach_override: Option<f64>,
    /// E10.2 — memo for M56's caravan provisioning field. The Dijkstra
    /// over the trade grid is a pure function of (victualling markets,
    /// purse); both change rarely, while colonisation asks for the field
    /// every month. Keyed on those inputs, so a hit returns exactly the
    /// grid a recompute would have produced — derived state only, never
    /// hashed, never packed. A Mutex, not a RefCell: the assay holds a
    /// generated World in a `static OnceLock` (M15), which needs `Sync` —
    /// a RefCell here broke that lane's *build* silently. Uncontended
    /// lock, same bits either way.
    pub(crate) caravan_memo: std::sync::Mutex<Option<(u64, Array2<f32>)>>,

    /// The coastal band (land within 2 cells of sea) — the ground the
    /// shelter field scores; pub so diagnostics read the same mask.
    pub coast: Array2<bool>,
    /// pub since M81: the harness re-holds the colonisation gates.
    pub max_settlements: usize,
    /// pub since M46: diagnose re-walks route legs against the current.
    pub trade: trade::TradeGrid,
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
    plates: Option<crate::plates::Plates>,
    sealevel: Option<crate::sealevel::SeaLevel>,
    ice: Option<crate::ice::Ice>,
    coastform: Option<crate::coast::Coast>,
    sediment: Option<crate::erosion::Sediment>,
    height64: Option<Array2<f64>>,
    water: Option<Array2<bool>>,
    tmean64: Option<Array2<f64>>,
    tamp64: Option<Array2<f64>>,
    precip64: Option<Array2<f64>>,
    pamp64: Option<Array2<f64>>,
    /// M38 — the climate stage's continentality, kept alive one stage
    /// longer so the biome pass can read the permafrost table depth.
    cont64: Option<Array2<f64>>,
    hydro: Option<hydrology::Hydrology>,
    /// M93 — the terminal basins, solved beside the hydrology; named
    /// and handed to the world at dawn.
    basins: Option<Vec<hydrology::Basin>>,
    biome_map: Option<Array2<u8>>,
    crops: Option<Array2<u8>>,
    /// M90 — the margin ledger, solved beside the crop classification.
    margins: Option<agriculture::MarginLedger>,
    /// M91 — the ice-margin ledger, solved beside the modern mask.
    ice_margin: Option<crate::ice::IceLedger>,
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
    rock: Option<Array2<u8>>,
    soil: Option<Array2<u8>>,
    aquifer: Option<Array2<f32>>,
    world: Option<World>,
}

impl GenBuilder {
    /// The ladder, in running order. Names double as progress labels.
    pub const STAGES: [&'static str; 10] = [
        "terrain",
        "erosion",
        "glacial",
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
            plates: None,
            sealevel: None,
            ice: None,
            coastform: None,
            sediment: None,
            height64: None,
            water: None,
            tmean64: None,
            tamp64: None,
            precip64: None,
            pamp64: None,
            cont64: None,
            hydro: None,
            basins: None,
            biome_map: None,
            crops: None,
            margins: None,
            ice_margin: None,
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
            rock: None,
            soil: None,
            aquifer: None,
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
            2 => self.stage_glacial(),
            3 => self.stage_climate(),
            4 => self.stage_hydrology(),
            5 => self.stage_biomes(),
            6 => self.stage_fertility(),
            7 => self.stage_naming(),
            8 => self.stage_resources(),
            9 => self.stage_dawn(),
            _ => panic!("generation already complete"),
        }
        self.stage += 1;
        name
    }

    /// Hand over the finished world. Panics unless `done()`.
    pub fn finish(&mut self) -> World {
        self.world.take().expect("generation not complete")
    }

    #[inline(never)]
    fn stage_terrain(&mut self) {
        // M16/ADR-0024 — the deep past first: the plate-history sketch is
        // dealt before the land, and the land is drawn over it.
        let tp = now_ms();
        let plates = crate::plates::generate(self.seed, self.size);
        self.timings.push(("plates", now_ms() - tp));
        let t = now_ms();
        let mut h = geo::heightmap(self.seed, self.size, &plates);
        // M25 — the waterline remembers the ice ages: eustatic stand and
        // post-glacial isostasy land before erosion fixes the coast.
        let sl = crate::sealevel::generate(self.seed, self.size);
        sl.apply(&mut h);
        self.height64 = Some(h);
        self.sealevel = Some(sl);
        self.plates = Some(plates);
        self.timings.push(("terrain", now_ms() - t));
    }

    #[inline(never)]
    fn stage_erosion(&mut self) {
        let te = now_ms();
        let height = self.height64.as_mut().unwrap();
        // M59 — the carve keeps books now: detachment, floodplain and
        // lake settling, delta fans at the mouths, abyssal export.
        self.sediment = Some(erosion::erode(height));
        let water = height.mapv(|h| h < 0.0);
        self.water = Some(water);
        self.timings.push(("erosion", now_ms() - te));
    }

    /// M28/M29 — the ice ages: cut the LGM footprint from the eroded
    /// land, then carve the relief the sheets left. Runs before climate
    /// so every downstream layer reads the glaciated world.
    #[inline(never)]
    fn stage_glacial(&mut self) {
        let t = now_ms();
        let h = self.height64.as_mut().unwrap();
        let mut ice = crate::ice::compute(self.seed, h);
        crate::ice::carve(h, &mut ice);
        // M30 — the legacy: till, moraines, drumlins, eskers (land-only raises)
        crate::ice::deposit(self.seed, h, &mut ice);
        // M31 — proglacial lakes behind the fresh moraines; their
        // outburst spillways cut the relief before the soil settles
        crate::ice::proglacial(h, &mut ice);
        // M32 — outwash plains: the melt planes braided aprons below
        // the margin before the loess blows off them
        crate::ice::outwash(h, &mut ice);
        // M30 — the loess mantle: glacial silt blown equatorward off the
        // aprons, the warm end of the depositional footprint (soil only)
        crate::ice::loess_mantle(h, &mut ice);
        let tg = now_ms();
        self.timings.push(("glacial", tg - t));
        // M44 — longshore drift: the last hand on the land before the
        // climate reads it. Waves walk sand along the windward shores;
        // spits hook off the headlands, offshore bars daylight into
        // barriers, and lagoons close behind them. M31's outburst
        // channels stay breached: an outlet that still drains flushes
        // the sand off its own mouth faster than the waves feed it.
        let mut keep_open = ndarray::Array2::<bool>::from_elem(h.dim(), false);
        for chain in &ice.spillways {
            for &(y, x) in chain {
                keep_open[[y as usize, x as usize]] = true;
            }
        }
        self.coastform = Some(crate::coast::drift(h, &keep_open));
        self.timings.push(("coast", now_ms() - tg));
        // the carve moves the waterline: fjords drown, floors drop —
        // and the drift's new ground stands above it
        self.water = Some(h.mapv(|v| v < 0.0));
        self.ice = Some(ice);
    }

    #[inline(never)]
    fn stage_climate(&mut self) {
        let t1 = now_ms();
        let height = self.height64.as_ref().unwrap();
        let water = self.water.as_ref().unwrap();
        let lat = climate::latitude_deg(self.size);
        // E5.11 — one continentality (EDT) shared by amplitude + monsoon.
        let cont = climate::continentality(water);
        let tmean = climate::temperature_mean(height, &lat);
        // M41 — heat transport: the currents bend the coasts they
        // touch. Solve the gyres on the pre-widen coastline (the very
        // law the dawn re-runs post-widen for the hashed ledger), let
        // the meridional flow remember its origin latitude, and fold
        // the anomaly into the annual mean before amplitude, rain and
        // everything downstream read it — Gulf-Stream warm rims,
        // Humboldt cold rims, ice calendars that obey the currents.
        let cur = crate::currents::Currents::compute(water);
        let heat = climate::current_bias(water, &cur.v);
        let tmean = tmean + &heat;
        let tamp = climate::temperature_amplitude(&lat, &cont);
        // M42 — the rain march reads the same anomaly: cold rims cap the
        // marine layer downwind (coastal deserts), warm rims feed it.
        let (mut precip, pamp) = climate::precipitation(height, water, &tmean, &lat, &cont, &heat);
        if self.precip_scale != 1.0 {
            let s = self.precip_scale;
            precip.mapv_inplace(|p| p * s);
        }
        self.tmean64 = Some(tmean);
        self.tamp64 = Some(tamp);
        self.precip64 = Some(precip);
        self.pamp64 = Some(pamp);
        // M38 — the biome pass reads the frozen ground off the same
        // continentality; dropped right after (stage_fertility).
        self.cont64 = Some(cont);
        // M34/M35 — the modern glacier balance is climate: computed
        // here over the pre-widen f64 grids so hydrology can feed the
        // melt to the rivers below; widened later with the ice ledger.
        let modern = crate::ice::modern_glaciers(
            self.water.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
            self.tamp64.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            self.pamp64.as_ref().unwrap(),
        );
        self.ice.as_mut().expect("glacial stage ran").modern = modern;
        // M91 — the ice-margin ledger: solved here, over the exact f64
        // grids the modern mask was just stamped from, so the ledger's
        // dawn verdict IS the mask. Timed as its own stage row so the
        // climate budget stays the climate budget.
        let tm = now_ms();
        let ledger = crate::ice::ice_ledger(
            self.water.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
            self.tamp64.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            self.pamp64.as_ref().unwrap(),
            &self.ice.as_ref().unwrap().modern,
        );
        let icemargin_ms = now_ms() - tm;
        self.ice_margin = Some(ledger);
        self.timings.push(("climate", now_ms() - t1 - icemargin_ms));
        self.timings.push(("icemargin", icemargin_ms));
    }

    #[inline(never)]
    fn stage_hydrology(&mut self) {
        let t2 = now_ms();
        let hydro = hydrology::hydrology(
            self.height64.as_ref().unwrap(),
            self.water.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            self.pamp64.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
            self.tamp64.as_ref().unwrap(),
            &self.ice.as_ref().unwrap().outwash,
            &self.ice.as_ref().unwrap().modern,
        );
        // M35 — the meltwater ledger rides the ice struct to the
        // finished world (f32 at rest, E3.2): diagnostics and
        // inspectors read melt/discharge for the glacier-fed regime.
        {
            let ice = self.ice.as_mut().unwrap();
            ice.melt = hydro.melt.mapv(|x| x as f32);
            ice.melt_amp = hydro.melt_amp.mapv(|x| x as f32);
        }
        // M93 — the terminal basins keep their geometry: lake cells,
        // whole D8 catchments and the balance constants their climate
        // sets. Solved here, while `dirs` and the filled surface still
        // exist; the world only ever sees the ledger.
        let basins = hydrology::endorheic_basins(
            &hydro,
            self.height64.as_ref().unwrap(),
            self.water.as_ref().unwrap(),
            self.tmean64.as_ref().unwrap(),
        );
        self.basins = Some(basins);
        self.hydro = Some(hydro);
        self.timings.push(("hydrology", now_ms() - t2));
    }

    #[inline(never)]
    fn stage_biomes(&mut self) {
        let t3 = now_ms();
        // M38 — the biome pass reads the frozen ground: the same
        // extent law the canonical (post-widen, hashed) M33 ledger
        // applies, evaluated on the pre-widen climate so the tundra
        // can split wet/dry on the permafrost table depth.
        let tmean = self.tmean64.as_ref().unwrap();
        let cont = self.cont64.as_ref().unwrap();
        let pf = Array2::from_shape_fn(tmean.dim(), |(y, x)| {
            crate::permafrost::extent_class(tmean[[y, x]], cont[[y, x]])
        });
        let biome_map = biomes_mod::classify(
            self.height64.as_ref().unwrap(),
            tmean,
            self.tamp64.as_ref().unwrap(),
            self.precip64.as_ref().unwrap(),
            &self.hydro.as_ref().unwrap().lakes,
            &pf,
        );
        self.biome_map = Some(biome_map);
        self.timings.push(("biomes", now_ms() - t3));
    }

    #[inline(never)]
    fn stage_fertility(&mut self) {
        let t4 = now_ms();
        // M51 — the basement is read here now, before the soil: the
        // orders need their parent material (M18). It is the same pure
        // classification stage_resources used to run, on the same f32
        // relief, so the ore pass downstream reads an identical grid.
        let height32 = self.height64.as_ref().unwrap().mapv(|x| x as f32);
        let rock = crate::rock::classify(
            self.seed,
            self.size,
            self.plates.as_ref().unwrap(),
            &height32,
        );
        let margin_ms;
        {
            let height = self.height64.as_ref().unwrap();
            let tmean = self.tmean64.as_ref().unwrap();
            let precip = self.precip64.as_ref().unwrap();
            let hydro = self.hydro.as_ref().unwrap();
            let ice = self.ice.as_ref().unwrap();
            // Jenny's factors, in one pass: parent material, climate,
            // organisms (the biome), relief, and the young-surface time
            // proxy carried by ash and glacial dust.
            let soil = agriculture::soil_genesis(
                height,
                tmean,
                precip,
                self.biome_map.as_ref().unwrap(),
                &rock,
                &hydro.rivers,
                &hydro.lakes,
                &hydro.discharge,
                &ice.till,
                &ice.loess,
            );
            let fert = agriculture::fertility(
                height,
                tmean,
                precip,
                &hydro.rivers,
                &hydro.lakes,
                &hydro.discharge,
                &ice.till,
                &ice.loess,
                &ice.outwash,
                &soil,
            );
            let crops =
                agriculture::crop_packages(height, tmean, precip, &hydro.rivers, &hydro.lakes, &soil);
            // M90 — the margin ledger, solved right here against the
            // same grids the classification just read: every cell whose
            // farmable verdict is a single threshold on the forcing.
            // Timed as its own stage row so the fertility budget stays
            // the fertility budget.
            let tm = now_ms();
            let margins = agriculture::margin_ledger(
                height, tmean, precip, &hydro.rivers, &hydro.lakes, &soil, &crops,
            );
            margin_ms = now_ms() - tm;
            self.margins = Some(margins);
            self.fertility = Some(fert.mapv(|x| x as f32));
            self.crops = Some(crops);
            // M54 — the water beneath: a steady-state Darcy head over
            // the basement's conductivity, drained by every river,
            // lake and shore the hydrology pass already carved.
            let aquifer = crate::hydrology::water_table(
                height,
                self.water.as_ref().unwrap(),
                &hydro.rivers,
                &hydro.lakes,
                &hydro.discharge,
                precip,
                &rock,
            );
            self.aquifer = Some(aquifer);
            self.soil = Some(soil);
        }
        self.rock = Some(rock);
        self.height = Some(height32);
        self.timings.push(("fertility", now_ms() - t4 - margin_ms));
        self.timings.push(("margins", margin_ms));


        // E3.2 — the physical stages are done; the world's float grids
        // drop to their resting f32 width here, and every human stage
        // below (naming, resources, settlements, trade, economy) reads
        // the same f32 the ticks will read.
        self.height64 = None; // the f32 relief was taken above (M51)
        self.tmean = Some(self.tmean64.take().unwrap().mapv(|x| x as f32));
        self.tamp = Some(self.tamp64.take().unwrap().mapv(|x| x as f32));
        self.precip = Some(self.precip64.take().unwrap().mapv(|x| x as f32));
        self.pamp = Some(self.pamp64.take().unwrap().mapv(|x| x as f32));
        let discharge = self.hydro.as_ref().unwrap().discharge.mapv(|x| x as f32);
        let flow_amp = self.hydro.as_ref().unwrap().flow_amp.mapv(|x| x as f32);
        self.discharge = Some(discharge);
        self.flow_amp = Some(flow_amp);
        self.water = None;
        self.cont64 = None; // M38 — the biome pass was its last reader
    }

    #[inline(never)]
    fn stage_naming(&mut self) {
        let t5 = now_ms();
        let (features, world_name) = naming::name_features(
            self.height.as_ref().unwrap(),
            self.sealevel.as_ref().unwrap(),
            self.ice.as_ref().unwrap(),
            &self.sediment.as_ref().expect("erosion stage ran").delta,
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

    #[inline(never)]
    fn stage_resources(&mut self) {
        let t6 = now_ms();
        // M18 — the basement: shield, basin, fold belt, volcanic, read
        // once off the sketch and the relief. Classified in the
        // fertility stage since M51 (the soil orders need their parent
        // material) and frozen from there; M19 re-seats deposits on it,
        // so geology still decides where ore belongs.
        let rock = self.rock.take().expect("rock classified in stage_fertility");
        let deposits = resources::place_resources(
            self.biome_map.as_ref().unwrap(),
            self.height.as_ref().unwrap(),
            &self.hydro.as_ref().unwrap().rivers,
            &self.hydro.as_ref().unwrap().lakes,
            &rock,
            self.seed,
        );
        self.rock = Some(rock);
        self.deposits = Some(deposits);
        self.timings.push(("resources", now_ms() - t6));
    }

    #[inline(never)]
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
        let plates = self.plates.take().unwrap();
        // M18 — the basement, classified back in stage_resources (M19
        // reads it for ore placement). It rides into the fields here.
        let rock = self.rock.take().unwrap();
        // M51 — the soil orders, classified in the fertility stage.
        let soil = self.soil.take().unwrap();
        // M54 — the water table, solved in the same stage off the rock.
        let aquifer = self.aquifer.take().unwrap();

        let t7 = now_ms();
        let mut taken: HashSet<String> = HashSet::new();
        let mut rng9000 = crate::util::rng(seed + 9000);
        // M45 — the sailor's reading of the shore: harbor shelter solved
        // on the same pre-widen grid the drift just shaped, before any
        // site is chosen. Founding, colony siting and harbour dues all
        // price the anchorage from this one field.
        let mut shelter = settlements::shelter_score(
            &height,
            &self.coastform.as_ref().expect("glacial stage ran").form_gen,
        );
        // M59 — the harbor pays for the river's load: where fan silt
        // lies in the 5×5 anchorage window shelter_score itself reads,
        // the anchorage shoals — divide by 1 + SILT_SHOAL·depth of the
        // deepest silt on still-standing water. Cells with no silt in
        // reach keep their score bit-for-bit (founding, colony siting
        // and harbour dues all price the shoaled reading).
        {
            let sed = self.sediment.as_ref().expect("erosion stage ran");
            let (gh, gw) = shelter.dim();
            for y in 0..gh {
                for x in 0..gw {
                    if shelter[[y, x]] <= 0.0 {
                        continue;
                    }
                    let mut silt = 0.0f32;
                    for dy in -2..=2isize {
                        for dx in -2..=2isize {
                            let ny = y as isize + dy;
                            let nx = x as isize + dx;
                            if ny < 0 || nx < 0 || ny >= gh as isize || nx >= gw as isize {
                                continue;
                            }
                            let (ny, nx) = (ny as usize, nx as usize);
                            if height[[ny, nx]] < 0.0 {
                                silt = silt.max(sed.depth_gen[[ny, nx]]);
                            }
                        }
                    }
                    if silt > 0.0 {
                        shelter[[y, x]] /= 1.0 + crate::erosion::SILT_SHOAL * silt;
                    }
                }
            }
        }
        // M55 — springs and oases: where the solved table daylights at a
        // break in slope, and where arid ground stands over water within
        // root reach. Both derive from the frozen aquifer grid, so they
        // are as deterministic as it is, and neither rides the wire.
        let sea_mask = height.mapv(|h| h < 0.0);
        let dry_water = crate::hydrology::springs_and_oases(
            &height,
            &sea_mask,
            &hydro.rivers,
            &hydro.lakes,
            &aquifer,
            &biome_map,
            &precip,
        );
        let founded = settlements::found_settlements(
            &height,
            &biome_map,
            &tmean,
            &hydro.rivers,
            &hydro.lakes,
            &discharge,
            &deposits,
            &fertility,
            &shelter,
            &dry_water.springs,
            &dry_water.oases,
            &precip,
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
        trade::assign_goods(&mut setts, &deposits, &fertility, &rock);

        // E1.7 — fold the five hydrology masks into one CellFlags byte grid;
        // this is the exact byte the pack ships, so pack() is now a memcpy.
        let flags = {
            let (fr, fc) = hydro.rivers.dim();
            let mut f = Array2::<u8>::zeros((fr, fc));
            for y in 0..fr {
                for x in 0..fc {
                    let mut c = CellFlags::empty();
                    c.set(CellFlags::RIVER, hydro.rivers[[y, x]]);
                    c.set(CellFlags::LAKE, hydro.lakes[[y, x]]);
                    c.set(CellFlags::SALT, hydro.salt[[y, x]]);
                    c.set(CellFlags::SEASONAL, hydro.seasonal[[y, x]]);
                    c.set(CellFlags::BRAIDED, hydro.braided[[y, x]]);
                    f[[y, x]] = c.bits();
                }
            }
            f
        };

        let trade_grid = trade::TradeGrid::build(
            &height,
            &flags,
            &biome_map,
            &discharge,
            &tmean,
            &tamp,
            &shelter,
            &pamp,
            (size / 128).max(1),
        );
        let routes = trade::build_routes(
            &trade_grid,
            &mut setts,
            &height,
            &flags,
            &discharge,
            &flow_amp,
            &shelter,
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
        // Register every composed display phrase ("The Caleth Delta",
        // "The Frost Bay") in the taken set — coin() only reserved the
        // bare words, so runtime renamers (patina wear, M9.2 layers)
        // checking `taken` could otherwise wear a name INTO an existing
        // phrase and mint a silent duplicate (M3 gate).
        for f in &features {
            taken.insert(f.name.clone());
            if !f.alt.is_empty() {
                taken.insert(f.alt.clone());
            }
        }
        let societies = society::init(&cultures);
        let mut market = Market::default();
        let people_style: Vec<usize> =
            cultures.iter().map(|p| culture::style_index(&p.style)).collect();
        economy::update_prices(&mut market, &setts, &people_style);
        // the first carve of the market areas (M5.2)
        let sidx0 = economy::sidx(&setts);
        let mut areas = economy::build_areas(&setts, &routes, None, &sidx0);
        economy::update_area_prices(&mut areas, &setts, &market, &people_style);
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
            .map(|s| registry.add(EntityKind::Settlement, &s.name, 0, Some(s.people), s.x, s.y))
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
            resources::Good::Pottery,
            resources::Good::Brick,
            resources::Good::Cloth,
            resources::Good::Leather,
            resources::Good::Wine,
        ]) {
            registry.add(EntityKind::Good, g.name(), 0, None, -1, -1);
        }

        // ADR-0018 — the dawn crowns: one realm per people, seated in its
        // largest town; every dawn town then flies its own people's banner.
        let realms = politics::init_realms(&cultures, &mut setts, &mut taken, &mut registry, seed);

        let mut chron = ChronicleState::default();
        chron.rulers =
            chronicle::init_rulers(&mut rng, &realms, &cultures, &mut taken, &mut registry);

        // M93 — the terminal lakes take their names: a basin that holds
        // a named lake feature answers to it; the rest are coined in the
        // tongue of the nearest people within reach (M3.1's rule), or in
        // the Old Tongue where no one lives near. Named from a stream of
        // their own so no other name on the map moves. Each unnamed
        // basin joins the cast so its strandlines link to a place.
        let lakes = {
            let mut rng93 = crate::util::rng(seed + 9300);
            let mut basins = self.basins.take().expect("hydrology stage ran");
            for b in basins.iter_mut() {
                let inside = |fx: i64, fy: i64| {
                    b.cells.iter().any(|&(cx, cy)| cx as i64 == fx && cy as i64 == fy)
                };
                if let Some(f) = features.iter().find(|f| f.t == "lake" && inside(f.x, f.y)) {
                    b.name = f.name.clone();
                    continue;
                }
                let mut style = "old".to_string();
                let mut best = f64::INFINITY;
                for s in &setts {
                    let d2 = ((s.x - b.x) as f64).powi(2) + ((s.y - b.y) as f64).powi(2);
                    if d2 < best {
                        best = d2;
                        style = cultures[s.people.idx()].style.clone();
                    }
                }
                if best.sqrt() > naming::TONGUE_REACH {
                    style = "old".to_string();
                }
                let c = naming::coin(&mut rng93, &style, &mut taken);
                let mut name = naming::styled_phrase(&mut rng93, &style, "lake", &c.word);
                if !taken.insert(name.clone()) {
                    name = format!("{} Pan", c.word);
                    taken.insert(name.clone());
                }
                b.name = name;
                registry.add(EntityKind::Feature, &b.name, 0, None, b.x, b.y);
            }
            hydrology::Lakes { basins, last_year: -1 }
        };

        let mut events: Vec<Event> =
            chronicle::founding_myths(&mut rng, &cultures, &features, &world_name);
        for (si, s) in setts.iter().enumerate() {
            let people = if !cultures.is_empty() {
                cultures[s.people.idx()].people.clone()
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
            fields: Fields {
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
                rock,
                soil,
                aquifer,
                // M47 — placeholder like territory: the upwelling shore is
                // solved at the dawn, off the final post-widen coastline.
                upwelling: Array2::from_elem((1, 1), 0.0f32),
                // M68 — placeholders: the drift and sediment ledgers own
                // their grids until the dawn hands them over post-widen.
                coastform: Array2::from_elem((1, 1), 0u8),
                silt: Array2::from_elem((1, 1), 0.0f32),
                // M60 — placeholder like upwelling: the vocabulary is
                // classified at the dawn, post-widen (widen never touches it).
                landform: Array2::from_elem((1, 1), 0u8),
                territory: Array2::from_elem((1, 1), -1),
                peoples_map: Array2::from_elem((1, 1), -1),
            },
            peoples: Peoples {
                coresidence: vec![vec![0.0; cultures.len()]; cultures.len()],
                settlements: setts,
                peoples: cultures,
                realms,
                societies,
                civs: Vec::new(),
            },
            economy: Economy {
                market,
                areas,
                merchants: Vec::new(),
                route_flow: Vec::new(),
            },
            chronicle: Chronicle {
                events,
                registry,
                artifacts: Vec::new(),
                state: chron,
            },
            flows: resources::Flows::for_deposits(deposits.len()),
            deposits,
            plates,
            sealevel: self.sealevel.take().expect("sealevel generated"),
            ice: self.ice.take().expect("glacial stage ran"),
            permafrost: crate::permafrost::Permafrost::empty(),
            currents: crate::currents::Currents::empty(),
            tides: crate::tides::Tides::empty(),
            coastform: self.coastform.take().expect("glacial stage ran"),
            sediment: self.sediment.take().expect("erosion stage ran"),
            seismic: crate::seismic::Seismic::empty(),
            volcanism: crate::seismic::Volcanism::empty(),
            features,
            routes,
            world_name,
            ruins: Vec::new(),
            rebuild_log: Vec::new(),
            scars: Vec::new(),
            route_idle: Vec::new(),
            margins_dt: 0.0,
            margins_web: 0,
            heat: 0.0,
            rng,
            taken,
            politics: Politics::init(n_cultures),
            dirty: Dirty::default(),
            sent: SentCache::default(),
            wire_buf: Vec::new(),
            variability: Perlin3::new(seed + 7717),
            oscillation: crate::oscillation::Oscillation::new(seed),
            drift: crate::climate::Drift::new(seed),
            year_drift_memo: std::sync::Mutex::new(BTreeMap::new()),
            ages: crate::ages::Ages::new(seed),
            storm_clim: None,
            storm_year: i64::MIN,
            storm_now: Vec::new(),
            storm_prev: Vec::new(),
            storm_marks: Vec::new(),
            droughts: crate::drought::Droughts::default(),
            floods: crate::flood::Floods::default(),
            lakes,
            fields_ledger: self.margins.take().expect("fertility stage ran"),
            fields_sky: 0.0,
            fields_log: Vec::new(),
            ice_ledger: self.ice_margin.take().expect("climate stage ran"),
            ice_sky: 0.0,
            ice_log: Vec::new(),
            storm_bites: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            storm_fell_probe: Vec::new(),
            year_weather: std::sync::Mutex::new(None),
            year_site_weather: std::sync::Mutex::new(None),
            grain_shock_year: -1,
            famine_ledger: Vec::new(),
            site_score: founded.site_score,
            food_grid: founded.food_grid,
            near_fresh: founded.near_fresh,
            arid_dry: founded.arid_dry,
            dry_site_score: founded.dry_site_score,
            dry_reach_override: None,
            caravan_memo: std::sync::Mutex::new(None),
            coast: founded.coast,
            shelter,
            max_settlements: founded.max_settlements,
            trade: trade_grid,
            timings,
        };
        // Open-ocean margins east and west: the world breathes a little wider.
        world.widen(size / 8);
        // M22 — fault seams read off the *final* boundary grid, so every
        // epicenter lands in shipped map coordinates. The sketch stays
        // frozen; only the seams' renewal clocks tick from here on.
        world.seismic = crate::seismic::derive(seed, &world.plates);
        // M23 — cones read off the *final* height and province grids:
        // every vent, ash apron and burn radius in shipped coordinates.
        world.volcanism = crate::seismic::derive_volcanism(
            seed,
            &world.fields.height,
            &world.fields.rock,
            &world.sealevel,
        );
        // M26 — the coasts read their own history: raised beaches where
        // the land outran the sea, rias and skerries where the sea won.
        // M59's fan-built delta plains are the river's fresh work, not
        // the sea's record, and stay out of the raised-beach census.
        world.fields.landform = crate::landform::classify(
            &world.fields.height,
            &world.sealevel,
            &world.ice,
            &world.sediment.delta,
        );
        // M33 — the cold rim reads its own signature: permafrost extent
        // off the continentality-shifted MAAT, micro-texture where the
        // frozen flats sort themselves into polygons and stripes.
        // Post-widen, like the coasts: every cell in shipped coordinates.
        world.permafrost = crate::permafrost::Permafrost::compute(
            &world.fields.height,
            &world.fields.tmean,
            &world.fields.flags,
        );
        crate::landform::stamp_patterned(
            &mut world.fields.landform,
            &world.permafrost.pattern,
            &world.fields.height,
        );
        // The wire learns the frozen ground for free: two CellFlags bits
        // in the byte the pack already ships (E1.7).
        {
            let (fr, fc) = world.fields.flags.dim();
            for y in 0..fr {
                for x in 0..fc {
                    if world.permafrost.extent[[y, x]] >= crate::permafrost::DISCONTINUOUS {
                        world.fields.flags[[y, x]] |= CellFlags::PERMAFROST.bits();
                    }
                    if world.permafrost.pattern[[y, x]] != crate::permafrost::PAT_NONE {
                        world.fields.flags[[y, x]] |= CellFlags::PATTERNED.bits();
                    }
                }
            }
        }
        // M40 — the ocean answers the winds: basin-scale gyres solved
        // over the final coastline geometry, western walls and all.
        // Post-widen, like the coasts: the margins widen the basins.
        let water_now = world.fields.height.mapv(|h| h < 0.0);
        world.currents = crate::currents::Currents::compute(&water_now);
        // M47 — the nutrient coasts: offshore trades and cold rims mark
        // the upwelling shore off the same final coastline, packed for
        // the inspector and banked for Era IV's fisheries.
        world.fields.upwelling = crate::climate::upwelling(&water_now, &world.currents.v);
        // M43 — the shore breathes daily: tidal range solved off the
        // final basin geometry, then the landform vocabulary learns
        // the flats and the estuary mouths. Post-widen, like every
        // coastal reading; the earlier stories keep precedence.
        world.tides = crate::tides::Tides::compute(&world.fields.height);
        crate::landform::stamp_tidal(
            &mut world.fields.landform,
            &world.tides,
            &world.fields.height,
            &world.fields.flags,
        );
        // M60 — the full fold: the era's remaining stories join the one
        // grid in precedence order (river fans, the drift's new coast,
        // the dry country's water, the ice's dry valleys), then the
        // generic relief vocabulary fills every untold land cell and
        // open shore. After this block, NONE survives only on open sea.
        crate::landform::stamp_delta(
            &mut world.fields.landform,
            &world.sediment.delta,
            &world.fields.height,
        );
        crate::landform::stamp_coastforms(
            &mut world.fields.landform,
            &world.fields.coastform,
            &world.fields.height,
        );
        {
            // The dry country's water re-read off the shipped grids: the
            // same M55 law founding priced pre-widen, here solved on the
            // final coordinates (margins are open ocean — no new springs).
            let rivers = world
                .fields
                .flags
                .mapv(|f| f & CellFlags::RIVER.bits() != 0);
            let lakes = world
                .fields
                .flags
                .mapv(|f| f & CellFlags::LAKE.bits() != 0);
            let dry = crate::hydrology::springs_and_oases(
                &world.fields.height,
                &water_now,
                &rivers,
                &lakes,
                &world.fields.aquifer,
                &world.fields.biomes,
                &world.fields.precip,
            );
            crate::landform::stamp_dry_water(
                &mut world.fields.landform,
                &dry.springs,
                &dry.oases,
                &world.fields.aquifer,
                &world.fields.height,
            );
        }
        crate::landform::stamp_trough(
            &mut world.fields.landform,
            &world.ice.carved,
            &world.fields.height,
        );
        crate::landform::finish(&mut world.fields.landform, &world.fields.height);
        // M34 — the ice that remains: modern mountain glaciers wherever
        // today's climate keeps the annual mass balance positive. Since
        // M35 the balance is computed at the climate stage (hydrology
        // feeds the melt to the rivers) and widened with the ice ledger;
        // here it only stamps the eighth (and last) flag bit.
        {
            let (fr, fc) = world.fields.flags.dim();
            for y in 0..fr {
                for x in 0..fc {
                    if world.ice.modern[[y, x]] > 0.0 {
                        world.fields.flags[[y, x]] |= CellFlags::GLACIER.bits();
                    }
                }
            }
        }
        // M62 — geomorphic toponymy. Only now does every cell carry its
        // landform word, so only now can the dawn towns take names that
        // tell the truth about the ground: a fjord town's name says
        // fjord, in its own tongue. Where a tongue has no word for the
        // ground, the plain coined name stands — no borrowed vocabulary.
        // The registry and the dawn Found events follow the new names.
        {
            let mut rng62 = crate::util::rng(world.seed + 6200);
            let styles: Vec<String> =
                world.peoples.peoples.iter().map(|p| p.style.clone()).collect();
            let mut renames: Vec<(i64, i64, String, String)> = Vec::new();
            for s in world.peoples.settlements.iter_mut() {
                let code = world.fields.landform[[s.y as usize, s.x as usize]];
                let style = styles
                    .get(s.namer.idx())
                    .map(|st| st.as_str())
                    .unwrap_or("old");
                if let Some(c) =
                    naming::coin_for_landform(&mut rng62, style, code, &mut world.taken)
                {
                    let old = std::mem::replace(&mut s.name, c.word.clone());
                    s.ety = c.ety;
                    renames.push((s.x, s.y, old, c.word));
                }
            }
            for (x, y, _old, new) in &renames {
                if let Some(id) =
                    world.chronicle.registry.find_alive(EntityKind::Settlement, *x, *y)
                {
                    world.chronicle.registry.rename(id, new);
                }
            }
            for ev in world.chronicle.events.iter_mut() {
                if !matches!(ev.k, EventKind::Found) {
                    continue;
                }
                if let Some((_, _, old, new)) =
                    renames.iter().find(|r| r.0 == ev.x && r.1 == ev.y)
                {
                    ev.s = new.clone();
                    ev.text = ev.text.replace(old.as_str(), new.as_str());
                }
            }
        }
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
        // M80 follow-up — measure what this world's own sky does to the
        // accumulated sum, so `famine::DROUGHT_Z` means the same crossing
        // rate it meant before the memory existed. Once, at the dawn,
        // from a fixed window of prehistory: pure in seed × size.
        world.droughts.norm = world.calibrate_drought_norm();
        self.world = Some(world);

    }
}

impl World {
    /// M71 — the year's anomaly grids computed from scratch, bypassing the
    /// memo. Diagnostics hold this against the memoized copy so the cache
    /// can never quietly become a second source of weather.
    /// M73 — the variability lattice itself. The year's sky is derived,
    /// not stored, so the harness reads its source directly: to measure
    /// the realized σ against the declared amplitude law, and to fold a
    /// fixed probe of it into the replay identity line (ADR-0003).
    /// M74 — the basin's seesaw, for the harness and for consumers that
    /// need the year's lean rather than the cell's anomaly.
    pub fn oscillation(&self) -> &crate::oscillation::Oscillation {
        &self.oscillation
    }

    pub fn variability(&self) -> &Perlin3 {
        &self.variability
    }

    /// M75 — the lean the far side is answering in `year`: the index read
    /// `TELE_LAG_MONTHS` before the year opens, so the tilt arrives on a
    /// season's delay rather than instantaneously.
    pub fn year_osc(&self, year: i64) -> f64 {
        self.oscillation
            .index(year * 12 - crate::climate::TELE_LAG_MONTHS)
    }

    pub fn year_anomaly_fresh(&self, year: i64) -> (Array2<f64>, Array2<f64>) {
        let (rows, cols) = self.fields.tmean.dim();
        climate::year_anomaly(
            &self.variability,
            rows,
            cols,
            year,
            self.year_osc(year),
            self.year_forcing(year),
        )
    }

    /// M75/M83 — the same year with the seesaw *and* the drift held at
    /// zero: the unforced latitude law alone, which is the quantity M71's
    /// amplitude law declares. The forced field (`year_anomaly_fresh`) is
    /// what the sim runs on; this is its counterfactual twin, used by
    /// diagnostics to separate the noise law from the forcings laid over
    /// it (the M75 tilt on the rain lane, the M83 drift on temperature).
    pub fn year_anomaly_unforced(&self, year: i64) -> (Array2<f64>, Array2<f64>) {
        let (rows, cols) = self.fields.tmean.dim();
        climate::year_anomaly(&self.variability, rows, cols, year, 0.0, 0.0)
    }

    /// M83 — the century's temperature drift in `year`, °C on the baseline
    /// `tmean`. The law is `climate::Drift` (pure in seed × year); this is
    /// the sim's read, cached per year so the O(year) walk is paid once
    /// per year, never per site. Prehistory reads 0 without touching the
    /// cache — the dawn is the baseline epoch by law.
    pub fn year_drift(&self, year: i64) -> f64 {
        if year <= 0 {
            return 0.0;
        }
        let mut cache = self.year_drift_memo.lock().unwrap();
        *cache
            .entry(year)
            .or_insert_with(|| self.drift.value(year))
    }

    /// M83 — the walk itself, for the harness: to gate the law directly
    /// and to fold its probe into the replay identity line (ADR-0003).
    pub fn drift(&self) -> &crate::climate::Drift {
        &self.drift
    }

    /// M86 — the cold-age schedule, for the harness: to gate the law
    /// directly and to fold its probe into the replay identity line.
    pub fn ages(&self) -> &crate::ages::Ages {
        &self.ages
    }

    /// M86/M87 — the composed global forcing in `year`, °C on the
    /// baseline `tmean`: the M83 drift plus the active age's ramped
    /// offset — a winter cools it, an optimum warms it. This is the
    /// single value the forced sky rides — every `year_anomaly` call
    /// site that used to take the drift alone takes this instead, so an
    /// age moves temperatures *and* walks the belts through one law,
    /// not two.
    pub fn year_forcing(&self, year: i64) -> f64 {
        self.year_drift(year) + self.ages.offset(year)
    }



    /// M71 — hand the caller the year's anomaly grids, computing them once
    /// per year and holding them until the year turns. `dt` is degrees on
    /// `tmean`, `dp` the fractional change on `precip`.

    pub fn with_year_weather<R>(&self, year: i64, f: impl FnOnce(&Array2<f64>, &Array2<f64>) -> R) -> R {
        self.with_year_sky(year, |dt, dp, _| f(dt, dp))
    }

    /// M72 — the full year: temperature anomaly, rain anomaly, and the
    /// catchment-integrated rain anomaly the rivers run on.
    pub fn with_year_sky<R>(
        &self,
        year: i64,
        f: impl FnOnce(&Array2<f64>, &Array2<f64>, &Array2<f64>) -> R,
    ) -> R {
        let mut slot = self.year_weather.lock().unwrap();
        let stale = match slot.as_ref() {
            Some((y, _, _, _)) => *y != year,
            None => true,
        };
        if stale {
            let (rows, cols) = self.fields.tmean.dim();
            let (dt, dp) = climate::year_anomaly(
                &self.variability,
                rows,
                cols,
                year,
                self.year_osc(year),
                self.year_forcing(year),
            );
            let dq = crate::ndimage::gaussian_filter(&dp, climate::CATCHMENT_SIGMA);
            *slot = Some((year, dt, dp, dq));
        }
        let (_, dt, dp, dq) = slot.as_ref().unwrap();
        f(dt, dp, dq)
    }

    /// Exact annual weather at one inhabited cell, memoized for the year.
    /// This is the simulation path; full grids remain available above for
    /// diagnostics and map-wide inspection.
    fn year_site_weather(&self, year: i64, y: usize, x: usize) -> (f64, f64) {
        let mut slot = self.year_site_weather.lock().unwrap();
        if slot.as_ref().is_none_or(|(cached_year, _)| *cached_year != year) {
            *slot = Some((year, BTreeMap::new()));
        }
        let (_, sites) = slot.as_mut().unwrap();
        let entry = sites.entry((y, x)).or_insert_with(|| {
            let rows = self.fields.tmean.dim().0;
            let (dt, dp) = climate::year_anomaly_at(
                &self.variability,
                rows,
                x,
                y,
                year,
                self.year_osc(year),
                self.year_forcing(year),
            );
            (dt, dp, None)
        });
        (entry.0, entry.1)
    }

    fn year_site_flow_anomaly(&self, year: i64, y: usize, x: usize) -> f64 {
        let mut slot = self.year_site_weather.lock().unwrap();
        if slot.as_ref().is_none_or(|(cached_year, _)| *cached_year != year) {
            *slot = Some((year, BTreeMap::new()));
        }
        let (_, sites) = slot.as_mut().unwrap();
        let entry = sites.entry((y, x)).or_insert_with(|| {
            let rows = self.fields.tmean.dim().0;
            // Same fill as `year_site_weather` — the entry is shared, so
            // the dt lane must carry the drift here too (M83).
            let (dt, dp) = climate::year_anomaly_at(
                &self.variability,
                rows,
                x,
                y,
                year,
                self.year_osc(year),
                self.year_forcing(year),
            );
            (dt, dp, None)
        });
        if entry.2.is_none() {
            let (rows, cols) = self.fields.tmean.dim();
            entry.2 = Some(climate::catchment_anomaly_at(
                &self.variability,
                rows,
                cols,
                x,
                y,
                year,
                self.year_osc(year),
                self.year_forcing(year),
            ));
        }
        entry.2.unwrap_or(0.0)
    }

    pub(crate) fn year_rain_anomaly_site(&self, year: i64, y: usize, x: usize) -> f64 {
        self.year_site_weather(year, y, x).1
    }

    /// M71 — the mean temperature this cell actually saw in `year`:
    /// the climate mean plus that year's anomaly (°C).
    pub fn year_tmean(&self, year: i64, y: usize, x: usize) -> f64 {
        self.with_year_weather(year, |dt, _| self.fields.tmean[[y, x]] as f64 + dt[[y, x]])
    }

    /// M71 — the rain this cell actually got in `year` (mm), the climate
    /// mean scaled by that year's fractional anomaly.
    pub fn year_precip(&self, year: i64, y: usize, x: usize) -> f64 {
        self.with_year_weather(year, |_, dp| {
            (self.fields.precip[[y, x]] as f64 * (1.0 + dp[[y, x]])).max(0.0)
        })
    }

    /// M72 — the flow this cell's river actually carried in `year`. The
    /// stored `discharge` grid is never touched: a river's rank, its
    /// Strahler order and every endorheic call are the dawn's, solved on
    /// the mean climate and frozen (ADR-0005). What breathes is the
    /// *year's* water, a bounded multiplier read by whoever needs it.
    pub fn year_discharge(&self, year: i64, y: usize, x: usize) -> f64 {
        self.fields.discharge[[y, x]] as f64 * self.year_flow_factor(year, y, x)
    }

    /// M72 — the bounded flow multiplier itself, catchment-integrated so a
    /// single dry cell cannot empty a trunk river.
    pub fn year_flow_factor(&self, year: i64, y: usize, x: usize) -> f64 {
        self.with_year_sky(year, |_, _, dq| {
            (1.0 + climate::FLOW_ANOM_GAIN * dq[[y, x]]).clamp(climate::FLOW_FACTOR_MIN, climate::FLOW_FACTOR_MAX)
        })
    }

    /// M72 — whether this cell stands within reach of fresh water, and so
    /// drinks the river's year rather than the cloud's.
    pub fn irrigable(&self, y: usize, x: usize) -> bool {
        self.near_fresh[[y, x]]
    }

    /// M72 — what the year did to this cell's harvest: the crop package
    /// standing here, scored through `agriculture::climatic_score` at the
    /// mean and at mean-plus-anomaly. Watered ground reads the catchment
    /// lane instead of the local rain — a canal is fed by the river's
    /// year, not by the cloud overhead.
    ///
    /// The irrigated branch reads the *published* flow law, clamp and all:
    /// `year_flow_factor` is what `year_discharge` multiplies by, so a canal
    /// can never be handed more water than the river is stated to carry.
    pub fn year_yield(&self, year: i64, y: usize, x: usize) -> f64 {
        let base = self.year_yield_bare(year, y, x);
        // M81 — the water takes the year it stands in: a spate this season
        // destroys the crop under it before any silt is ever farmed.
        let drown = self.floods.drown_loss(year, y, x);
        let base = if drown > 0.0 { base * (1.0 - drown) } else { base };
        // M81 — and last year's spate pays this year's harvest: the silt
        // sheet laid on the floodplain lifts the season it feeds, once.
        let silt = self.floods.silt_bonus(year, y, x);
        if silt <= 0.0 {
            base
        } else {
            (base * (1.0 + silt)).min(agriculture::YIELD_CEIL)
        }
    }


    /// The same harvest read with the floodplain's silt sheet ignored —
    /// the counterfactual the M81 gate measures the gift against.
    pub fn year_yield_bare(&self, year: i64, y: usize, x: usize) -> f64 {
        let pack = agriculture::CropPackage::from_code(self.fields.crops[[y, x]]);
        let irrigated = self.near_fresh[[y, x]];
        let t = self.fields.tmean[[y, x]] as f64;
        let p = self.fields.precip[[y, x]] as f64;
        let (dt, dp) = self.year_site_weather(year, y, x);
        let rain = if irrigated {
            self.year_site_flow_factor(year, y, x) - 1.0
        } else {
            dp
        };
        agriculture::year_yield_factor(pack, t, p, dt, rain, irrigated)
    }

    /// M81 harness probe — the live consumption path, one recorded row at
    /// a time. `Floods::sweep` keeps only the last seasons' sheets on the
    /// map, so a post-run gate cannot read a historical row's lift through
    /// `year_yield` unless the sheet is stood back up first. This re-stands
    /// the sheet exactly as the ledger recorded it, takes the live read at
    /// the year it feeds, and restores the map to the byte — the world is
    /// unperturbed for every later check and hash. Diagnostics-only: the
    /// simulation never calls it.
    ///
    /// The comparison toggles the sheet and nothing else: with-sheet
    /// versus without-sheet through the same `year_yield`, every other
    /// law left standing. Reading the "without" side from
    /// `year_yield_bare` conflated the sheet's gift with a co-incident
    /// spate's drown — a town flooded in consecutive years still carries
    /// the feed-year's drown entry, and the sheet was being asked to
    /// out-lift the destruction of a flood it never caused (M82: one
    /// holdout row on seed 12345 at paleoclimate flood rates).
    pub fn probe_silt_lift(&mut self, feed: i64, y: usize, x: usize, strength: f64) -> bool {
        let key = (y as i64, x as i64);
        let saved = self.floods.silt.insert(key, (feed, strength));
        let with_sheet = self.year_yield(feed, y, x);
        self.floods.silt.remove(&key);
        let without_sheet = self.year_yield(feed, y, x);
        let lifted = with_sheet > without_sheet + 1e-9;
        if let Some(prior) = saved {
            self.floods.silt.insert(key, prior);
        }
        lifted
    }



    /// M72 — `year_flow_factor` at one cell without materializing the grid:
    /// the same gain, the same clamp, over the per-site catchment reading.
    pub fn year_site_flow_factor(&self, year: i64, y: usize, x: usize) -> f64 {
        (1.0 + climate::FLOW_ANOM_GAIN * self.year_site_flow_anomaly(year, y, x))
            .clamp(climate::FLOW_FACTOR_MIN, climate::FLOW_FACTOR_MAX)
    }

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

    /// M4.1 — redraw both influence maps after borders move or towns grow:
    /// the realm map (political territory) and the people map (whose
    /// hearths lie where — ADR-0018's slow axis).
    pub fn recompute_territory(&mut self) {
        self.fields.territory = politics::influence_map(
            &self.fields.height,
            &self.peoples.settlements,
            &self.peoples.realms,
            &self.peoples.societies,
            &self.politics.asab,
            self.peoples.realms.len(),
        );
        self.fields.peoples_map = politics::peoples_influence_map(
            &self.fields.height,
            &self.peoples.settlements,
            self.peoples.peoples.len(),
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

        // M90 — the margin ledger's coordinates ride the same shift the
        // interior takes: `pad` columns of open ocean enter west of x=0,
        // and every solved cell keeps naming the same ground.
        for c in self.fields_ledger.cells.iter_mut() {
            c.x += pad as u32;
        }

        // M91 — the ice-margin ledger rides the same shift: every
        // solved threshold keeps naming the same ground.
        for c in self.ice_ledger.cells.iter_mut() {
            c.x += pad as u32;
        }

        // M93 — the lake ledger's cells, catchments and anchors ride it
        // too: a basin keeps naming the same water.
        for b in self.lakes.basins.iter_mut() {
            b.x += pad as i64;
            for c in b.cells.iter_mut() {
                c.0 += pad as u16;
            }
            for c in b.catchment.iter_mut() {
                c.0 += pad as u16;
            }
        }


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
        // M45 — the margins are open ocean: no anchorage, no shelter.
        self.shelter = grow(&self.shelter, pad, |_, _| 0.0f32);
        // M28/M29/M30 — the ice ledger rides along: margins are open water.
        self.ice.thickness = grow(&self.ice.thickness, pad, |_, _| 0.0f32);
        self.ice.carved = grow(&self.ice.carved, pad, |_, _| 0.0f32);
        self.ice.till = grow(&self.ice.till, pad, |_, _| 0.0f32);
        self.ice.loess = grow(&self.ice.loess, pad, |_, _| 0.0f32);
        self.ice.outwash = grow(&self.ice.outwash, pad, |_, _| 0.0f32);
        // M34/M35 — the modern balance and the meltwater ledger:
        // margins are open ocean, no ice and no melt.
        self.ice.modern = grow(&self.ice.modern, pad, |_, _| 0.0f32);
        self.ice.melt = grow(&self.ice.melt, pad, |_, _| 0.0f32);
        self.ice.melt_amp = grow(&self.ice.melt_amp, pad, |_, _| 0.0f32);
        // M44 — the drift ledger rides along: margins are open sea.
        self.coastform.widen(pad);
        // M59 — the sediment books ride along: no river ever reached
        // the margins, so their footprint there is exactly zero.
        self.sediment.widen(pad);
        // M68 — the handover: both grids reach their registry home at
        // full shipped width, and the ledgers keep only what is theirs
        // (the deposit record, the mouth books, the delta mask). One
        // grid, one owner — nothing hand-mirrored beside the registry.
        self.fields.coastform = std::mem::take(&mut self.coastform.form_gen);
        self.fields.silt = std::mem::take(&mut self.sediment.depth_gen);
        for p in self
            .ice
            .cirques
            .iter_mut()
            .chain(self.ice.hangs.iter_mut())
            .chain(self.ice.moraines.iter_mut())
            .chain(self.ice.drumlins.iter_mut())
            .chain(self.ice.eskers.iter_mut())
            .chain(self.ice.proglacial.iter_mut())
            .chain(self.ice.spillways.iter_mut().flat_map(|c| c.iter_mut()))
        {
            p.1 += pad as u16;
        }
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
        self.arid_dry = grow_bool(&self.arid_dry, pad);
        self.dry_site_score = grow(&self.dry_site_score, pad, |_, _| -1e9);
        self.coast = grow_bool(&self.coast, pad);
        self.fields.pamp = grow(&self.fields.pamp, pad, |e, _| e);
        self.fields.flow_amp = grow(&self.fields.flow_amp, pad, |_, _| 0.0);
        // The plate sketch rides along (M16): margins extend the edge
        // plate under the open ocean; no new boundaries appear.
        self.plates.cell = grow(&self.plates.cell, pad, |e, _| e);
        self.plates.boundary = grow(&self.plates.boundary, pad, |_, _| crate::plates::B_NONE);
        self.plates.edge_dist = grow(&self.plates.edge_dist, pad, |e, _| e);
        self.plates.seam_dist = grow(&self.plates.seam_dist, pad, |e, _| e);
        self.plates.seam_age = grow(&self.plates.seam_age, pad, |e, _| e);
        // The basement rides along (M18): the open-ocean margins are
        // young sea floor under sediment — basin, never shield.
        self.fields.rock = grow(&self.fields.rock, pad, |_, _| crate::rock::BASIN);
        // M51 — the margins are open ocean: no profile, no soil order.
        self.fields.soil = grow(&self.fields.soil, pad, |_, _| {
            crate::agriculture::SoilOrder::None.code()
        });
        // M54 — the margins are open ocean: the table is the sea.
        self.fields.aquifer = grow(&self.fields.aquifer, pad, |_, _| 0.0);
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
        // M45 — no land in the margins, so no anchorage to discount.
        let tsh = self.trade.shelter.clone();
        self.trade.shelter = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                tsh[[y, xi as usize]]
            } else {
                0.0
            }
        });
        // M46 — the margins are open blue water with no solved gyre:
        // zero current there (wind and the doldrum rows are latitude
        // laws and ride each row unchanged into the margins).
        let tcu = self.trade.cu.clone();
        self.trade.cu = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                tcu[[y, xi as usize]]
            } else {
                0.0
            }
        });
        let tcv = self.trade.cv.clone();
        self.trade.cv = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                tcv[[y, xi as usize]]
            } else {
                0.0
            }
        });
        let top = self.trade.open.clone();
        self.trade.open = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                top[[y, xi as usize]]
            } else {
                true
            }
        });
        // M37 — the freeze rides along: the margins are open ocean at the
        // same latitude with edge-extended climate, so the edge column's
        // ice calendar extends west and east unchanged.
        let fz = self.trade.frozen.clone();
        let (fh, fw) = fz.dim();
        self.trade.frozen = Array2::from_shape_fn((fh, fw + 2 * pad), |(y, x)| {
            let xi = (x as isize - p).clamp(0, fw as isize - 1) as usize;
            fz[[y, xi]]
        });
        // M48 — the monsoon lean rides the same law: the edge column's
        // sailing calendar extends unchanged (and the frame's coasts sit
        // far enough in that the margins read near zero anyway).
        let mn = self.trade.mons.clone();
        let (mh, mw) = mn.dim();
        self.trade.mons = Array2::from_shape_fn((mh, mw + 2 * pad), |(y, x)| {
            let xi = (x as isize - p).clamp(0, mw as isize - 1) as usize;
            mn[[y, xi]]
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
    pub(crate) fn tick_month(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        let month = month_abs.rem_euclid(12);
        let mods: Vec<society::Mods> =
            self.peoples.societies.iter().map(society::mods_for).collect();
        // M2.3/M10.4: the seat of kings — the realm's *named seat* keeps
        // a court, and courts import: grain barges, tribute, hungry
        // retinues. The head of the rank-size curve is political as much
        // as economic (ADR-0018: courts follow crowns, not tongues). The
        // seat is the one politics maintains — a fallen seat re-homes in
        // the statecraft pass, not here.
        let mut seat: Vec<usize> = vec![usize::MAX; self.peoples.realms.len()];
        for (i, s) in self.peoples.settlements.iter().enumerate() {
            if self
                .peoples
                .realms
                .get(s.realm.0)
                .is_some_and(|r| r.seat == s.id)
            {
                seat[s.realm.0] = i;
            }
        }
        // M72 — the year's harvest verdict per town, drawn before the loop
        // because it reads the whole world (crops, climate, the year's sky)
        // while the loop holds the settlements mutably. One entry per town,
        // in town order: the multiplier this year's weather puts on the
        // ground each town farms.
        let year_now = month_abs.div_euclid(12);
        let year_harvest: Vec<f64> = self
            .peoples
            .settlements
            .iter()
            .map(|s| self.year_yield(year_now, s.y as usize, s.x as usize))
            .collect();
        for (si, s) in self.peoples.settlements.iter_mut().enumerate() {
            let md = mods.get(s.people.idx()).cloned().unwrap_or_default();
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
            // M72: the year that was. Capacity is what the land feeds *this
            // year*, not what it feeds on average — the same crop curves,
            // scored against the sky the year actually delivered.
            k *= year_harvest[si];
            // M2.3: market towns import grain — the web of trade lifts K,
            // and the fat head of the rank-size curve lives in the hubs.
            k *= 1.0 + 0.26 * (s.connections.min(8) as f64);
            // NOTE: mirrored by explain.rs — the court term rides on s.k.
            if seat.get(s.realm.0) == Some(&si) {
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
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
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
                growth += pop as f64 * 0.004;
            }
            // a harbour draws trade, sailors and coin
            if s.port {
                growth += pop as f64 * 0.0012;
            }
            // M24 — the rebuild arc: a disaster-struck town regrows hot
            // while kin return and the stone is re-cut, until it stands
            // at its old strength or the window lapses (forty years).
            if s.rebuild_until > 0 {
                if pop >= s.rebuild_peak {
                    let took = (settlements::REBUILD_WINDOW
                        - (s.rebuild_until - month_abs))
                        .max(1);
                    self.rebuild_log.push(took as u32);
                    s.rebuild_until = 0;
                    s.rebuild_peak = 0;
                } else if month_abs >= s.rebuild_until {
                    s.rebuild_until = 0; // the window lapses; what stands, stands
                    s.rebuild_peak = 0;
                } else {
                    growth += pop as f64 * 0.012;
                }
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
                        // anchor the ground: the town may be renamed this
                        // very tick (M9.2) and the resolver must still
                        // find it by its one immovable property
                        x: s.x,
                        y: s.y,
                        ..Default::default()
                    });
                    // rising tier: something worth singing about may be raised
                    let wonders = chronicle::wonder_for(
                        &mut self.chronicle.state,
                        &mut self.rng,
                        s,
                        &self.peoples.peoples,
                        month_abs,
                    );
                    events.extend(wonders);
                } else {
                    events.push(Event {
                        m: month_abs,
                        s: s.name.clone(),
                        k: EventKind::Disaster,
                        text: format!("{} dwindles to a {}.", s.name, s.tier.to_lowercase()),
                        // anchor the ground (see the promotion twin above)
                        x: s.x,
                        y: s.y,
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
    /// pub since M55: diagnostics weigh the desert against the pull that
    /// would draw a colony into it.
    pub fn resource_pull(&self) -> Array2<f64> {
        self.resource_pull_for(&Default::default())
    }

    /// M58 — the same pull, heard by a particular crown. `claim` is the
    /// realm's per-good claim pressure (`economy::claim_pressure`): the
    /// weight of workshops it could run and cannot, for want of that ore.
    /// A deprived crown hears a known seam of the metal it lacks louder
    /// than a rich neighbour does — and, because the pressure also lifts
    /// the local cap, a single strategic seam can out-call a whole
    /// district of ordinary ones. No site score is touched: it is the
    /// *seam* that gains a voice, not the ground around it.
    pub fn resource_pull_for(&self, claim: &BTreeMap<Good, f64>) -> Array2<f64> {
        let (h, w) = self.site_score.dim();
        let mut pull = Array2::<f64>::zeros((h, w));
        const R: i64 = 5;
        for d in &self.deposits {
            if !d.live() {
                continue;
            }
            // renewables draw no rush; it is metal, coal and stone that
            // call — and two luxuries (M14.3/4): furs colonize the cold,
            // spices the fever coast, the way ore colonizes the dry.
            // Grapes and dyes lie in comfortable country and wait for
            // ordinary settlement to reach them.
            {
                if !(d.r.is_mineral() || d.r == Good::Furs || d.r == Good::Spices) {
                    continue;
                }
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
            // M58 — the crown's own hunger multiplies what this seam is
            // worth to *it*: an iron realm with dark forges bids for iron.
            let press = claim.get(&d.r).copied().unwrap_or(0.0).max(0.0);
            let voice = 1.0 + economy::CLAIM_GAIN * press;
            let worth = self.economy.market.price(d.r) * d.rich * 2.2 * voice;
            let cap = 7.0 * voice;
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
                    *c = (*c + v).min(cap);
                }
            }
        }
        pull
    }

/// M56 — the smallest town that can victual a caravan: below a market's
/// worth of people there is no grain surplus, no water to spare and no
/// beasts for hire, so the staging posts of the desert trade are towns,
/// not hamlets.
pub const CARAVAN_MARKET_POP: i64 = 400;

    /// M56 — the caravan's provisioning field over the live world: how
    /// well a victualling train out of the nearest *watered* town can
    /// supply each cell. Only watered towns victual a caravan — a camp
    /// that drinks from a shaft has no surplus water to send out with
    /// somebody else's camels — and only towns big enough to hold a
    /// market (`CARAVAN_MARKET_POP`) count as a staging post.
    ///
    /// pub since M56: diagnostics read the same field the siting does.
    pub fn caravan_provision(&self) -> Array2<f32> {
        self.caravan_provision_claim(0.0)
    }

    /// M58 — the provisioning field a crown under claim pressure can
    /// afford. `press` is the strongest claim that crown is pressing;
    /// the state's purse buys lane length (`CLAIM_REACH_GAIN`), which is
    /// how strategic ground far past ordinary trade got victualled.
    pub fn caravan_provision_claim(&self, press: f64) -> Array2<f32> {
        let budget = trade::CARAVAN_BUDGET
            * (1.0 + economy::CLAIM_REACH_GAIN * press.max(0.0));
        let markets: Vec<(usize, usize)> = self
            .peoples
            .settlements
            .iter()
            .filter(|s| {
                s.pop >= Self::CARAVAN_MARKET_POP
                    && !self.arid_dry[[s.y as usize, s.x as usize]]
            })
            .map(|s| (s.y as usize, s.x as usize))
            .collect();
        // E10.2 — the field is a pure function of these inputs; key on
        // them exactly (market cells in order, purse bit-for-bit) so a
        // hit is indistinguishable from the recompute it replaces.
        let mut key = 0xcbf29ce484222325u64;
        let mut mix = |v: u64| {
            key ^= v;
            key = key.wrapping_mul(0x100000001b3);
        };
        mix(budget.to_bits());
        mix(markets.len() as u64);
        for &(y, x) in markets.iter() {
            mix(((y as u64) << 32) | x as u64);
        }
        let mut memo = self.caravan_memo.lock().expect("caravan memo poisoned");
        if let Some((k, grid)) = memo.as_ref() {
            if *k == key {
                return grid.clone();
            }
        }
        let grid =
            trade::caravan_provision_budget(&self.trade, &markets, self.site_score.dim(), budget);
        *memo = Some((key, grid.clone()));
        grid
    }

    /// M58 — the claim pressures every crown is currently pressing.
    /// pub so diagnostics read exactly the map colonisation reads.
    pub fn claim_pressure(&self) -> BTreeMap<(RealmId, Good), f64> {
        economy::claim_pressure(
            &self.peoples.settlements,
            &self.peoples.societies,
            &self.economy.areas,
        )
    }

    /// M58 — one realm's slice of the claim map, plus its strongest press.
    pub fn realm_claim(
        claims: &BTreeMap<(RealmId, Good), f64>,
        realm: RealmId,
    ) -> (BTreeMap<Good, f64>, f64) {
        let mut per: BTreeMap<Good, f64> = BTreeMap::new();
        let mut top = 0.0f64;
        for (&(r, g), &v) in claims.iter() {
            if r == realm {
                per.insert(g, v);
                top = top.max(v);
            }
        }
        (per, top)
    }

    pub(crate) fn try_colonize(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        let mut founded = false;
        // M58 — the crowns' claim pressures, computed once per colonising
        // tick, and the per-realm pull/provision fields they imply. A
        // realm that can smelt a metal but owns no seam of it hears the
        // known distant seams louder and pays for a longer lane to them.
        let mut claims: Option<BTreeMap<(RealmId, Good), f64>> = None;
        let mut realm_fields: BTreeMap<RealmId, (Array2<f64>, Array2<f32>)> = BTreeMap::new();
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
                let md = mods_v.get(p.people.idx()).cloned().unwrap_or_default();
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
            if claims.is_none() {
                claims = Some(self.claim_pressure());
            }
            let prealm = self.peoples.settlements[pi].realm;
            if !realm_fields.contains_key(&prealm) {
                let (per, top) =
                    Self::realm_claim(claims.as_ref().unwrap(), prealm);
                // M56 — the caravan field: which dry ground a victualling
                // train out of a watered market can still reach; M58 sets
                // its purse from this crown's own hunger.
                let f = (self.resource_pull_for(&per), self.caravan_provision_claim(top));
                realm_fields.insert(prealm, f);
            }

            // E10.2 — borrowed, never cloned: `realm_fields` is a local
            // map, so the pull/provision grids can be read in place. The
            // clone here was copying ~4 MB of field per candidate parent
            // and dominated the grown-in tick.
            let (pull_r, prov_r) = realm_fields.get(&prealm).unwrap();

            let site = {
                let parent = self.peoples.settlements[pi].clone();
                let range = self
                    .peoples.societies
                    .get(parent.people.idx())
                    .map(|so| society::mods_for(so).colony_range)
                    .unwrap_or(1.0);
                let reach = self.dry_reach_override.unwrap_or_else(|| {
                    self.peoples
                        .societies
                        .get(parent.people.idx())
                        .map(settlements::well_reach_m)
                        .unwrap_or(0.0)
                });
                let dry = settlements::DryFrontier {
                    arid_dry: &self.arid_dry,
                    aquifer: &self.fields.aquifer,
                    dry_site_score: &self.dry_site_score,
                    well_reach_m: reach,
                    provision: &prov_r,
                };
                let sea = settlements::HarbourEye {
                    shelter: &self.shelter,
                    trade: self
                        .peoples
                        .societies
                        .get(parent.people.idx())
                        .map(|so| society::mods_for(so).trade)
                        .unwrap_or(1.0),
                };
                settlements::colony_site(
                    &self.site_score,
                    &pull_r,
                    &self.peoples.settlements,
                    &parent,
                    3600.0 * range * range,
                    &dry,
                    &sea,
                )
            };
            let Some((y, x)) = site else { continue };
            // an ore-led venture: the seams called louder than the soil
            let ore_led = pull_r[[y, x]] > self.site_score[[y, x]].max(0.0);
            // past the soft cap only miners still sail
            if self.peoples.settlements.len() >= self.max_settlements && !ore_led {
                continue;
            }
            let migrants = ((ppop as f64 * self.rng.gen_range(0.08..0.14)) as i64).max(40);
            self.peoples.settlements[pi].pop = (ppop - migrants).max(60);
            // colonists carry both their tongue and their banner (ADR-0018)
            let pid = self.peoples.settlements[pi].people;
            let rid = self.peoples.settlements[pi].realm;
            let (idx, eid) = self.found_settlement(y, x, migrants, pid, rid);
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
                ids: smallvec![eid],
                x: x as i64,
                y: y as i64,
                ..Default::default()
            });
        }
        (events, founded)
    }

    /// Raise a new settlement at (y, x): coin a name in the founding
    /// people's style, list its goods, size its land, and wire it into
    /// the trade web. Shared by colonists and rush camps alike. The new
    /// town speaks `pid`'s tongue and flies `rid`'s banner (ADR-0018).
    fn found_settlement(
        &mut self,
        y: usize,
        x: usize,
        migrants: i64,
        pid: PeopleId,
        rid: RealmId,
    ) -> (usize, EntityId) {
        let style = if !self.peoples.peoples.is_empty() {
            self.peoples.peoples[pid.0].style.clone()
        } else {
            "hellenic".to_string()
        };
        // M62 — the name's tail is the tongue's generic for the ground,
        // when the tongue has one; otherwise a plain coined name.
        let coined = naming::coin_for_landform(
            &mut self.rng,
            &style,
            self.fields.landform[[y, x]],
            &mut self.taken,
        )
        .unwrap_or_else(|| naming::coin(&mut self.rng, &style, &mut self.taken));
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
            people: pid,
            realm: rid,
            namer: pid,
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
            drift: 0.0,
            drift_to: None,
            exonym: None,
            quarry: "",
            rebuild_until: 0,
            rebuild_peak: 0,
            harbor_dmg: 0.0,
            harbor_until: 0,
        };
        trade::goods_for(&mut s, &self.deposits, &self.fields.fertility, &self.fields.rock);
        let mdc = self
            .peoples.societies
            .get(pid.0)
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
        let eid = {
            let t = &self.peoples.settlements[idx];
            self.chronicle.registry
                .add(EntityKind::Settlement, &t.name, self.month, Some(t.people), t.x, t.y)
        };
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
        (idx, eid)
    }

    /// The rush: a rich seam, known but unworked, calls chancers on its
    /// own. Where colonists weigh soil against distance, rushers weigh
    /// only the price of metal — a camp springs up hard by the diggings,
    /// peopled from the nearest town. This is the channel that reaches
    /// ore struck by far ventures in country no crowded parent would
    /// ever pick: found metal must reach the market, not rust in the hills.
    pub(crate) fn try_rush_camps(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
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
            if !d.live() {
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
            let pid = self.peoples.settlements[src].people;
            let rid = self.peoples.settlements[src].realm;
            let sname = self.peoples.settlements[src].name.clone();
            let (idx, eid) = self.found_settlement(y, x, migrants, pid, rid);
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
                ids: smallvec![eid],
                x: x as i64,
                y: y as i64,
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
    pub(crate) fn patina_pass(&mut self, month_abs: i64) -> Vec<Event> {
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
                    self.peoples.peoples[s.people.idx()].people.clone(),
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
            // the winning crown's own folk carry the tale (ADR-0018)
            let win_people = self.peoples.realms[winner.0].people;
            let people = self.peoples.peoples[win_people.idx()].people.clone();
            let eid =
                self.chronicle.registry.add(EntityKind::Feature, &name, m, Some(win_people), x, y);
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
            // the conquering crown renames in ITS people's tongue — the
            // town's own folk keep speaking theirs (ADR-0018)
            let to = self.peoples.realms[self.peoples.settlements[i].realm.0].people;
            // a people does not rename what already speaks its tongue,
            // and a place carries at most two former names (bounded strata)
            if self.peoples.settlements[i].namer == to || self.peoples.settlements[i].formerly.len() >= 2 {
                continue;
            }
            if self.rng.gen::<f64>() >= 0.35 {
                continue;
            }
            let style = self.peoples.peoples[to.0].style.clone();
            // M62 — the conqueror renames in its tongue, but the ground
            // stays the ground: the new name keeps the landform generic.
            let (sy, sx) = (
                self.peoples.settlements[i].y as usize,
                self.peoples.settlements[i].x as usize,
            );
            let coined = naming::coin_for_landform(
                &mut self.rng,
                &style,
                self.fields.landform[[sy, sx]],
                &mut self.taken,
            )
            .unwrap_or_else(|| naming::coin(&mut self.rng, &style, &mut self.taken));
            let old = self.peoples.settlements[i].name.clone();
            let people = self.peoples.peoples[to.0].people.clone();
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
        let mut counts = vec![0usize; self.peoples.peoples.len()];
        for s in &self.peoples.settlements {
            counts[s.people.idx()] += 1;
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
            if counts[s.people.idx()] <= 1 {
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
        let cause = {
            let dead = &self.peoples.settlements[i];
            if dead.goods.is_empty() && dead.exports.is_none() {
                "mines"
            } else if dead.fort > 0 && dead.pop * 3 < dead.peak {
                "war"
            } else if dead.failing {
                "decline" // the slow kind: ruin_why's default reading
            } else {
                "famine"
            }
        };
        let (dead, ruin_name, rid) = self.fell_settlement(i, month_abs, cause);
        let why = patina::ruin_why(cause);
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

    /// The one kill path (M24): closes the registry row, raises the
    /// ruin, cuts the dead town's routes, re-knits the web and
    /// recomputes territory. Every way a town dies — the slow
    /// abandonment of M9 or the sudden fall of a disaster — walks
    /// through here, so the ruin ledger and the chronicle can never
    /// disagree. Returns the dead row, the ruin's name and its registry
    /// id; the caller composes the beat.
    fn fell_settlement(
        &mut self,
        i: usize,
        month_abs: i64,
        cause: &str,
    ) -> (crate::settlements::Settlement, String, EntityId) {
        let dead = self.peoples.settlements[i].clone();
        let why = patina::ruin_why(cause);
        let reason = match cause {
            "quake" | "ash" => format!("fell — {}", why),
            _ => format!("abandoned — {}", why),
        };
        let ent = self.chronicle.registry.find_alive(EntityKind::Settlement, dead.x, dead.y);
        if let Some(id) = ent {
            self.chronicle.registry.close(id, month_abs, &reason);
        }
        let ruin_name = format!("Ruins of {}", dead.name);
        let rid = self
            .chronicle.registry
            .add(EntityKind::Ruin, &ruin_name, month_abs, Some(dead.people), dead.x, dead.y);
        self.ruins.push(Ruin {
            name: ruin_name.clone(),
            of: dead.name.clone(),
            x: dead.x,
            y: dead.y,
            since: month_abs,
            why: why.to_string(),
            people: self.peoples.peoples[dead.people.idx()].people.clone(),
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
        (dead, ruin_name, rid)
    }

    /// M24 — the shaking reaches the towns. Damage is a pure function
    /// of magnitude and distance (no RNG: the seismic ledger stays the
    /// cross-runtime replay identity — effects read it, never write
    /// it); a tenth of a town lost opens a rebuild arc; a great shock
    /// close under the walls fells the town through the one kill path,
    /// and every mark gets its chronicle beat.
    pub fn quake_effects(&mut self, from: usize, evs: &mut Vec<Event>) {
        let shocks: Vec<(i64, i64, i64, f64)> = self.seismic.log[from..]
            .iter()
            .map(|q| (q.m, q.y as i64, q.x as i64, q.mag))
            .collect();
        for (m, qy, qx, mag) in shocks {
            let r_felt = 2.2 * (mag - 4.5).max(0.0);
            let r_fell = (mag - 6.8).max(0.0) * 1.5;
            let dmg_center = (0.08 * (mag - 5.0)).clamp(0.0, 0.55);
            self.disaster_strike(
                m, qy, qx, r_felt, r_fell, dmg_center, "quake",
                &format!("magnitude {:.1}", mag),
                evs,
            );
        }
    }

    /// M24 — the mountain reaches the towns. Burn-and-bury damage,
    /// moved here from the volcanism pass so every mark opens its arc
    /// and the buried get their ruin and their beat, through the one
    /// kill path. Deterministic in the eruption log alone.
    pub fn eruption_effects(&mut self, from: usize, evs: &mut Vec<Event>) {
        let blows: Vec<(i64, i64, i64, f64)> = self.volcanism.log[from..]
            .iter()
            .map(|e| (e.m, e.y as i64, e.x as i64, e.vei))
            .collect();
        for (m, vy, vx, vei) in blows {
            let r_burn = 1.0 + 0.55 * vei;
            let r_fell = if vei >= 4.8 { 0.75 * r_burn } else { 0.0 };
            let dmg_center = (0.06 * (vei - 1.0)).clamp(0.0, 0.5);
            self.disaster_strike(
                m, vy, vx, r_burn, r_fell, dmg_center, "ash",
                &format!("VEI {:.1}", vei),
                evs,
            );
        }
    }

    /// The shared strike: linear-falloff damage inside `r_felt`, rebuild
    /// arcs on a tenth lost, at most one town felled inside `r_fell`
    /// (nearest wins), and one beat per strike — the fall if there is
    /// one, else the worst of the felt. Guards mirror the M9 floor:
    /// never a people's last hearth, never below seven towns, never a
    /// besieged town (sieges resolve their own endings).
    #[allow(clippy::too_many_arguments)]
    fn disaster_strike(
        &mut self,
        m: i64,
        cy: i64,
        cx: i64,
        r_felt: f64,
        r_fell: f64,
        dmg_center: f64,
        cause: &str,
        size: &str,
        evs: &mut Vec<Event>,
    ) {
        if r_felt <= 0.0 || dmg_center <= 0.0 {
            return;
        }
        let mut counts = vec![0usize; self.peoples.peoples.len()];
        for s in &self.peoples.settlements {
            counts[s.people.idx()] += 1;
        }
        let besieged: HashSet<SettlementId> = self
            .politics
            .wars
            .iter()
            .filter_map(|w| w.siege.as_ref().map(|sg| sg.target))
            .collect();
        let n_towns = self.peoples.settlements.len();
        let mut hit: Option<(String, i64, f64)> = None; // name, lost, dist
        let mut felled: Option<(usize, f64)> = None; // index, dist
        for (i, s) in self.peoples.settlements.iter_mut().enumerate() {
            if s.pop <= 0 {
                continue;
            }
            let d = (((s.y - cy).pow(2) + (s.x - cx).pow(2)) as f64).sqrt();
            if d > r_felt {
                continue;
            }
            let before = s.pop;
            // Square-root falloff: near-field intensity decays slowly
            // (Mercalli-like), so mid-strength shocks still bite hard
            // within half the felt radius instead of only at the pin.
            let dmg = dmg_center * (1.0 - d / r_felt.max(1e-9)).max(0.0).sqrt();
            s.pop = ((s.pop as f64) * (1.0 - dmg)).round().max(20.0) as i64;
            let lost = before - s.pop;
            // A twelfth or worse lost: the town will rebuild (M24 arc).
            // The target is the pre-disaster head-count capped just
            // under carrying capacity — a town whose k has drifted below
            // its old strength rebuilds to what the land now bears, not
            // to a number the crops can no longer feed.
            let target = before.min((s.k * 0.95).round() as i64);
            if lost * 12 >= before && s.pop < target {
                if target > s.rebuild_peak {
                    s.rebuild_peak = target;
                }
                s.rebuild_until = m + settlements::REBUILD_WINDOW;
            }
            if lost > 0 && hit.as_ref().map_or(true, |h| lost > h.1) {
                hit = Some((s.name.clone(), lost, d));
            }
            // A zero radius means "cannot fell", not "fell a town exactly at
            // the event pin". Storm landfalls are pinned to their harbour,
            // so accepting d == r_fell == 0 bypassed the entire M79
            // exceptionality verdict and caused every intense direct hit to
            // become a permanent ruin.
            if within_fell_radius(d, r_fell)
                && n_towns > 6
                && counts[s.people.idx()] > 1
                && !besieged.contains(&s.id)
                && felled.as_ref().map_or(true, |f| d < f.1)
            {
                felled = Some((i, d));
            }
        }
        if let Some((i, _)) = felled {
            let (dead, ruin_name, rid) = self.fell_settlement(i, m, cause);
            let text = if cause == "storm" {
                // M79 — the coast the sea kept.
                format!(
                    "The water goes clean over {} in the night — {} — and when it draws back there is nothing to rebuild on. Travellers call the place the {}.",
                    dead.name, size, ruin_name
                )
            } else if cause == "ash" {
                format!(
                    "Fire stands over the mountain and {} is gone by nightfall — {}; ash and stone take street and field alike. Travellers call the place the {}.",
                    dead.name, size, ruin_name
                )
            } else {
                format!(
                    "The earth breaks under {} — a great shaking, {} — and the town falls in a single morning. Travellers call the place the {}.",
                    dead.name, size, ruin_name
                )
            };
            evs.push(Event {
                m,
                s: dead.name.clone(),
                k: match cause {
                    "ash" => EventKind::Eruption,
                    "storm" => EventKind::Disaster,
                    _ => EventKind::Quake,
                },
                text,
                ids: smallvec![rid],
                x: dead.x,
                y: dead.y,
                ..Default::default()
            });
        } else if let Some((name, lost, _)) = hit {
            // the felt beat: only marks that drew real blood get told,
            // so the chronicle speaks of hard years, not of tremors
            if lost >= 25 {
                let text = if cause == "storm" {
                    format!(
                        "The sea rises on {} — {}; the low streets go under, the nets and the winter's salt with them, and {} souls are not found.",
                        name, size, lost
                    )
                } else if cause == "ash" {
                    format!(
                        "The mountain above {} throws fire — {}; ash falls for days, roofs are shovelled like snow, and {} souls are lost to the burning.",
                        name, size, lost
                    )
                } else {
                    format!(
                        "The earth heaves under {} — {}; walls crack, bells ring of themselves, and {} souls are pulled from the stones.",
                        name, size, lost
                    )
                };
                evs.push(Event {
                    m,
                    s: name,
                    k: match cause {
                        "ash" => EventKind::Eruption,
                        "storm" => EventKind::Disaster,
                        _ => EventKind::Quake,
                    },
                    text,
                    x: cx,
                    y: cy,
                    ..Default::default()
                });
            }
        }
    }

    /// M79 — the coasts remember. Every month, the year's landfall ledger
    /// (`storms::landfalls`, pure in seed × year) is read for strikes due
    /// now: quays and boats take a wound scaled by the intensity the storm
    /// arrived at and how far the town sits from the crossing, the water
    /// takes its souls through the one kill path, and the chronicle hears
    /// of it. Repairs run first, so a strike's own month is the dip.
    ///
    /// Nothing is stored between months but the wound itself: the ledger
    /// is re-derived, never advanced, so a replay lands on the same coast.
    pub fn storm_effects(&mut self, month_abs: i64, evs: &mut Vec<Event>) {
        // --- the yards work: last month's wound is a month older ---------
        for s in &mut self.peoples.settlements {
            if s.harbor_dmg > 0.0 {
                s.harbor_dmg = crate::util::round3(s.harbor_dmg * settlements::HARBOR_REPAIR);
                if s.harbor_dmg < settlements::HARBOR_CLEAR || month_abs >= s.harbor_until {
                    s.harbor_dmg = 0.0;
                    s.harbor_until = 0;
                }
            }
        }

        // --- this month's strikes ----------------------------------------
        let year = month_abs.div_euclid(12);
        if self.storm_year != year {
            if self.storm_clim.is_none() {
                self.storm_clim = Some(Box::new(crate::storms::StormClimatology::new(
                    &self.fields.height,
                    &self.fields.tmean,
                    &self.fields.tamp,
                )));
            }
            let seed = self.seed;
            let drift = self.year_forcing(year);
            let clim = self.storm_clim.as_ref().expect("climatology solved");
            // M84 — the year's corridor is the drifted year's corridor.
            let next = clim.landfalls(seed, year, &self.fields.height, drift);
            // Only the immediately previous year can still owe a January.
            self.storm_prev = if self.storm_year == year - 1 {
                std::mem::take(&mut self.storm_now)
            } else {
                Vec::new()
            };
            self.storm_now = next;
            self.storm_year = year;
        }
        let due: Vec<crate::storms::Landfall> = self
            .storm_prev
            .iter()
            .chain(self.storm_now.iter())
            .filter(|l| l.month == month_abs)
            .copied()
            .collect();
        if due.is_empty() {
            return;
        }

        for lf in due {
            // How hard it came ashore, 0..1 above the telling bar.
            let bite = ((lf.inten - crate::storms::LANDFALL_TELL_MIN)
                / (1.0 - crate::storms::LANDFALL_TELL_MIN))
                .clamp(0.0, 1.0);
            let reach = crate::storms::LANDFALL_REACH;

            // The harbours: quays, moles and boats, by distance from the
            // crossing. Only coastal towns own a harbour to lose.
            let mut worst: Option<(usize, f64)> = None;
            for (i, s) in self.peoples.settlements.iter_mut().enumerate() {
                if !s.coastal || s.pop <= 0 {
                    continue;
                }
                let d = (((s.y - lf.y as i64).pow(2) + (s.x - lf.x as i64).pow(2)) as f64).sqrt();
                if d > reach {
                    continue;
                }
                let fall = (1.0 - d / reach).max(0.0);
                let dmg = settlements::HARBOR_DMG_MAX * bite * fall;
                if dmg < settlements::HARBOR_MARK_MIN {
                    continue;
                }
                s.harbor_dmg = crate::util::round3(
                    (s.harbor_dmg + dmg).min(settlements::HARBOR_DMG_MAX),
                );
                s.harbor_until = month_abs + settlements::HARBOR_WINDOW;
                self.storm_marks.push((month_abs, s.id, s.harbor_dmg));
                self.storm_bites.push((month_abs, s.id, dmg));
                if worst.as_ref().map_or(true, |w| dmg > w.1) {
                    worst = Some((i, dmg));
                }
            }
            if self.storm_marks.len() > 4096 {
                let cut = self.storm_marks.len() - 4096;
                self.storm_marks.drain(..cut);
            }
            if self.storm_bites.len() > 4096 {
                let cut = self.storm_bites.len() - 4096;
                self.storm_bites.drain(..cut);
            }
            if let Some((i, dmg)) = worst {
                if dmg >= settlements::HARBOR_TELL_MIN {
                    let s = &self.peoples.settlements[i];
                    let kind = if lf.trop { "a great warm-sea storm" } else { "a winter gale" };
                    let text = format!(
                        "The sea comes over the wall at {} — {} out of the deep water; the mole is breached, boats are thrown up the strand, and the harbour will take {} months of hammering before it works again.",
                        s.name, kind, settlements::HARBOR_WINDOW
                    );
                    let (name, sx, sy) = (s.name.clone(), s.x, s.y);
                    evs.push(Event {
                        m: month_abs,
                        s: name,
                        k: EventKind::Disaster,
                        text,
                        x: sx,
                        y: sy,
                        ..Default::default()
                    });
                }
            }

            // The souls: the same kill path every disaster uses, so
            // rebuild arcs, ruins and the chronicle behave as they do
            // under quake and ash.
            //
            // A gale is not an earthquake. Storms come to the same coast
            // every few years, so the per-strike bite has to be the size
            // of a bad winter — a fraction of a percent to two percent of
            // a town, squared in the intensity so only the rare monster
            // is felt at all. Permanent felling is rarer still: the town
            // must have a substantial local strike history, and this hit's
            // empirical severe-hit return interval (hits at least this
            // damaging, including the present one) must be ≥100 years.
            // `storm_bites` is written at the mechanism before this test;
            // its settlement id makes the history local, while event damage
            // makes the comparison independent of any unrepaired old wound.
            // Ordinary damage, rebuilding and harbour recovery do not read
            // this verdict and therefore remain exactly as before.
            let fell_evidence = worst
                .as_ref()
                .map(|&(i, dmg)| {
                    let s = &self.peoples.settlements[i];
                    let age = month_abs.saturating_sub(s.born);
                    let local = self
                        .storm_bites
                        .iter()
                        .filter(|&&(m, sid, _)| m <= month_abs && sid == s.id)
                        .count();
                    let exceed = self
                        .storm_bites
                        .iter()
                        .filter(|&&(m, sid, wound)| {
                            m <= month_abs && sid == s.id && wound + 1e-9 >= dmg
                        })
                        .count();
                    let eligible = bite > 0.97
                        && age >= 1200
                        && local >= 12
                        && (exceed as i64) * 1200 <= age;
                    (s.id, dmg, age, local, exceed, eligible)
                });
            let fell_radius = if fell_evidence.map(|e| e.5).unwrap_or(false) { 1.0 } else { 0.0 };
            #[cfg(not(target_arch = "wasm32"))]
            let ruins_before = self.ruins.len();
            self.disaster_strike(
                month_abs,
                lf.y as i64,
                lf.x as i64,
                reach * (0.5 + 0.5 * bite),
                fell_radius,
                0.02 * bite * bite,
                "storm",
                if lf.trop { "a storm the old men had no name for" } else { "the worst gale in living memory" },
                evs,
            );
            #[cfg(not(target_arch = "wasm32"))]
            if let Some((settlement, damage, age, local, exceed, eligible)) = fell_evidence {
                self.storm_fell_probe.push(StormFellProbe {
                    month: month_abs,
                    settlement,
                    bite,
                    damage,
                    age,
                    local,
                    exceed,
                    eligible,
                    felled: self.ruins.len() > ruins_before,
                });
            }
        }
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
                    .peoples.settlements
                    .iter()
                    .find(|s| s.id == self.routes[i].a)
                    .map(|s| s.name.clone());
                let bn = self
                    .peoples.settlements
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
    pub(crate) fn veil_pass(&mut self, events: &mut [Event], from: usize) {
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

    /// Advance the world by `months`. The month itself is the ordered
    /// system list in `systems.rs` (E11.4) — this is only the driver:
    /// wind the clock, run what is due, ship the flags to the wire.
    pub fn tick(&mut self, months: i64) -> (Vec<Event>, bool, bool) {
        let months = months.clamp(1, 240).max(1);
        let mut sink = EventSink::new();
        let mut sidx = std::collections::HashMap::new();
        for _ in 0..months {
            sink.begin_month();
            self.month += 1;
            let month = self.month;
            let mut ctx = SimCtx { world: self, sidx };
            for sys in SYSTEMS {
                if sys.cadence().due(month) {
                    sys.run(&mut ctx, &mut sink);
                }
            }
            sidx = ctx.sidx;
        }
        // change tracking for the wire (E4.5): foundings reship routes,
        // strikes and dead mines reship the mineral ledger
        if sink.founded {
            self.dirty.mark(Dirty::ROUTES);
        }
        if sink.deposits_changed {
            self.dirty.mark(Dirty::DEPOSITS);
        }
        // the full log is the chronicle's memory — the sifter reads all of it,
        // and the client pages it with events_range (M6)
        self.chronicle.events.extend(sink.events.iter().cloned());
        (sink.events, sink.founded, sink.deposits_changed)
    }

    /// Native-only per-system profiler (E11.7): the same driver as `tick`,
    /// but each system's wall time and call count accumulate into slices
    /// parallel to `systems::SYSTEMS`. Timing reads touch neither the RNG
    /// nor any state, so a profiled run stays byte-identical to a plain one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn tick_profiled(
        &mut self,
        months: i64,
        totals: &mut [f64],
        calls: &mut [u64],
    ) -> (Vec<Event>, bool, bool) {
        assert_eq!(totals.len(), SYSTEMS.len());
        assert_eq!(calls.len(), SYSTEMS.len());
        let months = months.clamp(1, 240).max(1);
        let mut sink = EventSink::new();
        let mut sidx = std::collections::HashMap::new();
        for _ in 0..months {
            sink.begin_month();
            self.month += 1;
            let month = self.month;
            let mut ctx = SimCtx { world: self, sidx };
            for (i, sys) in SYSTEMS.iter().enumerate() {
                if sys.cadence().due(month) {
                    let t = std::time::Instant::now();
                    sys.run(&mut ctx, &mut sink);
                    totals[i] += t.elapsed().as_secs_f64();
                    calls[i] += 1;
                }
            }
            sidx = ctx.sidx;
        }
        if sink.founded {
            self.dirty.mark(Dirty::ROUTES);
        }
        if sink.deposits_changed {
            self.dirty.mark(Dirty::DEPOSITS);
        }
        self.chronicle.events.extend(sink.events.iter().cloned());
        (sink.events, sink.founded, sink.deposits_changed)
    }


    /// M27 — the deep-earth identity line: every Year-1 layer's hash,
    /// labeled, so a cross-runtime divergence names the layer it lives
    /// in. The ADR-0025 replay family (plates, seismic, sealevel) is
    /// IEEE-exact by construction; rock, volcanism and landform sit
    /// downstream of transcendental terrain and are printed so the
    /// wasm-replay leg can *measure* rather than assume their fate.
    /// M59 measured that fate for raw f64: the sediment ledger's full
    /// bit-width hash diverges native↔wasm by heightfield ulps (geo.rs
    /// runs on host libm) while the wire-precision world is identical —
    /// so the line carries the ledger's integer-robust footprint
    /// (mouth cells + delta land) and the raw books stay under native
    /// determinism in `hash_state`.
    pub fn earth_hash_line(&self) -> String {
        format!(
            "plates={:016x} rock={:016x} seismic={:016x} volcanism={:016x} sealevel={:016x} landform={:016x} ice={:016x} permafrost={:016x} tides={:016x} coast={:016x} sediment={:016x}",
            self.plates.hash(),
            crate::util::fnv1a64(self.fields.rock.as_slice().expect("rock grid is contiguous")),
            self.seismic.hash(),
            self.volcanism.hash(),
            self.sealevel.hash(),
            crate::landform::hash(&self.fields.landform),
            self.ice.hash(),
            self.permafrost.hash(),
            self.tides.hash(),
            self.coastform.hash(&self.fields.coastform),
            self.sediment.footprint_hash(),
        )
    }

    /// The second reading of the month's events (M6): any entry whose
    /// subject the registry knows gets its ids back-filled, any entry
    /// without a map anchor inherits its subject's, and the loudest
    /// entries pass into legend (M6.9). Purely derived — no rng, no
    /// state change beyond the event fields and mention counters.
    pub(crate) fn resolve_events(&mut self, from: usize, events: &mut [Event]) {
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

#[cfg(test)]
mod storm_felling_tests {
    use super::within_fell_radius;

    #[test]
    fn zero_radius_can_never_fell_a_settlement() {
        assert!(!within_fell_radius(0.0, 0.0));
        assert!(!within_fell_radius(f64::MIN_POSITIVE, 0.0));
        assert!(!within_fell_radius(0.0, -1.0));
        assert!(within_fell_radius(0.0, 1.0));
        assert!(within_fell_radius(1.0, 1.0));
        assert!(!within_fell_radius(1.000_001, 1.0));
    }
}


// ---------------------------------------------------------------- bands

// ===== M80 — the drought laws, hosted here =====
// E11.8: `drought.rs` is a leaf (it may not import `crate::world`), so
// the `impl World` half of M80 lives with the type it extends. The
// ledger, the lattice and the constants stay in `drought.rs`.
impl World {
    /// M80 follow-up — this world's renormalization of the accumulated
    /// sum, measured against its own sky rather than assumed from
    /// independent years (see `drought::NORM`). Falls back to the
    /// independence baseline on an uncalibrated ledger.
    pub fn drought_norm(&self) -> f64 {
        if self.droughts.norm > 0.0 {
            self.droughts.norm
        } else {
            NORM
        }
    }

    /// Measure the scalar: over a fixed deterministic sample of the
    /// sky's own prehistory, compare the spread of the weighted sum with
    /// the spread of the single standardized year it replaces, and
    /// return the factor that makes them equal. That equality *is* the
    /// M80 contract — memory changes when the ground fails, never how
    /// often a threshold is crossed. Pure in seed × size: the sample
    /// grid, the year window and the land test are all fixed.
    pub(crate) fn calibrate_drought_norm(&self) -> f64 {
        let rows = self.fields.tmean.dim().0;
        let cols = self.fields.tmean.dim().1;
        let (mut zn, mut z1, mut z2) = (0.0f64, 0.0f64, 0.0f64);
        let (mut sn, mut s1, mut s2) = (0.0f64, 0.0f64, 0.0f64);
        for y in (0..rows).step_by(CAL_STRIDE) {
            let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
            for x in (0..cols).step_by(CAL_STRIDE) {
                if self.fields.height[[y, x]] < 0.0 {
                    continue;
                }
                // The window runs over negative years — the sky exists
                // before the founding — so the scalar never shifts as the
                // world ages.
                let mut ring = [0.0f64; MEMO_YEARS];
                for t in 0..(CAL_YEARS + MEMO_YEARS as i64) {
                    let yr = -(CAL_YEARS + MEMO_YEARS as i64) + t;
                    // Prehistory: the drift is zero by law there (M83), so
                    // this is the unshifted rain law — and since M84 the
                    // norm stays what it was: a property of the unforced
                    // sky the walk wanders around.
                    let (_, dp) = climate::year_anomaly_at(
                        self.variability(),
                        rows,
                        x,
                        y,
                        yr,
                        self.year_osc(yr),
                        self.year_forcing(yr),
                    );
                    let z = dp / sigma;
                    // newest first
                    ring.rotate_right(1);
                    ring[0] = z;
                    if t < MEMO_YEARS as i64 - 1 {
                        continue; // window not yet full
                    }
                    let mut acc = 0.0f64;
                    let mut w = 1.0f64;
                    for k in 0..MEMO_YEARS {
                        acc += w * ring[k];
                        w *= MEM;
                    }
                    zn += 1.0;
                    z1 += z;
                    z2 += z * z;
                    sn += 1.0;
                    s1 += acc;
                    s2 += acc * acc;
                }
            }
        }
        if zn < 2.0 || sn < 2.0 {
            return NORM;
        }
        let zv = (z2 / zn - (z1 / zn) * (z1 / zn)).max(1e-12);
        let sv = (s2 / sn - (s1 / sn) * (s1 / sn)).max(1e-12);
        (zv / sv).sqrt()
    }

    /// The drought index at one cell in one year — the law itself
    /// (see the module header). Pure in seed × cell × year.
    pub fn drought_index(&self, year: i64, y: usize, x: usize) -> f64 {
        let rows = self.fields.tmean.dim().0;
        let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
        let mut acc = 0.0;
        let mut w = 1.0;
        for k in 0..MEMO_YEARS as i64 {
            let yr = year - k;
            let (_, dp) =
                // M84 — the belt rides `dp`: an age that walks the rain
                // belts off a flank *is* that flank's drought, and the
                // ledger must read the same sky the harvests felt.
                climate::year_anomaly_at(
                    self.variability(), rows, x, y, yr, self.year_osc(yr), self.year_forcing(yr));
            acc += w * dp / sigma;
            w *= MEM;
        }
        acc * self.drought_norm()
    }


    /// The single year's standardized anomaly, kept public because the
    /// harness and the explain layer both want to show the year apart
    /// from the memory it lands on.
    pub fn year_spi(&self, year: i64, y: usize, x: usize) -> f64 {
        let rows = self.fields.tmean.dim().0;
        let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
        self.year_rain_anomaly_site(year, y, x) / sigma
    }

    /// M92 — the monsoon-strength index at one cell in one year (1.0 =
    /// a normal year): the composed sky's rain anomaly read against the
    /// gale-grade monsoon scale, with the dawn's own `pamp` at the cell
    /// as the lean. A riverine paddy (`catchment` = true) reads the
    /// basin's sky — the exact gaussian the M81 floods read — because
    /// the pulse that fills it is the monsoon over the whole catchment.
    /// Pure in seed × cell × year either way.
    pub fn monsoon_index(&self, year: i64, y: usize, x: usize, catchment: bool) -> f64 {
        let (rows, cols) = self.fields.tmean.dim();
        let lean = self.fields.pamp[[y, x]] as f64;
        if catchment {
            climate::monsoon_index_catchment(
                self.variability(),
                rows,
                cols,
                x,
                y,
                year,
                self.year_osc(year),
                self.year_forcing(year),
                lean,
            )
        } else {
            climate::monsoon_index(
                self.variability(),
                rows,
                x,
                y,
                year,
                self.year_osc(year),
                self.year_forcing(year),
                lean,
            )
        }
    }

    /// M90 — fields at the edge: the year's composed forcing walks the
    /// margin ledger and the crops grid answers, before any town
    /// harvests. Every flip is a solved threshold crossing — the pass
    /// draws no die, and re-applying the same forcing is a no-op, so
    /// `fields.crops` at any month is a pure function of seed × the
    /// forcing history. Opens and failures gather into chronicle
    /// entries: a town within reach claims its own margin; a year that
    /// moves the world without a witness speaks with the world's voice.
    pub(crate) fn fields_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 0 {
            return events;
        }
        let year = month_abs.div_euclid(12);
        let f = self.year_forcing(year);
        if f == self.fields_sky {
            return events;
        }
        let wild = agriculture::CropPackage::Wildland.code();
        // (x, y) of every cell that opened / failed this pass.
        let mut opened: Vec<(i64, i64)> = Vec::new();
        let mut shut: Vec<(i64, i64)> = Vec::new();
        for c in &self.fields_ledger.cells {
            let want = c.code_at(f);
            let cur = self.fields.crops[[c.y as usize, c.x as usize]];
            if want == cur {
                continue;
            }
            self.fields.crops[[c.y as usize, c.x as usize]] = want;
            if cur == wild {
                opened.push((c.x as i64, c.y as i64));
            } else if want == wild {
                shut.push((c.x as i64, c.y as i64));
            }
        }
        self.fields_sky = f;
        if opened.is_empty() && shut.is_empty() {
            return events;
        }

        // The chronicle speaks where the change gathers: each living
        // town within reach claims its nearest flips; a year that moved
        // enough ground speaks the world row too.
        let setts = &self.peoples.settlements;
        let nearest = |x: i64, y: i64| -> Option<usize> {
            let r2 = FIELDS_TOWN_REACH * FIELDS_TOWN_REACH;
            let mut best = None;
            let mut bd = r2 + 1.0;
            for (i, s) in setts.iter().enumerate() {
                if s.pop <= 0 {
                    continue;
                }
                let d = ((s.x - x) as f64).powi(2) + ((s.y - y) as f64).powi(2);
                if d <= r2 && d < bd {
                    bd = d;
                    best = Some(i);
                }
            }
            best
        };
        let mut town_open: BTreeMap<usize, usize> = BTreeMap::new();
        let mut town_shut: BTreeMap<usize, usize> = BTreeMap::new();
        for &(x, y) in &opened {
            if let Some(i) = nearest(x, y) {
                *town_open.entry(i).or_insert(0) += 1;
            }
        }
        for &(x, y) in &shut {
            if let Some(i) = nearest(x, y) {
                *town_shut.entry(i).or_insert(0) += 1;
            }
        }
        for (kind, tally) in [
            (EventKind::Clearing, &town_open),
            (EventKind::Abandon, &town_shut),
        ] {
            for (&si, &n) in tally {
                if n < FIELDS_EVENT_MIN {
                    continue;
                }
                let s = &self.peoples.settlements[si];
                let ids: EventIds = self
                    .chronicle
                    .registry
                    .find_alive(EntityKind::Settlement, s.x, s.y)
                    .into_iter()
                    .collect();
                let text = if kind == EventKind::Clearing {
                    format!(
                        "The sky turns kind and the ploughs walk uphill: {} marginal fields open above {}.",
                        n, s.name
                    )
                } else {
                    format!(
                        "The margin fails above {}: {} upland fields go back to the wild.",
                        s.name, n
                    )
                };
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: kind,
                    text,
                    ids,
                    x: s.x,
                    y: s.y,
                    ..Default::default()
                });
            }
        }
        let world_ids: EventIds = self
            .chronicle
            .registry
            .find_kind(EntityKind::World, &self.world_name)
            .into_iter()
            .collect();
        if opened.len() >= FIELDS_WORLD_MIN {
            events.push(Event {
                m: month_abs,
                s: self.world_name.clone(),
                k: EventKind::Clearing,
                text: format!(
                    "Across {} the long warmth opens {} fields at the edge of the plough-lands.",
                    self.world_name,
                    opened.len()
                ),
                ids: world_ids.clone(),
                ..Default::default()
            });
        }
        if shut.len() >= FIELDS_WORLD_MIN {
            events.push(Event {
                m: month_abs,
                s: self.world_name.clone(),
                k: EventKind::Abandon,
                text: format!(
                    "The cold takes {} marginal fields across {}, and the wild walks back down.",
                    shut.len(),
                    self.world_name
                ),
                ids: world_ids,
                ..Default::default()
            });
        }
        self.fields_log.push(FieldsRow {
            year,
            f,
            opened: opened.len(),
            shut: shut.len(),
            open_now: self.fields_ledger.opened_at(f),
            farm_now: self.fields_ledger.farmed_at(f),
        });
        events
    }

    /// M91 — the ice remembers time: the year's composed forcing walks
    /// the ice-margin ledger and the GLACIER flag answers — advance in
    /// the cold ages, retreat in the optima. Every flip is a solved
    /// threshold crossing; the pass draws no die, re-applying the same
    /// forcing is a no-op, and the base terrain is never touched — the
    /// ice walks a flag, never the ground. Rows land in `ice_log`: the
    /// atlas's extent snapshots.
    pub(crate) fn ice_pass(&mut self, month_abs: i64) {
        if month_abs.rem_euclid(12) != 0 {
            return;
        }
        let year = month_abs.div_euclid(12);
        let f = self.year_forcing(year);
        if f == self.ice_sky {
            return;
        }
        let bit = CellFlags::GLACIER.bits();
        let mut advanced = 0usize;
        let mut retreated = 0usize;
        for c in &self.ice_ledger.cells {
            let want = c.frozen_at(f);
            let cur = self.fields.flags[[c.y as usize, c.x as usize]] & bit != 0;
            if want == cur {
                continue;
            }
            if want {
                self.fields.flags[[c.y as usize, c.x as usize]] |= bit;
                advanced += 1;
            } else {
                self.fields.flags[[c.y as usize, c.x as usize]] &= !bit;
                retreated += 1;
            }
        }
        self.ice_sky = f;
        if advanced == 0 && retreated == 0 {
            return;
        }
        self.ice_log.push(IceRow {
            year,
            f,
            extent: self.ice_ledger.extent_at(f),
            advanced,
            retreated,
        });
    }

    /// M86/M87/M88 — the year the sky turns: if the schedule dates an
    /// onset or a release to this year, the chronicle speaks it —
    /// exactly once, in the year's first month, before any harvest
    /// reads the changed sky. The subject is the age's own christened
    /// name (M88), so onset and release bind to one remembered thing;
    /// the world's entity id owns both entries: an age belongs to
    /// everyone. A winter speaks as `Age` (fortune falling), an optimum
    /// as `Optimum` (fortune rising).
    pub(crate) fn ages_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 0 {
            return events;
        }
        let year = month_abs.div_euclid(12);
        let subject = self
            .chronicle
            .registry
            .find_kind(EntityKind::World, &self.world_name);
        if let Some(i) = self.ages.arcs().iter().position(|a| a.onset == year) {
            let a = self.ages.arcs()[i];
            let name = self.ages.name(i).to_string();
            let (k, text) = if a.warm {
                (
                    EventKind::Optimum,
                    format!(
                        "The sky turns kind over {}: the summers lengthen, the snows draw back, and the chroniclers set down the first of {}.",
                        self.world_name, name
                    ),
                )
            } else {
                (
                    EventKind::Age,
                    format!(
                        "The sky forgets the sun: a great winter settles over {}, and the chroniclers, shivering, give it its name — {}.",
                        self.world_name, name
                    ),
                )
            };
            events.push(Event {
                m: month_abs,
                s: name,
                k,
                text,
                ids: subject.iter().copied().collect(),
                ..Default::default()
            });
        }
        if let Some(i) = self.ages.arcs().iter().position(|a| a.release == year) {
            let a = self.ages.arcs()[i];
            let name = self.ages.name(i).to_string();
            let (k, text) = if a.warm {
                (
                    EventKind::Optimum,
                    format!(
                        "After {} years {} closes over {}, and the uplands are let go to the frost, field by field.",
                        a.duration(),
                        name,
                        self.world_name
                    ),
                )
            } else {
                (
                    EventKind::Age,
                    format!(
                        "After {} years {} loosens its grip on {}, and the high pastures open again.",
                        a.duration(),
                        name,
                        self.world_name
                    ),
                )
            };
            events.push(Event {
                m: month_abs,
                s: name,
                k,
                text,
                ids: subject.iter().copied().collect(),
                ..Default::default()
            });
        }
        events
    }

    /// M80 — the yearly mapping pass: read the index over the lattice,
    /// group the dry ground, carry names forward, name what is new.
    /// Runs once a year, in the same month as the harvest verdict, so a
    /// famine and the drought it belongs to always agree.
    pub(crate) fn drought_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let year = month_abs / 12;
        let mut d = std::mem::take(&mut self.droughts);
        if d.rows == 0 {
            let norm = d.norm;
            d = Droughts::new(&self.fields.height);
            d.norm = norm; // the calibration survives the lazy build
        }
        let out = self.drought_map(&mut d, year, month_abs);
        self.droughts = d;
        out
    }

    /// M81 — the yearly spate. In the fourth month, the melt-and-rain
    /// stage every river carries this year is read at each river town's
    /// own cell and compared with the stage its banks and levees hold.
    /// Where the water is higher the levees are overtopped: souls are
    /// lost, bounded by [`crate::flood::DMG_CAP`], and the floodplain around the town
    /// is silted for the *following* growing season.
    ///
    /// Runs before the harvest verdict's month, and the silt it lays is
    /// dated a year ahead, so a flood never flatters the harvest it
    /// drowned — only the one after it.
    pub(crate) fn flood_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 3 {
            return events;
        }
        let year = month_abs / 12;
        self.floods.sweep(year);
        for i in 0..self.peoples.settlements.len() {
            let (y, x, pop, culture, river, name, sid) = {
                let s = &self.peoples.settlements[i];
                (s.y, s.x, s.pop, s.people, s.river, s.name.clone(), s.id)
            };
            if !river || pop < crate::flood::MIN_POP {
                continue;
            }
            let cap = {
                let so = self.peoples.societies.get(culture.0);
                crate::flood::capacity(|t| so.map_or(false, |s| s.knows(t)))
            };
            let factor = self.year_site_flow_factor(year, y as usize, x as usize);
            let excess = factor - cap;
            if excess <= 0.0 {
                continue;
            }
            let frac = (crate::flood::DMG_GAIN * excess).min(crate::flood::DMG_CAP);
            let hit = ((pop as f64) * frac) as i64;

            let silt = (crate::flood::SILT_GAIN * excess).min(crate::flood::SILT_CAP);
            let drown = (crate::flood::DROWN_GAIN * excess).min(crate::flood::DROWN_CAP);
            // the sheets: the drowned ground and the valley floor around
            // it lose this year's crop and are fed the next year's season
            let (rows, cols) = self.fields.height.dim();
            for dy in -crate::flood::SILT_REACH..=crate::flood::SILT_REACH {
                for dx in -crate::flood::SILT_REACH..=crate::flood::SILT_REACH {
                    let (ny, nx) = (y + dy, x + dx);
                    if ny < 0 || nx < 0 || ny >= rows as i64 || nx >= cols as i64 {
                        continue;
                    }
                    if self.fields.height[[ny as usize, nx as usize]] < 0.0 {
                        continue;
                    }
                    // the ring is thinner than the channel's own ground
                    let thin = if dy == 0 && dx == 0 { 1.0 } else { 0.6 };
                    self.floods.lay(year + 1, ny, nx, silt * thin);
                    self.floods.lay_drown(year, ny, nx, drown * thin);
                }
            }
            if hit > 0 {
                self.peoples.settlements[i].pop = (pop - hit).max(30);
            }

            self.floods.rows.push(crate::flood::FloodRow {
                m: month_abs,
                year,
                x,
                y,
                sid: sid.0 as usize,
                pop,
                factor,
                cap,
                frac,
                hit,
                silt,
                drown,
            });

            let text = if hit >= 4 {
                format!(
                    "The river rises over {} — {} are lost to the water, and the fields it drowns come back richer.",
                    name, hit
                )
            } else {
                format!(
                    "The river spills its banks at {} — the levees hold what they can, and the silt is laid over the fields.",
                    name
                )
            };
            events.push(Event {
                m: month_abs,
                s: name,
                k: EventKind::Flood,
                text,
                x,
                y,
                ..Default::default()
            });
        }
        events
    }

    /// M93 — lakes that breathe. In the year's last month every terminal
    /// basin's balance is struck: the catchment's rain anomaly (weighted
    /// by the rain each cell normally gets) sets the inflow through the
    /// closed-basin runoff elasticity, the water's temperature anomaly sets the
    /// evaporation, and the level answers by the bowl's bathymetry. The
    /// records are kept exactly; a strandline is dated only when the
    /// level has moved [`hydrology::STRANDLINE_STEP_M`] beyond the stand
    /// at the last terrace of its sign, and never inside the burn-in.
    /// No die is drawn: every level is re-derivable from the seed.
    pub(crate) fn lake_pass(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if month_abs.rem_euclid(12) != 11 || self.lakes.basins.is_empty() {
            return events;
        }
        let year = month_abs / 12;
        if self.lakes.last_year >= year {
            return events;
        }
        // The year's forcing per basin, read off the same pointwise sky
        // law the rivers and harvests read (dt in °C, dp as a fraction).
        // The full-grid sky is a diagnostics path and costs a whole map
        // per call; here the catchment is sampled at a fixed stride
        // through its cell list (a few dozen points per basin — the
        // anomaly field is smooth at the scale of a catchment) and every
        // sample is weighted by the rain that cell normally gets.
        let rows = self.fields.tmean.dim().0;
        let osc = self.year_osc(year);
        let drift = self.year_forcing(year);
        let anom = |cx: u16, cy: u16| -> (f64, f64) {
            climate::year_anomaly_at(
                &self.variability,
                rows,
                cx as usize,
                cy as usize,
                year,
                osc,
                drift,
            )
        };
        let forcing: Vec<(f64, f64)> = self
            .lakes
            .basins
            .iter()
            .map(|b| {
                let mut wsum = 0.0f64;
                let mut psum = 0.0f64;
                let mut tsum = 0.0f64;
                let mut tn = 0usize;
                let stride = |n: usize| (n / hydrology::LAKE_FORCING_SAMPLES).max(1);
                for &(cx, cy) in b.catchment.iter().step_by(stride(b.catchment.len())) {
                    let w = self.fields.precip[[cy as usize, cx as usize]].max(0.0) as f64;
                    let (_, dp) = anom(cx, cy);
                    wsum += w * dp;
                    psum += w;
                }
                for &(cx, cy) in b.cells.iter().step_by(stride(b.cells.len())) {
                    let w = self.fields.precip[[cy as usize, cx as usize]].max(0.0) as f64;
                    let (dt, dp) = anom(cx, cy);
                    wsum += w * dp;
                    psum += w;
                    tsum += dt;
                    tn += 1;
                }
                let dpb = if psum > 0.0 { wsum / psum } else { 0.0 };
                let dtb = if tn > 0 { tsum / tn as f64 } else { 0.0 };
                (dtb, dpb)
            })
            .collect();
        let burn_in = year < hydrology::STRANDLINE_BURN_IN_YEARS;
        let mut lines: Vec<(usize, i64, bool, f64)> = Vec::new();
        for (i, b) in self.lakes.basins.iter_mut().enumerate() {
            let (dtb, dpb) = forcing[i];
            let inflow = (1.0 + hydrology::LAKE_INFLOW_GAIN * dpb)
                .clamp(hydrology::LAKE_INFLOW_FACTOR.0, hydrology::LAKE_INFLOW_FACTOR.1);
            let evap = (1.0 + hydrology::LAKE_EVAP_T_GAIN * dtb)
                .clamp(hydrology::LAKE_EVAP_FACTOR.0, hydrology::LAKE_EVAP_FACTOR.1);
            b.advance(inflow, evap);
            b.last_dp = dpb;
            b.last_dt = dtb;
            b.last_inflow = inflow;
            b.last_evap = evap;
            let h = b.level_m;
            if h > b.hi_m {
                b.hi_m = h;
                b.hi_year = year;
                if !burn_in && h - b.mark_hi_m >= hydrology::STRANDLINE_STEP_M {
                    b.mark_hi_m = h;
                    b.strandlines.push(hydrology::Strandline { year, rel_m: h - b.full_m, rising: true });
                    lines.push((i, year, true, h - b.full_m));
                }
            }
            if h < b.lo_m {
                b.lo_m = h;
                b.lo_year = year;
                if !burn_in && b.mark_lo_m - h >= hydrology::STRANDLINE_STEP_M {
                    b.mark_lo_m = h;
                    b.strandlines.push(hydrology::Strandline { year, rel_m: h - b.full_m, rising: false });
                    lines.push((i, year, false, h - b.full_m));
                }
            }
            if burn_in {
                // the watched generation: records set, no terrace dated
                b.mark_hi_m = b.mark_hi_m.max(h);
                b.mark_lo_m = b.mark_lo_m.min(h);
            }
        }
        self.lakes.last_year = year;
        for (i, year, rising, rel) in lines {
            let b = &self.lakes.basins[i];
            // how long since a stand this extreme: the previous terrace of
            // this sign, or the dawn if there was none
            let since = b
                .strandlines
                .iter()
                .rev()
                .skip(1)
                .find(|t| t.rising == rising)
                .map(|t| year - t.year)
                .unwrap_or(year);
            let text = if rising {
                format!(
                    "{} rises past every shore the living remember — the water stands {:.1} m above its founding mark, the highest in {} years, and a new strandline is cut along the hills.",
                    b.name, rel, since
                )
            } else {
                format!(
                    "{} shrinks below its lowest remembered shore — the water stands {:.1} m under its founding mark, the lowest in {} years, and a white strandline of salt shows where the old beach lay.",
                    b.name, -rel, since
                )
            };
            events.push(Event {
                m: month_abs,
                s: b.name.clone(),
                k: EventKind::Strandline,
                text,
                x: b.x,
                y: b.y,
                ..Default::default()
            });
        }
        events
    }

    fn lattice_year(&self, d: &Droughts, year: i64) -> Vec<f32> {
        let rows = self.fields.tmean.dim().0;
        let osc = self.year_osc(year);
        // M84 — the belt rides `dp`; the map must show the drought the
        // index (and the harvests) actually felt.
        let drift = self.year_forcing(year);
        let mut z = vec![0.0f32; d.rows * d.cols];
        for cy in 0..d.rows {
            let y = cy * STRIDE;
            let sigma = climate::anomaly_amp_p(row_lat(rows, y)).max(1e-6);
            for cx in 0..d.cols {
                if !d.land[cy * d.cols + cx] {
                    continue;
                }
                let x = cx * STRIDE;
                let (_, dp) =
                    climate::year_anomaly_at(self.variability(), rows, x, y, year, osc, drift);
                z[cy * d.cols + cx] = (dp / sigma) as f32;
            }
        }
        z
    }

    fn drought_map(&self, d: &mut Droughts, year: i64, month_abs: i64) -> Vec<Event> {
        // The window: newest first. On the first pass the whole window is
        // filled from the sky's own prehistory (years before the founding
        // exist on the lattice), so year 0 reads the same law as year 300.
        if d.hist.is_empty() {
            for k in (1..MEMO_YEARS as i64).rev() {
                d.hist.insert(0, self.lattice_year(d, year - k));
            }
            d.hist.insert(0, self.lattice_year(d, year));
        } else {
            d.hist.insert(0, self.lattice_year(d, year));
            d.hist.truncate(MEMO_YEARS);
        }
        let n = d.rows * d.cols;
        for i in 0..n {
            let mut acc = 0.0f64;
            let mut w = 1.0f64;
            for g in d.hist.iter() {
                acc += w * g[i] as f64;
                w *= MEM;
            }
            d.index[i] = (acc * self.drought_norm()) as f32;
        }

        // Two thresholds, not one (hysteresis). A drought must ENTER on
        // genuinely failed ground — a core of `MIN_CORE` nodes at or past
        // SPI −1 — but it HOLDS while the ground stays merely parched
        // (`DRY_HOLD`). Mapping both edges at the same line was what made
        // the ledger blink: a region flickering across a single contour
        // dies and is re-named every other year, which reads as a
        // one-year median span and a footprint that never matches
        // yesterday's. The extent is grown on the holding contour; the
        // core decides only whether a *new* name is owed.
        let prev_owner = std::mem::replace(&mut d.owner, vec![-1; n]);
        let hold: Vec<bool> = (0..n)
            .map(|i| {
                d.land[i]
                    && (d.index[i] as f64 <= DROUGHT_Z
                        || (prev_owner[i] >= 0 && d.index[i] as f64 <= DRY_HOLD))
            })
            .collect();
        let mut seen = vec![false; n];
        let mut regs: Vec<(Vec<usize>, bool)> = Vec::new();
        for start in 0..n {
            if !hold[start] || seen[start] {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut cells = Vec::new();
            while let Some(i) = stack.pop() {
                cells.push(i);
                let (cy, cx) = (i / d.cols, i % d.cols);
                let push = |ny: usize, nx: usize, stack: &mut Vec<usize>, seen: &mut Vec<bool>| {
                    let j = ny * d.cols + nx;
                    if hold[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                };
                if cy > 0 {
                    push(cy - 1, cx, &mut stack, &mut seen);
                }
                if cy + 1 < d.rows {
                    push(cy + 1, cx, &mut stack, &mut seen);
                }
                if cx > 0 {
                    push(cy, cx - 1, &mut stack, &mut seen);
                }
                if cx + 1 < d.cols {
                    push(cy, cx + 1, &mut stack, &mut seen);
                }
            }
            cells.sort_unstable();
            if cells.len() < MIN_NODES {
                continue;
            }
            // Spatial hysteresis is asymmetric as temporal hysteresis must
            // be: owned ground may remain at the holding contour, but new
            // ground enters only after crossing SPI -1. This stops a named
            // footprint annexing a different merely-parched margin each
            // year while preserving the exact 12-year index beneath it.
            let inherited = cells.iter().any(|&i| prev_owner[i] >= 0);
            if inherited {
                // A named drought may advance into newly failed ground, but
                // only along its existing edge. Without this rate limit, a
                // one-node bridge across the entry mask annexed a remote dry
                // core in one year: the sky's mask retained 0.35 of its area,
                // while the named footprint retained only 0.21. One lattice
                // step is 32 km/year — movement, not teleportation.
                cells.retain(|&i| {
                    if prev_owner[i] >= 0 {
                        return true;
                    }
                    let (cy, cx) = (i / d.cols, i % d.cols);
                    (cy > 0 && prev_owner[i - d.cols] >= 0)
                        || (cy + 1 < d.rows && prev_owner[i + d.cols] >= 0)
                        || (cx > 0 && prev_owner[i - 1] >= 0)
                        || (cx + 1 < d.cols && prev_owner[i + 1] >= 0)
                });
                if cells.len() < MIN_NODES {
                    continue;
                }
            }
            let core = cells.iter().filter(|&&i| d.index[i] as f64 <= DROUGHT_Z).count();
            if core >= MIN_CORE || inherited {
                regs.push((cells, core >= MIN_CORE));
            }
        }
        // Deterministic order: by the region's lowest node.
        regs.sort_by(|a, b| a.0[0].cmp(&b.0[0]));
        let cored: Vec<bool> = regs.iter().map(|r| r.1).collect();
        let regions: Vec<Vec<usize>> = regs.into_iter().map(|r| r.0).collect();


        // Inheritance: a region belongs to last year's drought it overlaps
        // most. One ancestor can only be claimed once — where a drought
        // splits, the larger half keeps the name and the other half is a
        // new failed year of its own.

        // Event ids are dense indices into `d.events`; a byte ledger says
        // the same thing as a temporary HashSet without shipping a second
        // hash-table monomorphization in the wasm engine.
        let mut claimed = vec![false; d.events.len()];
        let mut assign: Vec<Option<usize>> = vec![None; regions.len()];
        let mut order: Vec<usize> = (0..regions.len()).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(regions[i].len()), regions[i][0]));
        for &ri in &order {
            let mut tally: std::collections::BTreeMap<usize, usize> = Default::default();
            for &cell in &regions[ri] {
                let o = prev_owner[cell];
                if o >= 0 {
                    *tally.entry(o as usize).or_default() += 1;
                }
            }
            let best = tally
                .iter()
                .filter(|(e, _)| !claimed[**e])
                .max_by_key(|(e, n)| (**n, std::cmp::Reverse(**e)))
                .map(|(e, _)| *e);
            if let Some(e) = best {
                claimed[e] = true;
                assign[ri] = Some(e);
            }
        }

        let mut events = Vec::new();
        for (ri, cells) in regions.iter().enumerate() {
            let nodes = cells.len();
            let (mut sx, mut sy) = (0.0f64, 0.0f64);
            let mut peak = 0.0f64;
            let mut deep = cells[0];
            for &cell in cells {
                sx += ((cell % d.cols) * STRIDE) as f64;
                sy += ((cell / d.cols) * STRIDE) as f64;
                if (d.index[cell] as f64) < peak {
                    deep = cell;
                }
                peak = peak.min(d.index[cell] as f64);
            }
            let (cx, cy) = (sx / nodes as f64, sy / nodes as f64);
            // This year's anchor: the deepest node of *this* year's
            // footprint, in fine-grid cells.
            let deep_x = ((deep % d.cols) * STRIDE) as i64;
            let deep_y = ((deep / d.cols) * STRIDE) as i64;

            let idx = match assign[ri] {
                Some(e) => {
                    // Both footprints are kept in ascending lattice order.
                    // Intersect them directly instead of materializing a
                    // one-year hash table; this preserves the exact Jaccard
                    // and removes harness-shaped weight from the web build.
                    let prev = &d.events[e].prev;
                    let (mut a, mut b, mut inter) = (0usize, 0usize, 0usize);
                    while a < cells.len() && b < prev.len() {
                        match cells[a].cmp(&prev[b]) {
                            std::cmp::Ordering::Less => a += 1,
                            std::cmp::Ordering::Greater => b += 1,
                            std::cmp::Ordering::Equal => {
                                inter += 1;
                                a += 1;
                                b += 1;
                            }
                        }
                    }
                    let union = prev.len() + nodes - inter;
                    let jac = if union == 0 { 0.0 } else { inter as f64 / union as f64 };
                    let ev = &mut d.events[e];
                    ev.last_year = year;
                    ev.peak = ev.peak.min(peak);
                    ev.peak_nodes = ev.peak_nodes.max(nodes);
                    ev.years.push((year, nodes, cx, cy, jac, deep_x, deep_y));
                    ev.prev = cells.clone();
                    e
                }
                None => {
                    // Orphaned holding ground: a region whose ancestor was
                    // already claimed by a larger sibling and which never
                    // reached a failing core of its own is not a drought —
                    // it is the parched margin of one. It goes unowned and
                    // unnamed rather than minting a name for a wet year.
                    if !cored[ri] {
                        continue;
                    }
                    let id = d.events.len();

                    let (name, place) = self.name_drought(&mut d.taken, id, cx, cy);
                    // M6.1 — every entry in the telling must reach the cast.
                    // A drought is not itself a registry entity, but the
                    // ground it withers is, and that ground is exactly what
                    // the entry speaks of: the feature or town it was named
                    // for, or the world itself when the naming fell back to
                    // the world's own name. `resolve_events` cannot find it
                    // for us — the subject line carries the drought's name,
                    // which the registry has never heard, and the centroid
                    // rarely lands on an entity's own cell.
                    let subject = self
                        .chronicle
                        .registry
                        .find_kind(EntityKind::Feature, &place)
                        .or_else(|| {
                            self.chronicle.registry.find_kind(EntityKind::Settlement, &place)
                        })
                        .or_else(|| self.chronicle.registry.find(&place))
                        .or_else(|| {
                            self.chronicle.registry.find_kind(EntityKind::World, &self.world_name)
                        });
                    d.events.push(DroughtEvent {
                        id,
                        name: name.clone(),
                        place: place.clone(),
                        start_year: year,
                        last_year: year,
                        peak,
                        peak_nodes: nodes,
                        onset_nodes: nodes,
                        x: cx.round() as i64,
                        y: cy.round() as i64,
                        ax: deep_x,
                        ay: deep_y,
                        years: vec![(year, nodes, cx, cy, 1.0, deep_x, deep_y)],
                        announced: true,
                        prev: cells.clone(),
                    });
                    // The chronicle speaks a drought's name exactly once:
                    // in the year it takes hold.
                    events.push(Event {
                        m: month_abs,
                        s: name.clone(),
                        k: EventKind::Drought,
                        text: format!(
                            "The rains withdraw from {}: {} begins, and {:.0} thousand square leagues of field and pasture go dry.",
                            place,
                            name,
                            (nodes as f64 * NODE_KM2 / 1000.0).max(1.0)
                        ),
                        ids: subject.into_iter().collect(),
                        x: cx.round() as i64,
                        y: cy.round() as i64,
                        ..Default::default()
                    });
                    id
                }
            };
            for &cell in cells {
                d.owner[cell] = idx as i32;
            }
        }
        d.year = year;
        events
    }

    /// A drought is named for the ground it withers: the nearest named
    /// feature, or failing that the nearest town, or failing that the
    /// world itself. The form is picked by the event's own ordinal, so no
    /// die is rolled and no other stream is disturbed.
    fn name_drought(
        &self,
        taken: &mut HashSet<String>,
        id: usize,
        cx: f64,
        cy: f64,
    ) -> (String, String) {
        let mut best: Option<(f64, String)> = None;
        let mut consider = |x: i64, y: i64, name: &str| {
            if name.is_empty() {
                return;
            }
            let dd = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
            if best.as_ref().is_none_or(|(bd, _)| dd < *bd) {
                best = Some((dd, name.to_string()));
            }
        };
        for f in &self.features {
            consider(f.x, f.y, &f.name);
        }
        for s in &self.peoples.settlements {
            consider(s.x, s.y, &s.name);
        }
        let place = best.map(|(_, n)| n).unwrap_or_else(|| self.world_name.clone());
        for k in 0..FORMS.len() {
            let cand = FORMS[(id + k) % FORMS.len()].replace("{P}", &place);
            if taken.insert(cand.clone()) {
                return (cand, place);
            }
        }
        let cand = format!("{} of the year {}", FORMS[id % FORMS.len()].replace("{P}", &place), id);
        taken.insert(cand.clone());
        (cand, place)
    }
}

fn row_lat(rows: usize, y: usize) -> f64 {
    (-90.0 + (y as f64) * 180.0 / (rows as f64 - 1.0)).abs()
}
