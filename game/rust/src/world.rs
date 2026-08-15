//! World orchestration — port of world.py: generation pipeline + simulation.

use std::collections::HashSet;

use ndarray::Array2;
use rand::Rng;
use rand_pcg::Pcg64Mcg;
use serde::Serialize;
use serde_json::{json, Value};

use crate::agriculture;
use crate::biomes as biomes_mod;
use crate::climate;
use crate::constants;
use crate::culture::{self, Culture};
use crate::geo;
use crate::hydrology;
use crate::naming::{self, Feature};
use crate::resources::{self, Deposit};
use crate::settlements::{self, Settlement};
use crate::trade::{self, Route};
use crate::util::{now_ms, round3};

#[derive(Serialize, Clone)]
pub struct Event {
    pub m: i64,
    pub s: String,
    pub text: String,
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
    pub rivers: Array2<bool>,
    pub lakes: Array2<bool>,

    pub deposits: Vec<Deposit>,
    pub settlements: Vec<Settlement>,
    pub cultures: Vec<Culture>,
    pub features: Vec<Feature>,
    pub routes: Vec<Route>,
    pub events: Vec<Event>,
    pub world_name: String,

    rng: Pcg64Mcg,
    taken: HashSet<String>,
    site_score: Array2<f64>,
    food_grid: Array2<f64>,
    near_fresh: Array2<bool>,
    coast: Array2<bool>,
    max_settlements: usize,
    trade_cost: Array2<f64>,
    trade_f: usize,
    timings: Vec<(&'static str, f64)>,
}

impl World {
    pub fn generate(seed: i64, size: usize) -> World {
        let t0 = now_ms();
        let mut timings: Vec<(&'static str, f64)> = Vec::new();

        let height = geo::heightmap(seed, size);
        let water = height.mapv(|h| h < 0.0);
        timings.push(("terrain", now_ms() - t0));

        let t1 = now_ms();
        let lat = climate::latitude_deg(size);
        let tmean = climate::temperature_mean(&height, &lat);
        let tamp = climate::temperature_amplitude(&lat, &water);
        let precip = climate::precipitation(&height, &water, &tmean, &lat);
        timings.push(("climate", now_ms() - t1));

        let t2 = now_ms();
        let hydro = hydrology::hydrology(&height, &water, &precip);
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
        timings.push(("fertility", now_ms() - t4));

        let t5 = now_ms();
        let (features, world_name) = naming::name_features(
            &height,
            &biome_map,
            &hydro.rivers,
            &hydro.lakes,
            &hydro.discharge,
            seed,
        );
        timings.push(("naming", now_ms() - t5));

        let t6 = now_ms();
        let deposits =
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
            &deposits,
            &fertility,
            &mut rng9000,
            &mut taken,
        );
        let mut setts = founded.settlements;
        let cultures = culture::assign_cultures(&biome_map, &mut setts, &mut taken, seed);
        trade::assign_goods(&mut setts, &deposits, &fertility);

        let trade_f = (size / 128).max(1);
        let cost_full = trade::cost_grid(&height, &hydro.rivers, &hydro.lakes, &biome_map);
        let trade_cost = trade::downsample(&cost_full, trade_f);
        let routes = trade::build_routes(&trade_cost, trade_f, &mut setts);
        timings.push(("settlements", now_ms() - t7));

        let mut events: Vec<Event> = Vec::new();
        for s in &setts {
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
                text: format!("{} founded by the {}{}", s.name, people, suffix),
            });
        }
        timings.push(("total", now_ms() - t0));

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
            rivers: hydro.rivers,
            lakes: hydro.lakes,
            deposits,
            settlements: setts,
            cultures,
            features,
            routes,
            events,
            world_name,
            rng: crate::util::rng(seed + 777),
            taken,
            site_score: founded.site_score,
            food_grid: founded.food_grid,
            near_fresh: founded.near_fresh,
            coast: founded.coast,
            max_settlements: founded.max_settlements,
            trade_cost,
            trade_f,
            timings,
        };
        // Open-ocean margins east and west: the world breathes a little wider.
        world.widen(size / 8);
        world
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
        self.rivers = grow_bool(&self.rivers, pad);
        self.lakes = grow_bool(&self.lakes, pad);
        self.near_fresh = grow_bool(&self.near_fresh, pad);
        self.coast = grow_bool(&self.coast, pad);

        // Downsampled trade grid: margins are open sea lanes (cost 0.8).
        let dpad = pad / self.trade_f;
        let (dh, dw) = self.trade_cost.dim();
        let dp = dpad as isize;
        let tc = self.trade_cost.clone();
        self.trade_cost = Array2::from_shape_fn((dh, dw + 2 * dpad), |(y, x)| {
            let xi = x as isize - dp;
            if xi >= 0 && (xi as usize) < dw {
                tc[[y, xi as usize]]
            } else {
                0.8
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
        self.width = w + 2 * pad;
    }

    /// One month of growth for every settlement; returns events.
    fn tick_month(&mut self, month_abs: i64) -> Vec<Event> {
        let mut events = Vec::new();
        let month = month_abs.rem_euclid(12);
        for s in self.settlements.iter_mut() {
            let (y, x) = (s.y as usize, s.x as usize);
            let t_now =
                climate::month_temperature(self.tmean[[y, x]], self.tamp[[y, x]], month);
            let mut r = 0.014;
            if t_now < -8.0 {
                r *= 0.25;
            } else if t_now < 0.0 {
                r *= 0.6;
            }
            r *= 1.0 + 0.04 * (s.connections.min(4) as f64); // trade bonus
            let k = 900.0 * s.food.max(0.3);
            let mut pop = s.pop;
            let mut growth = pop as f64 * r * (1.0 - pop as f64 / k);
            // harsh winter shock
            if t_now < -14.0 && self.rng.gen::<f64>() < 0.10 && pop > 60 {
                let loss = (pop as f64 * self.rng.gen_range(0.02..0.06)) as i64;
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    text: format!("A brutal winter grips {} — {} lost.", s.name, loss),
                });
            }
            // plague finds the crowded streets
            if pop > 2200 && self.rng.gen::<f64>() < 0.004 {
                let loss = (pop as f64 * self.rng.gen_range(0.06..0.16)) as i64;
                pop -= loss;
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    text: format!("Plague stalks the streets of {} — {} souls perish.", s.name, loss),
                });
            }
            // a golden harvest, in high summer, on good soil
            if month == 6 && s.food > 2.2 && self.rng.gen::<f64>() < 0.05 {
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    text: format!("The harvest overflows in {}; granaries groan.", s.name),
                });
                growth *= 2.0;
            }
            pop = ((pop as f64 + growth).round() as i64).max(20);
            let old_tier = s.tier.clone();
            s.pop = pop;
            s.tier = settlements::tier(pop);
            if s.tier != old_tier {
                events.push(Event {
                    m: month_abs,
                    s: s.name.clone(),
                    text: format!("{} has grown into a {}.", s.name, s.tier.to_lowercase()),
                });
            }
        }
        events
    }

    /// Crowded settlements send out settlers to found colonies.
    fn try_colonize(&mut self, month_abs: i64) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        let mut founded = false;
        let initial = self.settlements.len();
        for pi in 0..initial {
            if self.settlements.len() >= self.max_settlements {
                break;
            }
            let (ppop, pcap, pname) = {
                let p = &self.settlements[pi];
                (p.pop, settlements::capacity(p), p.name.clone())
            };
            if ppop < 380 || (ppop as f64) < 0.72 * pcap {
                continue;
            }
            if self.rng.gen::<f64>() > 0.02 {
                continue;
            }
            let site = {
                let parent = self.settlements[pi].clone();
                settlements::colony_site(&self.site_score, &self.settlements, &parent)
            };
            let Some((y, x)) = site else { continue };
            let migrants = ((ppop as f64 * self.rng.gen_range(0.08..0.14)) as i64).max(40);
            self.settlements[pi].pop = (ppop - migrants).max(60);
            let cid = self.settlements[pi].culture;
            let style = if !self.cultures.is_empty() {
                self.cultures[cid].style.clone()
            } else {
                "hellenic".to_string()
            };
            let name = naming::make_word(&mut self.rng, &style, &mut self.taken);
            let new_id = self.settlements.iter().map(|o| o.id).max().unwrap_or(-1) + 1;
            let mut s = Settlement {
                id: new_id,
                name: name.clone(),
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
                coastal: self.coast[[y, x]],
                river: self.near_fresh[[y, x]],
                culture: cid,
                connections: 0,
                goods: Vec::new(),
                exports: None,
            };
            trade::goods_for(&mut s, &self.deposits, &self.fertility);
            self.settlements.push(s);
            let idx = self.settlements.len() - 1;
            trade::connect_settlement(
                idx,
                &mut self.settlements,
                &mut self.routes,
                &self.trade_cost,
                self.trade_f,
            );
            founded = true;
            let coastal = self.settlements[idx].coastal;
            let river = self.settlements[idx].river;
            let place = if coastal {
                " by the sea."
            } else if river {
                " on fresh water."
            } else {
                " in the wilds."
            };
            events.push(Event {
                m: month_abs,
                s: name.clone(),
                text: format!("Settlers out of {} raise {}{}", pname, name, place),
            });
        }
        (events, founded)
    }

    pub fn tick(&mut self, months: i64) -> (Vec<Event>, bool) {
        let months = months.clamp(1, 240).max(1);
        let mut new_events: Vec<Event> = Vec::new();
        let mut founded = false;
        for _ in 0..months {
            self.month += 1;
            let evs = self.tick_month(self.month);
            new_events.extend(evs);
            let (col_evs, did) = self.try_colonize(self.month);
            if did {
                founded = true;
                new_events.extend(col_evs);
            }
        }
        self.events.extend(new_events.iter().cloned());
        let keep = self.events.len().saturating_sub(200);
        self.events.drain(..keep);
        (new_events, founded)
    }

    pub fn tick_json(&mut self, months: i64) -> String {
        let (events, founded) = self.tick(months);
        let mut out = json!({
            "month": self.month,
            "settlements": self.settlements,
            "events": events,
        });
        if founded {
            out["routes"] = json!(self.routes);
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
            "world_name": self.world_name,
            "biomes": constants::biome_meta(),
            "resources": resources::resource_meta(),
            "deposits": self.deposits,
            "settlements": self.settlements,
            "cultures": self.cultures,
            "features": self.features,
            "routes": self.routes,
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
        let flags: Vec<u8> = self
            .rivers
            .iter()
            .zip(self.lakes.iter())
            .map(|(&r, &l)| (r as u8) | ((l as u8) << 1))
            .collect();
        let biomes: Vec<u8> = self.biomes.iter().cloned().collect();

        let arrays: Vec<(&str, &str, Vec<u8>)> = vec![
            ("height", "float32", f32_bytes(&self.height)),
            ("tmean", "float32", f32_bytes(&self.tmean)),
            ("tamp", "float32", f32_bytes(&self.tamp)),
            ("precip", "float32", f32_bytes(&self.precip)),
            ("discharge", "float32", f32_bytes(&self.discharge)),
            ("fertility", "float32", f32_bytes(&self.fertility)),
            ("biomes", "uint8", biomes),
            ("flags", "uint8", flags),
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
