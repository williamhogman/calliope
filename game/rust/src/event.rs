//! The chronicle's wire vocabulary (E11.8): `Event`, its kinds, and the
//! per-kind table — the one module every teller imports, so no leaf ever
//! needs `world.rs` just to speak.

use serde::Serialize;
use smallvec::SmallVec;

use crate::ids::EntityId;

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
    /// M12 — the kindred clock: assimilation, divergence, fusion. Appended
    /// last so existing kinds keep their wire discriminants.
    Kindred,
    /// M13 — the arc of empires: civilization set-pieces (a golden age
    /// dawns, the court rots, the empire breaks, successors are named).
    /// Appended last; wire discriminants hold.
    Era,
    /// M22 — the deep earth speaks: earthquakes off the fault seams.
    /// Vocabulary lands here; M24 wires the chronicle beats. Appended
    /// last; wire discriminants hold.
    Quake,
    /// M24 — the mountain gives its answer: eruptions off the cone
    /// record, felled towns and all. Appended last; wire discriminants
    /// hold.
    Eruption,
    /// M80 — the failed year named: a multi-year drought takes hold over
    /// named ground. Appended last; wire discriminants hold.
    Drought,
    /// M81 — the river that drowns and gives: a spate overtops the levees
    /// and leaves silt behind it. Appended last; wire discriminants hold.
    Flood,
    /// M86 — the cold ages: a multidecadal winter settles or releases,
    /// dated by the schedule the seed drew. Appended last; wire
    /// discriminants hold.
    Age,
    /// M87 — the generous centuries: a warm optimum opens or closes,
    /// dated by the same schedule. Appended last; wire discriminants
    /// hold.
    Optimum,
    /// M90 — fields at the edge: a kind sky opens marginal ground to
    /// the plough. Appended last; wire discriminants hold.
    Clearing,
    /// M90 — the margin fails: upland fields go back to the wild under
    /// a cold sky. Appended last; wire discriminants hold.
    Abandon,
    /// M93 — lakes that breathe: a terminal lake crosses a recorded
    /// extreme and leaves a dated strandline. Appended last; wire
    /// discriminants hold.
    Strandline,
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
    Kindred   => family realm,   weight 3, fortune 0;
    Era       => family realm,   weight 4, fortune 0;
    Quake     => family nature,  weight 3, fortune -1;
    Eruption  => family nature,  weight 3, fortune -1;
    Drought   => family nature,  weight 3, fortune -1;
    Flood     => family nature,  weight 3, fortune -1;
    Age       => family nature,  weight 4, fortune -1;
    Optimum   => family nature,  weight 4, fortune 1;
    // M90 — fields at the edge: the margin opens under a kind sky…
    Clearing  => family nature,  weight 2, fortune 1;
    // …and fails under a cold one. Appended last: wire codes are stable.
    Abandon   => family nature,  weight 2, fortune -1;
    // M93 — a shore that rose or fell past every mark. Appended last.
    Strandline => family nature, weight 2, fortune 0;
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
            | EventKind::Kindred
            | EventKind::Era
            | EventKind::Quake
            | EventKind::Eruption
            | EventKind::Drought
            | EventKind::Age
            | EventKind::Optimum
            | EventKind::Strandline
    )
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
