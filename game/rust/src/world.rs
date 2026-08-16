//! World orchestration — port of world.py: generation pipeline + simulation.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use serde_json::{json, Value};

use crate::agriculture;
use crate::artifact;
use crate::biomes as biomes_mod;
use crate::chronicle::{self, ChronicleState};
use crate::climate;
use crate::constants;
use crate::culture::{self, Culture};
use crate::economy::{self, Market};
use crate::entity::Registry;
use crate::erosion;
use crate::geo;
use crate::hydrology;
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

#[derive(Serialize, Clone)]
pub struct Event {
    pub m: i64,
    pub s: String,
    pub k: String,
    pub text: String,
    /// Entities this event speaks of (M6.1); the first id is the subject.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<i64>,
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

impl Default for Event {
    fn default() -> Self {
        Event {
            m: 0,
            s: String::new(),
            k: String::new(),
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

    pub height: Array2<f64>,
    pub tmean: Array2<f64>,
    pub tamp: Array2<f64>,
    pub precip: Array2<f64>,
    pub discharge: Array2<f64>,
    pub fertility: Array2<f64>,
    pub biomes: Array2<u8>,
    /// Crop package per cell (M2.1): 0 wild · 1 wheat · 2 rice · 3 maize · 4 pastoral.
    pub crops: Array2<u8>,
    pub rivers: Array2<bool>,
    pub lakes: Array2<bool>,
    /// Signed monsoon share of the year's rain (positive peaks month 0).
    pub pamp: Array2<f64>,
    /// Signed seasonal discharge swing per cell, -1..1.
    pub flow_amp: Array2<f64>,
    /// Strahler stream order, 0 off-river.
    pub strahler: Array2<u8>,
    /// Endorheic salt lakes.
    pub salt: Array2<bool>,
    /// Rivers that run dry half the year.
    pub seasonal: Array2<bool>,

    pub deposits: Vec<Deposit>,
    pub settlements: Vec<Settlement>,
    pub cultures: Vec<Culture>,
    pub features: Vec<Feature>,
    /// Set when mid-run naming alters features; tick_json ships and clears it.
    features_dirty: bool,
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
    /// Set when a town falls to ruin mid-run; tick_json ships and clears.
    ruins_dirty: bool,
    /// Set when the route web changes outside a founding (M9.1 removals,
    /// M9.4 roads falling disused); tick_json reships routes.
    routes_dirty: bool,
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
    territory_dirty: bool,
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
    timings: Vec<(&'static str, f64)>,
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

        let t5 = now_ms();
        let (mut features, world_name) = naming::name_features(
            &height,
            &biome_map,
            &hydro.rivers,
            &hydro.lakes,
            &hydro.discharge,
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
            &hydro.discharge,
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

        let trade_grid = trade::TradeGrid::build(
            &height,
            &hydro.rivers,
            &hydro.lakes,
            &biome_map,
            &hydro.discharge,
            (size / 128).max(1),
        );
        let routes = trade::build_routes(
            &trade_grid,
            &mut setts,
            &height,
            &hydro.rivers,
            &hydro.discharge,
            &hydro.flow_amp,
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
            &hydro.discharge,
        );
        // M3.1/M3.4 — the peoples lay their tongues over the nearby land;
        // border features gain an exonym from the second-closest people.
        naming::culture_toponyms(&mut features, &setts, &cultures, &mut taken, seed);
        let societies = society::init(&cultures);
        let mut market = Market::default();
        economy::update_prices(&mut market, &setts);
        // the first carve of the market areas (M5.2)
        let mut areas = economy::build_areas(&setts, &routes, None);
        economy::update_area_prices(&mut areas, &setts, &market);
        timings.push(("settlements", now_ms() - t7));

        let mut rng = crate::util::rng(seed + 777);

        // The cast enters the telling: peoples, towns and the named land
        // each get one stable id for their whole life (M6.1). The world
        // itself is entity zero, so even the creation myth has a subject.
        let mut registry = Registry::default();
        registry.add("world", &world_name, 0, None, -1, -1);
        for c in &cultures {
            registry.add("culture", &c.people, 0, Some(c.id), -1, -1);
        }
        let sett_ents: Vec<i64> = setts
            .iter()
            .map(|s| registry.add("settlement", &s.name, 0, Some(s.culture), s.x, s.y))
            .collect();
        for f in &features {
            registry.add("feature", &f.name, 0, None, f.x, f.y);
        }
        // The goods trade under their own names: market shocks, gluts and
        // strikes all speak of them, so they join the cast too (M6.1).
        for g in resources::ALL_PLACEABLE
            .iter()
            .chain(["grain", "tools", "weapons", "jewelry"].iter())
        {
            registry.add("good", g, 0, None, -1, -1);
        }

        let mut chron = ChronicleState::default();
        chron.rulers = chronicle::init_rulers(&mut rng, &cultures, &mut taken, &mut registry);

        let mut events: Vec<Event> =
            chronicle::founding_myths(&mut rng, &cultures, &features, &world_name);
        for (si, s) in setts.iter().enumerate() {
            let people = if !cultures.is_empty() {
                cultures[s.culture].people.clone()
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
                k: "found".to_string(),
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
            discharge: hydro.discharge,
            fertility,
            biomes: biome_map,
            crops,
            rivers: hydro.rivers,
            lakes: hydro.lakes,
            pamp,
            flow_amp: hydro.flow_amp,
            strahler: hydro.strahler,
            salt: hydro.salt,
            seasonal: hydro.seasonal,
            deposits,
            settlements: setts,
            cultures,
            features,
            features_dirty: false,
            routes,
            events,
            world_name,
            societies,
            market,
            areas,
            merchants: Vec::new(),
            route_flow: Vec::new(),
            ruins: Vec::new(),
            ruins_dirty: false,
            routes_dirty: false,
            route_idle: Vec::new(),
            registry,
            artifacts: Vec::new(),
            heat: 0.0,
            rng,
            taken,
            chron,
            politics: Politics::init(n_cultures),
            territory: Array2::from_elem((1, 1), -1),
            territory_dirty: false,
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
        world.territory_dirty = false; // ships with the pack, not the first tick
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
        self.territory_dirty = true;
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

        fn grow_f64(
            a: &Array2<f64>,
            pad: usize,
            margin: impl Fn(f64, f64) -> f64,
        ) -> Array2<f64> {
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
        self.height = grow_f64(&self.height, pad, |e, t| {
            let shelf = e.min(-0.03);
            let deep = (-0.62_f64).min(e);
            shelf + (deep - shelf) * t
        });
        // Climate margins keep zonal continuity by extending the edge column.
        self.tmean = grow_f64(&self.tmean, pad, |e, _| e);
        self.tamp = grow_f64(&self.tamp, pad, |e, _| e);
        self.precip = grow_f64(&self.precip, pad, |e, _| e);
        self.discharge = grow_f64(&self.discharge, pad, |_, _| 0.0);
        self.fertility = grow_f64(&self.fertility, pad, |_, _| 0.0);
        self.site_score = grow_f64(&self.site_score, pad, |_, _| 0.0);
        self.food_grid = grow_f64(&self.food_grid, pad, |_, _| 0.0);
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
        self.rivers = grow_bool(&self.rivers, pad);
        self.lakes = grow_bool(&self.lakes, pad);
        self.salt = grow_bool(&self.salt, pad);
        self.seasonal = grow_bool(&self.seasonal, pad);
        self.near_fresh = grow_bool(&self.near_fresh, pad);
        self.coast = grow_bool(&self.coast, pad);
        self.pamp = grow_f64(&self.pamp, pad, |e, _| e);
        self.flow_amp = grow_f64(&self.flow_amp, pad, |_, _| 0.0);
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
            while seat.len() <= s.culture {
                seat.push(usize::MAX);
            }
            if seat[s.culture] == usize::MAX
                || s.pop > self.settlements[seat[s.culture]].pop
            {
                seat[s.culture] = i;
            }
        }
        for (si, s) in self.settlements.iter_mut().enumerate() {
            let md = mods.get(s.culture).cloned().unwrap_or_default();
            let (y, x) = (s.y as usize, s.x as usize);
            let t_now =
                climate::month_temperature(self.tmean[[y, x]], self.tamp[[y, x]], month);
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
            if seat.get(s.culture) == Some(&si) {
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
                    k: "disaster".to_string(),
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
                    k: "disaster".to_string(),
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
                    k: "disaster".to_string(),
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
                    k: "disaster".to_string(),
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
                    k: "disaster".to_string(),
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
                    k: "disaster".to_string(),
                    text: format!("A black storm off the open sea lashes {} — {} lost to the waves.", s.name, loss),
                    ..Default::default()
                });
            }
            // a golden harvest, in high summer, on good soil
            if month == 6 && s.food > 2.2 && self.rng.gen::<f64>() < 0.05 {
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: "growth".to_string(),
                    text: format!("The harvest overflows in {}; granaries groan.", s.name),
                    ..Default::default()
                });
                growth *= 2.0;
            }
            // markets overflow where many roads meet
            if s.connections >= 3 && pop > 400 && self.rng.gen::<f64>() < 0.0015 {
                let good = s
                    .exports
                    .clone()
                    .unwrap_or_else(|| "grain".to_string());
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    k: "trade".to_string(),
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
                        k: "realm".to_string(),
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
                        k: "growth".to_string(),
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
                        k: "disaster".to_string(),
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
            if !matches!(
                d.r.as_str(),
                "stone" | "coal" | "copper" | "iron" | "silver" | "gold" | "mithril"
            ) {
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
            let worth = self.market.price(&d.r) * d.rich * 2.2;
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
                let md = mods_v.get(p.culture).cloned().unwrap_or_default();
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
                    .get(parent.culture)
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
                k: "found".to_string(),
                text,
                ..Default::default()
            });
        }
        (events, founded)
    }

    /// Raise a new settlement at (y, x): coin a name in the founding
    /// culture's style, list its goods, size its land, and wire it into
    /// the trade web. Shared by colonists and rush camps alike.
    fn found_settlement(&mut self, y: usize, x: usize, migrants: i64, cid: usize) -> usize {
        let style = if !self.cultures.is_empty() {
            self.cultures[cid].style.clone()
        } else {
            "hellenic".to_string()
        };
        let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
        let new_id = self.settlements.iter().map(|o| o.id).max().unwrap_or(-1) + 1;
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
            goods: Vec::new(),
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
            .get(cid)
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
                .add("settlement", &t.name, self.month, Some(t.culture), t.x, t.y);
        }
        trade::connect_settlement(
            idx,
            &mut self.settlements,
            &mut self.routes,
            &self.trade,
            &self.height,
            &self.rivers,
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
            if !matches!(
                d.r.as_str(),
                "stone" | "coal" | "copper" | "iron" | "silver" | "gold" | "mithril"
            ) {
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
            let worth = (self.market.price(&d.r) * d.rich / 2.0).clamp(0.2, 3.0);
            let (dx0, dy0) = (d.x, d.y);
            let kind = d.r.clone();
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
                k: "found".to_string(),
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
                    self.cultures[s.culture].people.clone(),
                    s.x,
                    s.y,
                )
            };
            let t = patina::UNEXPLAINED
                [self.rng.gen_range(0..patina::UNEXPLAINED.len())];
            let ent = self.registry.find_alive("settlement", x, y);
            evs.push(Event {
                m: month_abs,
                s: name.clone(),
                k: "myth".to_string(),
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
            let people = self.cultures[winner].people.clone();
            let eid = self.registry.add("feature", &name, m, Some(winner), x, y);
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
            self.features_dirty = true;
            evs.push(Event {
                m: month_abs,
                s: name.clone(),
                k: "war".to_string(),
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
            let style = self.cultures[to].style.clone();
            let coined = naming::coin(&mut self.rng, &style, &mut self.taken);
            let old = self.settlements[i].name.clone();
            let people = self.cultures[to].people.clone();
            let (x, y) = (self.settlements[i].x, self.settlements[i].y);
            {
                let s = &mut self.settlements[i];
                s.formerly.push(old.clone());
                s.name = coined.word.clone();
                s.ety = coined.ety.clone();
                s.namer = to;
            }
            let ent = self.registry.find_alive("settlement", x, y);
            if let Some(id) = ent {
                self.registry.rename(id, &coined.word);
            }
            evs.push(Event {
                m: month_abs,
                s: coined.word.clone(),
                k: "society".to_string(),
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
            counts[s.culture] += 1;
        }
        let besieged: HashSet<i64> = self
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
            if counts[s.culture] <= 1 {
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
        let ent = self.registry.find_alive("settlement", dead.x, dead.y);
        if let Some(id) = ent {
            self.registry
                .close(id, month_abs, &format!("abandoned — {}", why));
        }
        let ruin_name = format!("Ruins of {}", dead.name);
        let rid = self
            .registry
            .add("ruin", &ruin_name, month_abs, Some(dead.culture), dead.x, dead.y);
        self.ruins.push(Ruin {
            name: ruin_name.clone(),
            of: dead.name.clone(),
            x: dead.x,
            y: dead.y,
            since: month_abs,
            why: why.to_string(),
            people: self.cultures[dead.culture].people.clone(),
            ety: dead.ety.clone(),
            eid: rid,
        });
        self.ruins_dirty = true;
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
            &self.rivers,
            &self.discharge,
            &self.flow_amp,
        );
        trade::bridge_components(
            &mut self.settlements,
            &mut self.routes,
            &self.trade,
            &self.height,
            &self.rivers,
            &self.discharge,
            &self.flow_amp,
        );
        trade::recount_connections(&mut self.settlements, &self.routes);
        trade::mark_ports(&mut self.settlements, &self.routes);
        self.routes_dirty = true;
        self.recompute_territory();
        evs.push(Event {
            m: month_abs,
            s: dead.name.clone(),
            k: "realm".to_string(),
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
                    self.routes_dirty = true;
                }
                continue;
            }
            self.route_idle[i] = self.route_idle[i].saturating_add(1);
            if self.route_idle[i] == 25 && !self.routes[i].old {
                self.routes[i].old = true;
                self.routes_dirty = true;
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
                        k: "economy".to_string(),
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
            let ent = self.registry.find_alive("settlement", x, y);
            if let Some(id) = ent {
                self.registry.rename(id, &worn);
            }
            evs.push(Event {
                m: month_abs,
                s: worn.clone(),
                k: "society".to_string(),
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
            self.features_dirty = true;
            evs.push(Event {
                m: month_abs,
                s: worn_phrase.clone(),
                k: "society".to_string(),
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
                e.k.as_str(),
                "myth" | "omen" | "wonder" | "society" | "nature" | "disaster" | "famine" | "growth"
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
                    self.features_dirty = true;
                    new_events.push(Event {
                        m: self.month,
                        s: fname.clone(),
                        k: "society".to_string(),
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
            // M5.2: re-carve the market areas when towns appeared, and
            // refresh every other year as the route web thickens
            if self.areas.area.len() != self.settlements.len()
                || self.month.rem_euclid(24) == 2
            {
                self.areas =
                    economy::build_areas(&self.settlements, &self.routes, Some(&self.areas));
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
            // the relics ride the month's tides: forged, plundered, lost (M6.3)
            let month_slice: Vec<Event> = new_events[month_start..].to_vec();
            let art_evs = artifact::monthly(
                &mut self.artifacts,
                &mut self.registry,
                &month_slice,
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
                .map(|e| telling::weight(&e.k) - 1)
                .sum();
            self.heat = self.heat * 0.94 + (m_heat as f64 / 6.0) * 0.06;
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
                let found = ["settlement", "war", "culture", "person", "artifact", "feature"]
                    .iter()
                    .find_map(|k| self.registry.find_kind(k, &e.s))
                    .or_else(|| self.registry.find(&e.s));
                if let Some(id) = found {
                    e.ids.push(id);
                }
            }
            if e.ids.is_empty() && e.x >= 0 {
                // the subject may have been renamed this very tick (M9.2):
                // fall back to the one thing a rename never moves — its ground
                let found = ["settlement", "ruin", "feature"]
                    .iter()
                    .find_map(|k| self.registry.find_alive(k, e.x, e.y));
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
            if e.legend.is_empty() && telling::weight(&e.k) >= 3 {
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
        for i in 0..self.settlements.len() {
            let (y, x, pop, culture, river, name) = {
                let s = &self.settlements[i];
                (s.y, s.x, s.pop, s.culture, s.river, s.name.clone())
            };
            let pack = self.crops[[y as usize, x as usize]];
            let rainfed = matches!(
                pack,
                agriculture::PACK_WHEAT | agriculture::PACK_MAIZE
            ) && !river;
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
                .get(culture)
                .map_or(false, |so| so.knows("pottery"))
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
            // the hungry walk to the nearest kin-town outside the blight
            let mut target: Option<(usize, f64)> = None;
            for (j, o) in self.settlements.iter().enumerate() {
                if j == i || o.culture != culture {
                    continue;
                }
                if dry(o.x, o.y) < -0.30 {
                    continue; // starving too
                }
                let dy = (o.y - y) as f64;
                let dx = (o.x - x) as f64;
                let d2 = dy * dy + dx * dx;
                if target.map_or(true, |(_, b)| d2 < b) {
                    target = Some((j, d2));
                }
            }
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
                k: "famine".to_string(),
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
            self.market.shock("grain", 1.0 + 0.30 * worst);
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
        let mut found: Vec<(usize, usize)> = Vec::new();
        for (di, d) in self.deposits.iter().enumerate() {
            if d.known {
                continue;
            }
            // closest town, measured in multiples of its own ranging distance
            let mut best: Option<(f64, usize)> = None;
            for (si, s) in self.settlements.iter().enumerate() {
                let reach = settlements::territory_radius(s.pop)
                    * 2.4
                    * mods.get(s.culture).map(|m| m.prospecting).unwrap_or(1.0);
                let dx = (d.x - s.x) as f64;
                let dy = (d.y - s.y) as f64;
                let ratio = (dx * dx + dy * dy).sqrt() / reach.max(1e-9);
                if best.map_or(true, |(b, _)| ratio < b) {
                    best = Some((ratio, si));
                }
            }
            let Some((ratio, si)) = best else { continue };
            let rarity = match resources::abundance(&d.r) {
                "uncommon" => 0.6,
                "rare" => 0.35,
                "legendary" => 0.12,
                _ => 1.0,
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
            let kind = self.deposits[di].r.clone();
            let rich = self.deposits[di].rich;
            let precious = matches!(kind.as_str(), "silver" | "gold" | "mithril");
            // the market feels the strike before the first cart arrives —
            // hardest where the ore actually is (M5.2: local first)
            self.market.shock(&kind, if precious { 0.88 } else { 0.84 });
            let ka = self.areas.area_of(si);
            if let Some(mk) = self.areas.markets.get_mut(ka) {
                mk.shock(&kind, if precious { 0.80 } else { 0.75 });
            }
            let sname = self.settlements[si].name.clone();
            let strike = 12.0 + 45.0 * rich * economy::base_value(&kind);
            self.settlements[si].wealth = round2(self.settlements[si].wealth + strike);
            if precious {
                // a rush: prospectors, chancers and mule-trains pour in
                let influx = ((self.settlements[si].pop as f64 * 0.05) as i64).max(10);
                self.settlements[si].pop += influx;
            }
            let text = match kind.as_str() {
                "gold" => format!(
                    "Gold! Panners out of {} lift bright dust from the gravels — word runs faster than horses.",
                    sname
                ),
                "silver" => format!(
                    "A silver seam glitters by torchlight in the diggings above {}.",
                    sname
                ),
                "mithril" => format!(
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
                k: "discovery".to_string(),
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
            let kind = self.deposits[di].r.clone();
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
            self.market.shock(&kind, 1.22);
            // the pit's own market feels the silence first (M5.2)
            if let Some(i) = near_i {
                let ka = self.areas.area_of(i);
                if let Some(mk) = self.areas.markets.get_mut(ka) {
                    mk.shock(&kind, 1.35);
                }
            }
            events.push(Event {
                m: month_abs,
                s: near.clone(),
                k: "depletion".to_string(),
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
                let polity = self.societies.get(c.id).map(|s| s.polity).unwrap_or(0);
                if let Some(r) = self.chron.rulers.iter().find(|r| r.culture == c.id) {
                    let title = society::RULER_TITLES[polity];
                    v["ruler"] = if title.is_empty() {
                        json!(r.title())
                    } else {
                        json!(format!("{} {}", title, r.title()))
                    };
                }
                if let Some(soc) = self.societies.get(c.id) {
                    v["era"] = json!(society::ERAS[soc.era]);
                    v["polity"] = json!(society::POLITIES[soc.polity]);
                    v["treasury"] = json!(round2(soc.treasury));
                    let names: Vec<&'static str> = soc
                        .techs
                        .iter()
                        .filter_map(|id| society::tech_by_id(id).map(|t| t.name))
                        .collect();
                    v["techs"] = json!(names);
                }
                // statecraft readouts (M4): solidarity, the crown's standing,
                // whose leash they wear, and whether the realm still stands
                if let Some(a) = self.politics.asab.get(c.id) {
                    v["asab"] = json!(round2(*a));
                }
                if let Some(l) = self.politics.legit.get(c.id) {
                    v["legit"] = json!(round2(*l));
                }
                if let Some(Some(suz)) = self.politics.vassal_of.get(c.id) {
                    v["vassal_of"] = json!(self.cultures[*suz].people.clone());
                }
                v["alive"] = json!(politics::alive(&self.settlements, c.id));
                v
            })
            .collect();
        Value::Array(arr)
    }

    pub fn tick_json(&mut self, months: i64) -> String {
        let (events, founded, deposits_changed) = self.tick(months);
        let mut out = json!({
            "month": self.month,
            "settlements": self.settlements,
            "events": events,
            "cultures": self.cultures_json(),
            "wars": self.politics.wars,
            "market": self.market.snapshot(),
            "areas": economy::areas_json(&self.areas, &self.settlements),
            "merchants": self.merchants,
        });
        if founded || self.routes_dirty {
            out["routes"] = json!(self.routes);
            self.routes_dirty = false;
        }
        if deposits_changed {
            out["deposits"] = json!(self.known_deposits());
            out["deposits_hidden"] =
                json!(self.deposits.iter().filter(|d| !d.known).count());
        }
        if self.features_dirty {
            out["features"] = json!(self.features);
            self.features_dirty = false;
        }
        if self.ruins_dirty {
            out["ruins"] = json!(self.ruins);
            self.ruins_dirty = false;
        }
        if self.territory_dirty {
            out["territory"] = json!(politics::territory_rle(&self.territory));
            self.territory_dirty = false;
        }
        serde_json::to_string(&out).unwrap()
    }

    pub fn meta(&self) -> Value {
        let ev_start = self.events.len().saturating_sub(60);
        let timings: serde_json::Map<String, Value> = self
            .timings
            .iter()
            .map(|(k, v)| (k.to_string(), json!(round3(*v / 1000.0))))
            .collect();
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
            "biomes": constants::biome_meta(),
            "crop_packages": agriculture::PACK_NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| json!({
                    "id": i,
                    "name": n,
                    "density": agriculture::PACK_DENSITY[i],
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
            "timings": timings,
        })
    }

    /// [u32 header_len][header json (padded to 4)][raw little-endian arrays]
    pub fn pack(&self) -> Vec<u8> {
        fn f32_bytes(a: &Array2<f64>) -> Vec<u8> {
            let v: Vec<f32> = a.iter().map(|&x| x as f32).collect();
            bytemuck::cast_slice(&v).to_vec()
        }
        let mut flags: Vec<u8> = Vec::with_capacity(self.rivers.len());
        for (((&r, &l), &s), &sw) in self
            .rivers
            .iter()
            .zip(self.lakes.iter())
            .zip(self.salt.iter())
            .zip(self.seasonal.iter())
        {
            flags.push(
                (r as u8) | ((l as u8) << 1) | ((s as u8) << 2) | ((sw as u8) << 3),
            );
        }
        let biomes: Vec<u8> = self.biomes.iter().cloned().collect();
        let crops: Vec<u8> = self.crops.iter().cloned().collect();
        let strahler: Vec<u8> = self.strahler.iter().cloned().collect();
        let territory: Vec<i16> = self.territory.iter().cloned().collect();

        let arrays: Vec<(&str, &str, Vec<u8>)> = vec![
            ("height", "float32", f32_bytes(&self.height)),
            ("tmean", "float32", f32_bytes(&self.tmean)),
            ("tamp", "float32", f32_bytes(&self.tamp)),
            ("precip", "float32", f32_bytes(&self.precip)),
            ("pamp", "float32", f32_bytes(&self.pamp)),
            ("discharge", "float32", f32_bytes(&self.discharge)),
            ("flow_amp", "float32", f32_bytes(&self.flow_amp)),
            ("fertility", "float32", f32_bytes(&self.fertility)),
            ("biomes", "uint8", biomes),
            ("crops", "uint8", crops),
            ("strahler", "uint8", strahler),
            ("flags", "uint8", flags),
            ("territory", "int16", bytemuck::cast_slice(&territory).to_vec()),
        ];

        let mut entries: Vec<Value> = Vec::new();
        let mut offset = 0usize;
        for (name, dtype, raw) in &arrays {
            entries.push(json!({
                "name": name,
                "dtype": dtype,
                "shape": [self.size, self.width],
                "offset": offset,
                "nbytes": raw.len(),
            }));
            offset += raw.len();
        }

        let mut header = self.meta();
        header["id"] = json!(format!("{}-{}", self.seed, self.size));
        header["arrays"] = Value::Array(entries);
        let mut hjson = serde_json::to_string(&header).unwrap().into_bytes();
        while hjson.len() % 4 != 0 {
            hjson.push(b' ');
        }

        let total = 4 + hjson.len() + offset;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&(hjson.len() as u32).to_le_bytes());
        out.extend_from_slice(&hjson);
        for (_, _, raw) in &arrays {
            out.extend_from_slice(raw);
        }
        out
    }
}
