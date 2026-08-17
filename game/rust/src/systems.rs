//! The system lattice (E11.4/E11.5).
//!
//! The tick loop is an ordered list of systems, each with a name and a
//! declared cadence, instead of 170 inline lines. Systems emit into an
//! `EventSink` — the sink keeps the month's slice boundaries and the
//! cross-month flags (founded / deposits / borders) that used to be six
//! loose locals threaded through the loop.
//!
//! Order is law: the list below IS the month, top to bottom, and any
//! reordering is a balance change that must clear the determinism gate.

use std::collections::HashMap;

use crate::economy;
use crate::naming;
use crate::settlements::SettlementId;
use crate::society;
use crate::telling;
use crate::trade;
use crate::world::{Dirty, Event, EventKind, World};
use crate::{artifact, chronicle, politics};

// ---------------------------------------------------------------- cadence

/// When a system runs (E11.4): every month, or every `n` months on `phase`.
#[derive(Clone, Copy, PartialEq)]
pub enum Cadence {
    Monthly,
    EveryN { n: i64, phase: i64 },
}

impl Cadence {
    #[inline]
    pub fn due(self, month: i64) -> bool {
        match self {
            Cadence::Monthly => true,
            Cadence::EveryN { n, phase } => month.rem_euclid(n) == phase,
        }
    }
}

// ---------------------------------------------------------------- sink

/// The month's ledger (E11.5): systems emit, the sink orders and stamps.
pub struct EventSink {
    pub events: Vec<Event>,
    /// Index of the current month's first event — `resolve`, `veil`, the
    /// relic pass and the heat integral all read this month's slice.
    pub month_start: usize,
    /// A town was founded this tick (colony or rush camp) — routes reship.
    pub founded: bool,
    /// A seam was struck or died this tick — the mineral ledger reships.
    pub deposits_changed: bool,
    /// Land changed hands THIS MONTH — the political map redraws.
    pub borders_changed: bool,
}

impl EventSink {
    pub fn new() -> EventSink {
        EventSink {
            events: Vec::new(),
            month_start: 0,
            founded: false,
            deposits_changed: false,
            borders_changed: false,
        }
    }

    /// A new month begins: slice boundary moves, the border flag resets
    /// (founded / deposits accumulate across the whole tick call).
    pub fn begin_month(&mut self) {
        self.month_start = self.events.len();
        self.borders_changed = false;
    }

    #[inline]
    pub fn emit(&mut self, evs: impl IntoIterator<Item = Event>) {
        self.events.extend(evs);
    }

    /// This month's slice, oldest first.
    #[inline]
    pub fn month(&self) -> &[Event] {
        &self.events[self.month_start..]
    }
}

impl Default for EventSink {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- context

/// What a system sees: the world, plus the month's shared scratch.
pub struct SimCtx<'w> {
    pub world: &'w mut World,
    /// E5.2 — one id→index map per month, built by `Census`, read by the
    /// economy block. Settlement membership is fixed from the census to
    /// the end of the economy passes.
    pub sidx: HashMap<SettlementId, usize>,
}

// ---------------------------------------------------------------- trait

/// One simulation system: a name for the report, a cadence, a body.
pub trait System: Sync {
    fn name(&self) -> &'static str;
    fn cadence(&self) -> Cadence {
        Cadence::Monthly
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink);
}

/// The month, top to bottom. Reordering is a balance change.
pub static SYSTEMS: &[&dyn System] = &[
    &Towns,
    &Famine,
    &Colonize,
    &Prospect,
    &RushCamps,
    &Goods,
    &Exonyms,
    &Society,
    &Census,
    &MarketAreas,
    &Crafts,
    &EconomyPulse,
    &Merchants,
    &Statecraft,
    &Patina,
    &Territory,
    &ChroniclePulse,
    &Relics,
    &SecondReading,
    &Veil,
    &Heat,
];

// ---------------------------------------------------------------- systems

/// Growth, decline, plague, fire — every town's own month.
struct Towns;
impl System for Towns {
    fn name(&self) -> &'static str {
        "towns"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = w.tick_month(w.month);
        sink.emit(evs);
    }
}

/// M2.4 — thin years: drought against the granary.
struct Famine;
impl System for Famine {
    fn name(&self) -> &'static str {
        "famine"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = w.famine_pass(w.month);
        sink.emit(evs);
    }
}

/// New towns where the land calls loudest.
struct Colonize;
impl System for Colonize {
    fn name(&self) -> &'static str {
        "colonize"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let (evs, did) = w.try_colonize(w.month);
        if did {
            sink.founded = true;
        }
        sink.emit(evs);
    }
}

/// Hidden seams struck, worked seams spent (M5.3).
struct Prospect;
impl System for Prospect {
    fn name(&self) -> &'static str {
        "prospect"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let (evs, changed) = w.prospect_and_deplete(w.month);
        if changed {
            sink.deposits_changed = true;
        }
        sink.emit(evs);
    }
}

/// Rushes ride behind the strikes: unworked seams call their own camps.
struct RushCamps;
impl System for RushCamps {
    fn name(&self) -> &'static str {
        "rush-camps"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let (evs, did) = w.try_rush_camps(w.month);
        if did {
            sink.founded = true;
        }
        sink.emit(evs);
    }
}

/// Once a year every town re-reads its hinterland: territories grow with
/// population, and a seam struck beyond yesterday's reach must not rust
/// in the hills once the town has grown to it.
struct Goods;
impl System for Goods {
    fn name(&self) -> &'static str {
        "goods"
    }
    fn cadence(&self) -> Cadence {
        Cadence::EveryN { n: 12, phase: 0 }
    }
    fn run(&self, ctx: &mut SimCtx, _sink: &mut EventSink) {
        let w = &mut *ctx.world;
        trade::assign_goods(&mut w.peoples.settlements, &w.deposits, &w.fields.fertility);
    }
}

/// Once a decade the tongues catch up with the map: a people spread near
/// a named feature coins its own word for it (M3.4).
struct Exonyms;
impl System for Exonyms {
    fn name(&self) -> &'static str {
        "exonyms"
    }
    fn cadence(&self) -> Cadence {
        Cadence::EveryN { n: 120, phase: 0 }
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let doubled = naming::exonym_pass(
            &mut w.features,
            &w.peoples.settlements,
            &w.peoples.cultures,
            &mut w.taken,
            &mut w.rng,
        );
        for (fname, people, alt) in doubled {
            w.dirty.mark(Dirty::FEATURES);
            sink.emit([Event {
                m: w.month,
                s: fname.clone(),
                k: EventKind::Society,
                text: format!(
                    "Spread now into that country, the {} keep their own word for {} — in their tongue it is {}.",
                    people, fname, alt
                ),
                ..Default::default()
            }]);
        }
    }
}

/// The arts advance: technologies discovered, eras entered (M4/M5).
struct Society;
impl System for Society {
    fn name(&self) -> &'static str {
        "society"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = society::monthly(&mut w.peoples, &w.deposits, w.month, &mut w.rng);
        sink.emit(evs);
    }
}

/// E5.2 — one id→index map for every pass this month. Settlement
/// membership is fixed from here to the end of the economy block
/// (the passes below take slices, which cannot grow or shrink).
struct Census;
impl System for Census {
    fn name(&self) -> &'static str {
        "census"
    }
    fn run(&self, ctx: &mut SimCtx, _sink: &mut EventSink) {
        ctx.sidx = economy::sidx(&ctx.world.peoples.settlements);
    }
}

/// M5.2 — re-carve the market areas when towns appeared, and refresh
/// every other year as the route web thickens.
struct MarketAreas;
impl System for MarketAreas {
    fn name(&self) -> &'static str {
        "market-areas"
    }
    fn run(&self, ctx: &mut SimCtx, _sink: &mut EventSink) {
        let w = &mut *ctx.world;
        if w.economy.areas.area.len() != w.peoples.settlements.len()
            || w.month.rem_euclid(24) == 2
        {
            w.economy.areas = economy::build_areas(
                &w.peoples.settlements,
                &w.routes,
                Some(&w.economy.areas),
                &ctx.sidx,
            );
        }
    }
}

/// M5.1 — forges light where ore, fuel, hands and the art meet.
struct Crafts;
impl System for Crafts {
    fn name(&self) -> &'static str {
        "crafts"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = economy::craft_pass(&mut w.peoples, &w.economy.areas, w.month, &mut w.rng);
        sink.emit(evs);
    }
}

/// Prices move, wealth accrues, booms and busts are called.
struct EconomyPulse;
impl System for EconomyPulse {
    fn name(&self) -> &'static str {
        "economy"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = economy::monthly(
            &mut w.economy,
            &mut w.peoples,
            &w.routes,
            w.month,
            &mut w.rng,
            &ctx.sidx,
        );
        sink.emit(evs);
    }
}

/// M5.5 — the merchants ride the widest gaps.
struct Merchants;
impl System for Merchants {
    fn name(&self) -> &'static str {
        "merchants"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = economy::merchant_pass(
            &mut w.economy,
            &mut w.peoples,
            &w.routes,
            &mut w.taken,
            w.month,
            &mut w.rng,
            &mut w.chronicle.registry,
            &ctx.sidx,
        );
        sink.emit(evs);
    }
}

/// Statecraft: wars that move borders, dread, risings (M4).
struct Statecraft;
impl System for Statecraft {
    fn name(&self) -> &'static str {
        "statecraft"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let (evs, borders) = politics::monthly(
            &mut w.politics,
            &mut w.chronicle,
            &mut w.peoples,
            &w.fields.territory,
            &mut w.taken,
            w.month,
            &mut w.rng,
        );
        if borders {
            sink.borders_changed = true;
        }
        sink.emit(evs);
    }
}

/// The patina settles behind the drums: battlefields earn names,
/// conquerors rename, towns die to ruin, roads fade, names wear (M9).
struct Patina;
impl System for Patina {
    fn name(&self) -> &'static str {
        "patina"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = w.patina_pass(w.month);
        sink.emit(evs);
    }
}

/// Redraw the political map when land changed hands, and once a year
/// regardless — growing towns push their reach outward.
struct Territory;
impl System for Territory {
    fn name(&self) -> &'static str {
        "territory"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        if sink.borders_changed || w.month.rem_euclid(12) == 6 {
            w.recompute_territory();
        }
    }
}

/// The human pulse, paced by how loud the world already is (M6.4).
struct ChroniclePulse;
impl System for ChroniclePulse {
    fn name(&self) -> &'static str {
        "chronicle"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let pace = (1.30 - 0.22 * w.heat).clamp(0.55, 1.30);
        let evs = chronicle::monthly(
            &mut w.chronicle,
            &mut w.peoples,
            &w.features,
            &w.world_name,
            &mut w.taken,
            w.month,
            &mut w.rng,
            pace,
        );
        sink.emit(evs);
    }
}

/// The relics ride the month's tides: forged, plundered, lost (M6.3) —
/// read straight off the month's slice, no clone (E5.6).
struct Relics;
impl System for Relics {
    fn name(&self) -> &'static str {
        "relics"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let w = &mut *ctx.world;
        let evs = artifact::monthly(
            &mut w.chronicle,
            sink.month(),
            &w.peoples,
            &mut w.taken,
            w.month,
            &mut w.rng,
        );
        sink.emit(evs);
    }
}

/// Second reading: back-fill ids, anchor coordinates, and let the great
/// deeds pass into legend (M6.1, M6.9).
struct SecondReading;
impl System for SecondReading {
    fn name(&self) -> &'static str {
        "second-reading"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let from = sink.month_start;
        ctx.world.resolve_events(from, &mut sink.events);
    }
}

/// Third reading: the record admits what it does not know (M9.5).
struct Veil;
impl System for Veil {
    fn name(&self) -> &'static str {
        "veil"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let from = sink.month_start;
        ctx.world.veil_pass(&mut sink.events, from);
    }
}

/// Narrative heat: the month's weighted noise, slowly cooling (M6.4).
struct Heat;
impl System for Heat {
    fn name(&self) -> &'static str {
        "heat"
    }
    fn run(&self, ctx: &mut SimCtx, sink: &mut EventSink) {
        let m_heat: i32 = sink
            .month()
            .iter()
            .map(|e| telling::weight(e.k) - 1)
            .sum();
        let w = &mut *ctx.world;
        w.heat = w.heat * 0.94 + (m_heat as f64 / 6.0) * 0.06;
    }
}
