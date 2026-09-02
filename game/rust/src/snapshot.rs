//! Wire snapshots — every JSON surface the client reads (E11.1).
//!
//! The tick payload with its delta lanes (E4), the once-per-world
//! bootstrap (E3.1), and the `SentCache` that decides what reships.
//! Moved verbatim out of `world.rs`; wire bytes are unchanged.

use serde::Serialize;
use serde_json::{json, Value};
use strum::IntoEnumIterator;

use crate::constants;
use crate::economy;
use crate::politics;
use crate::agriculture;
use crate::resources::{self, Deposit};
use crate::society;
use crate::settlements::Settlement;
use crate::util::{round2, round3};
use crate::world::{headline_worthy, Dirty, EventKind, World};

/// E4.2/E4.3 — FNV hashes of the last-shipped JSON per wire surface; a
/// section crosses the boundary only when its bytes moved. Seeded by
/// `prime_sent()` to the freshly generated world, which is exactly what
/// `bootstrap()` ships.
#[derive(Default)]
pub(crate) struct SentCache {
    /// (settlement id, cold-form hash, heartbeat quanta), engine order.
    /// The cold form zeroes the monthly heartbeat (pop/food/k/wealth); the
    /// quanta are the heartbeat at wire precision (pop · food×10 · k ·
    /// wealth×10), so a town ships a per-field patch only when a value the
    /// client can actually see has moved (E4.2).
    settlements: Vec<(i64, u64, [i64; 4])>,
    /// realm-heartbeat · wars · merchants (full-form hashes).
    blocks: [u64; 3],
    /// Hash of the peoples block (ADR-0018: slow axis, plain gate).
    cultures_cold: u64,
    /// Hash of the civilizations block (M13: derived tier, yearly clock).
    civs_cold: u64,
    /// Cold-form hash of the realms block (heartbeat stripped).
    realms_cold: u64,
    /// Hash of the people-axis influence RLE (M10.6) — reships whole
    /// when any people border moved; it moves on generational clocks.
    peoples_rle: u64,
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
    /// E4.7 — the territory grid exactly as the client last received it
    /// (pack at dawn, then every shipped patch); tile diffs run against
    /// this, never against a guess.
    territory: ndarray::Array2<i16>,
    /// M89 — the composed forcing as last shipped, at wire precision
    /// (×100): the sky scalar crosses only when the value the client
    /// can see has moved.
    sky: i64,
}

impl World {
    /// Deposits the world has actually found — all the client ever sees.
    fn known_deposits(&self) -> Vec<&Deposit> {
        self.deposits.iter().filter(|d| d.known).collect()
    }

    /// Peoples with era, polity and arts attached (ADR-0018: the slow
    /// axis — tongue, gods, knowledge; no coin, no crown).
    fn cultures_json(&self) -> Value {
        let arr: Vec<Value> = self
            .peoples.peoples
            .iter()
            .map(|c| {
                let mut v = serde_json::to_value(c).unwrap();
                if let Some(soc) = self.peoples.societies.get(c.id.0) {
                    v["era"] = json!(society::ERAS[soc.era]);
                    v["polity"] = json!(society::POLITIES[soc.polity]);
                    v["techs"] = json!(soc
                        .techs
                        .iter()
                        .map(|&id| society::tech(id).name)
                        .collect::<Vec<&'static str>>());
                }
                // how many hearths still keep the tongue
                v["towns"] = json!(self
                    .peoples.settlements
                    .iter()
                    .filter(|s| s.people == c.id)
                    .count());
                v
            })
            .collect();
        Value::Array(arr)
    }

    /// Civilizations for the wire (M13/ADR-0019) — the serialized rows
    /// plus display joins the client would otherwise recompute: member
    /// demonyms, member realm names, hearth count.
    fn civs_json(&self) -> Value {
        let arr: Vec<Value> = self
            .peoples
            .civs
            .iter()
            .map(|c| {
                let mut v = serde_json::to_value(c).unwrap();
                v["folk"] = json!(c
                    .peoples
                    .iter()
                    .filter_map(|p| self.peoples.peoples.get(p.idx()))
                    .map(|p| p.people.clone())
                    .collect::<Vec<String>>());
                let members: Vec<String> = self
                    .peoples
                    .realms
                    .iter()
                    .filter(|r| r.alive && c.peoples.contains(&r.people))
                    .filter(|r| self.peoples.settlements.iter().any(|s| s.realm == r.id))
                    .map(|r| r.name.clone())
                    .collect();
                v["towns"] = json!(self
                    .peoples
                    .settlements
                    .iter()
                    .filter(|s| c.peoples.contains(&s.people))
                    .count());
                v["members"] = json!(members);
                v
            })
            .collect();
        Value::Array(arr)
    }


    /// One realm row for the wire (ADR-0018: the political axis). With
    /// the heartbeat (treasury/asab/legit) for full blocks; without it
    /// for the cold gate.
    fn realm_row(&self, i: usize, with_heartbeat: bool) -> Value {
        let r = &self.peoples.realms[i];
        let mut v = serde_json::to_value(r).unwrap();
        if !with_heartbeat {
            v.as_object_mut().unwrap().remove("treasury");
        } else {
            v["treasury"] = json!(round2(r.treasury));
            if let Some(a) = self.politics.asab.get(i) {
                v["asab"] = json!(round2(*a));
            }
            if let Some(l) = self.politics.legit.get(i) {
                v["legit"] = json!(round2(*l));
            }
            if let Some(u) = self.politics.unrest.get(i) {
                v["unrest"] = json!(round2(*u));
            }
        }
        let polity = self
            .peoples.societies
            .get(r.people.idx())
            .map(|s| s.polity)
            .unwrap_or(0);
        if let Some(ru) = self.chronicle.state.rulers.iter().find(|ru| ru.realm == r.id) {
            let title = society::RULER_TITLES[polity];
            v["ruler"] = if title.is_empty() {
                json!(ru.title())
            } else {
                json!(format!("{} {}", title, ru.title()))
            };
        }
        if let Some(Some(suz)) = self.politics.vassal_of.get(i) {
            v["vassal_of"] = json!(self.peoples.realms[suz.0].name.clone());
        }
        v
    }

    /// Realms with ruler, vassalage and the statecraft heartbeat.
    fn realms_json(&self) -> Value {
        Value::Array(
            (0..self.peoples.realms.len())
                .map(|i| self.realm_row(i, true))
                .collect(),
        )
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

    /// The area block's identity: the settlement→area vector AND the hub
    /// ids. A hub re-election can leave "of" byte-identical while the hub
    /// set changes — gate on "of" alone and the client keeps the dead hub
    /// forever (E4.3 replay divergence).
    fn areas_set_hash(areas_v: &Value) -> u64 {
        let mut src = areas_v["of"].to_string();
        if let Some(hubs) = areas_v["hubs"].as_array() {
            for h in hubs {
                src.push('|');
                src.push_str(&h["id"].to_string());
            }
        }
        crate::util::fnv1a64(src.as_bytes())
    }



    /// E4.2 hot/cold split, realm side, built in one pass (E5.12):
    /// (cold string, hot patch rows). treasury/asab/legit are the
    /// heartbeat; the cold rows carry everything else. The full block is
    /// only assembled (`realms_json`) on the rare tick the cold half
    /// actually moved — succession, secession, conquest, vassalage —
    /// instead of being built, cloned and stripped every single month.
    fn realms_cold_hot(&self) -> (String, String) {
        let mut cold: Vec<Value> = Vec::with_capacity(self.peoples.realms.len());
        let mut rows: Vec<Value> = Vec::new();
        for i in 0..self.peoples.realms.len() {
            cold.push(self.realm_row(i, false));
            let r = &self.peoples.realms[i];
            let mut row = serde_json::Map::new();
            row.insert("i".into(), json!(i));
            row.insert("treasury".into(), json!(round2(r.treasury)));
            if let Some(a) = self.politics.asab.get(i) {
                row.insert("asab".into(), json!(round2(*a)));
            }
            if let Some(l) = self.politics.legit.get(i) {
                row.insert("legit".into(), json!(round2(*l)));
            }
            if let Some(u) = self.politics.unrest.get(i) {
                row.insert("unrest".into(), json!(round2(*u)));
            }
            if row.len() > 1 {
                rows.push(Value::Object(row));
            }
        }
        (
            Value::Array(cold).to_string(),
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
            /// M89 — the year's composed forcing (°C on the dawn mean,
            /// M83 drift + the M86 age's offset), shipped only when its
            /// wire-precision value moved: the renderer's snowline,
            /// pack ice and tundra dress breathe with it.
            #[serde(skip_serializing_if = "Option::is_none")]
            sky: Option<f64>,
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
            /// Peoples block (ADR-0018 slow axis) — reships whole on the
            /// rare tick its content moved (era, tech, divergence, death).
            #[serde(skip_serializing_if = "Option::is_none")]
            cultures: Option<Box<RawValue>>,
            /// Civilizations block (M13 derived tier) — yearly clock,
            /// whole-block gate like the peoples.
            #[serde(skip_serializing_if = "Option::is_none")]
            civs: Option<Box<RawValue>>,
            /// Realms block (ADR-0018 political axis) — cold form: name,
            /// house, seat, ruler, vassalage, alive.
            #[serde(skip_serializing_if = "Option::is_none")]
            realms: Option<Box<RawValue>>,
            /// Heartbeat patches for realms (treasury/asab/legit), by
            /// array index — ships when only the heartbeat moved (E4.2).
            #[serde(skip_serializing_if = "Option::is_none")]
            r_hot: Option<Box<RawValue>>,
            /// People-axis influence grid as RLE (M10.6) — generational
            /// clock; reships whole when any people border moved.
            #[serde(skip_serializing_if = "Option::is_none")]
            peoples: Option<Box<RawValue>>,
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
            #[serde(skip_serializing_if = "Option::is_none")]
            territory_tiles: Option<Box<RawValue>>,
        }

        fn raw(s: String) -> Option<Box<RawValue>> {
            Some(RawValue::from_string(s).unwrap())
        }

        let ev_from = self.chronicle.events.len();
        let _ = self.tick(months); // change tracking rides self.dirty (E4.5)
        let ev_to = self.chronicle.events.len();

        // E4.8 — the toast-worthy picks, as indices into the [from, to)
        // chronicle slice the client pulls anyway; no event ships twice.
        let heads: Vec<u32> = self.chronicle.events[ev_from..ev_to]
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
            Vec::with_capacity(self.peoples.settlements.len());
        for s in &self.peoples.settlements {
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
        // Peoples move on generational clocks, so a plain whole-block gate
        // is enough. Realms get the hot/cold split (E4.2), gated on the
        // two halves directly (E5.12): full block moved ⟺ cold moved ∨
        // hot moved, so the full string is only built on the rare
        // cold-change tick. blocks[0] carries the realm hot-rows hash;
        // realms_cold the cold hash.
        let cul_s = self.cultures_json().to_string();
        let cul_h = crate::util::fnv1a64(cul_s.as_bytes());
        let cultures = if self.sent.cultures_cold != cul_h {
            self.sent.cultures_cold = cul_h;
            raw(cul_s)
        } else {
            None
        };

        // M13 — same slow-clock gate for the derived tier.
        let civ_s = self.civs_json().to_string();
        let civ_h = crate::util::fnv1a64(civ_s.as_bytes());
        let civs = if self.sent.civs_cold != civ_h {
            self.sent.civs_cold = civ_h;
            raw(civ_s)
        } else {
            None
        };

        let (rlm_cold, rlm_hot) = self.realms_cold_hot();
        let rlm_cold_h = crate::util::fnv1a64(rlm_cold.as_bytes());
        let rlm_hot_h = crate::util::fnv1a64(rlm_hot.as_bytes());
        let mut realms = None;
        let mut r_hot = None;
        if self.sent.realms_cold != rlm_cold_h {
            realms = raw(self.realms_json().to_string());
        } else if self.sent.blocks[0] != rlm_hot_h {
            r_hot = raw(rlm_hot);
        }
        self.sent.blocks[0] = rlm_hot_h;
        self.sent.realms_cold = rlm_cold_h;

        // M10.6 — the people-axis influence grid, whole-block RLE gated by
        // hash: assimilation and divergence move it a few times a century.
        let peo_s = serde_json::to_string(&politics::territory_rle(&self.fields.peoples_map))
            .unwrap();
        let peo_h = crate::util::fnv1a64(peo_s.as_bytes());
        let peoples = if self.sent.peoples_rle != peo_h {
            self.sent.peoples_rle = peo_h;
            raw(peo_s)
        } else {
            None
        };

        let block_strings = [
            serde_json::to_string(&self.politics.wars).unwrap(),
            serde_json::to_string(&self.economy.merchants).unwrap(),
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
        let market_v = self.economy.market.snapshot();
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
        let areas_v = economy::areas_json(&self.economy.areas, &self.peoples.settlements);
        let of_h = Self::areas_set_hash(&areas_v);
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

        // E4.7 — territory crosses as dirty 32×32 tile patches against the
        // last-shipped grid. A recompute that moved no border ships nothing
        // at all. Otherwise both encodings are built and the smaller one
        // ships — measured on real runs, diffuse yearly growth compresses
        // better as full-grid RLE while local conquests win as tiles, and
        // bytes on the wire are the only judge that matters.
        let (terr_full, terr_tiles) = if self.dirty.take(Dirty::TERRITORY) {
            let cur = &self.fields.territory;
            let full_s = serde_json::to_string(&politics::territory_rle(cur)).unwrap();
            if self.sent.territory.dim() == cur.dim() {
                match politics::territory_tile_patch(&self.sent.territory, cur, 32) {
                    None => (None, None), // redrawn, but every border held
                    Some((patch, _, _)) => {
                        self.sent.territory = cur.clone();
                        let patch_s = patch.to_string();
                        if patch_s.len() < full_s.len() {
                            (None, raw(patch_s))
                        } else {
                            (raw(full_s), None)
                        }
                    }
                }
            } else {
                self.sent.territory = cur.clone();
                (raw(full_s), None)
            }
        } else {
            (None, None)
        };

        let dep = self.dirty.take(Dirty::DEPOSITS);
        // M89 — the sky scalar: composed forcing at the current year,
        // crossing only when the rounded value moved (E4.2 discipline).
        let sky_now = round2(self.year_forcing(self.month.div_euclid(12)));
        let sky_q = (sky_now * 100.0).round() as i64;
        let sky = if sky_q != self.sent.sky {
            self.sent.sky = sky_q;
            Some(sky_now)
        } else {
            None
        };
        let payload = Payload {
            month: self.month,
            sky,
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
            civs,
            realms,
            r_hot,
            peoples,
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
            territory: terr_full,
            territory_tiles: terr_tiles,
        };
        // E5.8 — serialize into the reused scratch (high-water capacity,
        // zero growth reallocations), then hand back one exact-size copy.
        self.wire_buf.clear();
        serde_json::to_writer(&mut self.wire_buf, &payload).unwrap();
        std::str::from_utf8(&self.wire_buf).unwrap().to_owned()
    }

    /// E4.2/E4.3 — seed the delta baseline to the freshly generated world,
    /// which is exactly what `bootstrap()` ships; the first tick then
    /// carries only what actually moved after month 0.
    pub(crate) fn prime_sent(&mut self) {
        self.sent.territory = self.fields.territory.clone();
        self.sent.settlements = self
            .peoples.settlements
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
        self.sent.cultures_cold =
            crate::util::fnv1a64(self.cultures_json().to_string().as_bytes());
        self.sent.civs_cold =
            crate::util::fnv1a64(self.civs_json().to_string().as_bytes());
        let (rlm_cold, rlm_hot) = self.realms_cold_hot();
        self.sent.realms_cold = crate::util::fnv1a64(rlm_cold.as_bytes());
        self.sent.peoples_rle = crate::util::fnv1a64(
            serde_json::to_string(&politics::territory_rle(&self.fields.peoples_map))
                .unwrap()
                .as_bytes(),
        );
        self.sent.blocks = [
            crate::util::fnv1a64(rlm_hot.as_bytes()),
            crate::util::fnv1a64(serde_json::to_string(&self.politics.wars).unwrap().as_bytes()),
            crate::util::fnv1a64(serde_json::to_string(&self.economy.merchants).unwrap().as_bytes()),
        ];
        self.sent.market_rows = self
            .economy.market
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
        let areas_v = economy::areas_json(&self.economy.areas, &self.peoples.settlements);
        self.sent.areas_of = Self::areas_set_hash(&areas_v);
        self.sent.areas_spread =
            crate::util::fnv1a64(areas_v["spread"].to_string().as_bytes());
        self.sent.areas_hubs = areas_v["hubs"]
            .as_array()
            .unwrap()
            .iter()
            .map(Self::hub_wire)
            .collect();
    }

    /// The once-per-world bootstrap (E3.1): vocabulary tables plus the
    /// entity state ticks also carry. Its own small JSON call — the
    /// multi-megabyte pack header stops duplicating the tick payload.
    pub fn bootstrap(&self) -> Value {
        let ev_start = self.chronicle.events.len().saturating_sub(60);
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
            "settlements": self.peoples.settlements,
            "cultures": self.cultures_json(),
            "civs": self.civs_json(),
            "realms": self.realms_json(),
            "peoples": politics::territory_rle(&self.fields.peoples_map),
            "features": self.features,
            "routes": self.routes,
            "ruins": self.ruins,
            "wars": self.politics.wars,
            "market": self.economy.market.snapshot(),
            "areas": economy::areas_json(&self.economy.areas, &self.peoples.settlements),
            "merchants": self.economy.merchants,
            "events": self.chronicle.events[ev_start..],
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
}
