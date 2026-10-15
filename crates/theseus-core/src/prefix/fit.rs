//! The pick, pure (theseus-rr15): a layer's sections, a header's fit to the
//! fresh layer, and the reuse, coarser or mint decision with its economics.
//!
//! **Sections.** Layer 3 is the system-level context files (one section,
//! `files`: every session on a config and a place class carries the same
//! ones) and then each guidance section of the session's categories, named
//! by its first line (the category's header). A section is its name, its
//! text's digest and its bytes.
//!
//! **Fit.** A header fits a fresh layer when every section it holds is in the
//! fresh layer byte for byte: fit is the share of the fresh layer's bytes it
//! covers. A header with a section of the same name and other bytes (a
//! context file or a guidance edited since: its source changed) or a section
//! the fresh layer does not hold (another project's or topic's guidance: the
//! near miss) never fits. Surplus is not tolerated: another project's test
//! command in the cached prefix, beside this one's in the delta, is the false
//! fit the guard exists for.
//!
//! **The decision** (`decide`), on the best fitting header (ties: the warm
//! one, then the bigger):
//! - fit at or above `confirm`: **reuse** it verbatim; the sections it lacks
//!   go after the cached prefix (the delta). Below `act` it is watched: a turn
//!   that goes wrong after it is a labelled false fit.
//! - below `confirm`: **mint** the fresh layer as a header of its own when
//!   `p(reuse within TTL) × reads × (write − read) × delta > (write − read) ×
//!   warm coarser`, i.e. when the expected reads of the new header save more
//!   rewriting of its new part than minting costs by writing the coarser
//!   header's part again instead of reading it. Else **coarser**: reuse the
//!   best (the project's header without the topic) with the bigger delta. A
//!   cold coarser header costs nothing to pass by, so a mint always pays then;
//!   with no header at all the fresh layer is minted.

use serde::{Deserialize, Serialize};

/// The files section's name.
pub const FILES: &str = "files";

/// A section of a layer: its name, its text's digest and its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub digest: String,
    pub bytes: u64,
}

/// A layer's sections with their text: the files' text, then each guidance
/// section. Empty ones are left out; joined by a blank line they are the
/// layer's text, as `ontology::guided` joins them.
pub fn sections_of<'a>(files: &'a str, guidance: &'a [String]) -> Vec<(Section, &'a str)> {
    let mut out = Vec::new();
    if !files.is_empty() {
        out.push((section(FILES, files), files));
    }
    for g in guidance.iter().filter(|g| !g.is_empty()) {
        let name = g.lines().next().unwrap_or_default().trim();
        out.push((section(name, g), g.as_str()));
    }
    out
}

fn section(name: &str, text: &str) -> Section {
    Section {
        name: name.to_string(),
        digest: crate::compiler::sha(text),
        bytes: text.len() as u64,
    }
}

/// The text of `sections`, joined as the layer joins them.
pub fn join<'a>(texts: impl IntoIterator<Item = &'a str>) -> String {
    texts.into_iter().collect::<Vec<_>>().join("\n\n")
}

/// How much of `fresh` the header of `held` covers, or none when it does not
/// fit (a section changed since, or one the fresh layer lacks).
pub fn fit(fresh: &[Section], held: &[Section]) -> Option<f64> {
    let total: u64 = fresh.iter().map(|s| s.bytes).sum();
    if total == 0 || held.is_empty() {
        return None;
    }
    let mut covered = 0;
    for h in held {
        let same = fresh.iter().find(|f| f.name == h.name)?;
        if same.digest != h.digest {
            return None;
        }
        covered += h.bytes;
    }
    Some(covered as f64 / total as f64)
}

/// The fresh sections a header of `held` lacks, in the fresh layer's order:
/// the delta that goes after the cached prefix.
pub fn missing<'a>(fresh: &'a [Section], held: &[Section]) -> Vec<&'a Section> {
    fresh
        .iter()
        .filter(|f| !held.iter().any(|h| h.digest == f.digest))
        .collect()
}

/// What the pick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// The best header at or above `confirm`, verbatim, with a delta.
    Reuse,
    /// Below `confirm`, minting did not pay: the best header with the bigger delta.
    Coarser,
    /// The fresh layer, a header of its own.
    Mint,
}

impl Choice {
    pub fn as_str(self) -> &'static str {
        match self {
            Choice::Reuse => "reuse",
            Choice::Coarser => "coarser",
            Choice::Mint => "mint",
        }
    }
}

/// The best fitting header, as the decision reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Best {
    pub fit: f64,
    /// Its prefix is in the provider's cache for this model now.
    pub warm: bool,
    /// Its size, in tokens.
    pub tokens: u64,
}

/// What the hit log says of headers like the one a mint would make: how
/// likely one is read again within its TTL, and how often once it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Odds {
    pub p_reuse: f64,
    pub reads: f64,
}

impl Odds {
    /// From the hit log of the headers on the same lower layers: `minted`
    /// headers, of which `reused` were read again at least once, `reads`
    /// times in all. Laplace's rule: no history is an even chance and one read.
    pub fn of(minted: u64, reused: u64, reads: u64) -> Odds {
        Odds {
            p_reuse: (reused as f64 + 1.0) / (minted as f64 + 2.0),
            reads: (reads as f64 + 1.0) / (reused as f64 + 1.0),
        }
    }
}

/// The model's prices per million tokens of a cache write (at the header's
/// TTL) and a cache read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices {
    pub write: f64,
    pub read: f64,
}

/// The bands: `[prefix] act` and `confirm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bands {
    pub act: f64,
    pub confirm: f64,
}

/// A mint's sums, in dollars, for the record: what its expected reads save,
/// and what it costs over reusing the coarser header.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sums {
    pub gain_usd: f64,
    pub premium_usd: f64,
}

/// The sums for minting a fresh layer of `fresh_tokens` over `best`.
pub fn sums(best: Option<Best>, fresh_tokens: u64, odds: Odds, prices: Prices) -> Sums {
    let per = (prices.write - prices.read).max(0.0) / 1e6;
    let coarser = best.map_or(0, |b| b.tokens);
    let delta = fresh_tokens.saturating_sub(coarser) as f64;
    let warm = best.filter(|b| b.warm).map_or(0, |b| b.tokens) as f64;
    Sums {
        gain_usd: odds.p_reuse * odds.reads * per * delta,
        premium_usd: per * warm,
    }
}

/// Reuse, coarser or mint (the module's rule).
pub fn decide(
    best: Option<Best>,
    fresh_tokens: u64,
    bands: Bands,
    odds: Odds,
    prices: Prices,
) -> (Choice, Sums) {
    let s = sums(best, fresh_tokens, odds, prices);
    let Some(b) = best else {
        return (Choice::Mint, s);
    };
    if b.fit >= bands.confirm {
        return (Choice::Reuse, s);
    }
    // A cold coarser header costs nothing to pass by: a mint then pays.
    let pays = s.gain_usd > s.premium_usd || (!b.warm && s.gain_usd >= s.premium_usd);
    if pays {
        (Choice::Mint, s)
    } else {
        (Choice::Coarser, s)
    }
}
