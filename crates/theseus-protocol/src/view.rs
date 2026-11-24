//! `view.show` (issue 44): what a turn was shown and why, from the view record the context assembler
//! writes for each view it builds. The owner's inspection, whole or not at all: only the owner in person may
//! ask. Ids, kinds, forms, tokens and reasons by default; a piece's text (`text`) and the view's counts
//! (`tally`) when asked for. A piece whose source may not be read today is not listed, and nothing counts
//! it. Enumerated values travel as their names, so this crate carries none of the core's types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `view.show`: the views of one turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewShowParams {
    pub turn_id: String,
    /// Each piece's text as well: the owner's own terminal only.
    pub text: bool,
    /// Each view's counts by slot, by reason and by why a piece was dropped.
    pub tally: bool,
    /// The job's session when the CLI runs inside a Theseus job (`THESEUS_SESSION`): refused then.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// A turn's views, one per compiled view, in the order they were built.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewShowResult {
    pub turn_id: String,
    pub views: Vec<ViewShown>,
    /// Each view's counts, when asked for (`tally`), in the order of `views`.
    pub tallies: Vec<ViewTally>,
    /// The compilations of the turn's views this build cannot read (a later build's names), in the order
    /// built: not in `views`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<String>,
    /// Why some of it is not here: text not asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<String>,
}

/// One view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// For a reader who is not the owner alone: one fixed sentence, whatever was left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub restricted: Option<String>,
}

/// The point of view the view was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    #[cfg_attr(test, ts(optional))]
    pub at_hand: Option<String>,
    pub now_ms: u64,
}

/// One slot's pieces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewSlot {
    /// `frame`, `who_and_where`, `waiting`, `in_play`, `past`, `elsewhere`, `here`, `called`, `now`.
    pub slot: String,
    pub pieces: Vec<ViewPiece>,
    pub tokens: u64,
}

/// One piece shown, by reference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// What Jev says about the items the piece is read from (issue 44), one line each, `<node>: <what>`:
    /// the ask and the reply of a piece's first and last exchange, or a recalled node. Absent for a piece
    /// made from items (a fold, a card, a fact): its labels are its sources'.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The piece's text, when asked for and allowed; none when its source holds no text this build reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// A fold's rung: 1 over exchanges, 2 and up over folds. Absent for every other kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub level: Option<u8>,
}

/// One piece left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// A fold's rung, as [`ViewPiece::level`]. Absent for every other kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub level: Option<u8>,
}

/// What a band cut, counted: stated, not silent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewResidue {
    pub band: String,
    pub pieces: u32,
    pub tokens: u64,
}

/// A view's totals: of what is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewTotals {
    pub shown: u64,
    pub dropped: u64,
    pub tokens: u64,
    pub dropped_tokens: u64,
}

/// How many pieces, and how many tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewCount {
    pub pieces: u64,
    pub tokens: u64,
}

/// One view's counts, by name: slots, reasons, and why pieces were dropped (`merged`, `over_budget`,
/// `unit_cut`) and the reasons of those dropped. A piece with two reasons counts under each.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewTally {
    pub compilation: String,
    pub present: String,
    pub slots: BTreeMap<String, ViewCount>,
    pub reasons: BTreeMap<String, ViewCount>,
    pub dropped: BTreeMap<String, ViewCount>,
    pub dropped_reasons: BTreeMap<String, ViewCount>,
    pub tokens: u64,
}

/// `view.preview`: the view a place's next turn would get, assembled now, sent nowhere and kept nowhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewPreviewParams {
    /// The place, as the ledger names it (`local:<dir>`, a binding's target).
    pub place: String,
    /// The next turn's message; none previews for an empty one (said in the result).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub message: Option<String>,
    /// Each piece's text as well: the owner's own terminal only.
    pub text: bool,
    /// The job's session when the CLI runs inside a Theseus job (`THESEUS_SESSION`): refused then.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// The view's budget, in tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewBudget {
    pub provider_window: u64,
    /// Kept for the reply and the turn's own calls.
    pub reserve: u64,
    pub ceiling: u64,
    pub here_allowance: u64,
}

/// The earlier turns of the place's sessions, counted from what the gate admits to this reader alone: how
/// many, and how many the ledger holds no exchange for. Another place's exchanges are not counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewEarlier {
    pub sessions: u64,
    pub turns: u64,
    pub no_exchange: u64,
}

/// What a place's next turn would be shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ViewPreviewResult {
    pub place: String,
    /// The session whose next turn it is.
    pub session: String,
    /// `owner`: the owner in person, at that place (the one reader a preview has).
    pub reader: String,
    /// The place's `[context] assembly` today: `view` or `transcript`.
    pub assembly: String,
    /// The view, as this reader may see it; none when the floor did not fit (`overflow`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub view: Option<ViewShown>,
    /// Its counts, of what is listed alone.
    pub tally: ViewTally,
    /// Why the turn would be the transcript's instead (`unledgered_history`, ...); none when it takes the view.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fallback: Option<String>,
    /// The floor's own text did not fit the model's window (the turn is told so).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub overflow: Option<String>,
    pub budget: ViewBudget,
    /// Exchanges the window lays.
    pub window: u64,
    /// Fold nodes laid, by level (`1`, `2`, ...).
    pub folds: BTreeMap<String, u64>,
    pub earlier: ViewEarlier,
    /// What the preview did not do as a turn would: no model, no recall, nothing written.
    pub notes: Vec<String>,
}

/// A preview in plain lines.
pub fn preview_lines(r: &ViewPreviewResult) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "preview of a hypothetical next turn at {}, opened by your message in person: session {}\n",
        r.place, r.session
    );
    let checked = "unledgered history and the floor's fit";
    let verdict = match (r.assembly.as_str(), r.fallback.as_deref()) {
        ("view", Some(why)) => format!("it would fall back to the transcript: {why}"),
        ("view", None) => format!("it would take the view: the checks made pass ({checked})"),
        (set, Some(why)) => format!(
            "the place is set to {set}: its turn is the transcript's; switched to view, it would fall \
             back: {why}"
        ),
        (set, None) => format!(
            "the place is set to {set}: its turn is the transcript's; switched to view, the checks made \
             pass ({checked})"
        ),
    };
    let _ = writeln!(out, "  {verdict}");
    if let Some(o) = &r.overflow {
        let _ = writeln!(out, "  overflow: {o}");
    }
    let b = &r.budget;
    let _ = writeln!(
        out,
        "  budget: window {}, reserve {}, ceiling {}, here {}",
        b.provider_window, b.reserve, b.ceiling, b.here_allowance
    );
    if let Some(v) = &r.view {
        out.push_str(&lines(v));
    }
    out.push_str(&tally_lines(&r.tally));
    let folds: Vec<String> = r
        .folds
        .iter()
        .map(|(l, n)| format!("level {l}: {n}"))
        .collect();
    let _ = writeln!(
        out,
        "  window: {} exchanges; folds: {}",
        r.window,
        match folds.is_empty() {
            true => "none".into(),
            false => folds.join(", "),
        }
    );
    let e = &r.earlier;
    let _ = writeln!(
        out,
        "  earlier turns here: {} in {} sessions, {} with no exchange",
        e.turns, e.sessions, e.no_exchange
    );
    for n in &r.notes {
        let _ = writeln!(out, "  ({n})");
    }
    out
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
                kind_said(&x.kind, x.level),
                range,
                x.band,
                x.form,
                x.tokens,
                x.label,
                x.reasons.join(", ")
            );
            for l in &x.labels {
                let _ = writeln!(out, "      jev {l}");
            }
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
                kind_said(&d.kind, d.level),
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
    out.push('\n');
    if let Some(r) = &v.restricted {
        let _ = writeln!(out, "  ({r})");
    }
    out
}

/// A piece's kind as the plain lines say it: a fold with its rung (`folds@2`).
fn kind_said(kind: &str, level: Option<u8>) -> String {
    match level {
        Some(l) => format!("{kind}@{l}"),
        None => kind.to_string(),
    }
}

/// A view's counts in plain lines.
pub fn tally_lines(t: &ViewTally) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "counts of view {} (present {}, {} tokens)\n",
        t.compilation, t.present, t.tokens
    );
    for (title, m) in [
        ("slots", &t.slots),
        ("reasons", &t.reasons),
        ("dropped", &t.dropped),
        ("dropped, by reason", &t.dropped_reasons),
    ] {
        if m.is_empty() {
            continue;
        }
        let parts: Vec<String> = m
            .iter()
            .map(|(k, c)| format!("{k} {} ({} tokens)", c.pieces, c.tokens))
            .collect();
        let _ = writeln!(out, "  {title}: {}", parts.join(", "));
    }
    out
}

#[cfg(test)]
mod tests;
