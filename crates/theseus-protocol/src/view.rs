//! `view.show` (issue 44): what a turn was shown and why, from the view record the context assembler
//! writes for each view it builds. Ids, kinds, forms, tokens, reasons and counts by default; a piece's text
//! only when asked for (`text`) and only for the owner at his own terminal. Enumerated values travel as their
//! names, so this crate carries none of the core's types.

use serde::{Deserialize, Serialize};

/// `view.show`: the views of one turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewShowParams {
    pub turn_id: String,
    /// Each piece's text as well: the owner's own terminal only; refused anywhere else.
    pub text: bool,
    /// The job's session when the CLI runs inside a Theseus job (`THESEUS_SESSION`): text is refused then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_job: Option<String>,
}

/// A turn's views, one per compiled view, in the order they were built.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewShowResult {
    pub turn_id: String,
    pub views: Vec<ViewShown>,
    /// Why some of it is not here, when it is not: text not asked for, or pieces of other places.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withheld: Option<String>,
}

/// One view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewShown {
    pub compilation: String,
    /// `transcript` or `view`.
    pub assembly: String,
    pub pov: ViewPov,
    /// Each slot that holds a piece, in the slots' order (A to I).
    pub slots: Vec<ViewSlot>,
    pub dropped: Vec<ViewDrop>,
    pub residue: Vec<ViewResidue>,
    pub totals: ViewTotals,
}

/// The point of view the view was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewPov {
    /// `place` or `task`, and its id.
    pub anchor_kind: String,
    pub anchor: String,
    /// `owner_alone`, `private`, `shared`, `open`, `unknown`.
    pub present: String,
    /// Whether the owner's mark named a person (never the name).
    pub person: bool,
    /// `line_of_work` or `task`, and its id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_hand: Option<String>,
    pub now_ms: u64,
}

/// One slot's pieces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSlot {
    /// `frame`, `who_and_where`, `waiting`, `in_play`, `past`, `elsewhere`, `here`, `called`, `now`.
    pub slot: String,
    pub pieces: Vec<ViewPiece>,
    pub tokens: u64,
}

/// One piece shown, by reference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewPiece {
    pub handle: String,
    /// What the range is a range of: `exchanges`, `folds`, `facts`, `card`, `task`, `code`.
    pub kind: String,
    /// The first and last ids of the range.
    pub first: String,
    pub last: String,
    pub band: String,
    pub form: String,
    pub tokens: u64,
    pub reasons: Vec<String>,
    /// The heading the model is told it is under: `here`, `his_word_elsewhere`, `testimony`, `author_unknown`,
    /// `outside_text`.
    pub label: String,
    /// The piece's text, when asked for and allowed; none when its source holds no text this build reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// One piece left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewDrop {
    pub handle: String,
    pub kind: String,
    pub slot: String,
    pub band: String,
    pub reasons: Vec<String>,
    pub tokens: u64,
    /// `merged`, `over_budget`, `unit_cut`.
    pub why: String,
}

/// What a band cut, counted: stated, not silent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewResidue {
    pub band: String,
    pub pieces: u32,
    pub tokens: u64,
}

/// A view's totals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewTotals {
    pub shown: u64,
    pub dropped: u64,
    pub tokens: u64,
    pub dropped_tokens: u64,
    /// Pieces of other places, counted and not listed (a reader in company).
    pub held_out: u64,
}

/// A shown view in plain lines, for the CLI and the model's look: the point of view, each slot and its
/// pieces, the drops, the residue, the totals.
pub fn lines(v: &ViewShown) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let p = &v.pov;
    let _ = writeln!(
        out,
        "view {} ({}): {} {}, present {}, at {}",
        v.compilation, v.assembly, p.anchor_kind, p.anchor, p.present, p.now_ms
    );
    if let Some(a) = &p.at_hand {
        let _ = writeln!(out, "  at hand: {a}");
    }
    for s in &v.slots {
        let _ = writeln!(out, "  {} ({} tokens)", s.slot, s.tokens);
        for x in &s.pieces {
            let range = match x.first == x.last {
                true => x.first.clone(),
                false => format!("{}..{}", x.first, x.last),
            };
            let _ = writeln!(
                out,
                "    {} {} {} [{}] {} {} tokens, {}: {}",
                x.handle,
                x.kind,
                range,
                x.band,
                x.form,
                x.tokens,
                x.label,
                x.reasons.join(", ")
            );
            if let Some(t) = &x.text {
                for line in t.lines() {
                    let _ = writeln!(out, "      | {line}");
                }
            }
        }
    }
    if !v.dropped.is_empty() {
        let _ = writeln!(out, "  dropped");
        for d in &v.dropped {
            let _ = writeln!(
                out,
                "    {} {} {} [{}] {} tokens, {}: {}",
                d.handle,
                d.kind,
                d.slot,
                d.band,
                d.tokens,
                d.why,
                d.reasons.join(", ")
            );
        }
    }
    for r in &v.residue {
        let _ = writeln!(
            out,
            "  residue {}: {} pieces, {} tokens",
            r.band, r.pieces, r.tokens
        );
    }
    let t = &v.totals;
    let _ = write!(
        out,
        "  total: {} shown, {} tokens; {} dropped, {} tokens",
        t.shown, t.tokens, t.dropped, t.dropped_tokens
    );
    if t.held_out > 0 {
        let _ = write!(out, "; {} of other places, not listed here", t.held_out);
    }
    out.push('\n');
    out
}
