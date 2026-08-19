//! Patina — residue, strata and the withheld (M9). Worlds feel old when
//! things are allowed to die and leave marks: towns fall to ruin and keep
//! their names, conquest lays a new name over an old one without erasing
//! it, long-spoken words wear smooth, battlefields stay named after the
//! armies have gone, and the chronicle sometimes refuses to explain
//! itself. Everything here is deterministic — worn forms and withheld
//! codas key off `det_hash`, never wall-clock, never iteration order.

use crate::ids::EntityId;
use serde::Serialize;

use crate::telling::det_hash;

/// A dead settlement's remains: a named place on the map where a town
/// stood, carrying who lived there, when it emptied, and why.
#[derive(Serialize, Clone)]
pub struct Ruin {
    /// "Ruins of {town}" — display name; the town's own name is `of`.
    pub name: String,
    /// The town that was.
    pub of: String,
    pub x: i64,
    pub y: i64,
    /// Month of abandonment.
    pub since: i64,
    /// One line of why: famine, spent mines, war, slow decline.
    pub why: String,
    /// People whose town it was.
    pub people: String,
    /// Reading of the old name's parts, carried over from the town.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ety: String,
    /// Registry entity id (kind "ruin").
    pub eid: EntityId,
}

const VOWELS: &[char] = &['a', 'e', 'i', 'o', 'u', 'y'];

fn is_vowel(c: char) -> bool {
    VOWELS.contains(&c.to_ascii_lowercase())
}

/// Longest run of consonants in the word — worn forms must stay sayable.
fn worst_cluster(w: &str) -> usize {
    let mut run = 0usize;
    let mut worst = 0usize;
    for c in w.chars() {
        if c.is_alphabetic() && !is_vowel(c) {
            run += 1;
            worst = worst.max(run);
        } else {
            run = 0;
        }
    }
    worst
}

/// Wear a name smooth the way centuries of speech do (M9.3): drop an
/// unstressed internal syllable (Aldenford → Aldford), then collapse any
/// doubled letter the contraction created. Deterministic and rule-based;
/// returns None when the word is too short or the result would be
/// unsayable, so callers can simply skip those.
pub fn erode_word(name: &str) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let n = chars.len();
    if n < 8 || !chars.iter().all(|c| c.is_alphabetic()) {
        return None;
    }
    // Find an internal vowel+consonant pair to elide: position 3..n-4 so
    // the head and the final syllable — the parts a name is known by —
    // both survive the wearing.
    for i in 3..n.saturating_sub(4) {
        if is_vowel(chars[i]) && !is_vowel(chars[i + 1]) {
            let mut worn: Vec<char> = Vec::with_capacity(n - 2);
            worn.extend_from_slice(&chars[..i]);
            worn.extend_from_slice(&chars[i + 2..]);
            // collapse a doubled letter at the new seam
            let mut out = String::with_capacity(worn.len());
            let mut prev = '\0';
            for &c in &worn {
                if c.to_ascii_lowercase() != prev {
                    out.push(c);
                }
                prev = c.to_ascii_lowercase();
            }
            if out.len() >= 5 && worst_cluster(&out) <= 3 && out != name {
                return Some(out);
            }
        }
    }
    None
}

/// Wear the headword inside a feature phrase ("the Maerenholt Hills" →
/// "the Maerholt Hills"): erode the longest capitalized token that is not
/// a leading article or generic. Returns (worn phrase, old word, new word).
pub fn erode_phrase(name: &str) -> Option<(String, String, String)> {
    let tokens: Vec<&str> = name.split(' ').collect();
    let (ti, tok) = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.len() >= 8
                && t.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !matches!(t.to_ascii_lowercase().as_str(), "the")
        })
        .max_by_key(|(i, t)| (t.len(), usize::MAX - *i))?;
    let worn = erode_word(tok)?;
    let mut out: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
    out[ti] = worn.clone();
    Some((out.join(" "), tok.to_string(), worn))
}

/// Withheld codas (M9.5): the chronicle admits the limits of its own
/// knowing. Appended to a bounded share of entries.
pub const WITHHELD: &[&str] = &[
    " Why, none now remember.",
    " The annals say no more.",
    " Some deny it happened at all.",
    " The record breaks off there.",
    " Two tellings survive, and they do not agree.",
    " What came of it, no page records.",
    " The chroniclers of that age wrote nothing further, and were perhaps wise.",
];

/// Berúthiel emissions (M9.5): things that happen and are never explained.
/// {T} town · {P} people. Emitted rarely, always veiled.
pub const UNEXPLAINED: &[&str] = &[
    "A ship with black sails came by night to {T}; no crew was found aboard, and the {P} burned it at the water's edge.",
    "For nine days no bird sang in the fields about {T}. The annals give no cause.",
    "A door was found in the hills above {T}, opening on bare earth. The {P} sealed it, and do not speak of it.",
    "Every cat in {T} vanished on one night and returned on another. Nothing else is recorded.",
    "A bell was heard from beneath the water near {T}. It rang thirteen times.",
    "Three riders in grey passed through {T} without stopping. None saw their faces; none has seen them since.",
    "The stars over {T} stood wrong for one night, say the herders. The court astronomers of the {P} deny it.",
    "A field near {T} bloomed in midwinter, with flowers of no known kind. By morning they were gone.",
    "A man came to {T} claiming to be its founder, dead these many lifetimes. He knew where the old wells were.",
    "The {P} of {T} woke one morning to find every door in the town standing open, and nothing taken.",
];

/// Pick a withheld coda for this event text, deterministically.
pub fn coda_for(seed: u64, text: &str) -> &'static str {
    WITHHELD[(det_hash(seed, text) % WITHHELD.len() as u64) as usize]
}

/// Why a town emptied, in the chronicle's voice.
pub fn ruin_why(cause: &str) -> &'static str {
    match cause {
        "famine" => "hunger emptied it",
        "mines" => "the seams gave out and the miners drifted away",
        "war" => "war broke it and none returned",
        // M24 — the sudden endings: the disaster passes fell through the
        // one kill path with these causes, and `diagnose civ` matches
        // ruins to their chronicle beats by these exact strings.
        "quake" => "the earth broke it in a single morning",
        "ash" => "the mountain buried it in ash and fire",
        _ => "it dwindled year by year until the last hearth went cold",
    }
}

// ---------------------------------------------------------------- bands

/// Diagnostics bands (E11.6): the residue budget of mature worlds.
pub const BANDS: &[crate::util::Band] = &[
    crate::util::Band { name: "ruins per century (after y100)", sweet: (1.0, 12.0), hard: (0.5, 20.0), target: "M9.1 gate: mature worlds carry ruins" },
    crate::util::Band { name: "withheld share of the chronicle", sweet: (0.02, 0.08), hard: (0.015, 0.10), target: "M9.5 gate: 2-8% of entries veiled" },
];
